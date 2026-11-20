use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::leak_guard::StaticLeakGuard;
use crate::math_diag::{bayesian_best_match, AutoFix, Diagnostic, Interval, Severity, TextEdit};
use crate::span::Span;
use crate::stdlib_spec::{builtins_in_namespace, find_builtin, ReturnKind, NAMESPACES};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Ptr,
    Volt,
    Freq,
    Duration,
    SampleRate,
    Dbm,
    Angle,
    List,
    Map,
    Void,
    Unknown,
    Named(String),
}

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub ty: Ty,
    pub mutable: bool,
    pub span: Span,
    pub kind: SymbolKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Variable,
    Function,
    Equation,
    Algorithm,
    Struct,
    Enum,
    Param,
}

pub struct AnalysisResult {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<SymbolInfo>,
}

pub fn analyze(program: &Program, source: &str) -> AnalysisResult {
    let mut analyzer = Analyzer::new(source);
    analyzer.run(program);
    AnalysisResult {
        diagnostics: analyzer.diagnostics,
        symbols: analyzer.recorded_symbols,
    }
}

struct Analyzer<'a> {
    source: &'a str,
    scopes: Vec<HashMap<String, SymbolInfo>>,
    intervals: HashMap<String, Interval>,
    functions: HashMap<String, (Vec<Ty>, Ty)>,
    equations: HashMap<String, Vec<String>>,
    algorithms: HashSet<String>,
    externs: HashSet<String>,
    in_unsafe_hw: bool,
    in_kernel_fn: bool,
    current_sdr_sample_rate: Option<f64>,
    leak_guard: StaticLeakGuard,
    pub diagnostics: Vec<Diagnostic>,
    pub recorded_symbols: Vec<SymbolInfo>,
}

impl<'a> Analyzer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            scopes: vec![HashMap::new()],
            intervals: HashMap::new(),
            functions: HashMap::new(),
            equations: HashMap::new(),
            algorithms: HashSet::new(),
            externs: HashSet::new(),
            in_unsafe_hw: false,
            in_kernel_fn: false,
            current_sdr_sample_rate: None,
            leak_guard: StaticLeakGuard::new(),
            diagnostics: Vec::new(),
            recorded_symbols: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.leak_guard.enter_scope();
    }

    fn pop_scope(&mut self) {
        self.leak_guard.exit_scope(self.source);
        self.scopes.pop();
    }

    fn define(&mut self, info: SymbolInfo) {
        self.recorded_symbols.push(info.clone());
        if let Some(top) = self.scopes.last_mut() {
            top.insert(info.name.clone(), info);
        }
    }

    fn lookup(&self, name: &str) -> Option<&SymbolInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.get(name) {
                return Some(sym);
            }
        }
        None
    }

    fn all_visible_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for scope in &self.scopes {
            for k in scope.keys() {
                if !names.contains(k) {
                    names.push(k.clone());
                }
            }
        }
        for k in self.functions.keys() {
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
        for k in self.equations.keys() {
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
        names
    }

    fn run(&mut self, program: &Program) {
        // First pass: register top-level functions, equations, algorithms, structs, externs
        for item in &program.items {
            match item {
                Item::Fn(f) => {
                    let param_tys = f.params.iter().map(|p| self.resolve_type_opt(p.ty.as_ref())).collect();
                    let ret_ty = self.resolve_type_opt(f.return_type.as_ref());
                    self.functions.insert(f.name.clone(), (param_tys, ret_ty.clone()));
                    self.define(SymbolInfo {
                        name: f.name.clone(),
                        ty: ret_ty,
                        mutable: false,
                        span: f.name_span,
                        kind: SymbolKind::Function,
                    });
                }
                Item::Equation(eq) => {
                    self.equations.insert(eq.name.clone(), eq.params.clone());
                    let param_tys = vec![Ty::Float; eq.params.len()];
                    self.functions.insert(eq.name.clone(), (param_tys, Ty::Float));
                    self.define(SymbolInfo {
                        name: eq.name.clone(),
                        ty: Ty::Float,
                        mutable: false,
                        span: eq.name_span,
                        kind: SymbolKind::Equation,
                    });
                }
                Item::Algorithm(alg) => {
                    self.algorithms.insert(alg.name.clone());
                    let param_tys = alg.params.iter().map(|p| self.resolve_type_opt(p.ty.as_ref())).collect();
                    self.functions.insert(alg.name.clone(), (param_tys, Ty::Str));
                    self.define(SymbolInfo {
                        name: alg.name.clone(),
                        ty: Ty::Str,
                        mutable: false,
                        span: alg.name_span,
                        kind: SymbolKind::Algorithm,
                    });
                }
                Item::ExternFn(ef) => {
                    self.externs.insert(ef.name.clone());
                    let param_tys = ef.params.iter().map(|p| self.resolve_type_opt(p.ty.as_ref())).collect();
                    let ret_ty = self.resolve_type_opt(ef.return_type.as_ref());
                    self.functions.insert(ef.name.clone(), (param_tys, ret_ty));
                }
                Item::Struct(s) => {
                    self.define(SymbolInfo {
                        name: s.name.clone(),
                        ty: Ty::Named(s.name.clone()),
                        mutable: false,
                        span: s.span,
                        kind: SymbolKind::Struct,
                    });
                }
                Item::Enum(e) => {
                    self.define(SymbolInfo {
                        name: e.name.clone(),
                        ty: Ty::Named(e.name.clone()),
                        mutable: false,
                        span: e.span,
                        kind: SymbolKind::Enum,
                    });
                }
                Item::Const(c) => {
                    let ty = self.resolve_type_opt(c.ty.as_ref());
                    self.define(SymbolInfo {
                        name: c.name.clone(),
                        ty,
                        mutable: false,
                        span: c.span,
                        kind: SymbolKind::Variable,
                    });
                }
                _ => {}
            }
        }

        // Second pass: analyze bodies & statements
        for item in &program.items {
            match item {
                Item::Fn(f) => self.analyze_fn(f),
                Item::Equation(eq) => self.analyze_equation(eq),
                Item::Algorithm(alg) => self.analyze_algorithm(alg),
                Item::Const(c) => {
                    self.analyze_expr(&c.value);
                }
                Item::Stmt(s) => self.analyze_stmt(s),
                _ => {}
            }
        }

        // Drain any remaining top-level leaks
        self.leak_guard.exit_scope(self.source);
        self.diagnostics.append(&mut self.leak_guard.diagnostics);
    }

    fn resolve_type_opt(&self, t: Option<&TypeExpr>) -> Ty {
        match t {
            None => Ty::Unknown,
            Some(te) => match te.name.as_str() {
                "int" | "i64" | "i32" | "u64" | "u32" | "u8" => Ty::Int,
                "float" | "f64" | "f32" => Ty::Float,
                "bool" => Ty::Bool,
                "str" | "string" => Ty::Str,
                "ptr" | "addr" => Ty::Ptr,
                "volt" => Ty::Volt,
                "freq" => Ty::Freq,
                "duration" => Ty::Duration,
                "sps" | "samplerate" => Ty::SampleRate,
                "dbm" => Ty::Dbm,
                "angle" => Ty::Angle,
                "list" => Ty::List,
                "map" => Ty::Map,
                "void" => Ty::Void,
                other => Ty::Named(other.to_string()),
            },
        }
    }

    fn analyze_fn(&mut self, f: &FnDef) {
        let prev_kernel = self.in_kernel_fn;
        self.in_kernel_fn = f.attrs.iter().any(|a| a.name == "kernel" || a.name == "interrupt");
        self.push_scope();
        for p in &f.params {
            let ty = self.resolve_type_opt(p.ty.as_ref());
            self.define(SymbolInfo {
                name: p.name.clone(),
                ty,
                mutable: true,
                span: p.span,
                kind: SymbolKind::Param,
            });
        }
        for st in &f.body {
            self.analyze_stmt(st);
        }
        self.pop_scope();
        self.in_kernel_fn = prev_kernel;
    }

    fn analyze_equation(&mut self, eq: &EquationDef) {
        self.push_scope();
        for p in &eq.params {
            self.define(SymbolInfo {
                name: p.clone(),
                ty: Ty::Float,
                mutable: false,
                span: eq.name_span,
                kind: SymbolKind::Param,
            });
        }
        self.analyze_expr(&eq.expr);
        self.pop_scope();
    }

    fn analyze_algorithm(&mut self, alg: &AlgorithmDef) {
        self.push_scope();
        for p in &alg.params {
            let ty = self.resolve_type_opt(p.ty.as_ref());
            self.define(SymbolInfo {
                name: p.name.clone(),
                ty,
                mutable: true,
                span: p.span,
                kind: SymbolKind::Param,
            });
        }
        for st in &alg.body {
            self.analyze_stmt(st);
        }
        self.pop_scope();
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, name_span, ty, value, span } => {
                let val_ty = self.analyze_expr(value);
                if let Some(iv) = self.eval_interval(value) {
                    self.intervals.insert(name.clone(), iv);
                }
                let final_ty = ty.as_ref().map(|t| self.resolve_type_opt(Some(t))).unwrap_or(val_ty);
                self.define(SymbolInfo {
                    name: name.clone(),
                    ty: final_ty,
                    mutable: false,
                    span: *name_span,
                    kind: SymbolKind::Variable,
                });
                self.track_alloc_if_needed(name, *name_span, *span, value);
            }
            Stmt::Var { name, name_span, ty, value, span } => {
                let val_ty = self.analyze_expr(value);
                if let Some(iv) = self.eval_interval(value) {
                    self.intervals.insert(name.clone(), iv);
                }
                let final_ty = ty.as_ref().map(|t| self.resolve_type_opt(Some(t))).unwrap_or(val_ty);
                self.define(SymbolInfo {
                    name: name.clone(),
                    ty: final_ty,
                    mutable: true,
                    span: *name_span,
                    kind: SymbolKind::Variable,
                });
                self.track_alloc_if_needed(name, *name_span, *span, value);
            }
            Stmt::Assign { target, value, span } => {
                self.analyze_expr(value);
                if let ExprKind::Ident(name) = &target.kind {
                    if let Some(iv) = self.eval_interval(value) {
                        self.intervals.insert(name.clone(), iv);
                    }
                    if let Some(sym) = self.lookup(name).cloned() {
                        if !sym.mutable {
                            let fix = self.make_var_autofix(sym.span);
                            let mut d = Diagnostic::error(
                                "RS201",
                                target.span,
                                format!("Cannot reassign immutable `let` variable `{}`", name),
                            )
                            .with_ar_message(format!("لا يمكن تعديل المتغير الثابت `let` المسمى `{}`", name))
                            .with_why(
                                "Variables declared with `let` are immutable by default to prevent accidental memory/state bugs.",
                                "المتغيرات المعرفة بـ `let` ثابتة افتراضياً لحماية الحالة والذاكرة.",
                            )
                            .with_solution(
                                format!("Change `let {}` to `var {}`.", name, name),
                                format!("غيّر `let {}` إلى `var {}`.", name, name),
                            );
                            if let Some(f) = fix {
                                d = d.with_fix(f);
                            }
                            self.diagnostics.push(d);
                        }
                    } else {
                        self.report_undefined_var(name, target.span);
                    }
                    self.track_alloc_if_needed(name, target.span, *span, value);
                } else {
                    self.analyze_expr(target);
                }
            }
            Stmt::CompoundAssign { target, op: _, value, .. } => {
                self.analyze_expr(value);
                if let ExprKind::Ident(name) = &target.kind {
                    if let Some(sym) = self.lookup(name).cloned() {
                        if !sym.mutable {
                            self.diagnostics.push(
                                Diagnostic::error("RS201", target.span, format!("Cannot mutate immutable `let` `{}`", name))
                                    .with_ar_message(format!("لا يمكن تعديل المتغير الثابت `{}`", name))
                                    .with_why("Declared with `let`.", "تم تعريفه بـ `let`.")
                                    .with_solution("Use `var` instead of `let`.", "استخدم `var` بدلاً من `let`."),
                            );
                        }
                    }
                }
            }
            Stmt::Expr { expr, .. } => {
                self.analyze_expr(expr);
            }
            Stmt::If { branches, else_branch, .. } => {
                for (cond, body) in branches {
                    self.analyze_expr(cond);
                    self.push_scope();
                    for s in body {
                        self.analyze_stmt(s);
                    }
                    self.pop_scope();
                }
                if let Some(eb) = else_branch {
                    self.push_scope();
                    for s in eb {
                        self.analyze_stmt(s);
                    }
                    self.pop_scope();
                }
            }
            Stmt::For { var, iter, body, span } => {
                self.analyze_expr(iter);
                self.push_scope();
                self.define(SymbolInfo {
                    name: var.clone(),
                    ty: Ty::Int,
                    mutable: false,
                    span: *span,
                    kind: SymbolKind::Variable,
                });
                for s in body {
                    self.analyze_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::While { cond, body, .. } => {
                self.analyze_expr(cond);
                self.push_scope();
                for s in body {
                    self.analyze_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::Loop { body, .. } => {
                self.push_scope();
                for s in body {
                    self.analyze_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::Match { subject, arms, .. } => {
                self.analyze_expr(subject);
                for arm in arms {
                    self.push_scope();
                    for s in &arm.body {
                        self.analyze_stmt(s);
                    }
                    self.pop_scope();
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    if let ExprKind::Ident(n) = &v.kind {
                        self.leak_guard.record_return(n);
                    }
                    self.analyze_expr(v);
                }
            }
            Stmt::UnsafeHardware { body, .. } => {
                let prev = self.in_unsafe_hw;
                self.in_unsafe_hw = true;
                self.push_scope();
                for s in body {
                    self.analyze_stmt(s);
                }
                self.pop_scope();
                self.in_unsafe_hw = prev;
            }
            Stmt::Defer { stmt, .. } => {
                if let Stmt::Expr { expr, .. } = stmt.as_ref() {
                    if let ExprKind::NamespaceCall { namespace, func, args, .. } = &expr.kind {
                        if let Some(spec) = find_builtin(namespace, func) {
                            if spec.is_free {
                                if let Some(ExprKind::Ident(var)) = args.first().map(|a| &a.kind) {
                                    self.leak_guard.record_free(var, expr.span, true);
                                    return;
                                }
                            }
                        }
                    }
                }
                self.analyze_stmt(stmt);
            }
            Stmt::TryCatch { try_body, err_var, catch_body, span } => {
                self.push_scope();
                for s in try_body {
                    self.analyze_stmt(s);
                }
                self.pop_scope();

                self.push_scope();
                self.define(SymbolInfo {
                    name: err_var.clone(),
                    ty: Ty::Str,
                    mutable: false,
                    span: *span,
                    kind: SymbolKind::Variable,
                });
                for s in catch_body {
                    self.analyze_stmt(s);
                }
                self.pop_scope();
            }
            Stmt::Throw { value, .. } => {
                self.analyze_expr(value);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }

    fn track_alloc_if_needed(&mut self, name: &str, name_span: Span, stmt_span: Span, value: &Expr) {
        match &value.kind {
            ExprKind::NamespaceCall { namespace, func, .. } => {
                if let Some(spec) = find_builtin(namespace, func) {
                    if spec.is_alloc {
                        self.leak_guard.record_alloc(name, name_span, stmt_span);
                    }
                }
            }
            ExprKind::Ident(src) => {
                self.leak_guard.record_alias(name, src);
            }
            _ => {}
        }
    }

    fn make_var_autofix(&self, name_span: Span) -> Option<AutoFix> {
        // Look backward from name_span on the same line for `let `
        let line_idx = name_span.line.saturating_sub(1);
        let line_str = self.source.lines().nth(line_idx)?;
        let before = &line_str[..name_span.col.saturating_sub(1).min(line_str.len())];
        if let Some(pos_in_line) = before.rfind("let") {
            let line_start_offset = self.source.lines().take(line_idx).map(|l| l.len() + 1).sum::<usize>();
            let abs_start = line_start_offset + pos_in_line;
            let sp = Span::new(abs_start, abs_start + 3, name_span.line, pos_in_line + 1, name_span.line, pos_in_line + 4);
            return Some(AutoFix {
                title: "Change `let` to `var`".into(),
                title_ar: "تغيير `let` إلى `var`".into(),
                edits: vec![TextEdit { span: sp, replacement: "var".into() }],
                is_preferred: true,
            });
        }
        None
    }

    fn analyze_expr(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::HexAddr(_) => Ty::Ptr,
            ExprKind::Float(_) => Ty::Float,
            ExprKind::Volt(_) => Ty::Volt,
            ExprKind::Freq(_) => Ty::Freq,
            ExprKind::Duration(_) => Ty::Duration,
            ExprKind::SampleRate(_) => Ty::SampleRate,
            ExprKind::Dbm(_) => Ty::Dbm,
            ExprKind::Angle(_) => Ty::Angle,
            ExprKind::Str(_) | ExprKind::FStr(_) => Ty::Str,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::NoneLit => Ty::Void,
            ExprKind::Ident(name) => {
                self.leak_guard.record_use(name, expr.span);
                if let Some(sym) = self.lookup(name) {
                    sym.ty.clone()
                } else {
                    self.report_undefined_var(name, expr.span);
                    Ty::Unknown
                }
            }
            ExprKind::NamespacePath { namespace, .. } => {
                if !NAMESPACES.contains(&namespace.as_str()) {
                    self.report_unknown_namespace(namespace, expr.span);
                }
                Ty::Unknown
            }
            ExprKind::NamespaceCall { namespace, func, args, ns_span, func_span } => {
                // First check arguments
                for a in args {
                    self.analyze_expr(a);
                }
                if !NAMESPACES.contains(&namespace.as_str()) {
                    self.report_unknown_namespace(namespace, *ns_span);
                    return Ty::Unknown;
                }
                let Some(spec) = find_builtin(namespace, func) else {
                    let members: Vec<&str> = builtins_in_namespace(namespace).into_iter().map(|b| b.name).collect();
                    let best = bayesian_best_match(func, &members);
                    let mut d = Diagnostic::error(
                        "RS203",
                        *func_span,
                        format!("Unknown function `{}::{}`", namespace, func),
                    )
                    .with_ar_message(format!("دالة غير موجودة `{}::{}`", namespace, func))
                    .with_why(
                        format!("Namespace `{}::` does not have member `{}`.", namespace, func),
                        format!("الوحدة `{}::` لا تحتوي على `{}`.", namespace, func),
                    );
                    if let Some(ranked) = best {
                        d = d
                            .with_math(
                                format!("Bayesian posterior P({}|{})={:.1}%, edit_d={}, cos={:.2}", ranked.candidate, func, ranked.posterior_prob * 100.0, ranked.edit_dist, ranked.cosine_sim),
                                ranked.posterior_prob,
                            )
                            .with_solution(
                                format!("Replace with `{}::{}`.", namespace, ranked.candidate),
                                format!("استبدلها بـ `{}::{}`.", namespace, ranked.candidate),
                            )
                            .with_fix(AutoFix {
                                title: format!("Replace with `{}`", ranked.candidate),
                                title_ar: format!("استبدال بـ `{}`", ranked.candidate),
                                edits: vec![TextEdit { span: *func_span, replacement: ranked.candidate.to_string() }],
                                is_preferred: true,
                            });
                    } else {
                        d = d.with_solution(
                            format!("Available functions in `{}::`: {}", namespace, members.join(", ")),
                            format!("الدوال المتاحة في `{}::`: {}", namespace, members.join(", ")),
                        );
                    }
                    self.diagnostics.push(d);
                    return Ty::Unknown;
                };

                // Leak guard free tracking
                if spec.is_free {
                    if let Some(ExprKind::Ident(var)) = args.first().map(|a| &a.kind) {
                        self.leak_guard.record_free(var, expr.span, false);
                    }
                }

                // unsafe_hardware check
                if spec.requires_unsafe_hw && !self.in_unsafe_hw && !self.in_kernel_fn {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "HW301",
                            expr.span,
                            format!("`{}::{}` accesses raw hardware and requires an `unsafe_hardware:` block", namespace, func),
                        )
                        .with_severity(Severity::HardwareSafety)
                        .with_ar_message(format!("الدالة `{}::{}` تتطلب كتلة `unsafe_hardware:` لأنها تصل للعتاد مباشرة", namespace, func))
                        .with_why("Direct MMIO, I/O ports, MSR, or interrupt control must be explicitly isolated.", "الوصول المباشر لمنافذ وسجلات العتاد يجب عزله لحماية النظام.")
                        .with_solution("Wrap inside `unsafe_hardware:` or annotate `@kernel`.", "ضعها داخل `unsafe_hardware:` أو أضف `@kernel`."),
                    );
                }

                // Hardware safety checks: voltage, fan speed, W^X, timing attacks, SDR Nyquist
                self.check_hardware_and_security_rules(namespace, func, args, expr.span);

                match spec.ret {
                    ReturnKind::Void => Ty::Void,
                    ReturnKind::Int => Ty::Int,
                    ReturnKind::Float => Ty::Float,
                    ReturnKind::Bool => Ty::Bool,
                    ReturnKind::Str => Ty::Str,
                    ReturnKind::Ptr => Ty::Ptr,
                    ReturnKind::List => Ty::List,
                    ReturnKind::Map => Ty::Map,
                    ReturnKind::Any => Ty::Unknown,
                }
            }
            ExprKind::Call { callee, args } => {
                for a in args {
                    self.analyze_expr(a);
                }
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some((_, ret)) = self.functions.get(name) {
                        return ret.clone();
                    }
                    if !self.externs.contains(name) && !self.algorithms.contains(name) && !self.equations.contains_key(name) {
                        self.report_undefined_var(name, callee.span);
                    }
                } else {
                    self.analyze_expr(callee);
                }
                Ty::Unknown
            }
            ExprKind::MethodCall { receiver, method, args } => {
                let rty = self.analyze_expr(receiver);
                for a in args {
                    self.analyze_expr(a);
                }
                match method.as_str() {
                    "len" | "char_code_at" | "find" | "to_int" => Ty::Int,
                    "starts_with" | "ends_with" | "contains" | "has" | "is_empty" => Ty::Bool,
                    "slice" | "trim" | "replace" | "char_at" | "upper" | "lower" => Ty::Str,
                    "split" | "keys" => Ty::List,
                    "push" | "set" | "clear" => Ty::Void,
                    _ => rty,
                }
            }
            ExprKind::Field { receiver, .. } => {
                self.analyze_expr(receiver);
                Ty::Unknown
            }
            ExprKind::Index { receiver, index } => {
                self.analyze_expr(receiver);
                self.analyze_expr(index);
                Ty::Unknown
            }
            ExprKind::List(items) => {
                for it in items {
                    self.analyze_expr(it);
                }
                Ty::List
            }
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.analyze_expr(k);
                    self.analyze_expr(v);
                }
                Ty::Map
            }
            ExprKind::StructLit { name, fields } => {
                for (_, v) in fields {
                    self.analyze_expr(v);
                }
                Ty::Named(name.clone())
            }
            ExprKind::Binary { left, op, right } => {
                let lt = self.analyze_expr(left);
                let rt = self.analyze_expr(right);
                if matches!(op, BinOp::Div | BinOp::Mod) {
                    if let Some(iv) = self.eval_interval(right) {
                        if iv.is_strictly_zero() {
                            self.diagnostics.push(
                                Diagnostic::error("MA401", right.span, "Mathematical division by zero proved via Interval Arithmetic")
                                    .with_ar_message("قسمة على صفر مثبتة رياضياً عبر حساب المجالات")
                                    .with_why("The divisor interval collapsed to [0, 0].", "قيمة المقسوم عليه تقع في المجال [0, 0].")
                                    .with_math(format!("Interval Evaluation: divisor in [{}, {}]", iv.lo, iv.hi), 1.0)
                                    .with_solution("Ensure the divisor is non-zero before division.", "تأكد من أن المقسوم عليه لا يساوي صفر."),
                            );
                        }
                    }
                }
                if matches!(op, BinOp::Eq | BinOp::NotEq) {
                    self.check_timing_side_channel(left, right, expr.span);
                    return Ty::Bool;
                }
                if matches!(op, BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq | BinOp::And | BinOp::Or) {
                    return Ty::Bool;
                }
                if matches!(lt, Ty::Float) || matches!(rt, Ty::Float) {
                    Ty::Float
                } else {
                    lt
                }
            }
            ExprKind::Unary { op, operand } => {
                let t = self.analyze_expr(operand);
                if matches!(op, UnaryOp::Not) { Ty::Bool } else { t }
            }
            ExprKind::Range { start, end, .. } => {
                self.analyze_expr(start);
                self.analyze_expr(end);
                Ty::List
            }
            ExprKind::Cast { expr: inner, ty } => {
                self.analyze_expr(inner);
                self.resolve_type_opt(Some(ty))
            }
        }
    }

    fn eval_interval(&self, expr: &Expr) -> Option<Interval> {
        match &expr.kind {
            ExprKind::Int(v) => Some(Interval::point(*v as f64)),
            ExprKind::Float(v) => Some(Interval::point(*v)),
            ExprKind::Volt(mv) => Some(Interval::point(*mv)),
            ExprKind::Freq(hz) => Some(Interval::point(*hz)),
            ExprKind::SampleRate(sr) => Some(Interval::point(*sr)),
            ExprKind::Ident(n) => self.intervals.get(n).copied(),
            ExprKind::Binary { left, op, right } => {
                let a = self.eval_interval(left)?;
                let b = self.eval_interval(right)?;
                match op {
                    BinOp::Add => Some(a.add(b)),
                    BinOp::Sub => Some(a.sub(b)),
                    BinOp::Mul => Some(a.mul(b)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn check_hardware_and_security_rules(&mut self, ns: &str, func: &str, args: &[Expr], span: Span) {
        if ns == "power" && func == "set_voltage" && args.len() >= 2 {
            if let Some(iv) = self.eval_interval(&args[1]) {
                if iv.hi > 1550.0 || iv.lo < 500.0 {
                    let safe_mv = iv.hi.clamp(500.0, 1550.0);
                    let fix_text = format!("{:.2}V", safe_mv / 1000.0);
                    self.diagnostics.push(
                        Diagnostic::error(
                            "HW302",
                            args[1].span,
                            format!("Unsafe core voltage [{:.0}mV, {:.0}mV] outside hardware envelope [500mV, 1550mV]", iv.lo, iv.hi),
                        )
                        .with_severity(Severity::HardwareSafety)
                        .with_ar_message(format!("جهد فولت خطير ({:.0}mV) خارج الحد الآمن [500mV..1550mV]", iv.hi))
                        .with_why("Over-volting permanently damages silicon MOSFETs and VRM phases.", "الجهد المرتفع يحرق المعالج ودوائر الطاقة.")
                        .with_math(format!("Interval Proof: V in [{:.1}, {:.1}] mV violates [500, 1550] mV", iv.lo, iv.hi), 1.0)
                        .with_solution(format!("Clamp voltage to safe range (e.g. {}).", fix_text), format!("اضبط الفولت ضمن النطاق الآمن مثل {}.", fix_text))
                        .with_fix(AutoFix {
                            title: format!("Clamp voltage to {}", fix_text),
                            title_ar: format!("تخفيض الجهد إلى {}", fix_text),
                            edits: vec![TextEdit { span: args[1].span, replacement: fix_text }],
                            is_preferred: true,
                        }),
                    );
                }
            }
        }

        if ns == "hardware" && func == "fan_speed" && !args.is_empty() {
            if let Some(iv) = self.eval_interval(&args[0]) {
                if iv.lo < 0.0 || iv.hi > 100.0 {
                    let clamped = (iv.hi as i64).clamp(0, 100);
                    self.diagnostics.push(
                        Diagnostic::error("HW303", args[0].span, format!("Fan PWM duty {:.0}% out of [0, 100]%", iv.hi))
                            .with_severity(Severity::HardwareSafety)
                            .with_ar_message("سرعة المروحة يجب أن تكون بين 0 و 100")
                            .with_why("PWM duty percentage cannot exceed 100% or be negative.", "نسبة نبضة المروحة محصورة بين 0 و 100.")
                            .with_solution(format!("Clamp to {}%.", clamped), format!("اضبط السرعة على {}.", clamped))
                            .with_fix(AutoFix {
                                title: format!("Clamp fan speed to {}", clamped),
                                title_ar: format!("ضبط السرعة إلى {}", clamped),
                                edits: vec![TextEdit { span: args[0].span, replacement: clamped.to_string() }],
                                is_preferred: true,
                            }),
                    );
                }
            }
        }

        if ns == "memory" && func == "protect" && args.len() >= 3 {
            if let ExprKind::Str(perms) = &args[2].kind {
                let p = perms.to_ascii_uppercase();
                if p.contains('W') && p.contains('X') {
                    self.diagnostics.push(
                        Diagnostic::error("SEC401", args[2].span, "W^X violation: memory page cannot be both Writable and Executable (`RWX`)")
                            .with_severity(Severity::SecurityAlert)
                            .with_ar_message("ثغرة أمنية W^X: لا يجوز أن تكون الصفحة قابلة للكتابة والتنفيذ معاً")
                            .with_why("RWX pages enable shellcode injection and JIT spraying.", "الصفحات القابلة للكتابة والتنفيذ تسمح بحقن الشيفرات الخبيثة.")
                            .with_solution("Use `RX` for code or `RW` for data.", "استخدم `RX` للكود أو `RW` للبيانات.")
                            .with_fix(AutoFix {
                                title: "Harden to `RX` (W^X compliant)".into(),
                                title_ar: "تحويل الصلاحية إلى `RX`".into(),
                                edits: vec![TextEdit { span: args[2].span, replacement: "\"RX\"".into() }],
                                is_preferred: true,
                            }),
                    );
                }
            }
        }

        if ns == "sdr" && func == "set_sample_rate" && args.len() >= 2 {
            if let Some(iv) = self.eval_interval(&args[1]) {
                self.current_sdr_sample_rate = Some(iv.hi);
                if iv.lo <= 0.0 {
                    self.diagnostics.push(
                        Diagnostic::error("SDR501", args[1].span, "SDR sample rate must be > 0 sps")
                            .with_severity(Severity::HardwareSafety)
                            .with_ar_message("معدل عينات الراديو SDR يجب أن يكون أكبر من 0")
                            .with_why("Nyquist theorem requires positive sampling frequency Fs > 0.", "مبرهنة نايكوست تتطلب تردد عينات موجباً.")
                            .with_solution("Set sample rate such as `2.4Msps`.", "استخدم معدل عينات صالح مثل `2.4Msps`."),
                    );
                }
            }
        }

        if ns == "sdr" && func == "set_bandwidth" && args.len() >= 2 {
            if let (Some(fs), Some(bw)) = (self.current_sdr_sample_rate, self.eval_interval(&args[1])) {
                if bw.hi > fs {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "SDR502",
                            span,
                            format!("Nyquist Aliasing Warning: SDR bandwidth ({:.0} Hz) exceeds sample rate ({:.0} sps)", bw.hi, fs),
                        )
                        .with_severity(Severity::Warning)
                        .with_ar_message("تحذير تداخل نايكوست: عرض النطاق أكبر من معدل العينات")
                        .with_why("When BW > Fs, high-frequency signals fold into baseband aliasing.", "عندما يتجاوز عرض النطاق معدل العينات يحدث تشوه تداخلي للإشارة.")
                        .with_math(format!("Nyquist Condition: BW ({:.0}) <= Fs ({:.0}) violated", bw.hi, fs), 0.99)
                        .with_solution("Lower bandwidth <= sample rate or raise sample rate.", "خفّض عرض النطاق أو ارفع معدل العينات."),
                    );
                }
            }
        }
    }

    fn check_timing_side_channel(&mut self, left: &Expr, right: &Expr, span: Span) {
        let is_secret = |e: &Expr| -> bool {
            if let ExprKind::Ident(n) = &e.kind {
                let l = n.to_ascii_lowercase();
                return l.contains("secret") || l.contains("token") || l.contains("password") || l.contains("hmac") || l.contains("key");
            }
            false
        };
        if is_secret(left) && (matches!(left.kind, ExprKind::Ident(_)) && matches!(right.kind, ExprKind::Ident(_) | ExprKind::Str(_))) {
            self.diagnostics.push(
                Diagnostic::error("SEC402", span, "Potential timing side-channel comparing sensitive variable with `==`")
                    .with_severity(Severity::SecurityAlert)
                    .with_ar_message("مقارنة متغير أمني بـ `==` تعرضه لهجوم التوقيت")
                    .with_why("Standard `==` short-circuits on the first mismatched byte, leaking prefix length via execution time.", "المقارنة العادية تتوقف عند أول حرف مختلف مما يكشف طول الجزء الصحيح.")
                    .with_solution("Use `security::constant_time_eq(a, b)`.", "استخدم `security::constant_time_eq(a, b)`.")
                    .with_fix(AutoFix {
                        title: "Replace with `security::constant_time_eq`".into(),
                        title_ar: "استخدام المقارنة الثابتة الزمن `security::constant_time_eq`".into(),
                        edits: vec![TextEdit {
                            span,
                            replacement: format!(
                                "security::constant_time_eq({}, {})",
                                snippet_from_span(self.source, left.span),
                                snippet_from_span(self.source, right.span)
                            ),
                        }],
                        is_preferred: true,
                    }),
            );
        }
    }

    fn report_undefined_var(&mut self, name: &str, span: Span) {
        let visible = self.all_visible_names();
        let refs: Vec<&str> = visible.iter().map(|s| s.as_str()).collect();
        let mut d = Diagnostic::error("RS202", span, format!("Undefined identifier `{}`", name))
            .with_ar_message(format!("المعرف `{}` غير معرف", name))
            .with_why("Variable or function was not declared in any visible scope.", "لم يتم تعريف هذا الاسم في النطاق الحالي.");

        if let Some(ranked) = bayesian_best_match(name, &refs) {
            d = d
                .with_math(
                    format!("Bayesian posterior P({}|{})={:.1}%, edit_d={}, cos={:.2}", ranked.candidate, name, ranked.posterior_prob * 100.0, ranked.edit_dist, ranked.cosine_sim),
                    ranked.posterior_prob,
                )
                .with_solution(format!("Did you mean `{}`?", ranked.candidate), format!("هل تقصد `{}`؟", ranked.candidate))
                .with_fix(AutoFix {
                    title: format!("Replace `{}` with `{}`", name, ranked.candidate),
                    title_ar: format!("استبدال `{}` بـ `{}`", name, ranked.candidate),
                    edits: vec![TextEdit { span, replacement: ranked.candidate.to_string() }],
                    is_preferred: true,
                });
        } else {
            d = d.with_solution(format!("Declare `let {} = ...` or `var {} = ...`.", name, name), format!("عرّف المتغير بـ `let {} = ...`.", name));
        }
        self.diagnostics.push(d);
    }

    fn report_unknown_namespace(&mut self, ns: &str, span: Span) {
        let mut d = Diagnostic::error("RS204", span, format!("Unknown namespace `{}::`", ns))
            .with_ar_message(format!("وحدة غير معروفة `{}::`", ns))
            .with_why("Not a built-in hardware, SDR, math, synth, or system namespace.", "هذه الوحدة غير موجودة في المكتبة القياسية.");
        if let Some(ranked) = bayesian_best_match(ns, NAMESPACES) {
            d = d
                .with_math(
                    format!("Bayesian posterior P({}|{})={:.1}%, edit_d={}, cos={:.2}", ranked.candidate, ns, ranked.posterior_prob * 100.0, ranked.edit_dist, ranked.cosine_sim),
                    ranked.posterior_prob,
                )
                .with_solution(format!("Replace with `{}::`.", ranked.candidate), format!("استبدلها بـ `{}::`.", ranked.candidate))
                .with_fix(AutoFix {
                    title: format!("Replace with `{}`", ranked.candidate),
                    title_ar: format!("استبدال بـ `{}`", ranked.candidate),
                    edits: vec![TextEdit { span, replacement: ranked.candidate.to_string() }],
                    is_preferred: true,
                });
        }
        self.diagnostics.push(d);
    }
}

fn snippet_from_span(src: &str, sp: Span) -> &str {
    src.get(sp.start..sp.end).unwrap_or("")
}
