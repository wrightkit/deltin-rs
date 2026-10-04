use crate::span::Span;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    Whitespace,
    LineComment,
    BlockComment,
    DocComment,

    Int,
    Real,
    Str,
    Bool,

    Ident,
    KwRule,
    KwDefine,
    KwGlobalVar,
    KwPlayerVar,
    KwIf,
    KwElse,
    KwFor,
    KwForeach,
    KwWhile,
    KwSwitch,
    KwCase,
    KwDefault,
    KwBreak,
    KwContinue,
    KwReturn,
    KwClass,
    KwStruct,
    KwEnum,
    KwConstructor,
    KwNew,
    KwDelete,
    KwIn,
    KwRef,
    KwRecursive,
    KwAsync,
    KwConst,
    KwImport,
    KwAs,
    KwIs,
    KwPublic,
    KwPrivate,
    KwProtected,
    KwStatic,
    KwVirtual,
    KwOverride,
    KwSingle,
    KwThis,
    KwRoot,
    KwTrue,
    KwFalse,
    KwNull,
    KwType,
    KwDisabled,
    KwPersist,
    KwVoid,
    KwJson,

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Dot,
    DotDot,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    PlusPlus,
    MinusMinus,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    CaretEq,
    Eq,
    EqEq,
    Bang,
    BangEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AmpAmp,
    PipePipe,
    Pipe,
    Tilde,
    Question,
    At,

    Error,
    Eof,
}

/// The keyword token kind for a lexeme, or `None` when the lexeme is an
/// ordinary identifier. Shared by the lexer and the OSTW reconstructor's
/// name validation.
pub(crate) fn keyword(text: &str) -> Option<TokenKind> {
    Some(match text {
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
        _ => return None,
    })
}

impl TokenKind {
    pub fn is_trivia(&self) -> bool {
        matches!(
            self,
            TokenKind::Whitespace
                | TokenKind::LineComment
                | TokenKind::BlockComment
                | TokenKind::DocComment
        )
    }

    /// Keyword names for diagnostics ("expected keyword ...").
    pub fn describe(&self) -> &'static str {
        match self {
            TokenKind::Int => "integer",
            TokenKind::Real => "number",
            TokenKind::Str => "string",
            TokenKind::Ident => "identifier",
            TokenKind::LParen => "'('",
            TokenKind::RParen => "')'",
            TokenKind::LBrace => "'{'",
            TokenKind::RBrace => "'}'",
            TokenKind::LBracket => "'['",
            TokenKind::RBracket => "']'",
            TokenKind::Comma => "','",
            TokenKind::Semicolon => "';'",
            TokenKind::Colon => "':'",
            TokenKind::Dot => "'.'",
            TokenKind::DotDot => "'..'",
            TokenKind::Arrow => "'=>'",
            TokenKind::Plus => "'+'",
            TokenKind::Minus => "'-'",
            TokenKind::Star => "'*'",
            TokenKind::Slash => "'/'",
            TokenKind::Percent => "'%'",
            TokenKind::Caret => "'^'",
            TokenKind::Eq => "'='",
            TokenKind::EqEq => "'=='",
            TokenKind::Bang => "'!'",
            TokenKind::BangEq => "'!='",
            TokenKind::Lt => "'<'",
            TokenKind::Gt => "'>'",
            TokenKind::LtEq => "'<='",
            TokenKind::GtEq => "'>='",
            TokenKind::AmpAmp => "'&&'",
            TokenKind::PipePipe => "'||'",
            TokenKind::Pipe => "'|'",
            TokenKind::Tilde => "'~'",
            TokenKind::Question => "'?'",
            TokenKind::Eof => "end of file",
            _ => "token",
        }
    }
}

/// Lexical form of a string token.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrForm {
    /// `"..."` or `'...'`
    Plain,
    /// `@"..."` or `@'...'`
    Localized,
    /// `$"..."` or `$'...'`
    Interpolated,
}

/// Interpolated-string hole (opening `{` / closing `}` markers and the
/// expression token slice inside).
#[derive(Clone, Debug)]
pub struct InterpHole {
    pub open: Span,
    pub close: Span,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Present only for `Str` tokens.
    pub str_form: Option<StrForm>,
    /// Holes for interpolated strings (empty for other kinds).
    pub holes: Vec<InterpHole>,
    /// `true`/`false` value for `Bool` tokens.
    pub bool_value: Option<bool>,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Token {
        Token {
            kind,
            span,
            str_form: None,
            holes: Vec::new(),
            bool_value: None,
        }
    }
}
