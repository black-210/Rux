use crate::math_diag::{AutoFix, Diagnostic, Severity, TextEdit};
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
    delimiter_depth: usize,
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

            if b == b'#' || (b == b'/' && self.peek_next() == Some(b'/')) {
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        break;
                    }
                    self.advance_char();
                }
                continue;
            }

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

            // f-string: f"..."
            if b == b'f' && (self.peek_next() == Some(b'"') || self.peek_next() == Some(b'\'')) {
                let start_pos = self.pos;
                let start_line = self.line;
                let start_col = self.col;
                self.advance_ascii(); // 'f'
                let quote = self.peek().unwrap();
                let (val, sp) = self.lex_string_raw(quote, start_pos, start_line, start_col);
                let lexeme = self.src[start_pos..self.pos].to_string();
                self.tokens.push(Token::new(TokenKind::FStr(val), lexeme, sp));
                continue;
            }

            if b == b'"' || b == b'\'' {
                let start_pos = self.pos;
                let start_line = self.line;
                let start_col = self.col;
                let (val, sp) = self.lex_string_raw(b, start_pos, start_line, start_col);
                let lexeme = self.src[start_pos..self.pos].to_string();
                self.tokens.push(Token::new(TokenKind::Str(val), lexeme, sp));
                continue;
            }

            if b.is_ascii_digit() {
                self.lex_number();
                continue;
            }

            if b.is_ascii_alphabetic() || b == b'_' {
                self.lex_ident();
                continue;
            }

            self.lex_punct();
        }

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
            self.diagnostics.push(
                Diagnostic::error("RS001", sp, "Mixed tabs and spaces in indentation")
                    .with_severity(Severity::Warning)
                    .with_ar_message("تم خلط Tab مع مسافات في الإزاحة")
                    .with_why("Mixing tabs and spaces causes indentation ambiguity.", "خلط Tab و Space يسبب التباساً في بنية الكتل.")
                    .with_solution("Normalize leading whitespace to spaces.", "وحّد الإزاحة إلى مسافات.")
                    .with_fix(AutoFix {
                        title: "Normalize indentation to spaces".into(),
                        title_ar: "توحيد الإزاحة إلى مسافات".into(),
                        edits: vec![TextEdit { span: sp, replacement: " ".repeat(indent) }],
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
                    Diagnostic::error("RS002", sp, format!("Inconsistent indentation: got {} spaces, expected {}", indent, expected))
                        .with_ar_message(format!("إزاحة غير متناسقة: {} مسافة والمتوقع {}", indent, expected))
                        .with_why("Blocks must align cleanly with an outer indentation column.", "يجب محاذاة السطور مع مستوى الإزاحة الخارجي.")
                        .with_solution(format!("Align to {} spaces.", expected), format!("اضبط الإزاحة إلى {} مسافة.", expected))
                        .with_fix(AutoFix {
                            title: format!("Align indentation to {} spaces", expected),
                            title_ar: format!("ضبط الإزاحة إلى {} مسافة", expected),
                            edits: vec![TextEdit { span: sp, replacement: " ".repeat(expected) }],
                            is_preferred: true,
                        }),
                );
            }
        }
        true
    }

    fn lex_string_raw(&mut self, quote: u8, start_pos: usize, start_line: usize, start_col: usize) -> (String, Span) {
        self.advance_ascii();
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
                Diagnostic::error("RS003", sp, "Unterminated string literal")
                    .with_ar_message("نص غير مغلق بعلامة تنصيص")
                    .with_why("String reached end of line without closing quote.", "انتهى السطر قبل إغلاق النص.")
                    .with_solution(format!("Add closing `{}`.", q), format!("أضف `{}` في نهاية النص.", q))
                    .with_fix(AutoFix {
                        title: "Close string literal".into(),
                        title_ar: "إغلاق النص".into(),
                        edits: vec![TextEdit {
                            span: Span::new(self.pos, self.pos, self.line, self.col, self.line, self.col),
                            replacement: q.to_string(),
                        }],
                        is_preferred: true,
                    }),
            );
        }
        (val, sp)
    }

    fn lex_number(&mut self) {
        let start_pos = self.pos;
        let start_line = self.line;
        let start_col = self.col;

        if self.peek() == Some(b'0') && matches!(self.peek_next(), Some(b'x' | b'X' | b'b' | b'B')) {
            let prefix = self.peek_next().unwrap();
            self.advance_ascii();
            self.advance_ascii();
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
            let val = u64::from_str_radix(&clean, radix).unwrap_or(0);
            self.tokens.push(Token::new(TokenKind::HexAddr(val), lexeme, sp));
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
            } else if b == b'.' && !is_float && self.peek_next() != Some(b'.') && self.peek_next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                is_float = true;
                num_str.push('.');
                self.advance_ascii();
            } else {
                break;
            }
        }

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
                "V" => return self.tokens.push(Token::new(TokenKind::Volt(fval * 1000.0), lexeme, sp)),
                "mV" => return self.tokens.push(Token::new(TokenKind::Volt(fval), lexeme, sp)),
                "GHz" => return self.tokens.push(Token::new(TokenKind::Freq(fval * 1e9), lexeme, sp)),
                "MHz" => return self.tokens.push(Token::new(TokenKind::Freq(fval * 1e6), lexeme, sp)),
                "kHz" => return self.tokens.push(Token::new(TokenKind::Freq(fval * 1e3), lexeme, sp)),
                "Hz" => return self.tokens.push(Token::new(TokenKind::Freq(fval), lexeme, sp)),
                "Msps" => return self.tokens.push(Token::new(TokenKind::SampleRate(fval * 1e6), lexeme, sp)),
                "ksps" => return self.tokens.push(Token::new(TokenKind::SampleRate(fval * 1e3), lexeme, sp)),
                "sps" => return self.tokens.push(Token::new(TokenKind::SampleRate(fval), lexeme, sp)),
                "dBm" => return self.tokens.push(Token::new(TokenKind::Dbm(fval), lexeme, sp)),
                "rad" => return self.tokens.push(Token::new(TokenKind::Angle(fval), lexeme, sp)),
                "deg" => return self.tokens.push(Token::new(TokenKind::Angle(fval * std::f64::consts::PI / 180.0), lexeme, sp)),
                "ns" => return self.tokens.push(Token::new(TokenKind::Duration(fval as u64), lexeme, sp)),
                "us" => return self.tokens.push(Token::new(TokenKind::Duration((fval * 1e3) as u64), lexeme, sp)),
                "ms" => return self.tokens.push(Token::new(TokenKind::Duration((fval * 1e6) as u64), lexeme, sp)),
                "s" => return self.tokens.push(Token::new(TokenKind::Duration((fval * 1e9) as u64), lexeme, sp)),
                _ => {
                    // Could be implicit math multiplication in `equation`, e.g., `2x` -> emit Int/Float + Ident!
                    if unit.len() <= 2 && unit.chars().all(|c| c.is_ascii_lowercase()) {
                        let num_sp = Span::new(start_pos, unit_start, start_line, start_col, start_line, start_col + num_str.len());
                        if is_float {
                            self.tokens.push(Token::new(TokenKind::Float(fval), num_str, num_sp));
                        } else {
                            self.tokens.push(Token::new(TokenKind::Int(fval as i64), num_str, num_sp));
                        }
                        let u_sp = Span::new(unit_start, self.pos, start_line, start_col + (unit_start - start_pos), self.line, self.col);
                        self.tokens.push(Token::new(TokenKind::Star, "*", u_sp));
                        self.tokens.push(Token::new(TokenKind::Ident(unit.clone()), unit, u_sp));
                        return;
                    }
                    self.diagnostics.push(
                        Diagnostic::error("RS005", sp, format!("Unknown physical unit `{}`", unit))
                            .with_ar_message(format!("وحدة غير معروفة `{}`", unit))
                            .with_why("Supported units: V, mV, GHz, MHz, kHz, Hz, Msps, ksps, sps, dBm, rad, deg, ns, us, ms, s.", "الوحدات المدعومة تشمل الكهرباء والتردد والـ SDR والزمن والزوايا.")
                            .with_solution("Use a valid unit suffix.", "استخدم وحدة قياس صالحة."),
                    );
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
            "equation" => TokenKind::Equation,
            "algorithm" => TokenKind::Algorithm,
            "defer" => TokenKind::Defer,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "throw" => TokenKind::Throw,
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
            (b':', Some(b':')) => { self.advance_ascii(); (TokenKind::ColonColon, 2) }
            (b':', _) => (TokenKind::Colon, 1),
            (b'-', Some(b'>')) => { self.advance_ascii(); (TokenKind::Arrow, 2) }
            (b'-', Some(b'=')) => { self.advance_ascii(); (TokenKind::MinusEq, 2) }
            (b'-', _) => (TokenKind::Minus, 1),
            (b'=', Some(b'>')) => { self.advance_ascii(); (TokenKind::FatArrow, 2) }
            (b'=', Some(b'=')) => { self.advance_ascii(); (TokenKind::EqEq, 2) }
            (b'=', _) => (TokenKind::Eq, 1),
            (b'!', Some(b'=')) => { self.advance_ascii(); (TokenKind::NotEq, 2) }
            (b'!', _) => (TokenKind::Not, 1),
            (b'<', Some(b'=')) => { self.advance_ascii(); (TokenKind::LtEq, 2) }
            (b'<', Some(b'<')) => { self.advance_ascii(); (TokenKind::Shl, 2) }
            (b'<', _) => (TokenKind::Lt, 1),
            (b'>', Some(b'=')) => { self.advance_ascii(); (TokenKind::GtEq, 2) }
            (b'>', Some(b'>')) => { self.advance_ascii(); (TokenKind::Shr, 2) }
            (b'>', _) => (TokenKind::Gt, 1),
            (b'+', Some(b'=')) => { self.advance_ascii(); (TokenKind::PlusEq, 2) }
            (b'+', _) => (TokenKind::Plus, 1),
            (b'*', Some(b'*')) => { self.advance_ascii(); (TokenKind::StarStar, 2) }
            (b'*', Some(b'=')) => { self.advance_ascii(); (TokenKind::StarEq, 2) }
            (b'*', _) => (TokenKind::Star, 1),
            (b'/', Some(b'=')) => { self.advance_ascii(); (TokenKind::SlashEq, 2) }
            (b'/', _) => (TokenKind::Slash, 1),
            (b'%', _) => (TokenKind::Percent, 1),
            (b'.', Some(b'.')) => {
                self.advance_ascii();
                if self.peek() == Some(b'=') { self.advance_ascii(); (TokenKind::DotDotEq, 3) } else { (TokenKind::DotDot, 2) }
            }
            (b'.', _) => (TokenKind::Dot, 1),
            (b',', _) => (TokenKind::Comma, 1),
            (b';', _) => (TokenKind::Semi, 1),
            (b'@', _) => (TokenKind::At, 1),
            (b'&', Some(b'&')) => { self.advance_ascii(); (TokenKind::And, 2) }
            (b'&', _) => (TokenKind::Amp, 1),
            (b'|', Some(b'>')) => { self.advance_ascii(); (TokenKind::PipeGt, 2) }
            (b'|', Some(b'|')) => { self.advance_ascii(); (TokenKind::Or, 2) }
            (b'|', _) => (TokenKind::Pipe, 1),
            (b'^', _) => (TokenKind::Caret, 1),
            (b'~', _) => (TokenKind::Tilde, 1),
            (b'(', _) => { self.delimiter_depth += 1; (TokenKind::LParen, 1) }
            (b')', _) => { self.delimiter_depth = self.delimiter_depth.saturating_sub(1); (TokenKind::RParen, 1) }
            (b'[', _) => { self.delimiter_depth += 1; (TokenKind::LBracket, 1) }
            (b']', _) => { self.delimiter_depth = self.delimiter_depth.saturating_sub(1); (TokenKind::RBracket, 1) }
            (b'{', _) => { self.delimiter_depth += 1; (TokenKind::LBrace, 1) }
            (b'}', _) => { self.delimiter_depth = self.delimiter_depth.saturating_sub(1); (TokenKind::RBrace, 1) }
            _ => {
                let sp = Span::new(start_pos, self.pos, start_line, start_col, self.line, self.col);
                let ch = b as char;
                self.diagnostics.push(
                    Diagnostic::error("RS006", sp, format!("Unexpected character `{}`", ch))
                        .with_ar_message(format!("رمز غير متوقع `{}`", ch))
                        .with_why("Invalid syntax character outside string or comment.", "رمز غير صالح خارج النص أو التعليق.")
                        .with_solution("Remove this character.", "احذف هذا الرمز.")
                        .with_fix(AutoFix {
                            title: format!("Remove `{}`", ch),
                            title_ar: format!("حذف `{}`", ch),
                            edits: vec![TextEdit { span: sp, replacement: String::new() }],
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
