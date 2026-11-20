use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostic::{closest_match, AutoFix, Diagnostic, Severity, TextEdit};
use crate::span::Span;
use crate::stdlib_spec::{find_builtin, find_builtin_const, functions_in_namespace, SafetyClass, NAMESPACES};

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Float,
    Volt,
    Freq,
    Duration,
    Addr,
    Bool,
    Str,
    List,
    Map,
    Void,
    Any,
    Struct(String),
    Enum(String),
}

impl Type {
    pub fn from_name(name: &str) -> Self {
        match name {
            "int" | "i64" | "i32" | "u8" | "u16" | "u32" | "u64" => Type::Int,
            "float" | "f64" | "f32" => Type::Float,
            "volt" => Type::Volt,
            "freq" => Type::Freq,
            "time" | "duration" => Type::Duration,
            "addr" | "ptr" => Type::Addr,
            "bool" => Type::Bool,
            "str" | "string" | "bytes" => Type::Str,
            "list" => Type::List,
            "map" => Type::Map,
            "void" => Type::Void,
            _ => Type::Any,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Type::Int => "int",
            Type::Float => "float",
            Type::Volt => "volt",
            Type::Freq => "freq",
            Type::Duration => "time",
            Type::Addr => "addr",
            Type::Bool => "bool",
            Type::Str => "str",
            Type::List => "list",
            Type::Map => "map",
            Type::Void => "void",
            Type::Any => "any",
            Type::Struct(_) => "struct",
            Type::Enum(_) => "enum",
        }
    }
}

#[derive(Debug, Clone)]
struct VarInfo {
    ty: Type,
    mutable: bool,
    decl_span: Span,
    keyword_span: Span, // covers 'let' or 'var'
    is_allocated_ptr: bool,
    freed: bool,
    returned: bool,
}

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: &'static str,
    pub span: Span,
    pub detail: String,
}

pub struct AnalysisResult {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<SymbolInfo>,
}

pub fn analyze(program: &Program, source: &str) -> AnalysisResult {
    let mut analyzer = Analyzer {
        source,
        diagnostics: Vec::new(),
        symbols: Vec::new(),
        fns: HashMap::new(),
        structs: HashMap::new(),
        enums: HashMap::new(),
        scopes: vec![HashMap::new()],
        in_unsafe_hardware: false,
    };
    analyzer.collect_top_level(program);
    analyzer.check_program(program);
    AnalysisResult {
        diagnostics: analyzer.diagnostics,
        symbols: analyzer.symbols,
    }
}

struct Analyzer<'a> {
    source: &'a str,
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<SymbolInfo>,
    fns: HashMap<String, (Vec<(String, Type)>, Type, Span)>,
    structs: HashMap<String, Vec<(String, Type)>>,
    enums: HashMap<String, Vec<String>>,
    scopes: Vec<HashMap<String, VarInfo>>,
    in_unsafe_hardware: bool,
}

impl<'a> Analyzer<'a> {
    fn collect_top_level(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Fn(f) => {
                    let params: Vec<(String, Type)> = f
                        .params
                        .iter()
                        .map(|p| {
                            let ty = p.ty.as_ref().map(|t| Type::from_name(&t.name)).unwrap_or(Type::Any);
                            (p.name.clone(), ty)
                        })
                        .collect();
                    let ret = f
                        .return_type
                        .as_ref()
                        .map(|t| Type::from_name(&t.name))
                        .unwrap_or(Type::Any);
                    self.fns.insert(f.name.clone(), (params, ret, f.name_span));
                    self.symbols.push(SymbolInfo {
                        name: f.name.clone(),
                        kind: "function",
                        span: f.name_span,
                        detail: format!("fn {}({})", f.name, f.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")),
                    });
                }
                Item::ExternFn(ef) => {
                    let params: Vec<(String, Type)> = ef
                        .params
                        .iter()
                        .map(|p| {
                            let ty = p.ty.as_ref().map(|t| Type::from_name(&t.name)).unwrap_or(Type::Any);
                            (p.name.clone(), ty)
                        })
                        .collect();
                    let ret = ef
                        .return_type
                        .as_ref()
                        .map(|t| Type::from_name(&t.name))
                        .unwrap_or(Type::Any);
                    self.fns.insert(ef.name.clone(), (params, ret, ef.span));
                    self.symbols.push(SymbolInfo {
                        name: ef.name.clone(),
                        kind: "extern_fn",
                        span: ef.span,
                        detail: format!("extern \"{}\" fn {}", ef.abi, ef.name),
                    });
                }
                Item::Struct(s) => {
                    let fields = s
                        .fields
                        .iter()
                        .map(|fl| (fl.name.clone(), Type::from_name(&fl.ty.name)))
                        .collect();
                    self.structs.insert(s.name.clone(), fields);
                    self.symbols.push(SymbolInfo {
                        name: s.name.clone(),
                        kind: "struct",
                        span: s.span,
                        detail: format!("struct {}", s.name),
                    });
                }
                Item::Enum(e) => {
                    self.enums.insert(e.name.clone(), e.variants.clone());
                    self.symbols.push(SymbolInfo {
                        name: e.name.clone(),
                        kind: "enum",
                        span: e.span,
                        detail: format!("enum {}", e.name),
                    });
                }
                Item::Const(c) => {
                    let ty = c.ty.as_ref().map(|t| Type::from_name(&t.name)).unwrap_or(Type::Any);
                    self.define_var(c.name.clone(), ty, false, c.span, c.span, false);
                }
                Item::Import(_) | Item::Stmt(_) => {}
            }
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope_and_check_leaks(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for (name, info) in scope {
                if info.is_allocated_ptr && !info.freed && !info.returned {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "NB305",
                            info.decl_span,
                            format!("Potential memory leak: allocated pointer `{}` is never freed", name),
                        )
                        .with_severity(Severity::Warning)
                        .with_ar_message(format!("احتمال تسرب ذاكرة: المؤشر المحجوز `{}` لم يتم تحريره", name))
                        .with_why(
                            "Memory allocated via `memory::alloc` stays reserved until `memory::free(...)` is called.",
                            "الذاكرة المحجوزة عبر `memory::alloc` تبقى محجوزة حتى يتم تحريرها بـ `memory::free(...)`.",
                        )
                        .with_solution(
                            format!("Call `memory::free({})` before exiting this scope.", name),
                            format!("استدعِ `memory::free({})` قبل نهاية الدالة أو الكتلة.", name),
                        ),
                    );
                }
            }
        }
    }

    fn define_var(
        &mut self,
        name: String,
        ty: Type,
        mutable: bool,
        decl_span: Span,
        keyword_span: Span,
        is_allocated_ptr: bool,
    ) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name,
                VarInfo {
                    ty,
                    mutable,
                    decl_span,
                    keyword_span,
                    is_allocated_ptr,
                    freed: false,
                    returned: false,
                },
            );
        }
    }

    fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v);
            }
        }
        None
    }

    fn mark_var_freed(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(v) = scope.get_mut(name) {
                v.freed = true;
                return;
            }
        }
    }

    fn mark_var_returned(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(v) = scope.get_mut(name) {
                v.returned = true;
                return;
            }
        }
    }

    fn all_visible_var_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        for scope in self.scopes.iter().rev() {
            for k in scope.keys() {
                out.push(k.clone());
            }
        }
        out
    }

    fn check_program(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Fn(f) => self.check_fn(f),
                Item::Stmt(s) => self.check_stmt(s),
                Item::Const(c) => {
                    self.check_expr(&c.value);
                }
                _ => {}
            }
        }
    }

    fn check_fn(&mut self, f: &FnDef) {
        self.push_scope();
        for p in &f.params {
            let ty = p.ty.as_ref().map(|t| Type::from_name(&t.name)).unwrap_or(Type::Any);
            self.define_var(p.name.clone(), ty, true, p.span, p.span, false);
        }
        for st in &f.body {
            self.check_stmt(st);
        }
        self.pop_scope_and_check_leaks();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, name_span, ty, value, span } => {
                let inferred = self.check_expr(value);
                let final_ty = ty.as_ref().map(|t| Type::from_name(&t.name)).unwrap_or(inferred);
                let is_alloc = is_alloc_call(value);
                let kw_span = Span::new(span.start, span.start + 3, span.line, span.col, span.line, span.col + 3);
                self.define_var(name.clone(), final_ty, false, *name_span, kw_span, is_alloc);
                self.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: "variable",
                    span: *name_span,
                    detail: format!("let {}", name),
                });
            }
            Stmt::Var { name, name_span, ty, value, span } => {
                let inferred = self.check_expr(value);
                let final_ty = ty.as_ref().map(|t| Type::from_name(&t.name)).unwrap_or(inferred);
                let is_alloc = is_alloc_call(value);
                let kw_span = Span::new(span.start, span.start + 3, span.line, span.col, span.line, span.col + 3);
                self.define_var(name.clone(), final_ty, true, *name_span, kw_span, is_alloc);
                self.symbols.push(SymbolInfo {
                    name: name.clone(),
                    kind: "variable",
                    span: *name_span,
                    detail: format!("var {}", name),
                });
            }
            Stmt::Assign { target, value, span } => {
                self.check_expr(value);
                self.check_mutability(target, *span);
            }
            Stmt::CompoundAssign { target, value, span, .. } => {
                self.check_expr(value);
                self.check_mutability(target, *span);
            }
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::If { branches, else_branch, .. } => {
                for (cond, body) in branches {
                    self.check_expr(cond);
                    self.push_scope();
                    for s in body {
                        self.check_stmt(s);
                    }
                    self.pop_scope_and_check_leaks();
                }
                if let Some(eb) = else_branch {
                    self.push_scope();
                    for s in eb {
                        self.check_stmt(s);
                    }
                    self.pop_scope_and_check_leaks();
                }
            }
            Stmt::For { var, iter, body, span } => {
                self.check_expr(iter);
                self.push_scope();
                self.define_var(var.clone(), Type::Int, false, *span, *span, false);
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope_and_check_leaks();
            }
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond);
                self.push_scope();
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope_and_check_leaks();
            }
            Stmt::Loop { body, .. } => {
                self.push_scope();
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope_and_check_leaks();
            }
            Stmt::Match { subject, arms, .. } => {
                self.check_expr(subject);
                for arm in arms {
                    self.push_scope();
                    for s in &arm.body {
                        self.check_stmt(s);
                    }
                    self.pop_scope_and_check_leaks();
                }
            }
            Stmt::Return { value: Some(v), .. } => {
                if let ExprKind::Ident(name) = &v.kind {
                    self.mark_var_returned(name);
                }
                self.check_expr(v);
            }
            Stmt::Return { value: None, .. } | Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::UnsafeHardware { body, .. } => {
                let prev = self.in_unsafe_hardware;
                self.in_unsafe_hardware = true;
                self.push_scope();
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope_and_check_leaks();
                self.in_unsafe_hardware = prev;
            }
        }
    }

    fn check_mutability(&mut self, target: &Expr, span: Span) {
        if let ExprKind::Ident(name) = &target.kind {
            if let Some(info) = self.lookup_var(name).cloned() {
                if !info.mutable {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "NB201",
                            span,
                            format!("Cannot reassign immutable variable `{}` declared with `let`", name),
                        )
                        .with_ar_message(format!("لا يمكن تعديل المتغير الثابت `{}` المعرف بـ `let`", name))
                        .with_why(
                            "Variables declared with `let` are read-only by default to prevent accidental state corruption.",
                            "المتغيرات المعرفة بـ `let` ثابتة افتراضياً لحماية الكود من التعديلات غير المقصودة.",
                        )
                        .with_solution(
                            format!("Change `let {}` to `var {}` where it is declared.", name, name),
                            format!("غيّر `let {}` إلى `var {}` عند تعريفه.", name, name),
                        )
                        .with_fix(AutoFix {
                            title: format!("Change `let {}` to `var {}`", name, name),
                            title_ar: format!("تحويل `let {}` إلى `var {}`", name, name),
                            edits: vec![TextEdit {
                                span: info.keyword_span,
                                replacement: "var".to_string(),
                            }],
                            is_preferred: true,
                        }),
                    );
                }
            } else {
                self.report_undefined_ident(name, target.span);
            }
        } else {
            self.check_expr(target);
        }
    }

    fn report_undefined_ident(&mut self, name: &str, span: Span) {
        let visible = self.all_visible_var_names();
        let mut refs: Vec<&str> = visible.iter().map(|s| s.as_str()).collect();
        for fname in self.fns.keys() {
            refs.push(fname.as_str());
        }
        let suggestion = closest_match(name, &refs, 2);
        let mut diag = Diagnostic::error(
            "NB202",
            span,
            format!("Undefined identifier `{}`", name),
        )
        .with_ar_message(format!("المتغير أو الدالة `{}` غير معرّفة", name))
        .with_why(
            format!("`{}` was referenced without being declared with `let`, `var`, or `fn`.", name),
            format!("تم استخدام `{}` قبل تعريفه باستخدام `let` أو `var` أو `fn`.", name),
        );

        if let Some(sug) = suggestion {
            diag = diag
                .with_solution(
                    format!("Did you mean `{}`?", sug),
                    format!("هل تقصد `{}`؟", sug),
                )
                .with_fix(AutoFix {
                    title: format!("Replace `{}` with `{}`", name, sug),
                    title_ar: format!("استبدال `{}` بـ `{}`", name, sug),
                    edits: vec![TextEdit {
                        span,
                        replacement: sug.to_string(),
                    }],
                    is_preferred: true,
                });
        } else {
            diag = diag.with_solution(
                format!("Declare it first: `let {} = ...`", name),
                format!("قم بتعريفه أولاً: `let {} = ...`", name),
            );
        }
        self.diagnostics.push(diag);
    }

    fn check_expr(&mut self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::Int(_) => Type::Int,
            ExprKind::HexAddr(_) => Type::Addr,
            ExprKind::Float(_) => Type::Float,
            ExprKind::Volt(_) => Type::Volt,
            ExprKind::Freq(_) => Type::Freq,
            ExprKind::Duration(_) => Type::Duration,
            ExprKind::Str(_) => Type::Str,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::NoneLit => Type::Void,
            ExprKind::Ident(name) => {
                if name == "_" {
                    return Type::Any;
                }
                if let Some(v) = self.lookup_var(name) {
                    v.ty.clone()
                } else if self.fns.contains_key(name) || self.enums.contains_key(name) || self.structs.contains_key(name) {
                    Type::Any
                } else {
                    self.report_undefined_ident(name, expr.span);
                    Type::Any
                }
            }
            ExprKind::NamespacePath { namespace, member, ns_span, member_span } => {
                if self.enums.contains_key(namespace) {
                    return Type::Enum(namespace.clone());
                }
                if !NAMESPACES.contains(&namespace.as_str()) {
                    self.report_unknown_namespace(namespace, *ns_span);
                } else if find_builtin_const(namespace, member).is_none() && find_builtin(namespace, member).is_none() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "NB203",
                            *member_span,
                            format!("Unknown member `{}` in `{}`", member, namespace),
                        )
                        .with_ar_message(format!("عنصر غير معروف `{}` داخل الوحدة `{}`", member, namespace))
                        .with_why("The constant or function name does not exist in this built-in module.", "هذا الاسم غير موجود داخل هذه الوحدة القياسية.")
                        .with_solution("Check autocomplete suggestions after `::`.", "تحقق من الاقتراحات الذكية بعد `::`."),
                    );
                }
                Type::Any
            }
            ExprKind::NamespaceCall { namespace, func, args, ns_span, func_span } => {
                for a in args {
                    self.check_expr(a);
                }
                if !NAMESPACES.contains(&namespace.as_str()) {
                    self.report_unknown_namespace(namespace, *ns_span);
                    return Type::Any;
                }
                if let Some(spec) = find_builtin(namespace, func) {
                    // Check arity
                    if args.len() != spec.params.len() {
                        let expected_sig = spec
                            .params
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t))
                            .collect::<Vec<_>>()
                            .join(", ");
                        self.diagnostics.push(
                            Diagnostic::error(
                                "NB204",
                                expr.span,
                                format!(
                                    "`{}::{}` expects {} argument(s) ({}), got {}",
                                    namespace, func, spec.params.len(), expected_sig, args.len()
                                ),
                            )
                            .with_ar_message(format!(
                                "الدالة `{}::{}` تتطلب {} معامل ({}) ولكن تم تمرير {}",
                                namespace, func, spec.params.len(), expected_sig, args.len()
                            ))
                            .with_why(
                                "Passing fewer or extra arguments breaks the call contract.",
                                "تمرير عدد خاطئ من المعاملات يسبب فشلاً في استدعاء الدالة.",
                            )
                            .with_solution(
                                format!("Example: `{}`", spec.example),
                                format!("مثال صحيح: `{}`", spec.example),
                            ),
                        );
                    }

                    // Track memory::free
                    if namespace == "memory" && func == "free" {
                        if let Some(first) = args.first() {
                            if let ExprKind::Ident(ptr_name) = &first.kind {
                                self.mark_var_freed(ptr_name);
                            }
                        }
                    }

                    // Check unsafe_hardware requirement
                    if spec.safety == SafetyClass::RequiresUnsafeHardware && !self.in_unsafe_hardware {
                        let line_indent = self.indent_of_line(expr.span.line);
                        let line_span = self.line_span(expr.span.line);
                        let orig_line = self.line_text(expr.span.line);
                        let wrapped = format!(
                            "{}unsafe_hardware:\n{}    {}",
                            line_indent,
                            line_indent,
                            orig_line.trim_start()
                        );
                        self.diagnostics.push(
                            Diagnostic::error(
                                "NB401",
                                expr.span,
                                format!(
                                    "`{}::{}` performs privileged hardware/kernel I/O and requires an `unsafe_hardware:` block",
                                    namespace, func
                                ),
                            )
                            .with_severity(Severity::HardwareSafety)
                            .with_ar_message(format!(
                                "الدالة `{}::{}` تتحكم مباشرة في عتاد حساس وتحتاج وضعها داخل `unsafe_hardware:`",
                                namespace, func
                            ))
                            .with_why(
                                "Direct MSR writes, I/O ports, GDT/IDT setup, and rail voltage pulses can crash or damage hardware if executed accidentally.",
                                "الكتابة المباشرة في مسجلات المعالج أو منافذ العتاد أو نبضات الجهد قد تعطل النظام إذا تمت بدون تصريح صريح.",
                            )
                            .with_solution(
                                "Wrap this call inside an `unsafe_hardware:` block.",
                                "ضع هذا السطر داخل كتلة `unsafe_hardware:`.",
                            )
                            .with_fix(AutoFix {
                                title: "Wrap in `unsafe_hardware:` block".to_string(),
                                title_ar: "تغليف داخل كتلة `unsafe_hardware:`".to_string(),
                                edits: vec![TextEdit {
                                    span: line_span,
                                    replacement: wrapped,
                                }],
                                is_preferred: true,
                            }),
                        );
                    }

                    // Hardware voltage guardrail: power::set_voltage(rail, volt)
                    if namespace == "power" && func == "set_voltage" && args.len() == 2 {
                        if let ExprKind::Volt(mv) = args[1].kind {
                            let max_safe_mv = match &args[0].kind {
                                ExprKind::Str(r) if r == "GPU_CORE" => 1200.0,
                                ExprKind::Str(r) if r == "VDDQ" => 1400.0,
                                _ => 1450.0,
                            };
                            if (mv > max_safe_mv || mv < 500.0) && !self.in_unsafe_hardware {
                                let safe_v = (max_safe_mv - 100.0) / 1000.0;
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        "NB402",
                                        args[1].span,
                                        format!(
                                            "Dangerous voltage `{:.3}V` exceeds safe silicon limit ({:.2}V max)",
                                            mv / 1000.0,
                                            max_safe_mv / 1000.0
                                        ),
                                    )
                                    .with_severity(Severity::HardwareSafety)
                                    .with_ar_message(format!(
                                        "الجهد `{:.3}V` يتجاوز الحد الآمن للعتاد (الحد الأقصى {:.2}V)",
                                        mv / 1000.0,
                                        max_safe_mv / 1000.0
                                    ))
                                    .with_why(
                                        "Over-volting above the rated VRM/silicon envelope risks permanent thermal or gate-oxide damage.",
                                        "رفع الفولتية فوق الحد المصمم قد يؤدي إلى تلف المعالج أو دوائر الطاقة حرارياً.",
                                    )
                                    .with_solution(
                                        format!("Clamp voltage to `{:.2}V` or explicitly use `unsafe_hardware:` for extreme overclocking.", safe_v),
                                        format!("اخفض الفولتية إلى `{:.2}V` أو استخدم `unsafe_hardware:` إذا كنت متأكداً.", safe_v),
                                    )
                                    .with_fix(AutoFix {
                                        title: format!("Clamp voltage to safe {:.2}V", safe_v),
                                        title_ar: format!("تحديد الفولتية إلى القيمة الآمنة {:.2}V", safe_v),
                                        edits: vec![TextEdit {
                                            span: args[1].span,
                                            replacement: format!("{:.2}V", safe_v),
                                        }],
                                        is_preferred: true,
                                    }),
                                );
                            }
                        }
                    }

                    // Fan speed range guardrail
                    if namespace == "hardware" && func == "fan_set_speed" && args.len() == 2 {
                        if let ExprKind::Int(pct) = args[1].kind {
                            if !(0..=100).contains(&pct) {
                                let clamped = pct.clamp(0, 100);
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        "NB403",
                                        args[1].span,
                                        format!("Fan speed `{}`% is outside valid 0..100% range", pct),
                                    )
                                    .with_severity(Severity::HardwareSafety)
                                    .with_ar_message(format!("سرعة المروحة `{}`% خارج النطاق المسموح (0..100%)", pct))
                                    .with_why("PWM fan duty cycle must be between 0% and 100%.", "نسبة دوران المروحة PWM يجب أن تكون بين 0 و 100.")
                                    .with_solution(format!("Clamp to `{}`.", clamped), format!("اضبط القيمة إلى `{}`.", clamped))
                                    .with_fix(AutoFix {
                                        title: format!("Clamp fan speed to {}%", clamped),
                                        title_ar: format!("تحديد سرعة المروحة إلى {}%", clamped),
                                        edits: vec![TextEdit {
                                            span: args[1].span,
                                            replacement: clamped.to_string(),
                                        }],
                                        is_preferred: true,
                                    }),
                                );
                            }
                        }
                    }

                    // Cybersecurity W^X audit: memory::protect(addr, size, "RWX")
                    if namespace == "memory" && func == "protect" && args.len() == 3 {
                        if let ExprKind::Str(perm) = &args[2].kind {
                            if perm.to_ascii_uppercase().contains("RWX") && !self.in_unsafe_hardware {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        "NB501",
                                        args[2].span,
                                        "W^X Security Violation: mapping memory as both Writable and Executable (`RWX`)",
                                    )
                                    .with_severity(Severity::SecurityAlert)
                                    .with_ar_message("ثغرة أمنية (W^X): تم منح الذاكرة صلاحية الكتابة والتنفيذ معاً (`RWX`)")
                                    .with_why(
                                        "Memory pages that are both writable and executable allow attackers to inject and execute shellcode.",
                                        "صفحات الذاكرة القابلة للكتابة والتنفيذ معاً تسمح بحقن وتنفيذ الشيفرات الخبيثة.",
                                    )
                                    .with_solution(
                                        "Use `\"RX\"` (read+execute) for code or `\"RW\"` (read+write) for data.",
                                        "استخدم `\"RX\"` للكود أو `\"RW\"` للبيانات.",
                                    )
                                    .with_fix(AutoFix {
                                        title: "Harden page permission to `\"RX\"` (W^X safe)".to_string(),
                                        title_ar: "تأمين الصلاحية إلى `\"RX\"`".to_string(),
                                        edits: vec![TextEdit {
                                            span: args[2].span,
                                            replacement: "\"RX\"".to_string(),
                                        }],
                                        is_preferred: true,
                                    }),
                                );
                            }
                        }
                    }

                    Type::from_name(spec.return_type)
                } else {
                    let fns = functions_in_namespace(namespace);
                    let names: Vec<&str> = fns.iter().map(|f| f.name).collect();
                    let sug = closest_match(func, &names, 3);
                    let mut diag = Diagnostic::error(
                        "NB205",
                        *func_span,
                        format!("Unknown function `{}::{}`", namespace, func),
                    )
                    .with_ar_message(format!("دالة غير معروفة `{}::{}`", namespace, func))
                    .with_why(
                        format!("Module `{}` does not contain a function named `{}`.", namespace, func),
                        format!("الوحدة `{}` لا تحتوي على دالة باسم `{}`.", namespace, func),
                    );
                    if let Some(s) = sug {
                        diag = diag
                            .with_solution(
                                format!("Did you mean `{}::{}`?", namespace, s),
                                format!("هل تقصد `{}::{}`؟", namespace, s),
                            )
                            .with_fix(AutoFix {
                                title: format!("Replace `{}` with `{}`", func, s),
                                title_ar: format!("تصحيح `{}` إلى `{}`", func, s),
                                edits: vec![TextEdit {
                                    span: *func_span,
                                    replacement: s.to_string(),
                                }],
                                is_preferred: true,
                            });
                    }
                    self.diagnostics.push(diag);
                    Type::Any
                }
            }
            ExprKind::Call { callee, args } => {
                for a in args {
                    self.check_expr(a);
                }
                if let ExprKind::Ident(fname) = &callee.kind {
                    if let Some((params, ret, _)) = self.fns.get(fname).cloned() {
                        if args.len() != params.len() {
                            self.diagnostics.push(
                                Diagnostic::error(
                                    "NB206",
                                    expr.span,
                                    format!("Function `{}` expects {} argument(s), got {}", fname, params.len(), args.len()),
                                )
                                .with_ar_message(format!("الدالة `{}` تتطلب {} معامل ولكن تم تمرير {}", fname, params.len(), args.len()))
                                .with_why("Argument count mismatch.", "عدد المعاملات لا يطابق تعريف الدالة.")
                                .with_solution("Pass the exact number of arguments.", "مرر العدد الصحيح من المعاملات."),
                            );
                        }
                        return ret;
                    } else {
                        self.report_undefined_ident(fname, callee.span);
                    }
                } else {
                    self.check_expr(callee);
                }
                Type::Any
            }
            ExprKind::MethodCall { receiver, args, .. } => {
                self.check_expr(receiver);
                for a in args {
                    self.check_expr(a);
                }
                Type::Any
            }
            ExprKind::Field { receiver, .. } => {
                self.check_expr(receiver);
                Type::Any
            }
            ExprKind::Index { receiver, index } => {
                self.check_expr(receiver);
                self.check_expr(index);
                Type::Any
            }
            ExprKind::List(items) => {
                for it in items {
                    self.check_expr(it);
                }
                Type::List
            }
            ExprKind::Map(entries) => {
                for (k, v) in entries {
                    self.check_expr(k);
                    self.check_expr(v);
                }
                Type::Map
            }
            ExprKind::StructLit { name, fields } => {
                for (_, v) in fields {
                    self.check_expr(v);
                }
                Type::Struct(name.clone())
            }
            ExprKind::Binary { left, op, right } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);

                // Cybersecurity side-channel check: comparing secret/token/hash with `==`
                if matches!(op, BinOp::Eq | BinOp::NotEq) && (looks_like_secret_expr(left) || looks_like_secret_expr(right)) {
                    let left_txt = self.slice_span(left.span);
                    let right_txt = self.slice_span(right.span);
                    let replacement = if matches!(op, BinOp::Eq) {
                        format!("security::constant_time_eq({}, {})", left_txt, right_txt)
                    } else {
                        format!("not security::constant_time_eq({}, {})", left_txt, right_txt)
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            "NB502",
                            expr.span,
                            "Timing side-channel risk: comparing secret/token/hash with `==`",
                        )
                        .with_severity(Severity::SecurityAlert)
                        .with_ar_message("ثغرة قناة التوقيت (Timing Attack): مقارنة رمز سري أو بصمة باستخدام `==`")
                        .with_why(
                            "Standard `==` stops early on the first mismatched byte, leaking secret bytes via response timing.",
                            "المقارنة العادية `==` تتوقف عند أول بايت مختلف مما يكشف أجزاء السر عبر قياس الزمن.",
                        )
                        .with_solution(
                            "Use `security::constant_time_eq(a, b)` for cryptographic comparisons.",
                            "استخدم `security::constant_time_eq(a, b)` للمقارنة الأمنية بزمن ثابت.",
                        )
                        .with_fix(AutoFix {
                            title: "Use `security::constant_time_eq(...)`".to_string(),
                            title_ar: "استبدال المقارنة بـ `security::constant_time_eq(...)`".to_string(),
                            edits: vec![TextEdit {
                                span: expr.span,
                                replacement,
                            }],
                            is_preferred: true,
                        }),
                    );
                }

                match op {
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq | BinOp::And | BinOp::Or => Type::Bool,
                    _ if lt == Type::Float || rt == Type::Float => Type::Float,
                    _ => lt,
                }
            }
            ExprKind::Unary { op, operand } => {
                let t = self.check_expr(operand);
                if matches!(op, UnaryOp::Not) {
                    Type::Bool
                } else {
                    t
                }
            }
            ExprKind::Range { start, end, .. } => {
                self.check_expr(start);
                self.check_expr(end);
                Type::List
            }
            ExprKind::Cast { expr, ty } => {
                self.check_expr(expr);
                Type::from_name(&ty.name)
            }
        }
    }

    fn report_unknown_namespace(&mut self, ns: &str, span: Span) {
        let sug = closest_match(ns, NAMESPACES, 3);
        let mut diag = Diagnostic::error(
            "NB207",
            span,
            format!("Unknown hardware/std module `{}`", ns),
        )
        .with_ar_message(format!("وحدة غير معروفة `{}`", ns))
        .with_why(
            format!("Built-in modules are: {}.", NAMESPACES.join(", ")),
            format!("الوحدات المدمجة المتاحة هي: {}.", NAMESPACES.join(", ")),
        );
        if let Some(s) = sug {
            diag = diag
                .with_solution(
                    format!("Did you mean `{}::`?", s),
                    format!("هل تقصد `{}::`؟", s),
                )
                .with_fix(AutoFix {
                    title: format!("Replace `{}` with `{}`", ns, s),
                    title_ar: format!("تصحيح `{}` إلى `{}`", ns, s),
                    edits: vec![TextEdit {
                        span,
                        replacement: s.to_string(),
                    }],
                    is_preferred: true,
                });
        }
        self.diagnostics.push(diag);
    }

    fn slice_span(&self, span: Span) -> String {
        self.source
            .get(span.start..span.end)
            .unwrap_or("")
            .to_string()
    }

    fn line_text(&self, line_1: usize) -> &str {
        self.source.lines().nth(line_1.saturating_sub(1)).unwrap_or("")
    }

    fn indent_of_line(&self, line_1: usize) -> String {
        let l = self.line_text(line_1);
        l.chars().take_while(|c| *c == ' ' || *c == '\t').collect()
    }

    fn line_span(&self, line_1: usize) -> Span {
        let mut offset = 0usize;
        for (idx, line) in self.source.split_inclusive('\n').enumerate() {
            if idx + 1 == line_1 {
                let trimmed = line.trim_end_matches(['\r', '\n']);
                return Span::new(
                    offset,
                    offset + trimmed.len(),
                    line_1,
                    1,
                    line_1,
                    trimmed.chars().count() + 1,
                );
            }
            offset += line.len();
        }
        Span::default()
    }
}

fn is_alloc_call(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::NamespaceCall { namespace, func, .. }
            if namespace == "memory" && (func == "alloc" || func == "alloc_zeroed" || func == "alloc_aligned")
    )
}

fn looks_like_secret_expr(expr: &Expr) -> bool {
    if let ExprKind::Ident(name) = &expr.kind {
        let low = name.to_ascii_lowercase();
        return low.contains("secret")
            || low.contains("token")
            || low.contains("password")
            || low.contains("passwd")
            || low.contains("hmac")
            || low.contains("hash")
            || low.contains("signature")
            || low.contains("api_key");
    }
    false
}
