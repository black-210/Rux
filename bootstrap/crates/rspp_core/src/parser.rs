use crate::ast::*;
use crate::math_diag::{bayesian_best_match, AutoFix, Diagnostic, TextEdit};
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
    in_equation_mode: bool,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
            in_equation_mode: false,
        }
    }

    fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or_else(|| self.tokens.last().unwrap())
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
        if self.pos > 0 { self.tokens[self.pos - 1].span } else { self.current().span }
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
                Diagnostic::error("RS101", tok.span, format!("Expected identifier {}, found `{}`", ctx, tok.lexeme))
                    .with_ar_message(format!("متوقع اسم {}، ولكن وجد `{}`", ctx, tok.lexeme))
                    .with_why("An identifier name was expected here.", "كان من المتوقع كتابة اسم معرّف هنا.")
                    .with_solution("Provide a valid identifier.", "اكتب اسماً صحيحاً."),
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
            let Some((name, _)) = self.expect_ident("for attribute") else { break };
            let mut args = Vec::new();
            if self.consume_if(&TokenKind::LParen) {
                while !self.check(&TokenKind::RParen) && !self.at_eof() {
                    match &self.current().kind {
                        TokenKind::Str(s) | TokenKind::Ident(s) => {
                            args.push(s.clone());
                            self.advance();
                        }
                        _ => { self.advance(); }
                    }
                    if !self.consume_if(&TokenKind::Comma) { break; }
                }
                self.consume_if(&TokenKind::RParen);
            }
            attrs.push(Attribute { name, args, span: start.merge(self.prev_span()) });
            self.skip_newlines();
        }
        attrs
    }

    fn parse_item(&mut self) -> Option<Item> {
        let attrs = self.parse_attributes();

        if let TokenKind::Ident(word) = &self.current().kind {
            let is_kw_typo = matches!(
                word.as_str(),
                "fnn" | "fun" | "def" | "func" | "improt" | "ipmort" | "strcut" | "enmu" | "reutrn" | "vvar" | "llet" | "equaton" | "algoritm" | "defr"
            );
            if is_kw_typo {
                let sp = self.current().span;
                let w = word.clone();
                let (sug, prob, math_note) = if matches!(w.as_str(), "fnn" | "fun" | "def" | "func") {
                    ("fn", 0.98, "Keyword grammar transition rule -> `fn`".to_string())
                } else if let Some(ranked) = bayesian_best_match(&w, KEYWORDS) {
                    (
                        ranked.candidate,
                        ranked.posterior_prob,
                        format!("Damerau-Levenshtein d={}, Cosine={:.2}", ranked.edit_dist, ranked.cosine_sim),
                    )
                } else {
                    ("fn", 0.70, "Keyword heuristic".to_string())
                };
                self.diagnostics.push(
                    Diagnostic::error("RS102", sp, format!("Unknown keyword `{}` — did you mean `{}`?", w, sug))
                        .with_ar_message(format!("كلمة غير صحيحة `{}` — هل تقصد `{}`؟", w, sug))
                        .with_why(format!("`{}` resembles keyword `{}`.", w, sug), format!("الكلمة `{}` تشبه `{}`.", w, sug))
                        .with_math(math_note, prob)
                        .with_solution(format!("Replace `{}` with `{}`.", w, sug), format!("استبدل `{}` بـ `{}`.", w, sug))
                        .with_fix(AutoFix {
                            title: format!("Replace `{}` with `{}`", w, sug),
                            title_ar: format!("استبدال `{}` بـ `{}`", w, sug),
                            edits: vec![TextEdit { span: sp, replacement: sug.to_string() }],
                            is_preferred: true,
                        }),
                );
            }
        }

        match &self.current().kind {
            TokenKind::Fn => self.parse_fn(attrs).map(Item::Fn),
            TokenKind::Equation => self.parse_equation().map(Item::Equation),
            TokenKind::Algorithm => self.parse_algorithm().map(Item::Algorithm),
            TokenKind::Struct => self.parse_struct().map(Item::Struct),
            TokenKind::Enum => self.parse_enum().map(Item::Enum),
            TokenKind::Extern => self.parse_extern(attrs).map(Item::ExternFn),
            TokenKind::Import | TokenKind::Use => self.parse_import().map(Item::Import),
            TokenKind::Const => self.parse_const().map(Item::Const),
            _ => self.parse_stmt().map(Item::Stmt),
        }
    }

    fn parse_fn(&mut self, attrs: Vec<Attribute>) -> Option<FnDef> {
        let start = self.advance().span;
        let (name, name_span) = self.expect_ident("for function name")?;

        if !self.consume_if(&TokenKind::LParen) {
            let sp = self.prev_span();
            let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
            self.diagnostics.push(
                Diagnostic::error("RS103", self.current().span, format!("Missing `(` after function `{}`", name))
                    .with_ar_message(format!("ينقص `(` بعد اسم الدالة `{}`", name))
                    .with_why("Function definitions require `(...)`.", "تعريف الدالة يتطلب أقواس `(...)`.")
                    .with_solution("Insert `()`.", "أضف `()`.")
                    .with_fix(AutoFix {
                        title: "Insert `()`".into(),
                        title_ar: "إضافة `()`".into(),
                        edits: vec![TextEdit { span: insert_sp, replacement: "()".into() }],
                        is_preferred: true,
                    }),
            );
        }

        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Colon) && !self.check(&TokenKind::LBrace) && !self.at_eof() {
            let Some((pname, pspan)) = self.expect_ident("for parameter") else { break };
            let ty = if self.consume_if(&TokenKind::Colon) { self.parse_type_expr() } else { None };
            let end = ty.as_ref().map(|t| t.span).unwrap_or(pspan);
            params.push(Param { name: pname, ty, span: pspan.merge(end) });
            if !self.consume_if(&TokenKind::Comma) { break; }
        }
        self.consume_if(&TokenKind::RParen);

        let return_type = if self.consume_if(&TokenKind::Arrow) { self.parse_type_expr() } else { None };
        let body = self.parse_block(&format!("function `{}`", name));
        Some(FnDef {
            attrs,
            name,
            params,
            return_type,
            body,
            span: start.merge(self.prev_span()),
            name_span,
        })
    }

    /// `equation quad(x) = x^2 - 5*x + 6`
    fn parse_equation(&mut self) -> Option<EquationDef> {
        let start = self.advance().span;
        let (name, name_span) = self.expect_ident("for equation name")?;
        let mut params = Vec::new();
        if self.consume_if(&TokenKind::LParen) {
            while !self.check(&TokenKind::RParen) && !self.at_eof() {
                let Some((p, _)) = self.expect_ident("for equation variable") else { break };
                params.push(p);
                if !self.consume_if(&TokenKind::Comma) { break; }
            }
            self.consume_if(&TokenKind::RParen);
        }
        if !self.consume_if(&TokenKind::Eq) {
            self.diagnostics.push(
                Diagnostic::error("RS106", self.current().span, format!("Expected `=` in equation `{}`", name))
                    .with_ar_message(format!("ينقص `=` في المعادلة `{}`", name))
                    .with_why("Equations follow `equation name(x) = <math expression>`.", "المعادلات تتبع الصيغة `equation name(x) = <تعبير رياضي>`.")
                    .with_solution("Add `= ...`.", "أضف `= ...`."),
            );
            return None;
        }
        let expr_start_idx = self.pos;
        let prev_mode = self.in_equation_mode;
        self.in_equation_mode = true;
        let expr = self.parse_expr()?;
        self.in_equation_mode = prev_mode;
        let raw_formula = self.tokens[expr_start_idx..self.pos]
            .iter()
            .map(|t| t.lexeme.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Some(EquationDef {
            name,
            params,
            span: start.merge(expr.span),
            expr,
            raw_formula,
            name_span,
        })
    }

    /// `algorithm synth_controller(target): ...`
    fn parse_algorithm(&mut self) -> Option<AlgorithmDef> {
        let start = self.advance().span;
        let (name, name_span) = self.expect_ident("for algorithm name")?;
        self.consume_if(&TokenKind::LParen);
        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Colon) && !self.at_eof() {
            let Some((pname, pspan)) = self.expect_ident("for algorithm parameter") else { break };
            let ty = if self.consume_if(&TokenKind::Colon) { self.parse_type_expr() } else { None };
            params.push(Param { name: pname, ty, span: pspan });
            if !self.consume_if(&TokenKind::Comma) { break; }
        }
        self.consume_if(&TokenKind::RParen);
        let body = self.parse_block(&format!("algorithm `{}`", name));
        Some(AlgorithmDef {
            name,
            params,
            body,
            span: start.merge(self.prev_span()),
            name_span,
        })
    }

    fn parse_extern(&mut self, attrs: Vec<Attribute>) -> Option<ExternFnDef> {
        let start = self.advance().span;
        let abi = if let TokenKind::Str(s) = &self.current().kind {
            let v = s.clone();
            self.advance();
            v
        } else {
            "C".to_string()
        };
        if !self.consume_if(&TokenKind::Fn) {
            return None;
        }
        let (name, _) = self.expect_ident("for extern fn")?;
        self.consume_if(&TokenKind::LParen);
        let mut params = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.at_eof() {
            let Some((pname, pspan)) = self.expect_ident("for extern parameter") else { break };
            let ty = if self.consume_if(&TokenKind::Colon) { self.parse_type_expr() } else { None };
            params.push(Param { name: pname, ty, span: pspan });
            if !self.consume_if(&TokenKind::Comma) { break; }
        }
        self.consume_if(&TokenKind::RParen);
        let return_type = if self.consume_if(&TokenKind::Arrow) { self.parse_type_expr() } else { None };
        Some(ExternFnDef {
            attrs,
            abi,
            name,
            params,
            return_type,
            span: start.merge(self.prev_span()),
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
                    if self.check(&TokenKind::Dedent) || self.at_eof() { break; }
                    if let Some((fname, fspan)) = self.expect_ident("for field") {
                        self.consume_if(&TokenKind::Colon);
                        if let Some(ty) = self.parse_type_expr() {
                            fields.push(StructField { name: fname, span: fspan.merge(ty.span), ty });
                        }
                    } else { self.advance(); }
                    self.skip_newlines();
                }
                self.consume_if(&TokenKind::Dedent);
            }
        } else {
            while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                if let Some((fname, fspan)) = self.expect_ident("for field") {
                    self.consume_if(&TokenKind::Colon);
                    if let Some(ty) = self.parse_type_expr() {
                        fields.push(StructField { name: fname, span: fspan.merge(ty.span), ty });
                    }
                } else { self.advance(); }
                self.consume_if(&TokenKind::Comma);
            }
            self.consume_if(&TokenKind::RBrace);
        }
        Some(StructDef { name, fields, span: start.merge(self.prev_span()) })
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
                    if self.check(&TokenKind::Dedent) || self.at_eof() { break; }
                    if let Some((vname, _)) = self.expect_ident("for enum variant") {
                        variants.push(vname);
                    } else { self.advance(); }
                    self.consume_if(&TokenKind::Comma);
                    self.skip_newlines();
                }
                self.consume_if(&TokenKind::Dedent);
            }
        } else {
            while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                if let Some((vname, _)) = self.expect_ident("for enum variant") {
                    variants.push(vname);
                } else { self.advance(); }
                self.consume_if(&TokenKind::Comma);
            }
            self.consume_if(&TokenKind::RBrace);
        }
        Some(EnumDef { name, variants, span: start.merge(self.prev_span()) })
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
        let tok = self.advance();
        let is_use = matches!(tok.kind, TokenKind::Use);
        let start = tok.span;
        let path = match &self.current().kind {
            TokenKind::Str(s) => { let p = s.clone(); self.advance(); p }
            TokenKind::Ident(first) => {
                let mut p = first.clone();
                self.advance();
                while self.consume_if(&TokenKind::ColonColon) || self.consume_if(&TokenKind::Dot) {
                    if let TokenKind::Ident(seg) = &self.current().kind {
                        p.push_str("::");
                        p.push_str(seg);
                        self.advance();
                    } else { break; }
                }
                p
            }
            _ => return None,
        };
        let alias = if self.consume_if(&TokenKind::As) {
            self.expect_ident("for alias").map(|(n, _)| n)
        } else { None };
        Some(ImportDecl { path, alias, is_use, span: start.merge(self.prev_span()) })
    }

    fn parse_const(&mut self) -> Option<ConstDef> {
        let start = self.advance().span;
        let (name, _) = self.expect_ident("for const name")?;
        let ty = if self.consume_if(&TokenKind::Colon) { self.parse_type_expr() } else { None };
        if !self.consume_if(&TokenKind::Eq) { return None; }
        let value = self.parse_expr()?;
        Some(ConstDef { name, ty, span: start.merge(value.span), value })
    }

    fn parse_type_expr(&mut self) -> Option<TypeExpr> {
        let (name, sp) = self.expect_ident("for type name")?;
        let mut args = Vec::new();
        let mut end = sp;
        if self.consume_if(&TokenKind::LBracket) {
            while !self.check(&TokenKind::RBracket) && !self.at_eof() {
                if let Some(arg) = self.parse_type_expr() { args.push(arg); } else { break; }
                if !self.consume_if(&TokenKind::Comma) { break; }
            }
            if self.consume_if(&TokenKind::RBracket) { end = self.prev_span(); }
        }
        Some(TypeExpr { name, args, span: sp.merge(end) })
    }

    fn expect_colon_before_block(&mut self, context: &str) {
        if self.consume_if(&TokenKind::Colon) { return; }
        let sp = self.prev_span();
        let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
        self.diagnostics.push(
            Diagnostic::error("RS107", insert_sp, format!("Missing `:` at end of {}", context))
                .with_ar_message(format!("ينقص نقطتان `:` في نهاية {}", context))
                .with_why("Blocks in rust++ open with `:` or `{ ... }`.", "الكتل البرمجية تبدأ بـ `:` أو `{ ... }`.")
                .with_math("Minimum-cost token repair: Insert(`:`) cost = 1", 0.99)
                .with_solution("Add `:` at end of header line.", "أضف `:` في نهاية السطر.")
                .with_fix(AutoFix {
                    title: "Insert missing `:`".into(),
                    title_ar: "إضافة `:` المفقودة".into(),
                    edits: vec![TextEdit { span: insert_sp, replacement: ":".into() }],
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
                if let Some(st) = self.parse_stmt() { stmts.push(st); }
                if self.pos == before { self.advance(); }
                self.skip_newlines();
            }
            self.consume_if(&TokenKind::RBrace);
            return stmts;
        }
        self.expect_colon_before_block(context);
        if !matches!(self.current().kind, TokenKind::Newline | TokenKind::Eof) {
            if let Some(st) = self.parse_stmt() { stmts.push(st); }
            return stmts;
        }
        self.skip_newlines();
        if !self.consume_if(&TokenKind::Indent) {
            return stmts;
        }
        while !self.check(&TokenKind::Dedent) && !self.at_eof() {
            self.skip_newlines();
            if self.check(&TokenKind::Dedent) || self.at_eof() { break; }
            let before = self.pos;
            if let Some(st) = self.parse_stmt() { stmts.push(st); }
            if self.pos == before { self.advance(); }
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
            TokenKind::Break => Some(Stmt::Break(self.advance().span)),
            TokenKind::Continue => Some(Stmt::Continue(self.advance().span)),
            TokenKind::UnsafeHardware => {
                let start = self.advance().span;
                let body = self.parse_block("unsafe_hardware block");
                Some(Stmt::UnsafeHardware { body, span: start.merge(self.prev_span()) })
            }
            TokenKind::Defer => {
                let start = self.advance().span;
                let inner = self.parse_stmt()?;
                Some(Stmt::Defer { span: start.merge(self.prev_span()), stmt: Box::new(inner) })
            }
            TokenKind::Try => self.parse_try_catch(),
            TokenKind::Throw => {
                let start = self.advance().span;
                let value = self.parse_expr()?;
                Some(Stmt::Throw { span: start.merge(value.span), value })
            }
            _ => {
                let expr = self.parse_expr()?;
                if self.consume_if(&TokenKind::Eq) {
                    let value = self.parse_expr()?;
                    Some(Stmt::Assign { span: expr.span.merge(value.span), target: expr, value })
                } else if let Some(op) = self.consume_compound_op() {
                    let value = self.parse_expr()?;
                    Some(Stmt::CompoundAssign { span: expr.span.merge(value.span), target: expr, op, value })
                } else {
                    Some(Stmt::Expr { span: expr.span, expr })
                }
            }
        }
    }

    fn parse_try_catch(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let try_body = self.parse_block("try block");
        self.skip_newlines();
        let mut err_var = "err".to_string();
        let mut catch_body = Vec::new();
        if self.consume_if(&TokenKind::Catch) {
            if let TokenKind::Ident(n) = &self.current().kind {
                err_var = n.clone();
                self.advance();
            }
            catch_body = self.parse_block("catch block");
        }
        Some(Stmt::TryCatch {
            try_body,
            err_var,
            catch_body,
            span: start.merge(self.prev_span()),
        })
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
        let ty = if self.consume_if(&TokenKind::Colon) { self.parse_type_expr() } else { None };
        if !self.consume_if(&TokenKind::Eq) {
            let sp = self.prev_span();
            let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
            self.diagnostics.push(
                Diagnostic::error("RS109", self.current().span, format!("Variable `{}` must be initialized with `=`", name))
                    .with_ar_message(format!("المتغير `{}` يجب تهيئته بـ `=`", name))
                    .with_why("Preventing uninitialized memory is mandatory.", "منع الذاكرة غير المهيأة إجباري.")
                    .with_solution("Add `= 0`.", "أضف `= 0`.")
                    .with_fix(AutoFix {
                        title: "Initialize with `= 0`".into(),
                        title_ar: "تهيئة بـ `= 0`".into(),
                        edits: vec![TextEdit { span: insert_sp, replacement: " = 0".into() }],
                        is_preferred: true,
                    }),
            );
            return None;
        }
        let value = self.parse_expr()?;
        let sp = start.merge(value.span);
        if is_var {
            Some(Stmt::Var { name, name_span, ty, value, span: sp })
        } else {
            Some(Stmt::Let { name, name_span, ty, value, span: sp })
        }
    }

    fn parse_if(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let cond = self.parse_expr()?;
        let body = self.parse_block("if");
        let mut branches = vec![(cond, body)];
        self.skip_newlines();
        while self.consume_if(&TokenKind::Elif) {
            let c = self.parse_expr()?;
            let b = self.parse_block("elif");
            branches.push((c, b));
            self.skip_newlines();
        }
        let else_branch = if self.consume_if(&TokenKind::Else) {
            Some(self.parse_block("else"))
        } else {
            None
        };
        Some(Stmt::If { branches, else_branch, span: start.merge(self.prev_span()) })
    }

    fn parse_for(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let (var, _) = self.expect_ident("for loop variable")?;
        self.consume_if(&TokenKind::In);
        let iter = self.parse_expr()?;
        let body = self.parse_block("for loop");
        Some(Stmt::For { var, iter, body, span: start.merge(self.prev_span()) })
    }

    fn parse_while(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let cond = self.parse_expr()?;
        let body = self.parse_block("while loop");
        Some(Stmt::While { cond, body, span: start.merge(self.prev_span()) })
    }

    fn parse_loop(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let body = self.parse_block("loop");
        Some(Stmt::Loop { body, span: start.merge(self.prev_span()) })
    }

    fn parse_match(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        let subject = self.parse_expr()?;
        let mut arms = Vec::new();
        self.expect_colon_before_block("match");
        self.skip_newlines();
        if self.consume_if(&TokenKind::Indent) {
            while !self.check(&TokenKind::Dedent) && !self.at_eof() {
                self.skip_newlines();
                if self.check(&TokenKind::Dedent) || self.at_eof() { break; }
                self.consume_if(&TokenKind::Case);
                if let Some(pattern) = self.parse_expr() {
                    let arm_start = pattern.span;
                    let body = if self.consume_if(&TokenKind::FatArrow) {
                        self.parse_stmt().map(|s| vec![s]).unwrap_or_default()
                    } else {
                        self.parse_block("match case")
                    };
                    arms.push(MatchArm { pattern, body, span: arm_start.merge(self.prev_span()) });
                } else { self.advance(); }
                self.skip_newlines();
            }
            self.consume_if(&TokenKind::Dedent);
        }
        Some(Stmt::Match { subject, arms, span: start.merge(self.prev_span()) })
    }

    fn parse_return(&mut self) -> Option<Stmt> {
        let start = self.advance().span;
        if matches!(self.current().kind, TokenKind::Newline | TokenKind::Dedent | TokenKind::RBrace | TokenKind::Eof | TokenKind::Semi) {
            Some(Stmt::Return { value: None, span: start })
        } else {
            let val = self.parse_expr()?;
            Some(Stmt::Return { span: start.merge(val.span), value: Some(val) })
        }
    }

    // ---- Expressions including |> pipeline and ** / equation ^ power ----

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_pipeline()
    }

    fn parse_pipeline(&mut self) -> Option<Expr> {
        let mut left = self.parse_or()?;
        while self.consume_if(&TokenKind::PipeGt) {
            let rhs = self.parse_or()?;
            let sp = left.span.merge(rhs.span);
            // Transform `x |> ns::func(a)` into `ns::func(x, a)` or `x |> f(a)` into `f(x, a)`
            left = match rhs.kind {
                ExprKind::NamespaceCall { namespace, func, mut args, ns_span, func_span } => {
                    args.insert(0, left);
                    Expr { kind: ExprKind::NamespaceCall { namespace, func, args, ns_span, func_span }, span: sp }
                }
                ExprKind::Call { callee, mut args } => {
                    args.insert(0, left);
                    Expr { kind: ExprKind::Call { callee, args }, span: sp }
                }
                _ => Expr { kind: ExprKind::Call { callee: Box::new(rhs), args: vec![left] }, span: sp },
            };
        }
        Some(left)
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut left = self.parse_and()?;
        while self.consume_if(&TokenKind::Or) {
            let right = self.parse_and()?;
            let sp = left.span.merge(right.span);
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op: BinOp::Or, right: Box::new(right) }, span: sp };
        }
        Some(left)
    }

    fn parse_and(&mut self) -> Option<Expr> {
        let mut left = self.parse_eq()?;
        while self.consume_if(&TokenKind::And) {
            let right = self.parse_eq()?;
            let sp = left.span.merge(right.span);
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op: BinOp::And, right: Box::new(right) }, span: sp };
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
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) }, span: sp };
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
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) }, span: sp };
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
            return Some(Expr { kind: ExprKind::Range { start: Box::new(left), end: Box::new(right), inclusive }, span: sp });
        }
        Some(left)
    }

    fn parse_bitor(&mut self) -> Option<Expr> {
        let mut left = self.parse_bitxor()?;
        while self.consume_if(&TokenKind::Pipe) {
            let right = self.parse_bitxor()?;
            let sp = left.span.merge(right.span);
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op: BinOp::BitOr, right: Box::new(right) }, span: sp };
        }
        Some(left)
    }

    fn parse_bitxor(&mut self) -> Option<Expr> {
        let mut left = self.parse_bitand()?;
        while !self.in_equation_mode && self.consume_if(&TokenKind::Caret) {
            let right = self.parse_bitand()?;
            let sp = left.span.merge(right.span);
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op: BinOp::BitXor, right: Box::new(right) }, span: sp };
        }
        Some(left)
    }

    fn parse_bitand(&mut self) -> Option<Expr> {
        let mut left = self.parse_shift()?;
        while self.consume_if(&TokenKind::Amp) {
            let right = self.parse_shift()?;
            let sp = left.span.merge(right.span);
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op: BinOp::BitAnd, right: Box::new(right) }, span: sp };
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
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) }, span: sp };
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
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) }, span: sp };
        }
        Some(left)
    }

    fn parse_mul(&mut self) -> Option<Expr> {
        let mut left = self.parse_pow()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_pow()?;
            let sp = left.span.merge(right.span);
            left = Expr { kind: ExprKind::Binary { left: Box::new(left), op, right: Box::new(right) }, span: sp };
        }
        Some(left)
    }

    fn parse_pow(&mut self) -> Option<Expr> {
        let left = self.parse_cast()?;
        if self.check(&TokenKind::StarStar) || (self.in_equation_mode && self.check(&TokenKind::Caret)) {
            self.advance();
            let right = self.parse_pow()?;
            let sp = left.span.merge(right.span);
            return Some(Expr { kind: ExprKind::Binary { left: Box::new(left), op: BinOp::Pow, right: Box::new(right) }, span: sp });
        }
        Some(left)
    }

    fn parse_cast(&mut self) -> Option<Expr> {
        let mut expr = self.parse_unary()?;
        while self.consume_if(&TokenKind::As) {
            let ty = self.parse_type_expr()?;
            let sp = expr.span.merge(ty.span);
            expr = Expr { kind: ExprKind::Cast { expr: Box::new(expr), ty }, span: sp };
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
        Some(Expr { kind: ExprKind::Unary { op, operand: Box::new(operand) }, span: start.merge(self.prev_span()) })
    }

    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.consume_if(&TokenKind::LParen) {
                let args = self.parse_arg_list();
                let sp = expr.span.merge(self.prev_span());
                expr = Expr { kind: ExprKind::Call { callee: Box::new(expr), args }, span: sp };
            } else if self.consume_if(&TokenKind::Dot) {
                let (field, fspan) = self.expect_ident("for field/method")?;
                if self.consume_if(&TokenKind::LParen) {
                    let args = self.parse_arg_list();
                    let sp = expr.span.merge(self.prev_span());
                    expr = Expr { kind: ExprKind::MethodCall { receiver: Box::new(expr), method: field, args }, span: sp };
                } else {
                    let sp = expr.span.merge(fspan);
                    expr = Expr { kind: ExprKind::Field { receiver: Box::new(expr), field }, span: sp };
                }
            } else if self.consume_if(&TokenKind::LBracket) {
                let idx = self.parse_expr()?;
                self.consume_if(&TokenKind::RBracket);
                let sp = expr.span.merge(self.prev_span());
                expr = Expr { kind: ExprKind::Index { receiver: Box::new(expr), index: Box::new(idx) }, span: sp };
            } else {
                break;
            }
        }
        Some(expr)
    }

    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        while !self.check(&TokenKind::RParen) && !self.at_eof() {
            if let Some(arg) = self.parse_expr() { args.push(arg); } else { break; }
            if !self.consume_if(&TokenKind::Comma) { break; }
        }
        if !self.consume_if(&TokenKind::RParen) {
            let sp = self.prev_span();
            let insert_sp = Span::new(sp.end, sp.end, sp.end_line, sp.end_col, sp.end_line, sp.end_col);
            self.diagnostics.push(
                Diagnostic::error("RS111", self.current().span, "Missing `)` in call")
                    .with_ar_message("ينقص قوس الإغلاق `)`")
                    .with_why("Unclosed argument list.", "قائمة المعاملات غير مغلقة.")
                    .with_solution("Add `)`.", "أضف `)`.")
                    .with_fix(AutoFix {
                        title: "Insert `)`".into(),
                        title_ar: "إضافة `)`".into(),
                        edits: vec![TextEdit { span: insert_sp, replacement: ")".into() }],
                        is_preferred: true,
                    }),
            );
        }
        args
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        let tok = self.current().clone();
        match tok.kind {
            TokenKind::Int(v) => { self.advance(); Some(Expr { kind: ExprKind::Int(v), span: tok.span }) }
            TokenKind::HexAddr(v) => { self.advance(); Some(Expr { kind: ExprKind::HexAddr(v), span: tok.span }) }
            TokenKind::Float(v) => { self.advance(); Some(Expr { kind: ExprKind::Float(v), span: tok.span }) }
            TokenKind::Volt(mv) => { self.advance(); Some(Expr { kind: ExprKind::Volt(mv), span: tok.span }) }
            TokenKind::Freq(hz) => { self.advance(); Some(Expr { kind: ExprKind::Freq(hz), span: tok.span }) }
            TokenKind::Duration(ns) => { self.advance(); Some(Expr { kind: ExprKind::Duration(ns), span: tok.span }) }
            TokenKind::SampleRate(sr) => { self.advance(); Some(Expr { kind: ExprKind::SampleRate(sr), span: tok.span }) }
            TokenKind::Dbm(db) => { self.advance(); Some(Expr { kind: ExprKind::Dbm(db), span: tok.span }) }
            TokenKind::Angle(rad) => { self.advance(); Some(Expr { kind: ExprKind::Angle(rad), span: tok.span }) }
            TokenKind::Str(s) => { self.advance(); Some(Expr { kind: ExprKind::Str(s), span: tok.span }) }
            TokenKind::FStr(s) => { self.advance(); Some(Expr { kind: ExprKind::FStr(s), span: tok.span }) }
            TokenKind::Bool(b) => { self.advance(); Some(Expr { kind: ExprKind::Bool(b), span: tok.span }) }
            TokenKind::NoneLit => { self.advance(); Some(Expr { kind: ExprKind::NoneLit, span: tok.span }) }
            TokenKind::Ident(name) => {
                self.advance();
                if self.consume_if(&TokenKind::ColonColon) {
                    let (member, member_span) = self.expect_ident("after `::`")?;
                    if self.consume_if(&TokenKind::LParen) {
                        let args = self.parse_arg_list();
                        return Some(Expr {
                            kind: ExprKind::NamespaceCall { namespace: name, func: member, args, ns_span: tok.span, func_span: member_span },
                            span: tok.span.merge(self.prev_span()),
                        });
                    }
                    return Some(Expr {
                        kind: ExprKind::NamespacePath { namespace: name, member, ns_span: tok.span, member_span },
                        span: tok.span.merge(member_span),
                    });
                }
                if self.check(&TokenKind::LBrace) && name.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                        let (fname, _) = self.expect_ident("for struct field")?;
                        self.consume_if(&TokenKind::Colon);
                        let val = self.parse_expr()?;
                        fields.push((fname, val));
                        if !self.consume_if(&TokenKind::Comma) { break; }
                    }
                    self.consume_if(&TokenKind::RBrace);
                    return Some(Expr { kind: ExprKind::StructLit { name, fields }, span: tok.span.merge(self.prev_span()) });
                }
                Some(Expr { kind: ExprKind::Ident(name), span: tok.span })
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
                    if let Some(el) = self.parse_expr() { elems.push(el); } else { break; }
                    if !self.consume_if(&TokenKind::Comma) { break; }
                }
                self.consume_if(&TokenKind::RBracket);
                Some(Expr { kind: ExprKind::List(elems), span: tok.span.merge(self.prev_span()) })
            }
            TokenKind::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                    let k = self.parse_expr()?;
                    self.consume_if(&TokenKind::Colon);
                    let v = self.parse_expr()?;
                    entries.push((k, v));
                    if !self.consume_if(&TokenKind::Comma) { break; }
                }
                self.consume_if(&TokenKind::RBrace);
                Some(Expr { kind: ExprKind::Map(entries), span: tok.span.merge(self.prev_span()) })
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error("RS112", tok.span, format!("Unexpected token `{}`", tok.lexeme))
                        .with_ar_message(format!("رمز غير متوقع `{}`", tok.lexeme))
                        .with_why("Expected expression.", "كان من المتوقع وجود تعبير برمجي.")
                        .with_solution("Check syntax on this line.", "تحقق من صياغة هذا السطر."),
                );
                self.advance();
                None
            }
        }
    }
}
