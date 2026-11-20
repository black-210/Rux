use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    HexAddr(u64),
    Float(f64),
    Volt(f64),     // stored in millivolts (mV)
    Freq(f64),     // stored in Hz
    Duration(u64), // stored in nanoseconds (ns)
    Str(String),
    Bool(bool),
    NoneLit,

    // Identifiers
    Ident(String),

    // Keywords
    Fn,
    Let,
    Var,
    If,
    Elif,
    Else,
    For,
    In,
    While,
    Loop,
    Match,
    Case,
    Return,
    Break,
    Continue,
    Struct,
    Enum,
    Import,
    Use,
    Extern,
    UnsafeHardware,
    And,
    Or,
    Not,
    As,
    Const,

    // Punctuation & Delimiters
    Colon,      // :
    ColonColon, // ::
    Arrow,      // ->
    FatArrow,   // =>
    Dot,        // .
    DotDot,     // ..
    DotDotEq,   // ..=
    Comma,      // ,
    Semi,       // ;
    At,         // @
    LParen,     // (
    RParen,     // )
    LBracket,   // [
    RBracket,   // ]
    LBrace,     // {
    RBrace,     // }

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,

    // Structure
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, lexeme: impl Into<String>, span: Span) -> Self {
        Self {
            kind,
            lexeme: lexeme.into(),
            span,
        }
    }
}

pub const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "var",
    "if",
    "elif",
    "else",
    "for",
    "in",
    "while",
    "loop",
    "match",
    "case",
    "return",
    "break",
    "continue",
    "struct",
    "enum",
    "import",
    "use",
    "extern",
    "unsafe_hardware",
    "true",
    "false",
    "none",
    "and",
    "or",
    "not",
    "as",
    "const",
];
