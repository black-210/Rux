use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    HexAddr(u64),
    Float(f64),
    Volt(f64),       // millivolts
    Freq(f64),       // Hz
    Duration(u64),   // nanoseconds
    SampleRate(f64), // samples/sec
    Dbm(f64),        // RF power dBm
    Angle(f64),      // radians
    Str(String),
    FStr(String),
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
    Equation,
    Algorithm,
    Defer,
    Try,
    Catch,
    Throw,
    And,
    Or,
    Not,
    As,
    Const,

    // Punctuation & Delimiters
    Colon,
    ColonColon,
    Arrow,    // ->
    FatArrow, // =>
    PipeGt,   // |>
    Dot,
    DotDot,
    DotDotEq,
    Comma,
    Semi,
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // Operators
    Plus,
    Minus,
    Star,
    StarStar, // **
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
        Self { kind, lexeme: lexeme.into(), span }
    }
}

pub const KEYWORDS: &[&str] = &[
    "fn", "let", "var", "if", "elif", "else", "for", "in", "while", "loop", "match", "case",
    "return", "break", "continue", "struct", "enum", "import", "use", "extern",
    "unsafe_hardware", "equation", "algorithm", "defer", "try", "catch", "throw",
    "true", "false", "none", "and", "or", "not", "as", "const",
];
