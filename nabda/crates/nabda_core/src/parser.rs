use crate::ast::*;
use crate::diagnostic::{closest_match, AutoFix, Diagnostic, TextEdit};
use crate::span::Span;
use crate::token::{Token, TokenKind, KEYWORDS};

pub struct ParseOutput {
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(tokens: Vec<Token>) -> ParseOutput {
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    ParseOutput {
        program,
        diagnostics: parser.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or_else(|| self.tokens.last().unwrap())
    }

    fn peek_next(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1)
    }

    fn at_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let tok = self.current().clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        tok
    }

    fn skip_newlines(&mut self) {
        while matches!(self.current().kind, TokenKind::Newline | TokenKind::Semi) {
            self.advance();
        }
    }

    fn prev_span(&self) -> Span {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            self.current().span
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind)
    }

    fn consume_if(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self, ctx: &str) -> Option<(String, Span)> {
        if let TokenKind::Ident(name) = &self.current().kind {
            let n = name.clone();
            let sp = self.current().span;
            self.advance();
            Some((n, sp))
        } else {
            let tok = self.current().clone();
            self.diagnostics.push(
                Diagnostic::error(
                    "NB101",
                    tok.span,
                    format!("Expected identifier {}, found `{}`", ctx, tok.lexeme),
                )
                .with_ar_message(format!("متوقع اسم معرّف {}، ولكن تم العثور على `{}`", ctx, tok.lexeme))
                .with_why(
                    "A variable, function, or type name was expected at this position.",
                    "كان من المتوقع كتابة اسم متغير أو دالة أو نوع في هذا الموضع.",
                )
                .with_solution(
                    "Provide a valid identifier name (letters, digits, underscores).",
                    "اكتب اسماً صحيحاً (أحرف أو أرقام أو شرطة سفلية).",
                ),
            );
            None
        }
    }

    fn parse_program(&mut self) -> Program {
        let mut items = Vec::new();
        self.skip_newlines();
        while !self.at_eof() {
            if matches!(self.current().kind, TokenKind::Dedent) {
                self.advance();
                self.skip_newlines();
                continue;
            }
            let before = self.pos;
            if let Some(item) = self.parse_item() {
                items.push(item);
            }
            if self.pos == before {
                self.advance();
            }
            self.skip_newlines();
        }
        Program { items }
    }

    fn parse_attributes(&mut self) -> Vec<Attribute> {
        let mut attrs = Vec::new();
        while self.check(&TokenKind::At) {
            let start = self.advance().span;
            let Some((name, _)) = self.expect_ident("for attribute name") else {
                break;
            };
            let mut args = Vec::new();
            if self.consume_if(&TokenKind::LParen) {
                while !self.check(&TokenKind::RParen) && !self.at_eof() {
                    match &self.current().kind {
                        TokenKind::Str(s) => {
                            args.push(s.clone());
                            self.advance();
                        }
                        TokenKind::Ident(s) => {
                            args.push(s.clone());
                            self.advance();
                        }
                        _ => {
                            self.advance();
                        }
                    }
                    if !self.consume_if(&TokenKind::Comma) {
                        break;
                    }
                }
                self.consume_if(&TokenKind::RParen);
            }
            let end = self.prev_span();
            attrs.push(Attribute {
                name,
                args,
                span: start.merge(end),
            });
            self.skip_newlines();
        }
        attrs
    }

    fn parse_item(&mut self) -> Option<Item> {
        let attrs = self.parse_attributes();

        // Check common keyword typos at statement start, e.g., "fnn main():" or "improt ..."
        if let TokenKind::Ident(word) = &self.current().kind {
            if let Some(next) = self.peek_next() {
                let looks_like_fn = matches!(next.kind, TokenKind::Ident(_))
                    && matches!(word.as_str(), "fnn" | "fun" | "def" | "func" | "function");
                let looks_like_kw = matches!(
                    word.as_str(),
                    "improt" | "ipmort" | "strcut" | "enmu" | "constt" | "reutrn" | "vvar" | "llet"
                );
                if looks_like_fn || looks_like_kw {
                    let sp = self.current().span;
                    let suggestion = if looks_like_fn {
                        "fn"
                    } else {
                        closest_match(word, KEYWORDS, 2).unwrap_or("fn")
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            "NB102",
                            sp,
                            format!("Unknown keyword `{}` — did you mean `{}`?", word, suggestion),
                        )
                        .with_ar_message(format!("كلمة غير صحيحة `{}` — هل تقصد `{}`؟", word, suggestion))
                        .with_why(
                            format!("`{}` looks like a typo for the Nabda keyword `{}`.", word, suggestion),
                            format!("الكلمة `{}` تبدو كخطأ إملائي للكلمة المفتاحية `{}`.", word, suggestion),
                        )
                        .with_solution(
                            format!("Replace `{}` with `{}`.", word, suggestion),
                            format!("استبدل `{}` بـ `{}`.", word, suggestion),
                        )
                        .with_fix(AutoFix {
                            title: format!("Replace `{}` with `{}`", word, suggestion),
                            title_ar: format!("استبدال `{}` بـ `{}`", word, suggestion),
                            edits: vec![TextEdit {
                                span: sp,
                                replacement: suggestion.to_string(),
                            }],
                            is_preferred: true,
                        }),
                    );
                }
            }
        }

        match &self.current().kind {
            TokenKind::Fn => self.parse_fn(attrs).map(Item::Fn),
            TokenKind::Struct => self.parse_struct().map(Item::Struct),
            TokenKind::Enum => self.parse_enum().map(Item::Enum),
            TokenKind::Extern => self.parse_extern(attrs).map(Item::ExternFn),
            TokenKind::Import | TokenKind::Use => self.parse_import().map(Item::Import),
            TokenKind::Const => self.parse_const().map(Item::Const),
            _ => self.parse_stmt().map(Item::Stmt),
        }
    }

    fn parse_fn(&mut self, attrs: Vec<Attribute>) -> Option<FnDef> {
        let start = self.advance().span; // 'fn'
        let (name, name_span) = self.expect_ident("for function name")?;

        if !self.consume_if(&TokenKind::LParen) {
            let sp = self.prev_span();
            let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
            self.diagnostics.push(
                Diagnostic::error("NB103", self.current().span, format!("Missing `(` after function name `{}`", name))
                    .with_ar_message(format!("ينقص قوس `(` بعد اسم الدالة `{}`", name))
                    .with_why("Function definitions in Nabda require parameter parentheses `(...)`.", "تعريف الدالة في نبضة يتطلب أقواس المعاملات `(...)`.")
                    .with_solution("Add `()` after the function name.", "أضف `()` بعد اسم الدالة.")
                    .with_fix(AutoFix {
                        title: "Insert `()`".to_string(),
                        title_ar: "إضافة `()`".to_string(),
                        edits: vec![TextEdit {
                            span: insert_sp,
                            replacement: "()".to_string(),
                        }],
                        is_preferred: true,
                    }),
            );
        }

        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Colon) && !self.check(&TokenKind::LBrace) && !self.at_eof() {
            let Some((pname, pspan)) = self.expect_ident("for parameter name") else {
                break;
            };
            let ty = if self.consume_if(&TokenKind::Colon) {
                self.parse_type_expr()
            } else {
                None
            };
            let end = ty.as_ref().map(|t| t.span).unwrap_or(pspan);
            params.push(Param {
                name: pname,
                ty,
                span: pspan.merge(end),
            });
            if !self.consume_if(&TokenKind::Comma) {
                break;
            }
        }
        self.consume_if(&TokenKind::RParen);

        let return_type = if self.consume_if(&TokenKind::Arrow) {
            self.parse_type_expr()
        } else {
            None
        };

        let body = self.parse_block(&format!("function `{}`", name));
        let end = self.prev_span();
        Some(FnDef {
            attrs,
            name,
            params,
            return_type,
            body,
            span: start.merge(end),
            name_span,
        })
    }

    fn parse_extern(&mut self, attrs: Vec<Attribute>) -> Option<ExternFnDef> {
        let start = self.advance().span; // 'extern'
        let abi = if let TokenKind::Str(s) = &self.current().kind {
            let v = s.clone();
            self.advance();
            v
        } else {
            "C".to_string()
        };
        if !self.consume_if(&TokenKind::Fn) {
            self.diagnostics.push(
                Diagnostic::error("NB104", self.current().span, "Expected `fn` after `extern`")
                    .with_ar_message("متوقع `fn` بعد `extern`")
                    .with_why("External FFI declarations follow the pattern `extern \"C\" fn name(...)`.", "تصريح الدوال الخارجية يتبع الصيغة `extern \"C\" fn name(...)`.")
                    .with_solution("Add `fn` before the external function name.", "أضف `fn` قبل اسم الدالة الخارجية."),
            );
            return None;
        }
        let (name, _) = self.expect_ident("for extern function name")?;
        self.consume_if(&TokenKind::LParen);
        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.at_eof() {
            let Some((pname, pspan)) = self.expect_ident("for extern parameter") else {
                break;
            };
            let ty = if self.consume_if(&TokenKind::Colon) {
                self.parse_type_expr()
            } else {
                None
            };
            params.push(Param {
                name: pname,
                ty,
                span: pspan,
            });
            if !self.consume_if(&TokenKind::Comma) {
                break;
            }
        }
        self.consume_if(&TokenKind::RParen);
        let return_type = if self.consume_if(&TokenKind::Arrow) {
            self.parse_type_expr()
        } else {
            None
        };
        let end = self.prev_span();
        Some(ExternFnDef {
            attrs,
            abi,
            name,
            params,
            return_type,
            span: start.merge(end),
        })
    }

    fn parse_struct(&mut self) -> Option<StructDef> {
        let start = self.advance().span;
        let (name, _) = self.expect_ident("for struct name")?;
        let mut fields = Vec::new();

        let use_braces = self.consume_if(&TokenKind::LBrace);
        if !use_braces {
            self.expect_colon_before_block(&format!("struct `{}`", name));
            self.skip_newlines();
            if self.consume_if(&TokenKind::Indent) {
                while !self.check(&TokenKind::Dedent) && !self.at_eof() {
                    self.skip_newlines();
                    if self.check(&TokenKind::Dedent) || self.at_eof() {
                        break;
                    }
                    if let Some((fname, fspan)) = self.expect_ident("for struct field") {
                        self.consume_if(&TokenKind::Colon);
                        if let Some(ty) = self.parse_type_expr() {
                            let sp = fspan.merge(ty.span);
                            fields.push(StructField { name: fname, ty, span: sp });
                        }
                    } else {
                        self.advance();
                    }
                    self.skip_newlines();
                }
                self.consume_if(&TokenKind::Dedent);
            }
        } else {
            while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                if let Some((fname, fspan)) = self.expect_ident("for struct field") {
                    self.consume_if(&TokenKind::Colon);
                    if let Some(ty) = self.parse_type_expr() {
                        let sp = fspan.merge(ty.span);
                        fields.push(StructField { name: fname, ty, span: sp });
                    }
                } else {
                    self.advance();
                }
                self.consume_if(&TokenKind::Comma);
            }
            self.consume_if(&TokenKind::RBrace);
        }

        let end = self.prev_span();
        Some(StructDef {
            name,
            fields,
            span: start.merge(end),
        })
    }

    fn parse_enum(&mut self) -> Option<EnumDef> {
        let start = self.advance().span;
        let (name, _) = self.expect_ident("for enum name")?;
        let mut variants = Vec::new();

        let use_braces = self.consume_if(&TokenKind::LBrace);
        if !use_braces {
            self.expect_colon_before_block(&format!("enum `{}`", name));
            self.skip_newlines();
            if self.consume_if(&TokenKind::Indent) {
                while !self.check(&TokenKind::Dedent) && !self.at_eof() {
                    self.skip_newlines();
                    if self.check(&TokenKind::Dedent) || self.at_eof() {
                        break;
                    }
                    if let Some((vname, _)) = self.expect_ident("for enum variant") {
                        variants.push(vname);
                    } else {
                        self.advance();
                    }
                    self.consume_if(&TokenKind::Comma);
                    self.skip_newlines();
                }
                self.consume_if(&TokenKind::Dedent);
            }
        } else {
            while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                if let Some((vname, _)) = self.expect_ident("for enum variant") {
                    variants.push(vname);
                } else {
                    self.advance();
                }
                self.consume_if(&TokenKind::Comma);
            }
            self.consume_if(&TokenKind::RBrace);
        }

        let end = self.prev_span();
        Some(EnumDef {
            name,
            variants,
            span: start.merge(end),
        })
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
        let tok = self.advance();
        let is_use = matches!(tok.kind, TokenKind::Use);
        let start = tok.span;

        let path = match &self.current().kind {
            TokenKind::Str(s) => {
                let p = s.clone();
                self.advance();
                p
            }
            TokenKind::Ident(first) => {
                let mut p = first.clone();
                self.advance();
                while self.consume_if(&TokenKind::ColonColon) || self.consume_if(&TokenKind::Dot) {
                    if let TokenKind::Ident(seg) = &self.current().kind {
                        p.push_str("::");
                        p.push_str(seg);
                        self.advance();
                    } else {
                        break;
                    }
                }
                p
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error("NB105", self.current().span, "Expected module path or package string after `import`")
                        .with_ar_message("متوقع مسار مكتبة أو نص بعد `import`")
                        .with_why("`import` needs either a quoted package/file path or a module name.", "أمر `import` يحتاج اسم مكتبة أو مسار ملف بين علامتي تنصيص.")
                        .with_solution("Example: `import \"crypto_lib\"` or `use memory`.", "مثال: `import \"crypto_lib\"` أو `use memory`."),
                );
                return None;
            }
        };

        let alias = if self.consume_if(&TokenKind::As) {
            self.expect_ident("for import alias").map(|(n, _)| n)
        } else {
            None
        };

        let end = self.prev_span();
        Some(ImportDecl {
            path,
            alias,
            is_use,
            span: start.merge(end),
        })
    }

    fn parse_const(&mut self) -> Option<ConstDef> {
        let start = self.advance().span;
        let (name, _) = self.expect_ident("for constant name")?;
        let ty = if self.consume_if(&TokenKind::Colon) {
            self.parse_type_expr()
        } else {
            None
        };
        if !self.consume_if(&TokenKind::Eq) {
            self.diagnostics.push(
                Diagnostic::error("NB106", self.current().span, format!("Expected `=` in constant `{}`", name))
                    .with_ar_message(format!("ينقص علامة `=` في تعريف الثابت `{}`", name))
                    .with_why("Constants must be initialized at declaration.", "الثوابت يجب إعطاؤها قيمة مباشرة عند التعريف.")
                    .with_solution("Add `= <value>`.", "أضف `= <قيمة>`."),
            );
            return None;
        }
        let value = self.parse_expr()?;
        let end = value.span;
        Some(ConstDef {
            name,
            ty,
            value,
            span: start.merge(end),
        })
    }

    fn parse_type_expr(&mut self) -> Option<TypeExpr> {
        let (name, sp) = self.expect_ident("for type name")?;
        let mut args = Vec::new();
        let mut end = sp;
        if self.consume_if(&TokenKind::LBracket) {
            while !self.check(&TokenKind::RBracket) && !self.at_eof() {
                if let Some(arg) = self.parse_type_expr() {
                    args.push(arg);
                } else {
                    break;
                }
                if !self.consume_if(&TokenKind::Comma) {
                    break;
                }
            }
            if self.consume_if(&TokenKind::RBracket) {
                end = self.prev_span();
            }
        }
        Some(TypeExpr {
            name,
            args,
            span: sp.merge(end),
        })
    }

    fn expect_colon_before_block(&mut self, context: &str) {
        if self.consume_if(&TokenKind::Colon) {
            return;
        }
        // Only emit missing ':' error if next token is newline/indent
        let sp = self.prev_span();
        let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
        self.diagnostics.push(
            Diagnostic::error(
                "NB107",
                insert_sp,
                format!("Missing `:` at the end of {}", context),
            )
            .with_ar_message(format!("ينقص نقطتان `:` في نهاية {}", context))
            .with_why(
                "Python-style blocks in Nabda start with `:` (or `{ ... }`) before an indented body.",
                "الكتل البرمجية في نبضة تبدأ بنقطتين `:` (أو أقواس `{ ... }`) قبل السطور المزاحة.",
            )
            .with_solution(
                "Add `:` right before the newline.",
                "أضف `:` في نهاية السطر قبل الانتقال للسطر التالي.",
            )
            .with_fix(AutoFix {
                title: "Insert missing `:`".to_string(),
                title_ar: "إضافة `:` المفقودة".to_string(),
                edits: vec![TextEdit {
                    span: insert_sp,
                    replacement: ":".to_string(),
                }],
                is_preferred: true,
            }),
        );
    }

    fn parse_block(&mut self, context: &str) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        if self.consume_if(&TokenKind::LBrace) {
            self.skip_newlines();
            while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                let before = self.pos;
                if let Some(st) = self.parse_stmt() {
                    stmts.push(st);
                }
                if self.pos == before {
                    self.advance();
                }
                self.skip_newlines();
            }
            self.consume_if(&TokenKind::RBrace);
            return stmts;
        }

        self.expect_colon_before_block(context);
        // Single-line block after ':' e.g. `if x > 0: return x`
        if !matches!(self.current().kind, TokenKind::Newline | TokenKind::Eof) {
            if let Some(st) = self.parse_stmt() {
                stmts.push(st);
            }
            return stmts;
        }

        self.skip_newlines();
        if !self.consume_if(&TokenKind::Indent) {
            let sp = self.current().span;
            self.diagnostics.push(
                Diagnostic::error("NB108", sp, format!("Expected indented block inside {}", context))
                    .with_ar_message(format!("متوقع إزاحة (مسافات) داخل {}", context))
                    .with_why(
                        "Statements belonging to this block must be indented by 4 spaces.",
                        "الأسطر التابعة لهذه الكتلة يجب أن تكون مزاحة بـ 4 مسافات.",
                    )
                    .with_solution(
                        "Indent the body lines by 4 spaces.",
                        "أضف 4 مسافات في بداية أسطر هذه الكتلة.",
                    ),
            );
            return stmts;
        }

        while !self.check(&TokenKind::Dedent) && !self.at_eof() {
            self.skip_newlines();
            if self.check(&TokenKind::Dedent) || self.at_eof() {
                break;
            }
            let before = self.pos;
            if let Some(st) = self.parse_stmt() {
                stmts.push(st);
            }
            if self.pos == before {
                self.advance();
            }
            self.skip_newlines();
        }
        self.consume_if(&TokenKind::Dedent);
        stmts
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        match &self.current().kind {
            TokenKind::Let => self.parse_var_decl(false),
            TokenKind::Var => self.parse_var_decl(true),
            TokenKind::If => self.parse_if(),
            TokenKind::For => self.parse_for(),
            TokenKind::While => self.parse_while(),
            TokenKind::Loop => self.parse_loop(),
            TokenKind::Match => self.parse_match(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Break => {
                let sp = self.advance().span;
                Some(Stmt::Break(sp))
            }
            TokenKind::Continue => {
                let sp = self.advance().span;
                Some(Stmt::Continue(sp))
            }
            TokenKind::UnsafeHardware => {
                let start = self.advance().span;
                let body = self.parse_block("unsafe_hardware block");
                let end = self.prev_span();
                Some(Stmt::UnsafeHardware {
                    body,
                    span: start.merge(end),
                })
            }
            _ => {
                let expr = self.parse_expr()?;
                // Assignment or compound assignment
                if self.consume_if(&TokenKind::Eq) {
                    let value = self.parse_expr()?;
                    let sp = expr.span.merge(value.span);
                    Some(Stmt::Assign {
                        target: expr,
                        value,
                        span: sp,
                    })
                } else if let Some(op) = self.consume_compound_op() {
                    let value = self.parse_expr()?;
                    let sp = expr.span.merge(value.span);
                    Some(Stmt::CompoundAssign {
                        target: expr,
                        op,
                        value,
                        span: sp,
                    })
                } else {
                    let sp = expr.span;
                    Some(Stmt::Expr { expr, span: sp })
                }
            }
        }
    }

    fn consume_compound_op(&mut self) -> Option<BinOp> {
        let op = match self.current().kind {
            TokenKind::PlusEq => BinOp::Add,
            TokenKind::MinusEq => BinOp::Sub,
            TokenKind::StarEq => BinOp::Mul,
            TokenKind::SlashEq => BinOp::Div,
            _ => return None,
        };
        self.advance();
        Some(op)
    }

    fn parse_var_decl(&mut self, is_var: bool) -> Option<Stmt> {
        let start = self.advance().span;
        let (name, name_span) = self.expect_ident("for variable name")?;
        let ty = if self.consume_if(&TokenKind::Colon) {
            self.parse_type_expr()
        } else {
            None
        };
        if !self.consume_if(&TokenKind::Eq) {
            let sp = self.prev_span();
            let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
            self.diagnostics.push(
                Diagnostic::error("NB109", self.current().span, format!("Variable `{}` must be initialized with `=`", name))
                    .with_ar_message(format!("المتغير `{}` يجب إعطاؤه قيمة ابتدائية باستخدام `=`", name))
                    .with_why(
                        "Nabda prevents uninitialized memory bugs by requiring an initial value for every `let`/`var`.",
                        "نبضة تمنع أخطاء الذاكرة غير المهيأة بإلزام إعطاء قيمة ابتدائية لكل متغير.",
                    )
                    .with_solution("Add `= 0` or another initial value.", "أضف `= 0` أو أي قيمة ابتدائية مناسبة.")
                    .with_fix(AutoFix {
                        title: "Initialize with `= 0`".to_string(),
                        title_ar: "تهيئة المتغير بقيمة `= 0`".to_string(),
                        edits: vec![TextEdit {
                            span: insert_sp,
                            replacement: " = 0".to_string(),
                        }],
                        is_preferred: true,
                    }),
            );
            return None;
        }
        let value = self.parse_expr()?;
        let sp = start.merge(value.span);
        if is_var {
            Some(Stmt::Var {
                name,
                name_span,
                ty,
                value,
                span: sp,
            })
        } else {
            Some(Stmt::Let {
                name,
                name_span,
                ty,
                value,
                span: sp,
            })
        }
    }

    fn parse_if(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let cond = self.parse_expr()?;
        let body = self.parse_block("if condition");
        let mut branches = vec![(cond, body)];
        self.skip_newlines();

        while self.consume_if(&TokenKind::Elif) {
            let c = self.parse_expr()?;
            let b = self.parse_block("elif condition");
            branches.push((c, b));
            self.skip_newlines();
        }

        let else_branch = if self.consume_if(&TokenKind::Else) {
            Some(self.parse_block("else branch"))
        } else {
            None
        };

        let end = self.prev_span();
        Some(Stmt::If {
            branches,
            else_branch,
            span: start.merge(end),
        })
    }

    fn parse_for(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let (var, _) = self.expect_ident("for loop variable")?;
        if !self.consume_if(&TokenKind::In) {
            let sp = self.current().span;
            self.diagnostics.push(
                Diagnostic::error("NB110", sp, "Missing `in` keyword in `for` loop")
                    .with_ar_message("تنقص كلمة `in` في حلقة `for`")
                    .with_why("Loops follow the clean pattern `for item in 0..10:`.", "الحلقات تتبع الصيغة البسيطة `for item in 0..10:`.")
                    .with_solution("Insert `in` between the loop variable and iterable.", "أضف `in` بين اسم المتغير والمجال.")
                    .with_fix(AutoFix {
                        title: "Insert `in`".to_string(),
                        title_ar: "إضافة `in`".to_string(),
                        edits: vec![TextEdit {
                            span: Span::new(sp.start, sp.start, sp.line, sp.col, sp.line, sp.col),
                            replacement: "in ".to_string(),
                        }],
                        is_preferred: true,
                    }),
            );
        }
        let iter = self.parse_expr()?;
        let body = self.parse_block("for loop");
        let end = self.prev_span();
        Some(Stmt::For {
            var,
            iter,
            body,
            span: start.merge(end),
        })
    }

    fn parse_while(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let cond = self.parse_expr()?;
        let body = self.parse_block("while loop");
        let end = self.prev_span();
        Some(Stmt::While {
            cond,
            body,
            span: start.merge(end),
        })
    }

    fn parse_loop(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let body = self.parse_block("loop");
        let end = self.prev_span();
        Some(Stmt::Loop {
            body,
            span: start.merge(end),
        })
    }

    fn parse_match(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let subject = self.parse_expr()?;
        let mut arms = Vec::new();

        let use_braces = self.consume_if(&TokenKind::LBrace);
        if !use_braces {
            self.expect_colon_before_block("match statement");
            self.skip_newlines();
            if self.consume_if(&TokenKind::Indent) {
                while !self.check(&TokenKind::Dedent) && !self.at_eof() {
                    self.skip_newlines();
                    if self.check(&TokenKind::Dedent) || self.at_eof() {
                        break;
                    }
                    self.consume_if(&TokenKind::Case);
                    if let Some(pattern) = self.parse_expr() {
                        let arm_start = pattern.span;
                        let body = if self.consume_if(&TokenKind::FatArrow) {
                            if let Some(st) = self.parse_stmt() {
                                vec![st]
                            } else {
                                vec![]
                            }
                        } else {
                            self.parse_block("match case")
                        };
                        let arm_end = self.prev_span();
                        arms.push(MatchArm {
                            pattern,
                            body,
                            span: arm_start.merge(arm_end),
                        });
                    } else {
                        self.advance();
                    }
                    self.skip_newlines();
                }
                self.consume_if(&TokenKind::Dedent);
            }
        } else {
            self.skip_newlines();
            while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                self.consume_if(&TokenKind::Case);
                if let Some(pattern) = self.parse_expr() {
                    let arm_start = pattern.span;
                    let body = if self.consume_if(&TokenKind::FatArrow) {
                        if let Some(st) = self.parse_stmt() {
                            vec![st]
                        } else {
                            vec![]
                        }
                    } else {
                        self.parse_block("match case")
                    };
                    let arm_end = self.prev_span();
                    arms.push(MatchArm {
                        pattern,
                        body,
                        span: arm_start.merge(arm_end),
                    });
                } else {
                    self.advance();
                }
                self.consume_if(&TokenKind::Comma);
                self.skip_newlines();
            }
            self.consume_if(&TokenKind::RBrace);
        }

        let end = self.prev_span();
        Some(Stmt::Match {
            subject,
            arms,
            span: start.merge(end),
        })
    }

    fn parse_return(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        if matches!(
            self.current().kind,
            TokenKind::Newline | TokenKind::Dedent | TokenKind::RBrace | TokenKind::Eof | TokenKind::Semi
        ) {
            Some(Stmt::Return {
                value: None,
                span: start,
            })
        } else {
            let val = self.parse_expr()?;
            let sp = start.merge(val.span);
            Some(Stmt::Return {
                value: Some(val),
                span: sp,
            })
        }
    }

    // ---- Expressions (Pratt / precedence climbing) ----

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_and()?;
        while self.consume_if(&TokenKind::Or) {
            let right = self.parse_and()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op: BinOp::Or,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_eq()?;
        while self.consume_if(&TokenKind::And) {
            let right = self.parse_eq()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op: BinOp::And,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_eq(&mut self) -> Option<Expr> {
        let mut left = self.parse_cmp()?;
        loop {
            let op = match self.current().kind {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::NotEq => BinOp::NotEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_cmp()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_cmp(&mut self) -> Option<Expr> {
        let mut left = self.parse_range()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::LtEq => BinOp::LtEq,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::GtEq => BinOp::GtEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_range()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_range(&mut self) -> Option<Expr> {
        let left = self.parse_bitor()?;
        if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
            let inclusive = self.check(&TokenKind::DotDotEq);
            self.advance();
            let right = self.parse_bitor()?;
            let sp = left.span.merge(right.span);
            return Some(Expr {
                kind: ExprKind::Range {
                    start: Box::new(left),
                    end: Box::new(right),
                    inclusive,
                },
                span: sp,
            });
        }
        Some(left)
    }

    fn parse_bitor(&mut self) -> Option<Expr> {
        let mut left = self.parse_bitxor()?;
        while self.consume_if(&TokenKind::Pipe) {
            let right = self.parse_bitxor()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op: BinOp::BitOr,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_bitxor(&mut self) -> Option<Expr> {
        let mut left = self.parse_bitand()?;
        while self.consume_if(&TokenKind::Caret) {
            let right = self.parse_bitand()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op: BinOp::BitXor,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_bitand(&mut self) -> Option<Expr> {
        let mut left = self.parse_shift()?;
        while self.consume_if(&TokenKind::Amp) {
            let right = self.parse_shift()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op: BinOp::BitAnd,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_shift(&mut self) -> Option<Expr> {
        let mut left = self.parse_add()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let right = self.parse_add()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_add(&mut self) -> Option<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_mul()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_mul(&mut self) -> Option<Expr> {
        let mut left = self.parse_cast()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_cast()?;
            let sp = left.span.merge(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                span: sp,
            };
        }
        Some(left)
    }

    fn parse_cast(&mut self) -> Option<Expr> {
        let mut expr = self.parse_unary()?;
        while self.consume_if(&TokenKind::As) {
            let ty = self.parse_type_expr()?;
            let sp = expr.span.merge(ty.span);
            expr = Expr {
                kind: ExprKind::Cast {
                    expr: Box::new(expr),
                    ty,
                },
                span: sp,
            };
        }
        Some(expr)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        let start = self.current().span;
        let op = match self.current().kind {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Not => UnaryOp::Not,
            TokenKind::Tilde => UnaryOp::BitNot,
            _ => return self.parse_postfix(),
        };
        self.advance();
        let operand = self.parse_unary()?;
        let sp = start.merge(operand.span);
        Some(Expr {
            kind: ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
            span: sp,
        })
    }

    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.consume_if(&TokenKind::LParen) {
                let args = self.parse_arg_list();
                let end = self.prev_span();
                let sp = expr.span.merge(end);
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span: sp,
                };
            } else if self.consume_if(&TokenKind::Dot) {
                let (field, fspan) = self.expect_ident("for field or method name")?;
                if self.consume_if(&TokenKind::LParen) {
                    let args = self.parse_arg_list();
                    let end = self.prev_span();
                    let sp = expr.span.merge(end);
                    expr = Expr {
                        kind: ExprKind::MethodCall {
                            receiver: Box::new(expr),
                            method: field,
                            args,
                        },
                        span: sp,
                    };
                } else {
                    let sp = expr.span.merge(fspan);
                    expr = Expr {
                        kind: ExprKind::Field {
                            receiver: Box::new(expr),
                            field,
                        },
                        span: sp,
                    };
                }
            } else if self.consume_if(&TokenKind::LBracket) {
                let idx = self.parse_expr()?;
                self.consume_if(&TokenKind::RBracket);
                let end = self.prev_span();
                let sp = expr.span.merge(end);
                expr = Expr {
                    kind: ExprKind::Index {
                        receiver: Box::new(expr),
                        index: Box::new(idx),
                    },
                    span: sp,
                };
            } else {
                break;
            }
        }
        Some(expr)
    }

    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.at_eof() {
            if let Some(arg) = self.parse_expr() {
                args.push(arg);
            } else {
                break;
            }
            if !self.consume_if(&TokenKind::Comma) {
                break;
            }
        }
        if !self.consume_if(&TokenKind::RParen) {
            let sp = self.prev_span();
            let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
            self.diagnostics.push(
                Diagnostic::error("NB111", self.current().span, "Missing closing parenthesis `)` in function call")
                    .with_ar_message("ينقص قوس الإغلاق `)` في استدعاء الدالة")
                    .with_why("An opening `(` was not closed before the end of the expression.", "تم فتح قوس `(` ولم يُغلق قبل نهاية التعبير.")
                    .with_solution("Add `)` after the arguments.", "أضف `)` بعد المعاملات.")
                    .with_fix(AutoFix {
                        title: "Insert `)`".to_string(),
                        title_ar: "إضافة `)`".to_string(),
                        edits: vec![TextEdit {
                            span: insert_sp,
                            replacement: ")".to_string(),
                        }],
                        is_preferred: true,
                    }),
            );
        }
        args
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let tok = self.current().clone();
        match tok.kind {
            TokenKind::Int(v) => {
                self.advance();
                Some(Expr { kind: ExprKind::Int(v), span: tok.span })
            }
            TokenKind::HexAddr(v) => {
                self.advance();
                Some(Expr { kind: ExprKind::HexAddr(v), span: tok.span })
            }
            TokenKind::Float(v) => {
                self.advance();
                Some(Expr { kind: ExprKind::Float(v), span: tok.span })
            }
            TokenKind::Volt(mv) => {
                self.advance();
                Some(Expr { kind: ExprKind::Volt(mv), span: tok.span })
            }
            TokenKind::Freq(hz) => {
                self.advance();
                Some(Expr { kind: ExprKind::Freq(hz), span: tok.span })
            }
            TokenKind::Duration(ns) => {
                self.advance();
                Some(Expr { kind: ExprKind::Duration(ns), span: tok.span })
            }
            TokenKind::Str(s) => {
                self.advance();
                Some(Expr { kind: ExprKind::Str(s), span: tok.span })
            }
            TokenKind::Bool(b) => {
                self.advance();
                Some(Expr { kind: ExprKind::Bool(b), span: tok.span })
            }
            TokenKind::NoneLit => {
                self.advance();
                Some(Expr { kind: ExprKind::NoneLit, span: tok.span })
            }
            TokenKind::Ident(name) => {
                self.advance();
                // Namespace access: `memory::alloc(...)` or `cpu::MSR_IA32_PERF`
                if self.consume_if(&TokenKind::ColonColon) {
                    let (member, member_span) = self.expect_ident("after `::`")?;
                    if self.consume_if(&TokenKind::LParen) {
                        let args = self.parse_arg_list();
                        let end = self.prev_span();
                        return Some(Expr {
                            kind: ExprKind::NamespaceCall {
                                namespace: name,
                                func: member,
                                args,
                                ns_span: tok.span,
                                func_span: member_span,
                            },
                            span: tok.span.merge(end),
                        });
                    }
                    return Some(Expr {
                        kind: ExprKind::NamespacePath {
                            namespace: name,
                            member,
                            ns_span: tok.span,
                            member_span,
                        },
                        span: tok.span.merge(member_span),
                    });
                }

                // Optional struct literal: `Point { x: 1, y: 2 }`
                if self.check(&TokenKind::LBrace)
                    && name.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
                {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                        let (fname, _) = self.expect_ident("for struct field")?;
                        self.consume_if(&TokenKind::Colon);
                        let val = self.parse_expr()?;
                        fields.push((fname, val));
                        if !self.consume_if(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.consume_if(&TokenKind::RBrace);
                    let end = self.prev_span();
                    return Some(Expr {
                        kind: ExprKind::StructLit { name, fields },
                        span: tok.span.merge(end),
                    });
                }

                Some(Expr {
                    kind: ExprKind::Ident(name),
                    span: tok.span,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.consume_if(&TokenKind::RParen);
                Some(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while !self.check(&TokenKind::RBracket) && !self.at_eof() {
                    if let Some(el) = self.parse_expr() {
                        elems.push(el);
                    } else {
                        break;
                    }
                    if !self.consume_if(&TokenKind::Comma) {
                        break;
                    }
                }
                self.consume_if(&TokenKind::RBracket);
                let end = self.prev_span();
                Some(Expr {
                    kind: ExprKind::List(elems),
                    span: tok.span.merge(end),
                })
            }
            TokenKind::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                    let k = self.parse_expr()?;
                    self.consume_if(&TokenKind::Colon);
                    let v = self.parse_expr()?;
                    entries.push((k, v));
                    if !self.consume_if(&TokenKind::Comma) {
                        break;
                    }
                }
                self.consume_if(&TokenKind::RBrace);
                let end = self.prev_span();
                Some(Expr {
                    kind: ExprKind::Map(entries),
                    span: tok.span.merge(end),
                })
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(
                        "NB112",
                        tok.span,
                        format!("Unexpected token `{}` in expression", tok.lexeme),
                    )
                    .with_ar_message(format!("رمز غير متوقع `{}` داخل التعبير", tok.lexeme))
                    .with_why(
                        "Expected a value, identifier, literal, or namespace call like `memory::alloc(...)`.",
                        "كان من المتوقع وجود قيمة أو متغير أو استدعاء مثل `memory::alloc(...)`.",
                    )
                    .with_solution(
                        "Check for missing operands or typos on this line.",
                        "تحقق من عدم وجود معامل مفقود أو خطأ كتابي في هذا السطر.",
                    ),
                );
                self.advance();
                None
            }
        }
    }
}
