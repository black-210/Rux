use crate::diagnostic::{AutoFix, Diagnostic, Severity, TextEdit};
use crate::span::Span;
use crate::token::{Token, TokenKind};

pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &str) -> LexOutput {
    let mut lexer = Lexer::new(source);
    lexer.run();
    LexOutput {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
    at_line_start: bool,
    indent_stack: Vec<usize>,
    delimiter_depth: usize, // inside (), [], {} we skip Indent/Dedent/Newline
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            at_line_start: true,
            indent_stack: vec![0],
            delimiter_depth: 0,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn advance_ascii(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn run(&mut self) {
        while self.pos < self.bytes.len() {
            if self.at_line_start && self.delimiter_depth == 0 {
                if !self.handle_indentation() {
                    continue;
                }
                self.at_line_start = false;
            }

            let Some(b) = self.peek() else { break };

            if b == b' ' || b == b'\t' || b == b'\r' {
                self.advance_ascii();
                continue;
            }

            if b == b'\n' {
                let sp = Span::point(self.pos, self.line, self.col);
                self.advance_ascii();
                if self.delimiter_depth == 0 {
                    // collapse duplicate newlines
                    if !matches!(
                        self.tokens.last().map(|t| &t.kind),
                        Some(TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent) | None
                    ) {
                        self.tokens.push(Token::new(TokenKind::Newline, "\n", sp));
                    }
                    self.at_line_start = true;
                }
                continue;
            }

            // Comments: '#' or '//'
            if b == b'#' || (b == b'/' && self.peek_next() == Some(b'/')) {
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        break;
                    }
                    self.advance_char();
                }
                continue;
            }

            // Block comment /* ... */
            if b == b'/' && self.peek_next() == Some(b'*') {
                self.advance_ascii();
                self.advance_ascii();
                let mut depth = 1usize;
                while self.pos < self.bytes.len() && depth > 0 {
                    if self.peek() == Some(b'/') && self.peek_next() == Some(b'*') {
                        depth += 1;
                        self.advance_ascii();
                        self.advance_ascii();
                    } else if self.peek() == Some(b'*') && self.peek_next() == Some(b'/') {
                        depth -= 1;
                        self.advance_ascii();
                        self.advance_ascii();
                    } else {
                        self.advance_char();
                    }
                }
                continue;
            }

            // Strings
            if b == b'"' || b == b'\'' {
                self.lex_string(b);
                continue;
            }

            // Numbers / Physical Units / Hex / Bin
            if b.is_ascii_digit() {
                self.lex_number();
                continue;
            }

            // Identifiers & Keywords
            if b.is_ascii_alphabetic() || b == b'_' {
                self.lex_ident();
                continue;
            }

            // Operators & Punctuation
            self.lex_punct();
        }

        // Flush trailing newline & dedents
        if !matches!(
            self.tokens.last().map(|t| &t.kind),
            Some(TokenKind::Newline | TokenKind::Dedent) | None
        ) {
            let sp = Span::point(self.pos, self.line, self.col);
            self.tokens.push(Token::new(TokenKind::Newline, "\n", sp));
        }

        let eof_sp = Span::point(self.pos, self.line, self.col);
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.tokens.push(Token::new(TokenKind::Dedent, "", eof_sp));
        }
        self.tokens.push(Token::new(TokenKind::Eof, "", eof_sp));
    }

    fn advance_char(&mut self) {
        if let Some(ch) = self.src[self.pos..].chars().next() {
            self.pos += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
    }

    /// Returns true if line has real code, false if blank/comment-only line was consumed.
    fn handle_indentation(&mut self) -> bool {
        let line_start_pos = self.pos;
        let line_num = self.line;
        let mut indent = 0usize;
        let mut saw_tab = false;
        let mut saw_space = false;

        while let Some(b) = self.peek() {
            if b == b' ' {
                saw_space = true;
                indent += 1;
                self.advance_ascii();
            } else if b == b'\t' {
                saw_tab = true;
                indent += 4;
                self.advance_ascii();
            } else if b == b'\r' {
                self.advance_ascii();
            } else {
                break;
            }
        }

        // Blank line or comment line -> ignore indentation change
        match self.peek() {
            None => return false,
            Some(b'\n') => {
                self.advance_ascii();
                return false;
            }
            Some(b'#') => {
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        self.advance_ascii();
                        break;
                    }
                    self.advance_char();
                }
                return false;
            }
            Some(b'/') if self.peek_next() == Some(b'/') => {
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        self.advance_ascii();
                        break;
                    }
                    self.advance_char();
                }
                return false;
            }
            _ => {}
        }

        if saw_tab && saw_space {
            let sp = Span::new(line_start_pos, self.pos, line_num, 1, line_num, self.col);
            let fixed_spaces = " ".repeat(indent);
            self.diagnostics.push(
                Diagnostic::error(
                    "NB001",
                    sp,
                    "Mixed tabs and spaces in indentation",
                )
                .with_severity(Severity::Warning)
                .with_ar_message("تم خلط مسافات Tab مع مسافات عادية في الإزاحة")
                .with_why(
                    "Mixing tabs and spaces makes block nesting ambiguous across editors.",
                    "خلط Tab و Space يسبب التباساً في تحديد بداية ونهاية الكتل البرمجية.",
                )
                .with_solution(
                    "Convert leading tabs to 4 spaces.",
                    "استبدل الـ Tab بـ 4 مسافات.",
                )
                .with_fix(AutoFix {
                    title: "Normalize indentation to spaces".to_string(),
                    title_ar: "توحيد الإزاحة إلى مسافات".to_string(),
                    edits: vec![TextEdit {
                        span: sp,
                        replacement: fixed_spaces,
                    }],
                    is_preferred: true,
                }),
            );
        }

        let current = *self.indent_stack.last().unwrap_or(&0);
        let sp = Span::new(line_start_pos, self.pos, line_num, 1, line_num, self.col);

        if indent > current {
            self.indent_stack.push(indent);
            self.tokens.push(Token::new(TokenKind::Indent, "", sp));
        } else if indent < current {
            while let Some(&top) = self.indent_stack.last() {
                if top > indent {
                    self.indent_stack.pop();
                    self.tokens.push(Token::new(TokenKind::Dedent, "", sp));
                } else {
                    break;
                }
            }
            if *self.indent_stack.last().unwrap_or(&0) != indent {
                let expected = *self.indent_stack.last().unwrap_or(&0);
                self.diagnostics.push(
                    Diagnostic::error("NB002", sp, format!("Inconsistent indentation: got {} spaces, expected {}", indent, expected))
                        .with_ar_message(format!("إزاحة غير متناسقة: وجدنا {} مسافة والمتوقع {} مسافة", indent, expected))
                        .with_why(
                            "Every block inside `fn`, `if`, `for`, `struct` must align cleanly with an outer indentation level.",
                            "كل سطر داخل دالة أو شرط يجب أن يطابق مستوى الإزاحة الخارجي.",
                        )
                        .with_solution(
                            format!("Align this line to {} spaces.", expected),
                            format!("اضبط إزاحة هذا السطر لتكون {} مسافة.", expected),
                        )
                        .with_fix(AutoFix {
                            title: format!("Align indentation to {} spaces", expected),
                            title_ar: format!("ضبط الإزاحة إلى {} مسافة", expected),
                            edits: vec![TextEdit {
                                span: sp,
                                replacement: " ".repeat(expected),
                            }],
                            is_preferred: true,
                        }),
                );
            }
        }

        true
    }

    fn lex_string(&mut self, quote: u8) {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.col;
        self.advance_ascii(); // opening quote

        let mut val = String::new();
        let mut closed = false;

        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == quote {
                self.advance_ascii();
                closed = true;
                break;
            }
            if b == b'\n' {
                break;
            }
            if b == b'\\' {
                self.advance_ascii();
                if let Some(esc) = self.peek() {
                    self.advance_ascii();
                    match esc {
                        b'n' => val.push('\n'),
                        b't' => val.push('\t'),
                        b'r' => val.push('\r'),
                        b'0' => val.push('\0'),
                        b'\\' => val.push('\\'),
                        b'"' => val.push('"'),
                        b'\'' => val.push('\''),
                        other => {
                            val.push('\\');
                            val.push(other as char);
                        }
                    }
                }
            } else if let Some(ch) = self.src[self.pos..].chars().next() {
                val.push(ch);
                self.advance_char();
            }
        }

        let sp = Span::new(start_pos, self.pos, start_line, start_col, self.line, self.col);
        if !closed {
            let q = quote as char;
            self.diagnostics.push(
                Diagnostic::error("NB003", sp, "Unterminated string literal")
                    .with_ar_message("نص غير مغلق بعلامة تنصيص")
                    .with_why(
                        "A string was opened with a quote but reached the end of the line without a closing quote.",
                        "تم فتح النص بعلامة تنصيص ولم يتم إغلاقه قبل نهاية السطر.",
                    )
                    .with_solution(
                        format!("Add a closing `{}` at the end of the string.", q),
                        format!("أضف علامة الإغلاق `{}` في نهاية النص.", q),
                    )
                    .with_fix(AutoFix {
                        title: "Close string literal".to_string(),
                        title_ar: "إغلاق النص".to_string(),
                        edits: vec![TextEdit {
                            span: Span::new(self.pos, self.pos, self.line, self.col, self.line, self.col),
                            replacement: q.to_string(),
                        }],
                        is_preferred: true,
                    }),
            );
        }

        let lexeme = self.src[start_pos..self.pos].to_string();
        self.tokens.push(Token::new(TokenKind::Str(val), lexeme, sp));
    }

    fn lex_number(&mut self) {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.col;

        // Check hex 0x or binary 0b
        if self.peek() == Some(b'0') && matches!(self.peek_next(), Some(b'x' | b'X' | b'b' | b'B')) {
            let prefix = self.peek_next().unwrap();
            self.advance_ascii();
            self.advance_ascii();
            let digit_start = self.pos;
            let mut clean = String::new();
            while let Some(b) = self.peek() {
                if b == b'_' {
                    self.advance_ascii();
                    continue;
                }
                let valid = if prefix == b'x' || prefix == b'X' {
                    b.is_ascii_hexdigit()
                } else {
                    b == b'0' || b == b'1'
                };
                if valid {
                    clean.push(b as char);
                    self.advance_ascii();
                } else {
                    break;
                }
            }
            let sp = Span::new(start_pos, self.pos, start_line, start_col, self.line, self.col);
            let lexeme = self.src[start_pos..self.pos].to_string();
            let radix = if prefix == b'x' || prefix == b'X' { 16 } else { 2 };
            if self.pos == digit_start || clean.is_empty() {
                self.diagnostics.push(
                    Diagnostic::error("NB004", sp, format!("Invalid numeric literal `{}`", lexeme))
                        .with_ar_message(format!("قيمة رقمية غير صالحة `{}`", lexeme))
                        .with_why("Hex (`0x`) and binary (`0b`) literals require at least one digit.", "الأرقام الست عشرية والثنائية تحتاج رقماً واحداً على الأقل بعد البادئة.")
                        .with_solution("Add valid digits (e.g. `0x00` or `0b01`).", "أضف أرقاماً صالحة مثل `0x00` أو `0b01`."),
                );
                self.tokens.push(Token::new(TokenKind::HexAddr(0), lexeme, sp));
            } else {
                let val = u64::from_str_radix(&clean, radix).unwrap_or(0);
                self.tokens.push(Token::new(TokenKind::HexAddr(val), lexeme, sp));
            }
            return;
        }

        let mut num_str = String::new();
        let mut is_float = false;

        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                num_str.push(b as char);
                self.advance_ascii();
            } else if b == b'_' {
                self.advance_ascii();
            } else if b == b'.' && !is_float {
                // Ensure not '..' range operator
                if self.peek_next() == Some(b'.') {
                    break;
                }
                if self.peek_next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    is_float = true;
                    num_str.push('.');
                    self.advance_ascii();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Check physical unit suffix: V, mV, GHz, MHz, kHz, Hz, ns, us, ms, s
        let unit_start = self.pos;
        let mut unit = String::new();
        while let Some(b) = self.peek() {
            if b.is_ascii_alphabetic() {
                unit.push(b as char);
                self.advance_ascii();
            } else {
                break;
            }
        }

        let sp = Span::new(start_pos, self.pos, start_line, start_col, self.line, self.col);
        let lexeme = self.src[start_pos..self.pos].to_string();

        if !unit.is_empty() {
            let fval: f64 = num_str.parse().unwrap_or(0.0);
            match unit.as_str() {
                "V" => {
                    self.tokens.push(Token::new(TokenKind::Volt(fval * 1000.0), lexeme, sp));
                    return;
                }
                "mV" => {
                    self.tokens.push(Token::new(TokenKind::Volt(fval), lexeme, sp));
                    return;
                }
                "GHz" => {
                    self.tokens.push(Token::new(TokenKind::Freq(fval * 1_000_000_000.0), lexeme, sp));
                    return;
                }
                "MHz" => {
                    self.tokens.push(Token::new(TokenKind::Freq(fval * 1_000_000.0), lexeme, sp));
                    return;
                }
                "kHz" => {
                    self.tokens.push(Token::new(TokenKind::Freq(fval * 1_000.0), lexeme, sp));
                    return;
                }
                "Hz" => {
                    self.tokens.push(Token::new(TokenKind::Freq(fval), lexeme, sp));
                    return;
                }
                "ns" => {
                    self.tokens.push(Token::new(TokenKind::Duration(fval as u64), lexeme, sp));
                    return;
                }
                "us" => {
                    self.tokens.push(Token::new(TokenKind::Duration((fval * 1_000.0) as u64), lexeme, sp));
                    return;
                }
                "ms" => {
                    self.tokens.push(Token::new(TokenKind::Duration((fval * 1_000_000.0) as u64), lexeme, sp));
                    return;
                }
                "s" => {
                    self.tokens.push(Token::new(TokenKind::Duration((fval * 1_000_000_000.0) as u64), lexeme, sp));
                    return;
                }
                _ => {
                    // Unknown unit suffix -> emit helpful error + auto-fix if case typo like mv/ghz
                    let fixed_unit = match unit.to_ascii_lowercase().as_str() {
                        "v" => Some("V"),
                        "mv" => Some("mV"),
                        "ghz" => Some("GHz"),
                        "mhz" => Some("MHz"),
                        "khz" => Some("kHz"),
                        "hz" => Some("Hz"),
                        _ => None,
                    };
                    let unit_sp = Span::new(unit_start, self.pos, start_line, start_col + num_str.len(), self.line, self.col);
                    let mut diag = Diagnostic::error(
                        "NB005",
                        sp,
                        format!("Unknown physical unit `{}` on number `{}`", unit, num_str),
                    )
                    .with_ar_message(format!("وحدة قياس غير معروفة `{}` للرقم `{}`", unit, num_str))
                    .with_why(
                        "Nabda supports hardware unit suffixes: V, mV, GHz, MHz, kHz, Hz, ns, us, ms, s.",
                        "نبضة تدعم وحدات العتاد التالية: V, mV, GHz, MHz, kHz, Hz, ns, us, ms, s.",
                    )
                    .with_solution(
                        "Use a valid unit suffix (case-sensitive, e.g. `1.25V`, `4.2GHz`, `10ms`).",
                        "استخدم وحدة صحيحة مثل `1.25V` أو `4.2GHz` أو `10ms`.",
                    );
                    if let Some(fu) = fixed_unit {
                        diag = diag.with_fix(AutoFix {
                            title: format!("Change unit `{}` to `{}`", unit, fu),
                            title_ar: format!("تعديل الوحدة `{}` إلى `{}`", unit, fu),
                            edits: vec![TextEdit {
                                span: unit_sp,
                                replacement: fu.to_string(),
                            }],
                            is_preferred: true,
                        });
                    }
                    self.diagnostics.push(diag);
                }
            }
        }

        if is_float {
            let val: f64 = num_str.parse().unwrap_or(0.0);
            self.tokens.push(Token::new(TokenKind::Float(val), lexeme, sp));
        } else {
            let val: i64 = num_str.parse().unwrap_or(0);
            self.tokens.push(Token::new(TokenKind::Int(val), lexeme, sp));
        }
    }

    fn lex_ident(&mut self) {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.col;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance_ascii();
            } else {
                break;
            }
        }
        let sp = Span::new(start_pos, self.pos, start_line, start_col, self.line, self.col);
        let text = self.src[start_pos..self.pos].to_string();
        let kind = match text.as_str() {
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "if" => TokenKind::If,
            "elif" => TokenKind::Elif,
            "else" => TokenKind::Else,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "while" => TokenKind::While,
            "loop" => TokenKind::Loop,
            "match" => TokenKind::Match,
            "case" => TokenKind::Case,
            "return" => TokenKind::Return,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            "import" => TokenKind::Import,
            "use" => TokenKind::Use,
            "extern" => TokenKind::Extern,
            "unsafe_hardware" => TokenKind::UnsafeHardware,
            "true" => TokenKind::Bool(true),
            "false" => TokenKind::Bool(false),
            "none" | "None" => TokenKind::NoneLit,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "as" => TokenKind::As,
            "const" => TokenKind::Const,
            _ => TokenKind::Ident(text.clone()),
        };
        self.tokens.push(Token::new(kind, text, sp));
    }

    fn lex_punct(&mut self) {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.col;
        let b = self.advance_ascii().unwrap();
        let next = self.peek();

        let (kind, len) = match (b, next) {
            (b':', Some(b':')) => {
                self.advance_ascii();
                (TokenKind::ColonColon, 2)
            }
            (b':', _) => (TokenKind::Colon, 1),
            (b'-', Some(b'>')) => {
                self.advance_ascii();
                (TokenKind::Arrow, 2)
            }
            (b'-', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::MinusEq, 2)
            }
            (b'-', _) => (TokenKind::Minus, 1),
            (b'=', Some(b'>')) => {
                self.advance_ascii();
                (TokenKind::FatArrow, 2)
            }
            (b'=', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::EqEq, 2)
            }
            (b'=', _) => (TokenKind::Eq, 1),
            (b'!', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::NotEq, 2)
            }
            (b'!', _) => (TokenKind::Not, 1),
            (b'<', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::LtEq, 2)
            }
            (b'<', Some(b'<')) => {
                self.advance_ascii();
                (TokenKind::Shl, 2)
            }
            (b'<', _) => (TokenKind::Lt, 1),
            (b'>', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::GtEq, 2)
            }
            (b'>', Some(b'>')) => {
                self.advance_ascii();
                (TokenKind::Shr, 2)
            }
            (b'>', _) => (TokenKind::Gt, 1),
            (b'+', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::PlusEq, 2)
            }
            (b'+', _) => (TokenKind::Plus, 1),
            (b'*', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::StarEq, 2)
            }
            (b'*', _) => (TokenKind::Star, 1),
            (b'/', Some(b'=')) => {
                self.advance_ascii();
                (TokenKind::SlashEq, 2)
            }
            (b'/', _) => (TokenKind::Slash, 1),
            (b'%', _) => (TokenKind::Percent, 1),
            (b'.', Some(b'.')) => {
                self.advance_ascii();
                if self.peek() == Some(b'=') {
                    self.advance_ascii();
                    (TokenKind::DotDotEq, 3)
                } else {
                    (TokenKind::DotDot, 2)
                }
            }
            (b'.', _) => (TokenKind::Dot, 1),
            (b',', _) => (TokenKind::Comma, 1),
            (b';', _) => (TokenKind::Semi, 1),
            (b'@', _) => (TokenKind::At, 1),
            (b'&', Some(b'&')) => {
                self.advance_ascii();
                (TokenKind::And, 2)
            }
            (b'&', _) => (TokenKind::Amp, 1),
            (b'|', Some(b'|')) => {
                self.advance_ascii();
                (TokenKind::Or, 2)
            }
            (b'|', _) => (TokenKind::Pipe, 1),
            (b'^', _) => (TokenKind::Caret, 1),
            (b'~', _) => (TokenKind::Tilde, 1),
            (b'(', _) => {
                self.delimiter_depth += 1;
                (TokenKind::LParen, 1)
            }
            (b')', _) => {
                self.delimiter_depth = self.delimiter_depth.saturating_sub(1);
                (TokenKind::RParen, 1)
            }
            (b'[', _) => {
                self.delimiter_depth += 1;
                (TokenKind::LBracket, 1)
            }
            (b']', _) => {
                self.delimiter_depth = self.delimiter_depth.saturating_sub(1);
                (TokenKind::RBracket, 1)
            }
            (b'{', _) => {
                self.delimiter_depth += 1;
                (TokenKind::LBrace, 1)
            }
            (b'}', _) => {
                self.delimiter_depth = self.delimiter_depth.saturating_sub(1);
                (TokenKind::RBrace, 1)
            }
            _ => {
                let sp = Span::new(start_pos, self.pos, start_line, start_col, self.line, self.col);
                let ch = b as char;
                self.diagnostics.push(
                    Diagnostic::error("NB006", sp, format!("Unexpected character `{}`", ch))
                        .with_ar_message(format!("رمز غير متوقع `{}`", ch))
                        .with_why(
                            "This character is not valid Nabda syntax outside of strings or comments.",
                            "هذا الرمز غير مستخدم في قواعد لغة نبضة خارج النصوص والتعليقات.",
                        )
                        .with_solution(
                            "Remove this character or wrap it in quotes if it is text.",
                            "احذف هذا الرمز أو ضعه داخل علامتي تنصيص إذا كان نصاً.",
                        )
                        .with_fix(AutoFix {
                            title: format!("Remove unexpected `{}`", ch),
                            title_ar: format!("حذف الرمز غير المتوقع `{}`", ch),
                            edits: vec![TextEdit {
                                span: sp,
                                replacement: String::new(),
                            }],
                            is_preferred: true,
                        }),
                );
                return;
            }
        };

        let sp = Span::new(start_pos, self.pos, start_line, start_col, self.line, self.col);
        let lexeme = self.src[start_pos..start_pos + len].to_string();
        self.tokens.push(Token::new(kind, lexeme, sp));
    }
}
