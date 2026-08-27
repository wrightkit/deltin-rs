//! Lexer: text -> token stream (trivia retained) + diagnostics.
//!
//! Evidence base: `docs/syntax-notes.md` (pinned upstream
//! ItsDeltin/Overwatch-Script-To-Workshop). Recovery policy: never panic,
//! always return a complete token vector ending in `Eof`; malformed input
//! becomes `Error` tokens with `LX`-coded diagnostics.

use crate::diagnostics::{error, Diagnostic, Phase};
use crate::span::{FileId, Span};
use crate::syntax::token::{InterpHole, StrForm, Token, TokenKind};

pub const MAX_LEX_ERRORS: usize = 200;

struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    chars: Vec<(usize, char)>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    /// Byte offset of the lexed region within `file` (nonzero for
    /// interpolated-string holes, whose tokens must carry true file spans).
    base: usize,
}

/// Lex `text` (belonging to `file`) into a token vector plus diagnostics.
pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_with_base(file, text, 0)
}

fn lex_with_base(file: FileId, text: &str, base: usize) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer {
        file,
        text,
        chars: text.char_indices().collect(),
        pos: 0,
        diagnostics: Vec::new(),
        base,
    };
    let mut tokens = Vec::new();
    loop {
        if lx.pos >= lx.chars.len() {
            let span = Span::new(file, (base + text.len()) as u32, (base + text.len()) as u32);
            tokens.push(Token::new(TokenKind::Eof, span));
            break;
        }
        let start = lx.pos;
        let c = lx.chars[lx.pos].1;
        let (kind, str_form, holes, bool_value) = match c {
            ' ' | '\t' | '\r' | '\n' => {
                lx.advance();
                (TokenKind::Whitespace, None, Vec::new(), None)
            }
            '/' if lx.peek(1) == Some('/') => {
                lx.skip_to_line_end();
                (TokenKind::LineComment, None, Vec::new(), None)
            }
            '/' if lx.peek(1) == Some('*') => lx.block_comment(),
            '#' => {
                lx.skip_to_line_end();
                (TokenKind::DocComment, None, Vec::new(), None)
            }
            '"' | '\'' => lx.string(c, None),
            '@' if matches!(lx.peek(1), Some('"' | '\'')) => {
                lx.string(lx.peek(1).unwrap(), Some(StrForm::Localized))
            }
            '$' if matches!(lx.peek(1), Some('"' | '\'')) => {
                lx.string(lx.peek(1).unwrap(), Some(StrForm::Interpolated))
            }
            '0'..='9' => lx.number(),
            '.' if lx.peek(1).is_some_and(|d| d.is_ascii_digit()) => lx.number(),
            c if c.is_ascii_alphanumeric() || c == '_' => {
                let kind = lx.identifier();
                let bool_value = match kind {
                    TokenKind::KwTrue => Some(true),
                    TokenKind::KwFalse => Some(false),
                    _ => None,
                };
                (kind, None, Vec::new(), bool_value)
            }
            _ => lx.symbol(),
        };
        let span = Span::new(
            file,
            lx.offset_at(start) as u32,
            lx.offset_at(lx.pos) as u32,
        );
        tokens.push(Token {
            kind,
            span,
            str_form,
            holes,
            bool_value,
        });
        if lx.diagnostics.len() >= MAX_LEX_ERRORS {
            let e = Span::new(
                file,
                lx.offset_at(lx.pos) as u32,
                lx.offset_at(lx.pos) as u32,
            );
            lx.diagnostics.push(error(
                Phase::Lex,
                "LX099",
                e,
                "too many lexical errors; stopping",
            ));
            break;
        }
    }
    (tokens, lx.diagnostics)
}

impl Lexer<'_> {
    fn offset_at(&self, i: usize) -> usize {
        let off = if i >= self.chars.len() {
            self.text.len()
        } else {
            self.chars[i].0
        };
        self.base + off
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).map(|(_, c)| *c)
    }

    fn advance(&mut self) {
        if self.pos < self.chars.len() {
            self.pos += 1;
        }
    }

    fn span_from(&self, from: usize) -> Span {
        Span::new(
            self.file,
            self.offset_at(from) as u32,
            self.offset_at(self.pos) as u32,
        )
    }

    fn skip_to_line_end(&mut self) {
        while self.peek(0).is_some_and(|c| c != '\n') {
            self.advance();
        }
    }

    fn block_comment(&mut self) -> (TokenKind, Option<StrForm>, Vec<InterpHole>, Option<bool>) {
        let start = self.pos;
        self.advance();
        self.advance();
        loop {
            match self.peek(0) {
                None => {
                    self.diagnostics.push(error(
                        Phase::Lex,
                        "LX003",
                        self.span_from(start),
                        "unterminated block comment",
                    ));
                    return (TokenKind::BlockComment, None, Vec::new(), None);
                }
                Some('*') if self.peek(1) == Some('/') => {
                    self.advance();
                    self.advance();
                    return (TokenKind::BlockComment, None, Vec::new(), None);
                }
                Some(_) => self.advance(),
            }
        }
    }

    fn number(&mut self) -> (TokenKind, Option<StrForm>, Vec<InterpHole>, Option<bool>) {
        let start = self.pos;
        if self.peek(0) == Some('.') {
            self.advance();
        }
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.advance();
        }
        let mut is_real = false;
        if self.peek(0) == Some('.') {
            // "5." and "5.0" are reals; but avoid eating ".." or a member dot
            // when no digits follow (corpus: "5." is a valid real).
            match self.peek(1) {
                Some('.') => {}
                Some(c) if c.is_ascii_digit() => {
                    is_real = true;
                    self.advance();
                    while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                        self.advance();
                    }
                }
                Some(_) | None => {
                    is_real = true;
                    self.advance();
                }
            }
        }
        // Malformed trailing form: "1.2.3" or "5abc" -> Error over the run.
        if self.peek(0) == Some('.') || self.peek(0).is_some_and(|c| c.is_ascii_alphabetic()) {
            while self
                .peek(0)
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '.')
            {
                self.advance();
            }
            self.diagnostics.push(error(
                Phase::Lex,
                "LX004",
                self.span_from(start),
                "invalid number literal",
            ));
            return (TokenKind::Error, None, Vec::new(), None);
        }
        (
            if is_real {
                TokenKind::Real
            } else {
                TokenKind::Int
            },
            None,
            Vec::new(),
            None,
        )
    }

    fn identifier(&mut self) -> TokenKind {
        while self
            .peek(0)
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.advance();
        }
        let start_off = self
            .chars
            .get(self.pos)
            .map_or(self.text.len(), |(i, _)| *i);
        // Re-derive the token text by scanning back from the current offset.
        let end = start_off;
        let mut begin = end;
        while begin > 0 {
            let prev = self.text[..begin].chars().next_back().unwrap();
            if prev.is_ascii_alphanumeric() || prev == '_' {
                begin -= prev.len_utf8();
            } else {
                break;
            }
        }
        let text = &self.text[begin..end];
        match text {
            "rule" => TokenKind::KwRule,
            "define" => TokenKind::KwDefine,
            "globalvar" => TokenKind::KwGlobalVar,
            "playervar" => TokenKind::KwPlayerVar,
            "if" => TokenKind::KwIf,
            "else" => TokenKind::KwElse,
            "for" => TokenKind::KwFor,
            "foreach" => TokenKind::KwForeach,
            "while" => TokenKind::KwWhile,
            "switch" => TokenKind::KwSwitch,
            "case" => TokenKind::KwCase,
            "default" => TokenKind::KwDefault,
            "break" => TokenKind::KwBreak,
            "continue" => TokenKind::KwContinue,
            "return" => TokenKind::KwReturn,
            "class" => TokenKind::KwClass,
            "struct" => TokenKind::KwStruct,
            "enum" => TokenKind::KwEnum,
            "constructor" => TokenKind::KwConstructor,
            "new" => TokenKind::KwNew,
            "delete" => TokenKind::KwDelete,
            "in" => TokenKind::KwIn,
            "ref" => TokenKind::KwRef,
            "recursive" => TokenKind::KwRecursive,
            "async" => TokenKind::KwAsync,
            "const" => TokenKind::KwConst,
            "import" => TokenKind::KwImport,
            "as" => TokenKind::KwAs,
            "is" => TokenKind::KwIs,
            "public" => TokenKind::KwPublic,
            "private" => TokenKind::KwPrivate,
            "protected" => TokenKind::KwProtected,
            "static" => TokenKind::KwStatic,
            "virtual" => TokenKind::KwVirtual,
            "override" => TokenKind::KwOverride,
            "single" => TokenKind::KwSingle,
            "this" => TokenKind::KwThis,
            "root" => TokenKind::KwRoot,
            "true" => TokenKind::KwTrue,
            "false" => TokenKind::KwFalse,
            "null" => TokenKind::KwNull,
            "type" => TokenKind::KwType,
            "disabled" => TokenKind::KwDisabled,
            "persist" => TokenKind::KwPersist,
            "void" => TokenKind::KwVoid,
            "json" => TokenKind::KwJson,
            _ => TokenKind::Ident,
        }
    }

    /// Lex a string body. `pos` points at the quote char; for prefixed strings
    /// (`@"`, `$"`) the prefix char is one behind `pos`.
    fn string(
        &mut self,
        quote: char,
        form: Option<StrForm>,
    ) -> (TokenKind, Option<StrForm>, Vec<InterpHole>, Option<bool>) {
        let form = form.unwrap_or(StrForm::Plain);
        let start = self.pos;
        if form != StrForm::Plain {
            // Consume the @ / $ prefix.
            self.advance();
        }
        debug_assert_eq!(self.peek(0), Some(quote));
        self.advance();
        let mut in_hole = false;
        let mut holes: Vec<InterpHole> = Vec::new();
        loop {
            match self.peek(0) {
                None | Some('\n') => {
                    self.diagnostics.push(error(
                        Phase::Lex,
                        "LX002",
                        self.span_from(start),
                        "unterminated string",
                    ));
                    break;
                }
                Some(c) if !in_hole && c == quote => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    self.advance();
                    self.advance();
                }
                Some('{') if form == StrForm::Interpolated && !in_hole => {
                    if self.peek(1) == Some('{') {
                        self.advance();
                        self.advance();
                    } else {
                        in_hole = true;
                        let hole_start = self.pos;
                        let mut hole_depth = 1usize;
                        self.advance();
                        let hole_open_off = self.offset_at(hole_start) as u32;
                        while in_hole {
                            match self.peek(0) {
                                None | Some('\n') => {
                                    // Unterminated hole: treat the rest of the
                                    // line as string content (the outer string
                                    // will report LX002 at its end).
                                    in_hole = false;
                                }
                                Some('{') => {
                                    hole_depth += 1;
                                    self.advance();
                                }
                                Some('}') => {
                                    hole_depth -= 1;
                                    if hole_depth == 0 {
                                        let close_off = self.offset_at(self.pos) as u32;
                                        let hole_text = &self.text
                                            [(hole_open_off as usize + 1)..close_off as usize];
                                        let (toks, _) = lex_with_base(
                                            self.file,
                                            hole_text,
                                            hole_open_off as usize + 1,
                                        );
                                        holes.push(InterpHole {
                                            open: Span::new(
                                                self.file,
                                                hole_open_off,
                                                hole_open_off + 1,
                                            ),
                                            close: Span::new(self.file, close_off, close_off + 1),
                                            tokens: toks,
                                        });
                                        self.advance();
                                        in_hole = false;
                                    } else {
                                        self.advance();
                                    }
                                }
                                Some('\\') => {
                                    self.advance();
                                    self.advance();
                                }
                                Some(_) => {
                                    self.advance();
                                }
                            }
                        }
                    }
                }
                Some('}') if form == StrForm::Interpolated => {
                    // "}}" escape outside a hole.
                    self.advance();
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
        (TokenKind::Str, Some(form), holes, None)
    }

    fn symbol(&mut self) -> (TokenKind, Option<StrForm>, Vec<InterpHole>, Option<bool>) {
        let start = self.pos;
        let c = self.peek(0).unwrap();
        let two = (c, self.peek(1));
        let kind = match two {
            ('{', _) => {
                self.advance();
                TokenKind::LBrace
            }
            ('}', _) => {
                self.advance();
                TokenKind::RBrace
            }
            ('(', _) => {
                self.advance();
                TokenKind::LParen
            }
            (')', _) => {
                self.advance();
                TokenKind::RParen
            }
            ('[', _) => {
                self.advance();
                TokenKind::LBracket
            }
            (']', _) => {
                self.advance();
                TokenKind::RBracket
            }
            (',', _) => {
                self.advance();
                TokenKind::Comma
            }
            (';', _) => {
                self.advance();
                TokenKind::Semicolon
            }
            (':', _) => {
                self.advance();
                TokenKind::Colon
            }
            ('.', Some('.')) => {
                self.advance();
                self.advance();
                TokenKind::DotDot
            }
            ('.', _) => {
                self.advance();
                TokenKind::Dot
            }
            ('~', _) => {
                self.advance();
                TokenKind::Tilde
            }
            ('?', _) => {
                self.advance();
                TokenKind::Question
            }
            ('@', _) => {
                self.advance();
                TokenKind::At
            }
            ('+', Some('+')) => {
                self.advance();
                self.advance();
                TokenKind::PlusPlus
            }
            ('-', Some('-')) => {
                self.advance();
                self.advance();
                TokenKind::MinusMinus
            }
            ('+', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::PlusEq
            }
            ('-', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::MinusEq
            }
            ('*', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::StarEq
            }
            ('/', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::SlashEq
            }
            ('%', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::PercentEq
            }
            ('^', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::CaretEq
            }
            ('^', _) => {
                self.advance();
                TokenKind::Caret
            }
            ('=', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::EqEq
            }
            ('=', Some('>')) => {
                self.advance();
                self.advance();
                TokenKind::Arrow
            }
            ('=', _) => {
                self.advance();
                TokenKind::Eq
            }
            ('!', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::BangEq
            }
            ('!', _) => {
                self.advance();
                TokenKind::Bang
            }
            ('<', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::LtEq
            }
            ('>', Some('=')) => {
                self.advance();
                self.advance();
                TokenKind::GtEq
            }
            ('<', _) => {
                self.advance();
                TokenKind::Lt
            }
            ('>', _) => {
                self.advance();
                TokenKind::Gt
            }
            ('&', Some('&')) => {
                self.advance();
                self.advance();
                TokenKind::AmpAmp
            }
            ('|', Some('|')) => {
                self.advance();
                self.advance();
                TokenKind::PipePipe
            }
            ('|', _) => {
                self.advance();
                TokenKind::Pipe
            }
            ('+', _) => {
                self.advance();
                TokenKind::Plus
            }
            ('-', _) => {
                self.advance();
                TokenKind::Minus
            }
            ('*', _) => {
                self.advance();
                TokenKind::Star
            }
            ('/', _) => {
                self.advance();
                TokenKind::Slash
            }
            ('%', _) => {
                self.advance();
                TokenKind::Percent
            }
            _ => {
                self.advance();
                self.diagnostics.push(error(
                    Phase::Lex,
                    "LX001",
                    self.span_from(start),
                    format!("invalid character '{c}'"),
                ));
                TokenKind::Error
            }
        };
        (kind, None, Vec::new(), None)
    }
}
