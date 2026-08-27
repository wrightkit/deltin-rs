//! Recoverable parser: tokens -> AST + diagnostics.
//!
//! Policy (architecture §8/§10): never panic; every consumed region maps to an
//! AST node; regions that cannot be parsed become `Error` nodes; diagnostics
//! are emitted at the point of first failure with the recovery skip recorded.

use crate::diagnostics::{error, Diagnostic, Phase};
use crate::span::{FileId, Span};
use crate::syntax::ast::*;
use crate::syntax::token::{InterpHole, StrForm, Token, TokenKind};

pub const MAX_PARSE_ERRORS: usize = 200;

pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    file: FileId,
    text: &'a str,
    diagnostics: Vec<Diagnostic>,
    next_node: u32,
    /// Open delimiters awaiting their close (for PR032 recovery).
    open_delims: Vec<(Span, TokenKind)>,
    /// DocComment spans preceding the next item.
    pending_docs: Vec<Span>,
    /// Lookahead depth: diagnostics are suppressed while > 0.
    silent: usize,
    /// Formatted-string argument context: a `>` is only a comparison if an
    /// expression follows it (upstream `StringCheck`).
    string_check: bool,
}

pub fn parse(tokens: &[Token], file: FileId, text: &str) -> (AstFile, Vec<Diagnostic>) {
    let mut p = Parser {
        tokens,
        pos: 0,
        file,
        text,
        diagnostics: Vec::new(),
        next_node: 1,
        open_delims: Vec::new(),
        pending_docs: Vec::new(),
        silent: 0,
        string_check: false,
    };
    let ast = p.parse_file();
    // Synthesized closes for unclosed delimiters.
    for (open, kind) in &p.open_delims {
        p.diagnostics.push(error(
            Phase::Parse,
            "PR032",
            *open,
            format!("unclosed {}; closing bracket synthesized", kind.describe()),
        ));
    }
    (ast, p.diagnostics)
}

impl Parser<'_> {
    fn node(&mut self) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        id
    }

    fn text_of(&self, span: Span) -> &str {
        &self.text[span.start as usize..span.end as usize]
    }

    fn err(&mut self, code: &str, span: Span, msg: impl Into<String>) {
        if self.silent > 0 {
            return;
        }
        if self.diagnostics.len() < MAX_PARSE_ERRORS {
            self.diagnostics.push(error(Phase::Parse, code, span, msg));
        }
    }

    /// Skip trivia; record doc comments.
    fn skip_trivia(&mut self) {
        while self.pos < self.tokens.len() {
            match self.tokens[self.pos].kind {
                TokenKind::DocComment => {
                    self.pending_docs.push(self.tokens[self.pos].span);
                    self.pos += 1;
                }
                k if k.is_trivia() => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&mut self) -> TokenKind {
        self.skip_trivia();
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].kind
        } else {
            TokenKind::Eof
        }
    }

    fn peek2(&mut self) -> TokenKind {
        self.skip_trivia();
        let mut i = self.pos;
        while i < self.tokens.len() && self.tokens[i].kind.is_trivia() {
            i += 1;
        }
        i += 1;
        while i < self.tokens.len() && self.tokens[i].kind.is_trivia() {
            i += 1;
        }
        if i < self.tokens.len() {
            self.tokens[i].kind
        } else {
            TokenKind::Eof
        }
    }

    fn peek_token(&mut self) -> Token {
        self.skip_trivia();
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].clone()
        } else {
            Token::new(
                TokenKind::Eof,
                Span::new(self.file, self.text.len() as u32, self.text.len() as u32),
            )
        }
    }

    fn at(&mut self, kind: TokenKind) -> bool {
        self.peek() == kind
    }

    /// Consume the next non-trivia token.
    fn consume(&mut self) -> Token {
        self.skip_trivia();
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            match t.kind {
                TokenKind::LParen | TokenKind::LBrace | TokenKind::LBracket => {
                    self.open_delims.push((t.span, t.kind));
                }
                TokenKind::RParen | TokenKind::RBrace | TokenKind::RBracket => {
                    self.open_delims.pop();
                }
                _ => {}
            }
            t
        } else {
            let span = Span::new(self.file, self.text.len() as u32, self.text.len() as u32);
            Token::new(TokenKind::Eof, span)
        }
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Option<Token> {
        if self.at(kind) {
            Some(self.consume())
        } else {
            let tok = self.peek_token();
            let span = tok.span;
            let found = tok.kind.describe();
            self.err("PR002", span, format!("expected {what}, found {found}"));
            None
        }
    }

    fn expect_ident(&mut self, what: &str) -> Option<Ident> {
        if self.at(TokenKind::Ident) {
            let t = self.consume();
            Some(Ident {
                id: self.node(),
                span: t.span,
                name: self.text_of(t.span).to_string(),
            })
        } else {
            let tok = self.peek_token();
            self.err(
                "PR010",
                tok.span,
                format!(
                    "expected identifier ({what}), found {}",
                    tok.kind.describe()
                ),
            );
            None
        }
    }

    fn span_join(&self, a: Span, b: Span) -> Span {
        Span::new(self.file, a.start.min(b.start), a.end.max(b.end))
    }

    fn span_from(&self, start: u32) -> Span {
        Span::new(self.file, start, self.prev_end().end)
    }

    /// End offset of the last consumed token (for item spans).
    fn prev_end(&self) -> Span {
        if self.pos > 0 {
            let mut i = self.pos - 1;
            while i > 0 && self.tokens[i].kind.is_trivia() {
                i -= 1;
            }
            self.tokens[i].span
        } else {
            Span::new(self.file, 0, 0)
        }
    }

    // ------------------------------------------------------------------
    // File / items
    // ------------------------------------------------------------------

    fn parse_file(&mut self) -> AstFile {
        let mut items = Vec::new();
        let mut doc_comments = Vec::new();
        loop {
            if self.at(TokenKind::Eof) {
                break;
            }
            let start = self.peek_token().span.start;
            let before = self.pos;
            let item = match self.parse_item() {
                Some(i) => i,
                None => {
                    // Recovery: skip to the next item start.
                    self.pending_docs.clear();
                    if self.pos == before {
                        self.consume();
                    }
                    self.skip_to_item_boundary();
                    continue;
                }
            };
            if self.pos == before {
                // Guarantee progress even for error items.
                self.consume();
            }
            for d in self.pending_docs.drain(..) {
                doc_comments.push((d, item.id));
            }
            items.push(Item {
                id: item.id,
                span: self.span_from(start),
                kind: item.kind,
            });
        }
        let id = self.node();
        let span = Span::new(self.file, 0, self.text.len() as u32);
        AstFile {
            id,
            span,
            items,
            doc_comments,
        }
    }

    fn skip_to_item_boundary(&mut self) {
        let mut consumed = 0usize;
        while !self.at(TokenKind::Eof) {
            let k = self.peek();
            if k == TokenKind::RBrace || is_item_start(k) {
                break;
            }
            let t = self.consume();
            if consumed == 0 {
                self.err(
                    "PR001",
                    t.span,
                    format!("unexpected token {}", t.kind.describe()),
                );
            }
            consumed += 1;
        }
    }

    fn parse_item(&mut self) -> Option<Item> {
        let id = self.node();
        let k = self.peek();
        let start = self.peek_token().span.start;
        let kind = match k {
            TokenKind::KwRule | TokenKind::KwDisabled if self.is_rule_start() => {
                self.parse_rule_or_vanilla()
            }
            TokenKind::KwImport => self.parse_import_decl(),
            TokenKind::KwGlobalVar | TokenKind::KwPlayerVar
                if self.peek2() == TokenKind::LBrace =>
            {
                self.parse_var_reservation()
            }
            TokenKind::KwGlobalVar
            | TokenKind::KwPlayerVar
            | TokenKind::KwDefine
            | TokenKind::KwVoid
            | TokenKind::KwConst
            | TokenKind::KwPublic
            | TokenKind::KwPrivate
            | TokenKind::KwProtected
            | TokenKind::KwStatic
            | TokenKind::KwVirtual
            | TokenKind::KwOverride
            | TokenKind::KwRecursive
            | TokenKind::KwPersist
            | TokenKind::KwRef
            | TokenKind::KwIn => self.parse_declaration(),
            TokenKind::KwType => self.parse_type_alias(),
            TokenKind::KwClass | TokenKind::KwStruct | TokenKind::KwEnum => self.parse_type_decl(),
            TokenKind::KwSingle
                if matches!(
                    self.peek2(),
                    TokenKind::KwClass | TokenKind::KwStruct | TokenKind::KwEnum
                ) =>
            {
                self.parse_type_decl()
            }
            TokenKind::Ident => {
                let name = {
                    let span = self.peek_token().span;
                    self.text_of(span).to_string()
                };
                if matches!(name.as_str(), "variables" | "subroutines" | "settings")
                    && self.peek2() == TokenKind::LBrace
                {
                    self.parse_vanilla_block()
                } else if self.is_declaration_lookahead() {
                    self.parse_declaration()
                } else if self.is_hook_lookahead() {
                    self.parse_hook_item()
                } else {
                    self.parse_declaration()
                }
            }
            _ => {
                let t = self.consume();
                self.err(
                    "PR001",
                    t.span,
                    format!("unexpected token {} at file level", t.kind.describe()),
                );
                return None;
            }
        };
        Some(Item {
            id,
            span: self.span_from(start),
            kind,
        })
    }

    fn is_rule_start(&mut self) -> bool {
        match self.peek() {
            TokenKind::KwRule => true,
            TokenKind::KwDisabled => self.peek2() == TokenKind::KwRule,
            _ => false,
        }
    }

    // ------------------------------------------------------------------
    // Rules
    // ------------------------------------------------------------------

    fn parse_rule_or_vanilla(&mut self) -> ItemKind {
        let disabled = self.at(TokenKind::KwDisabled);
        if disabled {
            self.consume();
        }
        self.expect(TokenKind::KwRule, "'rule'");
        if self.at(TokenKind::LParen) {
            return ItemKind::VanillaRule(self.parse_vanilla_rule());
        }
        self.expect(TokenKind::Colon, "':' after 'rule'");
        let name_tok = match self.expect(TokenKind::Str, "rule name string") {
            Some(t) => t,
            None => {
                // Recovery: consume a few tokens so the loop makes progress.
                self.consume();
                return ItemKind::Error {
                    consumed: self.prev_end(),
                };
            }
        };
        let name = self.str_expr(name_tok);

        // Optional sort order: [Number] or [- Number].
        let mut sort_order = None;
        if self.at(TokenKind::Int)
            || self.at(TokenKind::Real)
            || (self.at(TokenKind::Minus)
                && matches!(self.peek2(), TokenKind::Int | TokenKind::Real))
        {
            sort_order = Some(self.parse_number_expr());
        }

        // Settings / event: `Ident.Ident` entries. The first is the event
        // (per wiki "event token between name and conditions").
        let mut event = None;
        let mut settings = Vec::new();
        while self.at(TokenKind::Ident) && self.peek2() == TokenKind::Dot {
            let member = self.parse_member_expr();
            if event.is_none() {
                event = Some(member);
            } else {
                settings.push(member);
            }
        }

        // Conditions: [disabled] if (expr)
        let mut conditions = Vec::new();
        loop {
            let disabled_cond = self.at(TokenKind::KwDisabled);
            let save = self.pos;
            if disabled_cond {
                self.consume();
            }
            if !self.at(TokenKind::KwIf) {
                if disabled_cond {
                    self.pos = save; // disabled without if: not a condition
                }
                break;
            }
            let cond_span = self.consume().span; // if
            self.expect(TokenKind::LParen, "'(' after 'if'");
            let expr = self.parse_expr();
            self.expect(TokenKind::RParen, "')'");
            conditions.push(RuleCondition {
                expr,
                disabled: disabled_cond,
                span: cond_span,
            });
        }

        let body = self.parse_statement();

        ItemKind::Rule(RuleDecl {
            name,
            disabled,
            sort_order,
            settings,
            event,
            conditions,
            body: Box::new(body),
        })
    }

    fn parse_vanilla_rule(&mut self) -> VanillaRuleDecl {
        self.expect(TokenKind::LParen, "'('");
        let name = if self.at(TokenKind::Str) {
            let t = self.consume();
            Some(self.str_expr(t))
        } else {
            None
        };
        self.expect(TokenKind::RParen, "')'");
        let mut sections = VanillaSections {
            event: None,
            conditions: None,
            actions: None,
        };
        self.expect(TokenKind::LBrace, "'{'");
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Ident) && self.peek2() == TokenKind::LBrace {
                let name = {
                    let span = self.peek_token().span;
                    self.text_of(span).to_string()
                };
                self.consume();
                let body = self.capture_balanced_block();
                match name.as_str() {
                    "event" => sections.event = Some(body),
                    "conditions" => sections.conditions = Some(body),
                    "actions" => sections.actions = Some(body),
                    _ => {}
                }
            } else {
                self.consume();
            }
        }
        self.expect(TokenKind::RBrace, "'}'");
        VanillaRuleDecl { name, sections }
    }

    /// Upstream `IsHook`: `Ident(. Ident)* =` at item level.
    fn is_hook_lookahead(&mut self) -> bool {
        let save = self.pos;
        let save_delims = self.open_delims.len();
        let mut result = false;
        if self.at(TokenKind::Ident) {
            loop {
                self.consume();
                if self.at(TokenKind::Dot) {
                    self.consume();
                } else {
                    break;
                }
            }
            result = self.at(TokenKind::Eq);
        }
        self.pos = save;
        self.open_delims.truncate(save_delims);
        result
    }

    fn parse_hook_item(&mut self) -> ItemKind {
        // Parse the target without consuming the '=' as an assignment.
        let target = self.parse_binary(2);
        self.expect(TokenKind::Eq, "'='");
        let value = self.parse_expr();
        self.expect_semicolon();
        ItemKind::Hook { target, value }
    }

    fn parse_vanilla_block(&mut self) -> ItemKind {
        let name = {
            let span = self.peek_token().span;
            self.text_of(span).to_string()
        };
        self.consume();
        let kind = match name.as_str() {
            "variables" => VanillaBlockKind::Variables,
            "subroutines" => VanillaBlockKind::Subroutines,
            _ => VanillaBlockKind::Settings,
        };
        let body = self.capture_balanced_block();
        ItemKind::VanillaBlock(VanillaBlockDecl { kind, body })
    }

    /// Consume a `{` ... matching `}` block, returning the span of the whole
    /// block (including the braces).
    fn capture_balanced_block(&mut self) -> Span {
        let open = match self.expect(TokenKind::LBrace, "'{'") {
            Some(t) => t.span,
            None => return self.peek_token().span,
        };
        let mut depth = 1usize;
        let mut end = open.end;
        while depth > 0 && !self.at(TokenKind::Eof) {
            let k = self.peek();
            match k {
                TokenKind::LBrace => {
                    depth += 1;
                    end = self.consume().span.end;
                }
                TokenKind::RBrace => {
                    depth -= 1;
                    end = self.consume().span.end;
                }
                _ => {
                    end = self.consume().span.end;
                }
            }
        }
        Span::new(self.file, open.start, end)
    }

    // ------------------------------------------------------------------
    // Declarations
    // ------------------------------------------------------------------

    fn parse_attrs(&mut self) -> FuncAttrs {
        let mut attrs = FuncAttrs {
            access: None,
            static_: false,
            virtual_: false,
            override_: false,
            recursive: false,
            persist: false,
            ref_: false,
            storage: None,
            subroutine: None,
        };
        loop {
            match self.peek() {
                TokenKind::KwPublic => {
                    self.consume();
                    attrs.access = Some(Access::Public);
                }
                TokenKind::KwPrivate => {
                    self.consume();
                    attrs.access = Some(Access::Private);
                }
                TokenKind::KwProtected => {
                    self.consume();
                    attrs.access = Some(Access::Protected);
                }
                TokenKind::KwStatic => {
                    self.consume();
                    attrs.static_ = true;
                }
                TokenKind::KwVirtual => {
                    self.consume();
                    attrs.virtual_ = true;
                }
                TokenKind::KwOverride => {
                    self.consume();
                    attrs.override_ = true;
                }
                TokenKind::KwRecursive => {
                    self.consume();
                    attrs.recursive = true;
                }
                TokenKind::KwPersist => {
                    self.consume();
                    attrs.persist = true;
                }
                TokenKind::KwGlobalVar => {
                    self.consume();
                    attrs.storage = Some(StorageModifier::GlobalVar);
                }
                TokenKind::KwPlayerVar => {
                    self.consume();
                    attrs.storage = Some(StorageModifier::PlayerVar);
                }
                TokenKind::KwRef => {
                    self.consume();
                    attrs.ref_ = true;
                }
                TokenKind::KwIn => {
                    // Accepted by upstream ParseAttributes; recorded but unused
                    // at function level.
                    self.consume();
                }
                _ => break,
            }
        }
        attrs
    }

    fn parse_declaration(&mut self) -> ItemKind {
        let attrs = self.parse_attrs();
        match self.parse_decl_after_attrs(attrs, true) {
            DeclOutcome::Function(f) => ItemKind::Function(f),
            DeclOutcome::Var(v) => ItemKind::Var(v),
            DeclOutcome::Error { consumed } => ItemKind::Error { consumed },
        }
    }

    /// Parse a declaration after attributes. If `semicolon` is true, the
    /// terminating `;` is consumed for variable declarations.
    fn parse_decl_after_attrs(&mut self, attrs: FuncAttrs, semicolon: bool) -> DeclOutcome {
        let ty = self.parse_type();
        let name = match self.expect_ident("declaration") {
            Some(n) => n,
            None => return DeclOutcome::Error { consumed: ty.span },
        };
        // Optional generic type args for generic functions.
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_params()
        } else {
            Vec::new()
        };
        if self.at(TokenKind::LParen) {
            // Function.
            self.consume();
            let params = self.parse_params();
            self.expect(TokenKind::RParen, "')'");
            // Optional subroutine name: [Str] or [playervar/globalvar Str].
            let mut subroutine = None;
            if self.at(TokenKind::Str) {
                let t = self.consume();
                subroutine = Some(SubroutineInfo {
                    rule_name: self.str_expr(t),
                    playervar: false,
                });
            } else if matches!(self.peek(), TokenKind::KwPlayerVar | TokenKind::KwGlobalVar) {
                let is_player = self.peek() == TokenKind::KwPlayerVar;
                self.consume();
                let t = match self.expect(TokenKind::Str, "subroutine name string") {
                    Some(t) => t,
                    None => {
                        return DeclOutcome::Error {
                            consumed: name.span,
                        };
                    }
                };
                subroutine = Some(SubroutineInfo {
                    rule_name: self.str_expr(t),
                    playervar: is_player,
                });
            }
            let body = if self.at(TokenKind::Colon) {
                // Macro (expression body).
                self.consume();
                let e = self.parse_expr();
                if semicolon {
                    self.expect_semicolon();
                }
                FuncBody::Expr(e)
            } else if self.at(TokenKind::LBrace) {
                FuncBody::Block(self.parse_block())
            } else if self.at(TokenKind::Semicolon) {
                if semicolon {
                    self.consume();
                }
                FuncBody::None
            } else {
                let span = self.peek_token().span;
                self.err(
                    "PR031",
                    span,
                    "expected function body ('{' block, ':' expression, or ';')",
                );
                FuncBody::None
            };
            let mut attrs = attrs;
            attrs.subroutine = subroutine;
            DeclOutcome::Function(FunctionDecl {
                attrs,
                name,
                type_params,
                params,
                ret: Some(ty),
                body,
            })
        } else {
            // Variable / field.
            let (var_id, extended, target) = self.parse_variable_elements_tail();
            let init = self.parse_optional_init();
            if semicolon {
                self.expect_semicolon();
            }
            DeclOutcome::Var(VarDecl {
                storage: attrs.storage,
                kind: if self.type_is_define(&ty) {
                    VarDeclKind::Define
                } else {
                    VarDeclKind::Typed(ty)
                },
                name,
                var_id,
                extended,
                target,
                is_const_init: matches!(init, Some((InitKind::Colon, _))),
                init,
            })
        }
    }

    fn type_is_define(&self, ty: &TypeRef) -> bool {
        matches!(
            &ty.kind,
            TypeRefKind::Name(Ident { name, .. }) if name == "define"
        )
    }

    fn parse_variable_elements_tail(&mut self) -> (Option<Expr>, bool, Option<Span>) {
        let mut var_id = None;
        let mut extended = false;
        let mut target = None;
        if self.at(TokenKind::Int) || self.at(TokenKind::Real) {
            var_id = Some(self.parse_number_expr());
        } else if self.at(TokenKind::Bang) {
            self.consume();
            extended = true;
            // Vanilla target: !"a" / !{"a", "b"}
            if self.at(TokenKind::Str) || self.at(TokenKind::LBrace) {
                target = Some(self.capture_target_span());
            }
        } else if self.at(TokenKind::LBrace) {
            // Vanilla target: {'checkpoint_reached'}
            target = Some(self.capture_target_span());
        }
        (var_id, extended, target)
    }

    fn capture_target_span(&mut self) -> Span {
        let start = self.peek_token().span.start;
        let mut depth = 0usize;
        let mut end = start;
        loop {
            let k = self.peek();
            match k {
                TokenKind::LBrace | TokenKind::LBracket => {
                    depth += 1;
                    end = self.consume().span.end;
                }
                TokenKind::RBrace | TokenKind::RBracket => {
                    depth = depth.saturating_sub(1);
                    end = self.consume().span.end;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Eof => break,
                _ => {
                    end = self.consume().span.end;
                }
            }
        }
        Span::new(self.file, start, end)
    }

    fn parse_optional_init(&mut self) -> Option<(InitKind, Expr)> {
        if self.at(TokenKind::Eq) {
            self.consume();
            let e = self.parse_expr();
            Some((InitKind::Eq, e))
        } else if self.at(TokenKind::Colon) {
            self.consume();
            let e = self.parse_expr();
            Some((InitKind::Colon, e))
        } else {
            None
        }
    }

    fn expect_semicolon(&mut self) {
        if self.at(TokenKind::Semicolon) {
            self.consume();
        } else if !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let t = self.peek_token();
            self.err(
                "PR011",
                t.span,
                format!("expected ';', found {}", t.kind.describe()),
            );
        }
    }

    fn parse_params(&mut self) -> Vec<ParamDecl> {
        let mut params = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            let before = self.pos;
            let p = self.parse_param();
            if self.pos == before {
                // No progress: consume one token to guarantee termination.
                self.consume();
            }
            params.push(p);
            if !self.at(TokenKind::Comma) {
                break;
            }
            self.consume();
        }
        params
    }

    fn parse_param(&mut self) -> ParamDecl {
        let mut mode = ParamMode::Value;
        loop {
            match self.peek() {
                TokenKind::KwIn => {
                    self.consume();
                    mode = ParamMode::In;
                }
                TokenKind::KwRef => {
                    self.consume();
                    mode = ParamMode::Ref;
                }
                TokenKind::KwConst => {
                    self.consume();
                    mode = ParamMode::Const;
                }
                _ => break,
            }
        }
        let ty = if self.at(TokenKind::KwDefine) {
            self.consume();
            None
        } else {
            Some(self.parse_type())
        };
        let name = self.expect_ident("parameter").unwrap_or_else(|| Ident {
            id: self.node(),
            span: self.peek_token().span,
            name: String::new(),
        });
        let mut extended = false;
        if self.at(TokenKind::Bang) {
            self.consume();
            extended = true;
            // Optional vanilla target after !.
            if self.at(TokenKind::Str) || self.at(TokenKind::LBrace) {
                self.capture_target_span();
            }
        }
        let default = if self.at(TokenKind::Eq) {
            self.consume();
            Some(self.parse_expr())
        } else {
            None
        };
        ParamDecl {
            mode,
            name,
            ty,
            default,
            extended,
        }
    }

    fn parse_type_params(&mut self) -> Vec<TypeParamDecl> {
        let mut params = Vec::new();
        self.expect(TokenKind::Lt, "'<'");
        while !self.at(TokenKind::Gt) && !self.at(TokenKind::Eof) {
            let bound = if self.at(TokenKind::KwSingle) {
                self.consume();
                Some(TypeParamBound::Single)
            } else {
                None
            };
            let name = match self.expect_ident("type parameter") {
                Some(n) => n,
                None => break,
            };
            params.push(TypeParamDecl { name, bound });
            if !self.at(TokenKind::Comma) {
                break;
            }
            self.consume();
        }
        self.expect(TokenKind::Gt, "'>'");
        params
    }

    fn parse_type_alias(&mut self) -> ItemKind {
        self.consume(); // `type`
        let name = match self.expect_ident("type alias name") {
            Some(n) => n,
            None => {
                self.consume();
                return ItemKind::Error {
                    consumed: self.prev_end(),
                };
            }
        };
        self.expect(TokenKind::Eq, "'='");
        let target = self.parse_type();
        self.expect_semicolon();
        ItemKind::TypeAlias(TypeAliasDecl { name, target })
    }

    fn parse_var_reservation(&mut self) -> ItemKind {
        let storage = if self.at(TokenKind::KwGlobalVar) {
            self.consume();
            StorageModifier::GlobalVar
        } else {
            self.consume();
            StorageModifier::PlayerVar
        };
        self.expect(TokenKind::LBrace, "'{'");
        let mut names = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let k = self.peek();
            if k == TokenKind::Str || k == TokenKind::Int || k == TokenKind::Real {
                names.push(self.parse_expr());
            } else {
                self.consume();
            }
            if self.at(TokenKind::Comma) {
                self.consume();
            }
        }
        self.expect(TokenKind::RBrace, "'}'");
        self.expect_semicolon();
        ItemKind::VarReservation(VarReservation { storage, names })
    }

    // ------------------------------------------------------------------
    // Type declarations (class/struct/enum)
    // ------------------------------------------------------------------

    fn parse_type_decl(&mut self) -> ItemKind {
        let single = if self.at(TokenKind::KwSingle) {
            self.consume();
            true
        } else {
            false
        };
        let kind = match self.peek() {
            TokenKind::KwClass => {
                self.consume();
                TypeDeclKind::Class
            }
            TokenKind::KwStruct => {
                self.consume();
                TypeDeclKind::Struct
            }
            _ => {
                self.consume();
                TypeDeclKind::Enum
            }
        };
        let name = match self.expect_ident("type name") {
            Some(n) => n,
            None => {
                self.consume();
                return ItemKind::Error {
                    consumed: self.prev_end(),
                };
            }
        };
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_params()
        } else {
            Vec::new()
        };
        let mut base = None;
        let mut implements = Vec::new();
        if self.at(TokenKind::Colon) {
            self.consume();
            loop {
                let t = self.parse_type();
                if base.is_none() {
                    base = Some(t);
                } else {
                    implements.push(t);
                }
                if self.at(TokenKind::Comma) {
                    self.consume();
                } else {
                    break;
                }
            }
        }
        let mut members = Vec::new();
        self.expect(TokenKind::LBrace, "'{'");
        if kind == TypeDeclKind::Enum {
            self.parse_enum_members(&mut members);
        } else {
            self.parse_class_members(&mut members);
        }
        self.expect(TokenKind::RBrace, "'}'");
        ItemKind::TypeDecl(TypeDecl {
            kind,
            single,
            name,
            type_params,
            base,
            implements,
            members,
        })
    }

    fn parse_enum_members(&mut self, members: &mut Vec<MemberDecl>) {
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if !self.at(TokenKind::Ident) {
                let t = self.consume();
                self.err(
                    "PR001",
                    t.span,
                    format!("unexpected token {} in enum body", t.kind.describe()),
                );
                continue;
            }
            let id = self.node();
            let name = {
                let t = self.consume();
                Ident {
                    id: self.node(),
                    span: t.span,
                    name: self.text_of(t.span).to_string(),
                }
            };
            let mut fields = Vec::new();
            if self.at(TokenKind::LParen) {
                self.consume();
                while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                    fields.push(self.parse_type());
                    if !self.at(TokenKind::Comma) {
                        break;
                    }
                    self.consume();
                }
                self.expect(TokenKind::RParen, "')'");
            }
            let discriminant = if self.at(TokenKind::Eq) {
                self.consume();
                Some(self.parse_expr())
            } else {
                None
            };
            let span = match discriminant {
                Some(ref d) => d.span,
                None => name.span,
            };
            members.push(MemberDecl {
                id,
                span,
                kind: MemberDeclKind::EnumMember(EnumMemberDecl {
                    name,
                    discriminant,
                    fields,
                }),
            });
            if self.at(TokenKind::Comma) {
                self.consume();
            }
        }
    }

    fn parse_class_members(&mut self, members: &mut Vec<MemberDecl>) {
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let id = self.node();
            let start = self.peek_token().span.start;
            let before = self.pos;
            let attrs = self.parse_attrs();
            if self.at(TokenKind::KwConstructor) {
                let c = self.parse_constructor_body(attrs.access);
                members.push(MemberDecl {
                    id,
                    span: self.span_from(start),
                    kind: MemberDeclKind::Constructor(c),
                });
                continue;
            }
            match self.parse_decl_after_attrs(attrs, true) {
                DeclOutcome::Function(f) => {
                    members.push(MemberDecl {
                        id,
                        span: self.span_from(start),
                        kind: MemberDeclKind::Method(f),
                    });
                }
                DeclOutcome::Var(v) => {
                    members.push(MemberDecl {
                        id,
                        span: self.span_from(start),
                        kind: MemberDeclKind::Field(v),
                    });
                }
                DeclOutcome::Error { consumed } => {
                    self.err("PR031", consumed, "malformed class member declaration");
                }
            }
            if self.pos == before {
                self.consume();
            }
        }
    }

    fn parse_constructor_body(&mut self, access: Option<Access>) -> ConstructorDecl {
        self.consume(); // constructor
        self.expect(TokenKind::LParen, "'('");
        let params = self.parse_params();
        self.expect(TokenKind::RParen, "')'");
        let subroutine = if self.at(TokenKind::Str) {
            let t = self.consume();
            Some(self.str_expr(t))
        } else {
            None
        };
        let body = self.parse_block();
        ConstructorDecl {
            access,
            params,
            subroutine,
            body,
        }
    }

    // ------------------------------------------------------------------
    // Imports
    // ------------------------------------------------------------------

    fn parse_import_decl(&mut self) -> ItemKind {
        self.consume(); // import
        let path_tok = match self.expect(TokenKind::Str, "import path string") {
            Some(t) => t,
            None => {
                self.consume();
                return ItemKind::Error {
                    consumed: self.prev_end(),
                };
            }
        };
        let raw = self.string_value(&path_tok);
        let kind = if raw.starts_with('!') {
            ImportKind::BundledModule
        } else if raw.ends_with(".json") {
            ImportKind::JsonSettings
        } else if raw.ends_with(".lobby") {
            ImportKind::LobbySettings
        } else {
            ImportKind::Source
        };
        let as_name = if self.at(TokenKind::KwAs) {
            self.consume();
            self.expect_ident("import alias")
        } else {
            None
        };
        self.expect_semicolon();
        ItemKind::Import(ImportDecl {
            path: self.str_expr(path_tok),
            kind,
            as_name,
        })
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    fn parse_block(&mut self) -> BlockStmt {
        let id = self.node();
        let open = self.expect(TokenKind::LBrace, "'{'");
        let start = match open {
            Some(t) => t.span.start,
            None => self.peek_token().span.start,
        };
        let mut stmts = Vec::new();
        loop {
            if self.at(TokenKind::RBrace) || self.at(TokenKind::Eof) {
                break;
            }
            let before = self.pos;
            let s = self.parse_statement();
            if self.pos == before {
                // Guarantee progress.
                self.consume();
            }
            stmts.push(s);
        }
        let end = self
            .expect(TokenKind::RBrace, "'}'")
            .map_or(self.prev_end().end, |t| t.span.end);
        BlockStmt {
            id,
            span: Span::new(self.file, start, end),
            stmts,
        }
    }

    fn parse_statement(&mut self) -> Stmt {
        self.parse_statement_with(true)
    }

    /// Parse a statement. `semicolon` controls whether terminating `;`
    /// characters are required (false for for-loop headers per upstream
    /// `ParseStatement(false)`).
    fn parse_statement_with(&mut self, semicolon: bool) -> Stmt {
        let id = self.node();
        let start = self.peek_token().span.start;
        let kind = self.parse_statement_kind(semicolon);
        let end = self.prev_end();
        Stmt {
            id,
            span: Span::new(self.file, start, end.end),
            kind,
        }
    }

    fn parse_statement_kind(&mut self, semicolon: bool) -> StmtKind {
        match self.peek() {
            TokenKind::LBrace => {
                if self.peek2() == TokenKind::Str {
                    return self.parse_hook_statement();
                }
                StmtKind::Block(self.parse_block())
            }
            TokenKind::KwBreak => {
                self.consume();
                if semicolon {
                    self.expect_semicolon();
                }
                StmtKind::Break
            }
            TokenKind::KwContinue => {
                self.consume();
                if semicolon {
                    self.expect_semicolon();
                }
                StmtKind::Continue
            }
            TokenKind::KwReturn => {
                self.consume();
                let value = if matches!(
                    self.peek(),
                    TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
                ) {
                    None
                } else {
                    Some(self.parse_expr())
                };
                if semicolon {
                    self.expect_semicolon();
                }
                StmtKind::Return { value }
            }
            TokenKind::KwIf => self.parse_if(),
            TokenKind::KwSwitch => self.parse_switch(),
            TokenKind::KwFor => self.parse_for(),
            TokenKind::KwWhile => {
                self.consume();
                self.expect(TokenKind::LParen, "'('");
                let cond = self.parse_expr();
                self.expect(TokenKind::RParen, "')'");
                let body = self.parse_statement();
                StmtKind::While {
                    cond,
                    body: Box::new(body),
                }
            }
            TokenKind::KwForeach => {
                self.consume();
                self.expect(TokenKind::LParen, "'('");
                let ty = if self.at(TokenKind::KwDefine) {
                    self.consume();
                    None
                } else {
                    Some(self.parse_type())
                };
                let name = self.expect_ident("loop variable");
                let mut extended = false;
                if self.at(TokenKind::Bang) {
                    self.consume();
                    extended = true;
                }
                self.expect(TokenKind::KwIn, "'in'");
                let collection = self.parse_expr();
                self.expect(TokenKind::RParen, "')'");
                let body = self.parse_statement();
                StmtKind::Foreach {
                    var: VarDecl {
                        storage: None,
                        kind: match ty {
                            None => VarDeclKind::Define,
                            Some(t) => VarDeclKind::Typed(t),
                        },
                        name: name.unwrap_or_else(|| Ident {
                            id: self.node(),
                            span: self.peek_token().span,
                            name: String::new(),
                        }),
                        var_id: None,
                        extended,
                        target: None,
                        is_const_init: false,
                        init: None,
                    },
                    collection,
                    body: Box::new(body),
                }
            }
            TokenKind::KwDelete => {
                self.consume();
                let target = self.parse_expr();
                if semicolon {
                    self.expect_semicolon();
                }
                StmtKind::Delete { target }
            }
            _ => {
                if self.is_declaration_lookahead() {
                    let attrs = self.parse_attrs();
                    match self.parse_decl_after_attrs(attrs, true) {
                        DeclOutcome::Var(v) => StmtKind::Var(v),
                        DeclOutcome::Function(f) => {
                            self.err("PR031", f.name.span, "function declaration inside a block");
                            StmtKind::Error {
                                consumed: f.name.span,
                            }
                        }
                        DeclOutcome::Error { consumed } => StmtKind::Error { consumed },
                    }
                } else {
                    let e = self.parse_expr();
                    if semicolon {
                        self.expect_semicolon();
                    }
                    StmtKind::Expr(e)
                }
            }
        }
    }

    /// Upstream `IsDeclaration` lookahead: attrs + type + ident followed by
    /// `;`, `=`, `!`, a number (workshop ID), `:`, `{` (vanilla target), or
    /// EOF — a declaration; otherwise an expression statement.
    fn is_declaration_lookahead(&mut self) -> bool {
        let save = self.pos;
        let save_delims = self.open_delims.len();
        let saved_docs = self.pending_docs.len();
        self.parse_attrs();
        let type_ok = match self.peek() {
            TokenKind::KwVoid | TokenKind::KwDefine | TokenKind::KwConst => {
                self.consume();
                true
            }
            TokenKind::Ident => self.try_parse_type().is_some(),
            _ => false,
        };
        let result = if type_ok {
            self.skip_trivia();
            match self.peek() {
                TokenKind::Ident => {
                    self.consume();
                    self.skip_trivia();
                    matches!(
                        self.peek(),
                        TokenKind::Semicolon
                            | TokenKind::Eq
                            | TokenKind::Bang
                            | TokenKind::Int
                            | TokenKind::Real
                            | TokenKind::Colon
                            | TokenKind::LBrace
                            | TokenKind::Eof
                    )
                }
                _ => false,
            }
        } else {
            false
        };
        self.pos = save;
        self.open_delims.truncate(save_delims);
        self.pending_docs.truncate(saved_docs);
        result
    }

    fn parse_hook_statement(&mut self) -> StmtKind {
        self.consume(); // {
        let str_tok = match self.expect(TokenKind::Str, "vanilla variable name string") {
            Some(t) => t,
            None => {
                return StmtKind::Error {
                    consumed: self.prev_end(),
                }
            }
        };
        self.expect(TokenKind::RBrace, "'}'");
        let name = self.str_lit(str_tok.clone());
        let mut index = None;
        if self.at(TokenKind::LBracket) {
            self.consume();
            if self.at(TokenKind::DotDot) {
                // [..] spread indexer — opaque.
                self.consume();
                self.expect(TokenKind::RBracket, "']'");
            } else {
                let e = self.parse_expr();
                self.expect(TokenKind::RBracket, "']'");
                index = Some(Box::new(e));
            }
        }
        let span = self.span_join(str_tok.span, self.prev_end());
        let target = Expr {
            id: self.node(),
            span,
            kind: ExprKind::VanillaTarget { name, index },
        };
        self.expect(TokenKind::Eq, "'='");
        let value = self.parse_expr();
        self.expect_semicolon();
        StmtKind::Hook { target, value }
    }

    fn parse_if(&mut self) -> StmtKind {
        self.consume(); // if
        self.expect(TokenKind::LParen, "'('");
        let cond = self.parse_expr();
        self.expect(TokenKind::RParen, "')'");
        let then = self.parse_statement();
        let els = if self.at(TokenKind::KwElse) {
            self.consume();
            Some(Box::new(self.parse_statement()))
        } else {
            None
        };
        StmtKind::If {
            cond,
            then: Box::new(then),
            els,
        }
    }

    fn parse_switch(&mut self) -> StmtKind {
        self.consume(); // switch
        self.expect(TokenKind::LParen, "'('");
        let scrutinee = self.parse_expr();
        self.expect(TokenKind::RParen, "')'");
        self.expect(TokenKind::LBrace, "'{'");
        let mut arms = Vec::new();
        let mut current: Option<SwitchArm> = None;
        loop {
            let k = self.peek();
            if k == TokenKind::RBrace || k == TokenKind::Eof {
                break;
            }
            let start = self.peek_token().span.start;
            match k {
                TokenKind::KwCase | TokenKind::KwDefault => {
                    let is_default = k == TokenKind::KwDefault;
                    self.consume();
                    let label = if is_default {
                        None
                    } else {
                        Some(self.parse_expr())
                    };
                    self.expect(TokenKind::Colon, "':'");
                    if let Some(mut arm) = current.take() {
                        arm.span = Span::new(self.file, arm.span.start, start);
                        arms.push(arm);
                    }
                    current = Some(SwitchArm {
                        label,
                        stmts: Vec::new(),
                        span: Span::new(self.file, start, start),
                    });
                }
                _ => {
                    let s = self.parse_statement();
                    if let Some(arm) = current.as_mut() {
                        arm.stmts.push(s);
                    } else {
                        self.err("PR001", s.span, "statement outside a switch case");
                    }
                }
            }
        }
        if let Some(mut arm) = current.take() {
            arm.span = Span::new(self.file, arm.span.start, self.prev_end().end);
            arms.push(arm);
        }
        self.expect(TokenKind::RBrace, "'}'");
        StmtKind::Switch(SwitchStmt { scrutinee, arms })
    }

    fn parse_for(&mut self) -> StmtKind {
        self.consume(); // for
        self.expect(TokenKind::LParen, "'('");
        let init = if self.at(TokenKind::Semicolon) {
            self.consume();
            None
        } else {
            let s = self.parse_for_init_statement();
            if self.at(TokenKind::Semicolon) {
                self.consume();
            } else {
                let t = self.peek_token();
                self.err(
                    "PR011",
                    t.span,
                    format!(
                        "expected ';' after for initializer, found {}",
                        t.kind.describe()
                    ),
                );
            }
            Some(Box::new(s))
        };
        let cond = if self.at(TokenKind::Semicolon) {
            self.consume();
            None
        } else {
            let e = self.parse_expr();
            self.expect(TokenKind::Semicolon, "';'");
            Some(e)
        };
        let step = if self.at(TokenKind::RParen) {
            None
        } else {
            Some(Box::new(self.parse_statement_with(false)))
        };
        self.expect(TokenKind::RParen, "')'");
        let body = self.parse_statement();
        StmtKind::For(ForStmt {
            init,
            cond,
            step,
            body: Box::new(body),
        })
    }

    /// The for initializer is a statement that must NOT consume the trailing
    /// `;` (upstream `ParseStatement(false)` + explicit `ParseSemicolon`).
    fn parse_for_init_statement(&mut self) -> Stmt {
        let id = self.node();
        let start = self.peek_token().span.start;
        let kind = if self.is_declaration_lookahead() {
            let attrs = self.parse_attrs();
            match self.parse_decl_after_attrs(attrs, false) {
                DeclOutcome::Var(v) => StmtKind::Var(v),
                DeclOutcome::Function(f) => {
                    self.err(
                        "PR031",
                        f.name.span,
                        "function declaration inside a for initializer",
                    );
                    StmtKind::Error {
                        consumed: f.name.span,
                    }
                }
                DeclOutcome::Error { consumed } => StmtKind::Error { consumed },
            }
        } else {
            StmtKind::Expr(self.parse_expr())
        };
        let end = self.prev_end();
        Stmt {
            id,
            span: Span::new(self.file, start, end.end),
            kind,
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    pub fn parse_expr(&mut self) -> Expr {
        self.parse_ternary()
    }

    fn parse_single_struct_value(&mut self) -> Expr {
        // `single { value }` — single-valued struct literal.
        let single_tok = self.at(TokenKind::KwSingle);
        if single_tok {
            self.consume();
        }
        let open = match self.expect(TokenKind::LBrace, "'{'") {
            Some(t) => t.span,
            None => return self.error_expr(self.prev_end()),
        };
        let value = self.parse_expr();
        let end = self
            .expect(TokenKind::RBrace, "'}'")
            .map_or(value.span.end, |t| t.span.end);
        Expr {
            id: self.node(),
            span: Span::new(self.file, open.start, end),
            kind: ExprKind::StructLit(StructLit {
                fields: Vec::new(),
                base: None,
                single_value: Some(Box::new(value)),
            }),
        }
    }

    fn error_expr(&mut self, span: Span) -> Expr {
        Expr {
            id: self.node(),
            span,
            kind: ExprKind::Error { consumed: span },
        }
    }

    fn parse_ternary(&mut self) -> Expr {
        let cond = self.parse_binary(1);
        if self.at(TokenKind::Question) {
            self.consume();
            let then = self.parse_expr();
            self.expect(TokenKind::Colon, "':'");
            let els = self.parse_expr();
            let id = self.node();
            let span = self.span_join(cond.span, els.span);
            Expr {
                id,
                span,
                kind: ExprKind::Ternary {
                    cond: Box::new(cond),
                    then: Box::new(then),
                    els: Box::new(els),
                },
            }
        } else {
            cond
        }
    }

    fn binary_prec(kind: TokenKind) -> Option<u8> {
        match kind {
            TokenKind::PipePipe => Some(4),
            TokenKind::AmpAmp => Some(5),
            TokenKind::EqEq | TokenKind::BangEq => Some(6),
            TokenKind::Lt | TokenKind::Gt | TokenKind::LtEq | TokenKind::GtEq | TokenKind::KwIs => {
                Some(7)
            }
            TokenKind::Plus | TokenKind::Minus => Some(8),
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Some(9),
            TokenKind::Caret => Some(10),
            _ => None,
        }
    }

    fn parse_binary(&mut self, min_prec: u8) -> Expr {
        let mut lhs = self.parse_unary();
        loop {
            let k = self.peek();
            // Formatted-string context: `>` ends the string unless an
            // expression follows it (upstream StringCheck behavior).
            if k == TokenKind::Gt && self.string_check && !is_expr_start(self.peek2()) {
                break;
            }
            let prec = match Self::binary_prec(k) {
                Some(p) if p >= min_prec => p,
                _ => break,
            };
            if k == TokenKind::KwIs {
                self.consume();
                let pattern = self.parse_pattern();
                let id = self.node();
                let span = self.span_join(lhs.span, self.prev_end());
                lhs = Expr {
                    id,
                    span,
                    kind: ExprKind::Is {
                        operand: Box::new(lhs),
                        pattern,
                    },
                };
                continue;
            }
            self.consume();
            let rhs = self.parse_binary(prec + 1);
            let op = match k {
                TokenKind::PipePipe => BinaryOp::Or,
                TokenKind::AmpAmp => BinaryOp::And,
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::BangEq => BinaryOp::Ne,
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::LtEq => BinaryOp::Le,
                TokenKind::GtEq => BinaryOp::Ge,
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Mod,
                TokenKind::Caret => BinaryOp::Pow,
                _ => unreachable!(),
            };
            let id = self.node();
            let span = self.span_join(lhs.span, rhs.span);
            lhs = Expr {
                id,
                span,
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            };
        }
        // Assignment: lowest precedence, right-associative.
        if min_prec <= 1 && self.is_assignment_op() {
            let op_tok = self.consume();
            let op = match op_tok.kind {
                TokenKind::Eq => AssignOp::Assign,
                TokenKind::PlusEq => AssignOp::Add,
                TokenKind::MinusEq => AssignOp::Sub,
                TokenKind::StarEq => AssignOp::Mul,
                TokenKind::SlashEq => AssignOp::Div,
                TokenKind::PercentEq => AssignOp::Mod,
                TokenKind::CaretEq => AssignOp::Pow,
                _ => unreachable!(),
            };
            let value = self.parse_expr();
            let id = self.node();
            let span = self.span_join(lhs.span, value.span);
            lhs = Expr {
                id,
                span,
                kind: ExprKind::Assign {
                    target: Box::new(lhs),
                    op,
                    value: Box::new(value),
                },
            };
        }
        lhs
    }

    fn is_assignment_op(&mut self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Eq
                | TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::PercentEq
                | TokenKind::CaretEq
        )
    }

    fn parse_pattern(&mut self) -> Pattern {
        let mut enum_path = Vec::new();
        let mut bindings = Vec::new();
        let mut ok = true;
        if self.at(TokenKind::Ident) {
            loop {
                let t = self.consume();
                enum_path.push(Ident {
                    id: self.node(),
                    span: t.span,
                    name: self.text_of(t.span).to_string(),
                });
                if self.at(TokenKind::Dot) {
                    self.consume();
                } else {
                    break;
                }
            }
        } else {
            ok = false;
        }
        if self.at(TokenKind::LParen) {
            self.consume();
            while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                if self.at(TokenKind::Ident) {
                    let t = self.consume();
                    bindings.push(Ident {
                        id: self.node(),
                        span: t.span,
                        name: self.text_of(t.span).to_string(),
                    });
                } else {
                    let t = self.consume();
                    self.err(
                        "PR035",
                        t.span,
                        format!(
                            "expected pattern binding identifier, found {}",
                            t.kind.describe()
                        ),
                    );
                }
                if self.at(TokenKind::Comma) {
                    self.consume();
                }
            }
            self.expect(TokenKind::RParen, "')'");
        }
        if !ok {
            self.err(
                "PR035",
                self.prev_end(),
                "expected enum member path after 'is'",
            );
        }
        Pattern {
            enum_path,
            bindings,
        }
    }

    fn parse_unary(&mut self) -> Expr {
        match self.peek() {
            TokenKind::Bang => {
                let t = self.consume();
                let operand = self.parse_unary();
                let id = self.node();
                let span = self.span_join(t.span, operand.span);
                Expr {
                    id,
                    span,
                    kind: ExprKind::Unary {
                        op: UnaryOp::Not,
                        operand: Box::new(operand),
                    },
                }
            }
            TokenKind::Minus => {
                let t = self.consume();
                let operand = self.parse_unary();
                let id = self.node();
                let span = self.span_join(t.span, operand.span);
                Expr {
                    id,
                    span,
                    kind: ExprKind::Unary {
                        op: UnaryOp::Negate,
                        operand: Box::new(operand),
                    },
                }
            }
            TokenKind::Tilde => {
                let t = self.consume();
                let operand = self.parse_unary();
                let id = self.node();
                let span = self.span_join(t.span, operand.span);
                Expr {
                    id,
                    span,
                    kind: ExprKind::Unary {
                        op: UnaryOp::Indirect,
                        operand: Box::new(operand),
                    },
                }
            }
            TokenKind::Lt if self.is_type_cast() => {
                self.consume(); // <
                let ty = self.parse_type();
                self.expect(TokenKind::Gt, "'>'");
                let operand = self.parse_unary();
                let id = self.node();
                let span = self.span_join(ty.span, operand.span);
                Expr {
                    id,
                    span,
                    kind: ExprKind::Cast {
                        ty,
                        expr: Box::new(operand),
                    },
                }
            }
            _ => self.parse_postfix(),
        }
    }

    /// Upstream `IsTypeCast`: `<` followed by `>` or a valid type then `>`.
    fn is_type_cast(&mut self) -> bool {
        if !self.at(TokenKind::Lt) {
            return false;
        }
        let save = self.pos;
        let save_delims = self.open_delims.len();
        self.consume();
        let result = if self.at(TokenKind::Gt) {
            true
        } else {
            self.try_parse_type().is_some() && self.at(TokenKind::Gt)
        };
        self.pos = save;
        self.open_delims.truncate(save_delims);
        result
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut e = self.parse_atom();
        loop {
            match self.peek() {
                TokenKind::Dot => {
                    self.consume();
                    let name = match self.expect_ident("member name") {
                        Some(n) => n,
                        None => break,
                    };
                    let id = self.node();
                    let span = self.span_join(e.span, name.span);
                    e = Expr {
                        id,
                        span,
                        kind: ExprKind::Member {
                            base: Box::new(e),
                            name,
                        },
                    };
                }
                TokenKind::LBracket => {
                    self.consume();
                    let index = self.parse_expr();
                    self.expect(TokenKind::RBracket, "']'");
                    let id = self.node();
                    let span = self.span_join(e.span, self.prev_end());
                    e = Expr {
                        id,
                        span,
                        kind: ExprKind::Index {
                            base: Box::new(e),
                            index: Box::new(index),
                        },
                    };
                }
                TokenKind::LParen => {
                    let callee = Box::new(e);
                    let (args, _type_args) = self.parse_call_tail();
                    let id = self.node();
                    let span = self.span_join(callee.span, self.prev_end());
                    e = Expr {
                        id,
                        span,
                        kind: ExprKind::Call(CallExpr {
                            callee,
                            type_args: None,
                            args,
                        }),
                    };
                }
                TokenKind::PlusPlus | TokenKind::MinusMinus => {
                    let t = self.consume();
                    let id = self.node();
                    let span = self.span_join(e.span, t.span);
                    e = Expr {
                        id,
                        span,
                        kind: ExprKind::Postfix {
                            operand: Box::new(e),
                            op: if t.kind == TokenKind::PlusPlus {
                                PostfixOp::Increment
                            } else {
                                PostfixOp::Decrement
                            },
                        },
                    };
                }
                _ => break,
            }
        }
        e
    }

    fn parse_atom(&mut self) -> Expr {
        let k = self.peek();
        match k {
            TokenKind::Int | TokenKind::Real => self.parse_number_expr(),
            TokenKind::Str => {
                let t = self.consume();
                self.str_lit_to_expr(t)
            }
            TokenKind::KwTrue | TokenKind::KwFalse => {
                let t = self.consume();
                Expr {
                    id: self.node(),
                    span: t.span,
                    kind: ExprKind::Bool(t.bool_value.unwrap_or(false)),
                }
            }
            TokenKind::KwNull => {
                let t = self.consume();
                Expr {
                    id: self.node(),
                    span: t.span,
                    kind: ExprKind::Null,
                }
            }
            TokenKind::KwThis => {
                let t = self.consume();
                Expr {
                    id: self.node(),
                    span: t.span,
                    kind: ExprKind::This,
                }
            }
            TokenKind::KwRoot => {
                let t = self.consume();
                Expr {
                    id: self.node(),
                    span: t.span,
                    kind: ExprKind::Root,
                }
            }
            TokenKind::KwNew => {
                self.consume();
                let ty = self.parse_type();
                let args = if self.at(TokenKind::LParen) {
                    self.consume();
                    let args = self.parse_call_args();
                    self.expect(TokenKind::RParen, "')'");
                    args
                } else {
                    Vec::new()
                };
                let id = self.node();
                let span = self.span_join(ty.span, self.prev_end());
                Expr {
                    id,
                    span,
                    kind: ExprKind::New { ty, args },
                }
            }
            TokenKind::LBracket => {
                let open = self.consume();
                let mut elems = Vec::new();
                while !self.at(TokenKind::RBracket) && !self.at(TokenKind::Eof) {
                    elems.push(self.parse_expr());
                    if self.at(TokenKind::Comma) {
                        self.consume();
                    } else {
                        break;
                    }
                }
                let end = self
                    .expect(TokenKind::RBracket, "']'")
                    .map_or(open.span.end, |t| t.span.end);
                Expr {
                    id: self.node(),
                    span: Span::new(self.file, open.span.start, end),
                    kind: ExprKind::ArrayLit { elems },
                }
            }
            TokenKind::LBrace => self.parse_struct_lit(),
            TokenKind::KwSingle if self.peek2() == TokenKind::LBrace => {
                self.parse_single_struct_value()
            }
            TokenKind::KwAsync => {
                let t = self.consume();
                let bang = self.at(TokenKind::Bang);
                if bang {
                    self.consume();
                }
                let call = self.parse_expr();
                let id = self.node();
                let span = self.span_join(t.span, call.span);
                Expr {
                    id,
                    span,
                    kind: ExprKind::Async {
                        kind: if bang {
                            AsyncKind::AsyncBang
                        } else {
                            AsyncKind::Async
                        },
                        call: Box::new(call),
                    },
                }
            }
            TokenKind::KwImport => {
                let t = self.consume();
                self.expect(TokenKind::LParen, "'('");
                let path = match self.expect(TokenKind::Str, "import path string") {
                    Some(t) => self.str_expr(t),
                    None => {
                        self.consume();
                        return Expr {
                            id: self.node(),
                            span: self.prev_end(),
                            kind: ExprKind::Error {
                                consumed: self.prev_end(),
                            },
                        };
                    }
                };
                self.expect(TokenKind::RParen, "')'");
                let as_name = if self.at(TokenKind::KwAs) {
                    self.consume();
                    self.expect_ident("import alias")
                } else {
                    None
                };
                let span = self.span_join(t.span, self.prev_end());
                Expr {
                    id: self.node(),
                    span,
                    kind: ExprKind::JsonImport {
                        path: Box::new(path),
                        as_name,
                    },
                }
            }
            TokenKind::Lt if self.is_formatted_string() => self.parse_formatted_string(),
            TokenKind::KwConst if self.is_lambda_lookahead_const() => self.parse_lambda(),
            TokenKind::KwConst => {
                let t = self.consume();
                self.err("PR013", t.span, "expected expression after 'const'");
                Expr {
                    id: self.node(),
                    span: t.span,
                    kind: ExprKind::Error { consumed: t.span },
                }
            }
            TokenKind::Ident => self.parse_ident_atom(),
            TokenKind::LParen => {
                if self.is_lambda_lookahead() {
                    self.parse_lambda()
                } else {
                    self.consume();
                    let e = self.parse_expr();
                    self.expect(TokenKind::RParen, "')'");
                    e
                }
            }
            _ => {
                let t = self.peek_token();
                let span = t.span;
                self.err(
                    "PR013",
                    span,
                    format!("expected expression, found {}", t.kind.describe()),
                );
                self.consume();
                Expr {
                    id: self.node(),
                    span,
                    kind: ExprKind::Error { consumed: span },
                }
            }
        }
    }

    fn parse_ident_atom(&mut self) -> Expr {
        let t = self.consume();
        let name = Ident {
            id: self.node(),
            span: t.span,
            name: self.text_of(t.span).to_string(),
        };
        // Single-param lambda: `x => ...`
        if self.at(TokenKind::Arrow) {
            self.consume();
            let body = self.parse_lambda_body();
            let id = self.node();
            let span = self.span_join(name.span, self.prev_end());
            return Expr {
                id,
                span,
                kind: ExprKind::Lambda(LambdaExpr {
                    params: vec![LambdaParam { name, ty: None }],
                    body,
                    const_: false,
                }),
            };
        }
        // Generic call: `Name<Type,...>(args)`
        if self.at(TokenKind::Lt) {
            let save = self.pos;
            if let Some(targs) = self.try_parse_generic_call_args() {
                let callee = Expr {
                    id: self.node(),
                    span: t.span,
                    kind: ExprKind::Ident(name),
                };
                let (args, _) = self.parse_call_tail();
                let id = self.node();
                let span = self.span_join(callee.span, self.prev_end());
                return Expr {
                    id,
                    span,
                    kind: ExprKind::Call(CallExpr {
                        callee: Box::new(callee),
                        type_args: Some(targs),
                        args,
                    }),
                };
            }
            self.pos = save;
        }
        Expr {
            id: self.node(),
            span: t.span,
            kind: ExprKind::Ident(name),
        }
    }

    fn parse_number_expr(&mut self) -> Expr {
        // Supports an optional leading minus (rule sort order).
        let neg_tok = if self.at(TokenKind::Minus) {
            Some(self.consume())
        } else {
            None
        };
        let t = self.consume();
        let text = self.text_of(t.span).to_string();
        let num = Expr {
            id: self.node(),
            span: t.span,
            kind: ExprKind::Number(LitNumber {
                text: text.clone(),
                is_real: t.kind == TokenKind::Real,
            }),
        };
        if let Some(neg) = neg_tok {
            Expr {
                id: self.node(),
                span: self.span_join(neg.span, num.span),
                kind: ExprKind::Unary {
                    op: UnaryOp::Negate,
                    operand: Box::new(num),
                },
            }
        } else {
            num
        }
    }

    fn str_lit_to_expr(&mut self, t: Token) -> Expr {
        if t.str_form == Some(StrForm::Interpolated) {
            let mut parts = Vec::new();
            let mut args = Vec::new();
            let interior_start = (t.span.start + 2) as usize;
            let interior_end = (t.span.end - 1) as usize;
            let mut cursor = interior_start;
            for hole in &t.holes {
                if hole.open.start as usize > cursor {
                    parts.push(InterpPart::Text(
                        self.text[cursor..hole.open.start as usize].to_string(),
                    ));
                }
                args.push(self.parse_hole_expr(hole));
                cursor = hole.close.end as usize + 1;
            }
            if cursor < interior_end {
                parts.push(InterpPart::Text(
                    self.text[cursor..interior_end].to_string(),
                ));
            }
            Expr {
                id: self.node(),
                span: t.span,
                kind: ExprKind::StrInterp { parts, args },
            }
        } else {
            Expr {
                id: self.node(),
                span: t.span,
                kind: ExprKind::Str(self.str_lit(t)),
            }
        }
    }

    fn parse_hole_expr(&mut self, hole: &InterpHole) -> Expr {
        let mut sub = Parser {
            tokens: &hole.tokens,
            pos: 0,
            file: self.file,
            text: self.text,
            diagnostics: std::mem::take(&mut self.diagnostics),
            next_node: self.next_node,
            open_delims: Vec::new(),
            pending_docs: Vec::new(),
            silent: self.silent,
            string_check: false,
        };
        let e = sub.parse_expr();
        self.diagnostics = sub.diagnostics;
        self.next_node = sub.next_node;
        e
    }

    /// Wrap a string token into an `Expr` (for rule names, import paths, ...).
    fn str_expr(&mut self, t: Token) -> Expr {
        let id = self.node();
        let span = t.span;
        Expr {
            id,
            span,
            kind: ExprKind::Str(self.str_lit(t)),
        }
    }

    fn str_lit(&mut self, t: Token) -> StrLit {
        let quote = match t.str_form {
            Some(StrForm::Localized) => QuoteKind::Localized,
            Some(StrForm::Interpolated) => QuoteKind::Interpolated,
            _ => {
                let raw = self.text_of(t.span);
                if raw.starts_with('\'') {
                    QuoteKind::Single
                } else {
                    QuoteKind::Double
                }
            }
        };
        StrLit {
            quote,
            raw: self.text_of(t.span).to_string(),
        }
    }

    fn string_value(&self, t: &Token) -> String {
        let raw = self.text_of(t.span);
        let len = raw.len();
        if len >= 2 {
            let inner = &raw[1..len - 1];
            inner.replace("\\\"", "\"").replace("\\'", "'")
        } else {
            String::new()
        }
    }

    /// `<"str", args>` or `<@"str", args>`.
    fn is_formatted_string(&mut self) -> bool {
        if !self.at(TokenKind::Lt) {
            return false;
        }
        let save = self.pos;
        let save_delims = self.open_delims.len();
        self.consume();
        let result = if self.at(TokenKind::Str) {
            true
        } else if self.at(TokenKind::At) {
            self.consume();
            self.at(TokenKind::Str)
        } else {
            false
        };
        self.pos = save;
        self.open_delims.truncate(save_delims);
        result
    }

    fn parse_formatted_string(&mut self) -> Expr {
        self.consume(); // <
        let localized = self.at(TokenKind::At);
        if localized {
            self.consume();
        }
        let t = self.consume();
        let base = if localized {
            Expr {
                id: self.node(),
                span: t.span,
                kind: ExprKind::Str(StrLit {
                    quote: QuoteKind::Localized,
                    raw: self.text_of(t.span).to_string(),
                }),
            }
        } else {
            self.str_lit_to_expr(t)
        };
        let saved_string_check = self.string_check;
        self.string_check = true;
        let mut args = Vec::new();
        while self.at(TokenKind::Comma) {
            self.consume();
            args.push(self.parse_expr());
        }
        self.string_check = saved_string_check;
        self.expect(TokenKind::Gt, "'>'");
        let id = self.node();
        let span = self.span_join(base.span, self.prev_end());
        Expr {
            id,
            span,
            kind: ExprKind::Interp {
                base: Box::new(base),
                args,
            },
        }
    }

    fn parse_lambda_body(&mut self) -> LambdaBody {
        if self.at(TokenKind::LBrace) {
            LambdaBody::Block(self.parse_block())
        } else {
            let e = self.parse_expr();
            LambdaBody::Expr(Box::new(e))
        }
    }

    fn is_lambda_lookahead(&mut self) -> bool {
        if !self.at(TokenKind::LParen) {
            return false;
        }
        let save = self.pos;
        let save_delims = self.open_delims.len();
        self.consume();
        let mut depth = 1usize;
        let mut result = false;
        while depth > 0 {
            let k = self.peek();
            match k {
                TokenKind::LParen => {
                    depth += 1;
                    self.consume();
                }
                TokenKind::RParen => {
                    depth -= 1;
                    self.consume();
                }
                TokenKind::Eof => break,
                _ => {
                    self.consume();
                }
            }
        }
        if depth == 0 {
            result = self.at(TokenKind::Arrow);
        }
        self.pos = save;
        self.open_delims.truncate(save_delims);
        result
    }

    fn is_lambda_lookahead_const(&mut self) -> bool {
        self.at(TokenKind::KwConst) && {
            let save = self.pos;
            self.consume();
            let r = self.is_lambda_lookahead();
            self.pos = save;
            r
        }
    }

    fn parse_lambda(&mut self) -> Expr {
        let const_ = self.at(TokenKind::KwConst);
        if const_ {
            self.consume();
        }
        let start = self.prev_end();
        let mut params = Vec::new();
        if self.at(TokenKind::Ident) && self.peek2() == TokenKind::Arrow {
            let t = self.consume();
            params.push(LambdaParam {
                name: Ident {
                    id: self.node(),
                    span: t.span,
                    name: self.text_of(t.span).to_string(),
                },
                ty: None,
            });
            self.consume(); // =>
        } else if self.at(TokenKind::LParen) {
            self.consume();
            while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                let before = self.pos;
                if self.at(TokenKind::Ident) {
                    let k2 = self.peek2();
                    if k2 == TokenKind::Comma || k2 == TokenKind::RParen {
                        let t = self.consume();
                        params.push(LambdaParam {
                            name: Ident {
                                id: self.node(),
                                span: t.span,
                                name: self.text_of(t.span).to_string(),
                            },
                            ty: None,
                        });
                    } else {
                        let ty = self.parse_type();
                        let n = match self.expect_ident("lambda parameter") {
                            Some(n) => n,
                            None => break,
                        };
                        params.push(LambdaParam {
                            name: n,
                            ty: Some(ty),
                        });
                    }
                } else {
                    let ty = self.parse_type();
                    let n = match self.expect_ident("lambda parameter") {
                        Some(n) => n,
                        None => break,
                    };
                    params.push(LambdaParam {
                        name: n,
                        ty: Some(ty),
                    });
                }
                if self.pos == before {
                    self.consume();
                }
                if self.at(TokenKind::Comma) {
                    self.consume();
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RParen, "')'");
            self.expect(TokenKind::Arrow, "'=>'");
        } else {
            let span = self.peek_token().span;
            self.err("PR033", span, "expected lambda parameter list");
            return Expr {
                id: self.node(),
                span: self.prev_end(),
                kind: ExprKind::Error {
                    consumed: self.prev_end(),
                },
            };
        }
        let body = self.parse_lambda_body();
        let end = self.prev_end();
        let id = self.node();
        Expr {
            id,
            span: Span::new(self.file, start.start, end.end),
            kind: ExprKind::Lambda(LambdaExpr {
                params,
                body,
                const_,
            }),
        }
    }

    fn parse_member_expr(&mut self) -> Expr {
        let t = self.consume();
        let base = Ident {
            id: self.node(),
            span: t.span,
            name: self.text_of(t.span).to_string(),
        };
        let base_expr = Expr {
            id: self.node(),
            span: t.span,
            kind: ExprKind::Ident(base),
        };
        self.expect(TokenKind::Dot, "'.'");
        let name = self.expect_ident("member name");
        let name = name.unwrap_or_else(|| Ident {
            id: self.node(),
            span: t.span,
            name: String::new(),
        });
        let span = self.span_join(t.span, name.span);
        Expr {
            id: self.node(),
            span,
            kind: ExprKind::Member {
                base: Box::new(base_expr),
                name,
            },
        }
    }

    // ------------------------------------------------------------------
    // Calls / args
    // ------------------------------------------------------------------

    fn parse_call_tail(&mut self) -> (Vec<Arg>, Option<Vec<TypeRef>>) {
        self.expect(TokenKind::LParen, "'('");
        let args = self.parse_call_args();
        self.expect(TokenKind::RParen, "')'");
        (args, None)
    }

    fn parse_call_args(&mut self) -> Vec<Arg> {
        let mut args = Vec::new();
        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            let named = self.at(TokenKind::Ident) && self.peek2() == TokenKind::Colon;
            let value = if named {
                let t = self.consume();
                self.consume(); // :
                let name = Some(Ident {
                    id: self.node(),
                    span: t.span,
                    name: self.text_of(t.span).to_string(),
                });
                let v = self.parse_expr();
                Arg { name, value: v }
            } else {
                let v = self.parse_expr();
                Arg {
                    name: None,
                    value: v,
                }
            };
            args.push(value);
            if self.at(TokenKind::Comma) {
                self.consume();
            } else {
                break;
            }
        }
        // Interpolation call: a plain string positional arg followed by more
        // positional args is a format string (architecture §9).
        let mut out = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg.name.is_none() && i + 1 < args.len() {
                if let ExprKind::Str(ref s) = arg.value.kind {
                    if s.quote != QuoteKind::Interpolated {
                        let mut interp_args = Vec::new();
                        let mut j = i + 1;
                        while j < args.len() {
                            if args[j].name.is_some() {
                                break;
                            }
                            interp_args.push(args[j].value.clone());
                            j += 1;
                        }
                        let id = self.node();
                        let span = self.span_join(arg.value.span, self.prev_end());
                        out.push(Arg {
                            name: None,
                            value: Expr {
                                id,
                                span,
                                kind: ExprKind::Interp {
                                    base: Box::new(arg.value.clone()),
                                    args: interp_args,
                                },
                            },
                        });
                        i = j;
                        continue;
                    }
                }
            }
            out.push(arg.clone());
            i += 1;
        }
        out
    }

    /// `Name<Type, ...>(` lookahead; returns the type args if this is a
    /// generic call, otherwise None (position restored).
    fn try_parse_generic_call_args(&mut self) -> Option<Vec<TypeRef>> {
        let save = self.pos;
        let save_delims = self.open_delims.len();
        self.consume(); // <
        let mut args = Vec::new();
        let mut ok = true;
        while !self.at(TokenKind::Gt) && !self.at(TokenKind::Eof) {
            match self.try_parse_type() {
                Some(t) => args.push(t),
                None => {
                    ok = false;
                    break;
                }
            }
            if self.at(TokenKind::Comma) {
                self.consume();
            } else {
                break;
            }
        }
        if ok && self.at(TokenKind::Gt) {
            self.consume();
            if self.at(TokenKind::LParen) {
                Some(args)
            } else {
                self.pos = save;
                self.open_delims.truncate(save_delims);
                None
            }
        } else {
            self.pos = save;
            self.open_delims.truncate(save_delims);
            None
        }
    }

    // ------------------------------------------------------------------
    // Struct literals
    // ------------------------------------------------------------------

    fn parse_struct_lit(&mut self) -> Expr {
        let open = self.consume(); // {
        let mut fields = Vec::new();
        let mut base = None;
        let mut single_value = None;
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::DotDot) {
                self.consume();
                base = Some(Box::new(self.parse_expr()));
            } else if self.at(TokenKind::Ident) && self.peek2() == TokenKind::Colon {
                // Name-only field: X: value
                let t = self.consume();
                self.consume(); // :
                let name = Ident {
                    id: self.node(),
                    span: t.span,
                    name: self.text_of(t.span).to_string(),
                };
                let value = self.parse_expr();
                fields.push(StructField {
                    name,
                    ty: None,
                    value,
                });
            } else {
                let save = self.pos;
                let ty = self.try_parse_type();
                if self.at(TokenKind::Ident) && self.peek2() == TokenKind::Colon {
                    let t = self.consume();
                    self.consume(); // :
                    let name = Ident {
                        id: self.node(),
                        span: t.span,
                        name: self.text_of(t.span).to_string(),
                    };
                    let value = self.parse_expr();
                    fields.push(StructField {
                        name,
                        ty: Some(ty.unwrap()),
                        value,
                    });
                } else if single_value.is_none() && fields.is_empty() && base.is_none() {
                    // `{value}` single-valued struct literal (corpus:
                    // `Number value = {0};` in HighLevelTest if-chain).
                    single_value = Some(Box::new(self.parse_expr()));
                } else {
                    self.pos = save;
                    let t = self.peek_token();
                    self.err(
                        "PR037",
                        t.span,
                        format!(
                            "malformed struct literal field, found {}",
                            t.kind.describe()
                        ),
                    );
                    self.consume();
                }
            }
            if self.at(TokenKind::Comma) {
                self.consume();
            } else {
                break;
            }
        }
        let end = self
            .expect(TokenKind::RBrace, "'}'")
            .map_or(open.span.end, |t| t.span.end);
        Expr {
            id: self.node(),
            span: Span::new(self.file, open.span.start, end),
            kind: ExprKind::StructLit(StructLit {
                fields,
                base,
                single_value,
            }),
        }
    }

    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    /// Silent type parse for lookaheads; restores state on failure.
    fn try_parse_type(&mut self) -> Option<TypeRef> {
        let save = self.pos;
        let save_delims = self.open_delims.len();
        let saved_len = self.diagnostics.len();
        self.silent += 1;
        let t = self.parse_type_inner(false);
        self.silent -= 1;
        match t {
            Ok(t) => Some(t),
            Err(_) => {
                self.pos = save;
                self.open_delims.truncate(save_delims);
                self.diagnostics.truncate(saved_len);
                None
            }
        }
    }

    fn parse_type(&mut self) -> TypeRef {
        match self.parse_type_inner(true) {
            Ok(t) => t,
            Err(span) => TypeRef {
                id: self.node(),
                span,
                kind: TypeRefKind::Error,
            },
        }
    }

    fn parse_type_inner(&mut self, report: bool) -> Result<TypeRef, Span> {
        let start = self.peek_token().span.start;
        if self.at(TokenKind::KwVoid) {
            let t = self.consume();
            let ty = TypeRef {
                id: self.node(),
                span: t.span,
                kind: TypeRefKind::Name(Ident {
                    id: self.node(),
                    span: t.span,
                    name: "void".to_string(),
                }),
            };
            return Ok(ty);
        }
        let const_ = if self.at(TokenKind::KwConst) {
            self.consume();
            true
        } else {
            false
        };
        let base = if self.at(TokenKind::LParen) {
            // Lambda type or grouped type.
            self.consume();
            let mut params = Vec::new();
            if !self.at(TokenKind::RParen) {
                loop {
                    let p = self.parse_type_inner(report)?;
                    params.push(p);
                    if self.at(TokenKind::Comma) {
                        self.consume();
                    } else {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RParen, "')'");
            let is_lambda = if params.len() != 1 || const_ {
                if !self.at(TokenKind::Arrow) && report {
                    let span = self.peek_token().span;
                    self.err("PR034", span, "expected '=>' in lambda type");
                }
                true
            } else {
                self.at(TokenKind::Arrow)
            };
            if is_lambda {
                self.expect(TokenKind::Arrow, "'=>'");
                let ret = self.parse_type_inner(report)?;
                let id = self.node();
                let end = self.prev_end();
                return Ok(TypeRef {
                    id,
                    span: Span::new(self.file, start, end.end),
                    kind: TypeRefKind::Function(FunctionTypeRef {
                        const_,
                        params,
                        ret: Box::new(ret),
                    }),
                });
            }
            // Grouped: (T)[]...
            let mut current = params.pop().unwrap();
            while self.at(TokenKind::LBracket) {
                self.consume();
                self.expect(TokenKind::RBracket, "']'");
                let id = self.node();
                let span = self.span_join(current.span, self.prev_end());
                current = TypeRef {
                    id,
                    span,
                    kind: TypeRefKind::Array(Box::new(current)),
                };
            }
            current
        } else {
            // Name / generic / single-param lambda / union.
            let name_tok = match self.peek() {
                TokenKind::Ident | TokenKind::KwDefine => self.consume(),
                _ => {
                    if report {
                        let t = self.peek_token();
                        self.err(
                            "PR012",
                            t.span,
                            format!("expected type name, found {}", t.kind.describe()),
                        );
                    }
                    return Err(self.peek_token().span);
                }
            };
            let name = Ident {
                id: self.node(),
                span: name_tok.span,
                name: self.text_of(name_tok.span).to_string(),
            };
            let mut current = if self.at(TokenKind::Lt) {
                self.consume();
                let mut args = Vec::new();
                while !self.at(TokenKind::Gt) && !self.at(TokenKind::Eof) {
                    let a = self.parse_type_inner(report)?;
                    args.push(a);
                    if self.at(TokenKind::Comma) {
                        self.consume();
                    } else {
                        break;
                    }
                }
                let close = self.expect(TokenKind::Gt, "'>'");
                let end = close.map_or(self.prev_end().end, |t| t.span.end);
                TypeRef {
                    id: self.node(),
                    span: Span::new(self.file, name.span.start, end),
                    kind: TypeRefKind::GenericInstantiation { name, args },
                }
            } else {
                TypeRef {
                    id: self.node(),
                    span: name_tok.span,
                    kind: TypeRefKind::Name(name),
                }
            };
            // Array suffixes.
            while self.at(TokenKind::LBracket) {
                self.consume();
                if self.expect(TokenKind::RBracket, "']'").is_none() {
                    return Err(self.peek_token().span);
                }
                let id = self.node();
                let span = self.span_join(current.span, self.prev_end());
                current = TypeRef {
                    id,
                    span,
                    kind: TypeRefKind::Array(Box::new(current)),
                };
            }
            // Union: `| Type`
            let mut union = Vec::new();
            while self.at(TokenKind::Pipe) {
                self.consume();
                union.push(self.parse_type_inner(report)?);
            }
            if !union.is_empty() {
                let mut members = vec![current];
                members.extend(union);
                let id = self.node();
                let span = Span::new(
                    self.file,
                    members.first().unwrap().span.start,
                    self.prev_end().end,
                );
                current = TypeRef {
                    id,
                    span,
                    kind: TypeRefKind::Union(members),
                };
            }
            // Single-param lambda: `String => void`
            if self.at(TokenKind::Arrow) {
                self.consume();
                let ret = self.parse_type_inner(report)?;
                let id = self.node();
                let end = self.prev_end();
                return Ok(TypeRef {
                    id,
                    span: Span::new(self.file, start, end.end),
                    kind: TypeRefKind::Function(FunctionTypeRef {
                        const_,
                        params: vec![current],
                        ret: Box::new(ret),
                    }),
                });
            }
            current
        };
        let mut t = base;
        t.span = Span::new(self.file, start, t.span.end);
        Ok(t)
    }
}

fn is_expr_start(k: TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Ident
            | TokenKind::Int
            | TokenKind::Real
            | TokenKind::Str
            | TokenKind::KwTrue
            | TokenKind::KwFalse
            | TokenKind::KwNull
            | TokenKind::KwNew
            | TokenKind::KwThis
            | TokenKind::KwRoot
            | TokenKind::KwAsync
            | TokenKind::KwImport
            | TokenKind::KwConst
            | TokenKind::KwSingle
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::LBrace
            | TokenKind::Bang
            | TokenKind::Minus
            | TokenKind::Tilde
            | TokenKind::Lt
    )
}

enum DeclOutcome {
    Function(FunctionDecl),
    Var(VarDecl),
    Error { consumed: Span },
}

fn is_item_start(k: TokenKind) -> bool {
    matches!(
        k,
        TokenKind::KwRule
            | TokenKind::KwClass
            | TokenKind::KwStruct
            | TokenKind::KwEnum
            | TokenKind::KwImport
            | TokenKind::KwType
            | TokenKind::KwGlobalVar
            | TokenKind::KwPlayerVar
            | TokenKind::KwSingle
            | TokenKind::KwDisabled
            | TokenKind::KwDefine
            | TokenKind::KwVoid
            | TokenKind::KwConst
            | TokenKind::KwConstructor
            | TokenKind::KwPublic
            | TokenKind::KwPrivate
            | TokenKind::KwProtected
            | TokenKind::KwStatic
            | TokenKind::KwVirtual
            | TokenKind::KwOverride
            | TokenKind::KwRecursive
            | TokenKind::KwPersist
            | TokenKind::KwRef
            | TokenKind::KwIn
            | TokenKind::Ident
    )
}
