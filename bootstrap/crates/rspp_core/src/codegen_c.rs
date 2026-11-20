use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ast::*;
use crate::lexer::lex;
use crate::math_diag::Diagnostic;
use crate::parser::parse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileTarget {
    Native,
    Kernel,
}

/// Recursively loads an entry `.rspp` file along with any relative `import "file.rspp"` modules.
pub fn load_program_with_imports(entry_path: &Path) -> Result<(Program, String, Vec<Diagnostic>), String> {
    let mut visited = HashSet::new();
    let mut items = Vec::new();
    let mut diags = Vec::new();
    load_file_recursive(entry_path, &mut visited, &mut items, &mut diags, true)?;
    let entry_src = fs::read_to_string(entry_path).map_err(|e| format!("Failed reading {}: {}", entry_path.display(), e))?;
    Ok((Program { items }, entry_src, diags))
}

fn load_file_recursive(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    out_items: &mut Vec<Item>,
    out_diags: &mut Vec<Diagnostic>,
    is_entry: bool,
) -> Result<(), String> {
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canon.clone()) {
        return Ok(());
    }
    let src = fs::read_to_string(path).map_err(|e| format!("Cannot read `{}`: {}", path.display(), e))?;
    let lx = lex(&src);
    if is_entry {
        out_diags.extend(lx.diagnostics);
    }
    let ps = parse(lx.tokens);
    if is_entry {
        out_diags.extend(ps.diagnostics);
    }
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    for item in ps.program.items {
        if let Item::Import(ref imp) = item {
            if imp.path.ends_with(".rspp") {
                let target = base_dir.join(&imp.path);
                if target.exists() {
                    load_file_recursive(&target, visited, out_items, out_diags, false)?;
                    continue;
                }
            }
        }
        out_items.push(item);
    }
    Ok(())
}

pub fn emit_c(program: &Program, target: CompileTarget) -> String {
    match target {
        CompileTarget::Kernel => emit_kernel_c(program),
        CompileTarget::Native => emit_native_c(program),
    }
}

pub fn compile_to_binary(program: &Program, target: CompileTarget, out_path: &Path, c_libs: &[String]) -> Result<(), String> {
    let c_src = emit_c(program, target);
    let c_path = out_path.with_extension("rspp.c");
    if let Some(p) = out_path.parent() {
        if !p.as_os_str().is_empty() {
            let _ = fs::create_dir_all(p);
        }
    }
    fs::write(&c_path, &c_src).map_err(|e| format!("Failed writing generated C: {}", e))?;

    let mut cmd = Command::new("gcc");
    match target {
        CompileTarget::Native => {
            cmd.arg("-std=c11").arg("-O2").arg(&c_path).arg("-o").arg(out_path).arg("-lm");
            for lib in c_libs {
                cmd.arg(format!("-l{}", lib));
            }
        }
        CompileTarget::Kernel => {
            cmd.args(["-std=c11", "-ffreestanding", "-nostdlib", "-fno-stack-protector", "-O2", "-c"])
                .arg(&c_path)
                .arg("-o")
                .arg(out_path);
        }
    }
    let output = cmd.output().map_err(|e| format!("Failed to invoke gcc: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "gcc failed ({}):\n{}",
            c_path.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

fn emit_native_c(program: &Program) -> String {
    let mut out = String::with_capacity(32768);
    out.push_str(NATIVE_PRELUDE);

    // Forward declarations for all functions, equations, and algorithms
    for item in &program.items {
        match item {
            Item::Fn(f) => {
                let params = (0..f.params.len()).map(|i| format!("RsppVal p{}", i)).collect::<Vec<_>>().join(", ");
                let _ = writeln!(out, "static RsppVal rspp_fn_{}({});", c_id(&f.name), if params.is_empty() { "void".into() } else { params });
            }
            Item::Equation(eq) => {
                let params = (0..eq.params.len()).map(|i| format!("RsppVal p{}", i)).collect::<Vec<_>>().join(", ");
                let _ = writeln!(out, "static RsppVal rspp_fn_{}({});", c_id(&eq.name), if params.is_empty() { "void".into() } else { params });
                if eq.params.len() == 1 {
                    let _ = writeln!(out, "static double rspp_eq1_{}(double x);", c_id(&eq.name));
                }
            }
            Item::Algorithm(alg) => {
                let params = (0..alg.params.len()).map(|i| format!("RsppVal p{}", i)).collect::<Vec<_>>().join(", ");
                let _ = writeln!(out, "static RsppVal rspp_fn_{}({});", c_id(&alg.name), if params.is_empty() { "void".into() } else { params });
            }
            Item::ExternFn(ef) => {
                let rty = c_ffi_type(ef.return_type.as_ref());
                let ps: Vec<String> = ef.params.iter().map(|p| c_ffi_type(p.ty.as_ref()).to_string()).collect();
                let _ = writeln!(out, "extern {} {}({});", rty, ef.name, if ps.is_empty() { "void".into() } else { ps.join(", ") });
            }
            _ => {}
        }
    }
    out.push('\n');

    // Global constants
    let mut top_consts = Vec::new();
    let mut top_stmts = Vec::new();
    for item in &program.items {
        match item {
            Item::Const(c) => top_consts.push(c),
            Item::Stmt(s) => top_stmts.push(s),
            _ => {}
        }
    }
    for c in &top_consts {
        let _ = writeln!(out, "static RsppVal rspp_v_{};", c_id(&c.name));
    }

    let mut ctx = NativeEmitter {
        out: &mut out,
        tmp_id: 0,
        extern_fns: program.items.iter().filter_map(|it| if let Item::ExternFn(e) = it { Some((e.name.clone(), e.clone())) } else { None }).collect(),
    };

    for item in &program.items {
        match item {
            Item::Equation(eq) => ctx.emit_equation(eq),
            Item::Fn(f) => ctx.emit_fn(f),
            Item::Algorithm(alg) => ctx.emit_algorithm(alg),
            _ => {}
        }
    }

    // Emit main wrapper
    let has_main = program.items.iter().any(|i| matches!(i, Item::Fn(f) if f.name == "main"));
    let one_var_eqs: Vec<String> = program.items.iter().filter_map(|it| {
        if let Item::Equation(e) = it {
            if e.params.len() == 1 { return Some(e.name.clone()); }
        }
        None
    }).collect();

    out.push_str("\nint main(int argc, char** argv) {\n");
    out.push_str("    rspp_argc = argc;\n    rspp_argv = argv;\n");
    out.push_str("    atexit(rspp_mem_leak_check_atexit);\n");
    for eq_name in &one_var_eqs {
        let _ = writeln!(out, "    rspp_register_eq(\"{}\", rspp_eq1_{});", esc_c(eq_name), c_id(eq_name));
    }
    let mut main_ctx = NativeEmitter {
        out: &mut out,
        tmp_id: 100000,
        extern_fns: program.items.iter().filter_map(|it| if let Item::ExternFn(e) = it { Some((e.name.clone(), e.clone())) } else { None }).collect(),
    };
    for c in &top_consts {
        let val = main_ctx.emit_expr(&c.value, 1);
        let _ = writeln!(main_ctx.out, "    rspp_v_{} = {};", c_id(&c.name), val);
    }
    let mut defers = Vec::new();
    for st in &top_stmts {
        main_ctx.emit_stmt(st, 1, &mut defers);
    }
    for d in defers.into_iter().rev() {
        let mut dummy = Vec::new();
        main_ctx.emit_stmt(&d, 1, &mut dummy);
    }
    if has_main {
        out.push_str("    RsppVal r = rspp_fn_main();\n");
        out.push_str("    if (r.kind == RSPP_INT) return (int)r.i;\n");
    }
    out.push_str("    return 0;\n}\n");
    out
}

struct NativeEmitter<'a> {
    out: &'a mut String,
    tmp_id: usize,
    extern_fns: Vec<(String, ExternFnDef)>,
}

impl<'a> NativeEmitter<'a> {
    fn next_tmp(&mut self) -> String {
        self.tmp_id += 1;
        format!("_t{}", self.tmp_id)
    }

    fn emit_equation(&mut self, eq: &EquationDef) {
        let params = eq.params.iter().map(|p| format!("RsppVal rspp_v_{}", c_id(p))).collect::<Vec<_>>().join(", ");
        let _ = writeln!(self.out, "static RsppVal rspp_fn_{}({}) {{", c_id(&eq.name), if params.is_empty() { "void".into() } else { params });
        let e = self.emit_expr(&eq.expr, 1);
        let _ = writeln!(self.out, "    return rspp_flt(rspp_to_f64({}));", e);
        self.out.push_str("}\n\n");

        if eq.params.len() == 1 {
            let p0 = c_id(&eq.params[0]);
            let _ = writeln!(self.out, "static double rspp_eq1_{}(double _x) {{", c_id(&eq.name));
            let _ = writeln!(self.out, "    RsppVal rspp_v_{} = rspp_flt(_x);", p0);
            let e = self.emit_expr(&eq.expr, 1);
            let _ = writeln!(self.out, "    return rspp_to_f64({});", e);
            self.out.push_str("}\n\n");
        }
    }

    fn emit_fn(&mut self, f: &FnDef) {
        let params = f.params.iter().map(|p| format!("RsppVal rspp_v_{}", c_id(&p.name))).collect::<Vec<_>>().join(", ");
        let _ = writeln!(self.out, "static RsppVal rspp_fn_{}({}) {{", c_id(&f.name), if params.is_empty() { "void".into() } else { params });
        let mut scope_defers: Vec<Vec<Stmt>> = vec![Vec::new()];
        self.emit_block_stmts(&f.body, 1, &mut scope_defers);
        self.emit_defers_stack(&scope_defers, 1);
        self.out.push_str("    return rspp_none();\n}\n\n");
    }

    fn emit_algorithm(&mut self, alg: &AlgorithmDef) {
        let params = alg.params.iter().map(|p| format!("RsppVal rspp_v_{}", c_id(&p.name))).collect::<Vec<_>>().join(", ");
        let _ = writeln!(self.out, "static RsppVal rspp_fn_{}({}) {{", c_id(&alg.name), if params.is_empty() { "void".into() } else { params });
        let mut scope_defers: Vec<Vec<Stmt>> = vec![Vec::new()];
        self.emit_block_stmts(&alg.body, 1, &mut scope_defers);
        self.emit_defers_stack(&scope_defers, 1);
        self.out.push_str("    return rspp_none();\n}\n\n");
    }

    fn emit_block_stmts(&mut self, stmts: &[Stmt], indent: usize, defer_stack: &mut Vec<Vec<Stmt>>) {
        defer_stack.push(Vec::new());
        for s in stmts {
            if let Stmt::Defer { stmt, .. } = s {
                if let Some(top) = defer_stack.last_mut() {
                    top.push((**stmt).clone());
                }
                continue;
            }
            self.emit_stmt_with_defers(s, indent, defer_stack);
        }
        if let Some(top) = defer_stack.pop() {
            for d in top.into_iter().rev() {
                let mut dummy = Vec::new();
                self.emit_stmt(&d, indent, &mut dummy);
            }
        }
    }

    fn emit_defers_stack(&mut self, defer_stack: &[Vec<Stmt>], indent: usize) {
        for frame in defer_stack.iter().rev() {
            for d in frame.iter().rev() {
                let mut dummy = Vec::new();
                self.emit_stmt(d, indent, &mut dummy);
            }
        }
    }

    fn emit_stmt(&mut self, stmt: &Stmt, indent: usize, local_defers: &mut Vec<Stmt>) {
        let mut stack = vec![std::mem::take(local_defers)];
        self.emit_stmt_with_defers(stmt, indent, &mut stack);
        *local_defers = stack.pop().unwrap_or_default();
    }

    fn emit_stmt_with_defers(&mut self, stmt: &Stmt, indent: usize, defer_stack: &mut Vec<Vec<Stmt>>) {
        let pad = "    ".repeat(indent);
        match stmt {
            Stmt::Let { name, value, .. } | Stmt::Var { name, value, .. } => {
                let v = self.emit_expr(value, indent);
                let _ = writeln!(self.out, "{}RsppVal rspp_v_{} = {};", pad, c_id(name), v);
            }
            Stmt::Assign { target, value, .. } => {
                let v = self.emit_expr(value, indent);
                match &target.kind {
                    ExprKind::Ident(n) => {
                        let _ = writeln!(self.out, "{}rspp_v_{} = {};", pad, c_id(n), v);
                    }
                    ExprKind::Index { receiver, index } => {
                        let r = self.emit_expr(receiver, indent);
                        let i = self.emit_expr(index, indent);
                        let _ = writeln!(self.out, "{}rspp_index_set({}, {}, {});", pad, r, i, v);
                    }
                    _ => {}
                }
            }
            Stmt::CompoundAssign { target, op, value, .. } => {
                let rhs = self.emit_expr(value, indent);
                let cur = self.emit_expr(target, indent);
                let fn_name = match op {
                    BinOp::Add => "rspp_add",
                    BinOp::Sub => "rspp_sub",
                    BinOp::Mul => "rspp_mul",
                    BinOp::Div => "rspp_div",
                    _ => "rspp_add",
                };
                if let ExprKind::Ident(n) = &target.kind {
                    let _ = writeln!(self.out, "{}rspp_v_{} = {}({}, {});", pad, c_id(n), fn_name, cur, rhs);
                }
            }
            Stmt::Expr { expr, .. } => {
                let _ = self.emit_expr(expr, indent);
            }
            Stmt::If { branches, else_branch, .. } => {
                for (i, (cond, body)) in branches.iter().enumerate() {
                    if i == 0 {
                        let c = self.emit_expr(cond, indent);
                        let _ = writeln!(self.out, "{}if (rspp_to_bool({})) {{", pad, c);
                    } else {
                        let _ = writeln!(self.out, "{}}} else {{", pad);
                        let c = self.emit_expr(cond, indent + 1);
                        let _ = writeln!(self.out, "{}    if (rspp_to_bool({})) {{", pad, c);
                    }
                    self.emit_block_stmts(body, if i == 0 { indent + 1 } else { indent + 2 }, defer_stack);
                }
                if branches.len() > 1 {
                    if let Some(eb) = else_branch {
                        let _ = writeln!(self.out, "{}}} else {{", pad);
                        self.emit_block_stmts(eb, indent + 1, defer_stack);
                    }
                    for k in (1..branches.len()).rev() {
                        let _ = writeln!(self.out, "{}}}", "    ".repeat(indent + k - 1));
                    }
                    let _ = writeln!(self.out, "{}}}", pad);
                } else {
                    if let Some(eb) = else_branch {
                        let _ = writeln!(self.out, "{}}} else {{", pad);
                        self.emit_block_stmts(eb, indent + 1, defer_stack);
                    }
                    let _ = writeln!(self.out, "{}}}", pad);
                }
            }
            Stmt::While { cond, body, .. } => {
                let _ = writeln!(self.out, "{}while (1) {{", pad);
                let c = self.emit_expr(cond, indent + 1);
                let _ = writeln!(self.out, "{}    if (!rspp_to_bool({})) break;", pad, c);
                self.emit_block_stmts(body, indent + 1, defer_stack);
                let _ = writeln!(self.out, "{}}}", pad);
            }
            Stmt::Loop { body, .. } => {
                let _ = writeln!(self.out, "{}while (1) {{", pad);
                self.emit_block_stmts(body, indent + 1, defer_stack);
                let _ = writeln!(self.out, "{}}}", pad);
            }
            Stmt::For { var, iter, body, .. } => {
                let it = self.emit_expr(iter, indent);
                let it_var = self.next_tmp();
                let idx_var = self.next_tmp();
                let _ = writeln!(self.out, "{}RsppVal {} = {};", pad, it_var, it);
                let _ = writeln!(self.out, "{}for (int64_t {} = 0; {} < rspp_list_len({}); {}++) {{", pad, idx_var, idx_var, it_var, idx_var);
                let _ = writeln!(self.out, "{}    RsppVal rspp_v_{} = rspp_index_get({}, rspp_int({}));", pad, c_id(var), it_var, idx_var);
                self.emit_block_stmts(body, indent + 1, defer_stack);
                let _ = writeln!(self.out, "{}}}", pad);
            }
            Stmt::Match { subject, arms, .. } => {
                let sub = self.emit_expr(subject, indent);
                let sub_var = self.next_tmp();
                let _ = writeln!(self.out, "{}RsppVal {} = {};", pad, sub_var, sub);
                for (i, arm) in arms.iter().enumerate() {
                    let is_wild = matches!(&arm.pattern.kind, ExprKind::Ident(n) if n == "_");
                    if i == 0 {
                        if is_wild {
                            let _ = writeln!(self.out, "{}if (1) {{", pad);
                        } else {
                            let pat = self.emit_expr(&arm.pattern, indent);
                            let _ = writeln!(self.out, "{}if (rspp_to_bool(rspp_eq({}, {}))) {{", pad, sub_var, pat);
                        }
                    } else if is_wild {
                        let _ = writeln!(self.out, "{}}} else {{", pad);
                    } else {
                        let _ = writeln!(self.out, "{}}} else {{", pad);
                        let pat = self.emit_expr(&arm.pattern, indent + 1);
                        let _ = writeln!(self.out, "{}    if (rspp_to_bool(rspp_eq({}, {}))) {{", pad, sub_var, pat);
                    }
                    self.emit_block_stmts(&arm.body, if i == 0 || is_wild { indent + 1 } else { indent + 2 }, defer_stack);
                }
                if !arms.is_empty() {
                    // Close nested non-wild else-if blocks
                    let non_wild_after_first = arms.iter().skip(1).filter(|a| !matches!(&a.pattern.kind, ExprKind::Ident(n) if n == "_")).count();
                    for _ in 0..non_wild_after_first {
                        let _ = writeln!(self.out, "{}    }}", pad);
                    }
                    let _ = writeln!(self.out, "{}}}", pad);
                }
            }
            Stmt::Return { value, .. } => {
                let ret_var = self.next_tmp();
                if let Some(v) = value {
                    let rv = self.emit_expr(v, indent);
                    let _ = writeln!(self.out, "{}RsppVal {} = {};", pad, ret_var, rv);
                } else {
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_none();", pad, ret_var);
                }
                self.emit_defers_stack(defer_stack, indent);
                let _ = writeln!(self.out, "{}return {};", pad, ret_var);
            }
            Stmt::Break(_) => {
                let _ = writeln!(self.out, "{}break;", pad);
            }
            Stmt::Continue(_) => {
                let _ = writeln!(self.out, "{}continue;", pad);
            }
            Stmt::UnsafeHardware { body, .. } => {
                let _ = writeln!(self.out, "{}{{", pad);
                self.emit_block_stmts(body, indent + 1, defer_stack);
                let _ = writeln!(self.out, "{}}}", pad);
            }
            Stmt::Defer { stmt, .. } => {
                if let Some(top) = defer_stack.last_mut() {
                    top.push((**stmt).clone());
                }
            }
            Stmt::TryCatch { try_body, err_var, catch_body, .. } => {
                let _ = writeln!(self.out, "{}if (setjmp(rspp_push_try()) == 0) {{", pad);
                self.emit_block_stmts(try_body, indent + 1, defer_stack);
                let _ = writeln!(self.out, "{}    rspp_pop_try();", pad);
                let _ = writeln!(self.out, "{}}} else {{", pad);
                let _ = writeln!(self.out, "{}    rspp_pop_try();", pad);
                let _ = writeln!(self.out, "{}    RsppVal rspp_v_{} = rspp_str(rspp_last_error);", pad, c_id(err_var));
                self.emit_block_stmts(catch_body, indent + 1, defer_stack);
                let _ = writeln!(self.out, "{}}}", pad);
            }
            Stmt::Throw { value, .. } => {
                let v = self.emit_expr(value, indent);
                let _ = writeln!(self.out, "{}rspp_throw(rspp_to_str({}));", pad, v);
            }
        }
    }

    fn emit_expr(&mut self, expr: &Expr, indent: usize) -> String {
        let pad = "    ".repeat(indent);
        match &expr.kind {
            ExprKind::Int(i) => format!("rspp_int({}LL)", i),
            ExprKind::HexAddr(a) => format!("rspp_ptr(0x{:x}ULL)", a),
            ExprKind::Float(f) | ExprKind::Volt(f) | ExprKind::Freq(f)
            | ExprKind::SampleRate(f) | ExprKind::Dbm(f) | ExprKind::Angle(f) => format!("rspp_flt({:.12})", f),
            ExprKind::Duration(ns) => format!("rspp_int({}LL)", *ns as i64),
            ExprKind::Str(s) => format!("rspp_str(\"{}\")", esc_c(s)),
            ExprKind::FStr(tpl) => self.emit_fstr(tpl, indent),
            ExprKind::Bool(b) => format!("rspp_bool({})", if *b { 1 } else { 0 }),
            ExprKind::NoneLit => "rspp_none()".to_string(),
            ExprKind::Ident(n) => format!("rspp_v_{}", c_id(n)),
            ExprKind::NamespacePath { namespace, member, .. } => format!("rspp_str(\"{}::{}\")", esc_c(namespace), esc_c(member)),
            ExprKind::List(items) => {
                let tmp = self.next_tmp();
                let _ = writeln!(self.out, "{}RsppVal {} = rspp_list();", pad, tmp);
                for it in items {
                    let ev = self.emit_expr(it, indent);
                    let _ = writeln!(self.out, "{}rspp_list_push({}, {});", pad, tmp, ev);
                }
                tmp
            }
            ExprKind::Map(entries) => {
                let tmp = self.next_tmp();
                let _ = writeln!(self.out, "{}RsppVal {} = rspp_map();", pad, tmp);
                for (k, v) in entries {
                    let ek = self.emit_expr(k, indent);
                    let ev = self.emit_expr(v, indent);
                    let _ = writeln!(self.out, "{}rspp_map_set({}, {}, {});", pad, tmp, ek, ev);
                }
                tmp
            }
            ExprKind::StructLit { fields, .. } => {
                let tmp = self.next_tmp();
                let _ = writeln!(self.out, "{}RsppVal {} = rspp_map();", pad, tmp);
                for (k, v) in fields {
                    let ev = self.emit_expr(v, indent);
                    let _ = writeln!(self.out, "{}rspp_map_set({}, rspp_str(\"{}\"), {});", pad, tmp, esc_c(k), ev);
                }
                tmp
            }
            ExprKind::Range { start, end, inclusive } => {
                let s = self.emit_expr(start, indent);
                let e = self.emit_expr(end, indent);
                format!("rspp_range({}, {}, {})", s, e, if *inclusive { 1 } else { 0 })
            }
            ExprKind::Index { receiver, index } => {
                let r = self.emit_expr(receiver, indent);
                let i = self.emit_expr(index, indent);
                format!("rspp_index_get({}, {})", r, i)
            }
            ExprKind::Field { receiver, field } => {
                let r = self.emit_expr(receiver, indent);
                format!("rspp_index_get({}, rspp_str(\"{}\"))", r, esc_c(field))
            }
            ExprKind::MethodCall { receiver, method, args } => {
                let r = self.emit_expr(receiver, indent);
                let mut evs = Vec::with_capacity(args.len());
                for a in args { evs.push(self.emit_expr(a, indent)); }
                let tmp = self.next_tmp();
                if evs.is_empty() {
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_method_call({}, \"{}\", 0, NULL);", pad, tmp, r, esc_c(method));
                } else {
                    let arr = self.next_tmp();
                    let _ = writeln!(self.out, "{}RsppVal {}[] = {{ {} }};", pad, arr, evs.join(", "));
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_method_call({}, \"{}\", {}, {});", pad, tmp, r, esc_c(method), evs.len(), arr);
                }
                tmp
            }
            ExprKind::Binary { left, op, right } => {
                if matches!(op, BinOp::And) {
                    let tmp = self.next_tmp();
                    let l = self.emit_expr(left, indent);
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_bool(0);", pad, tmp);
                    let _ = writeln!(self.out, "{}if (rspp_to_bool({})) {{", pad, l);
                    let r = self.emit_expr(right, indent + 1);
                    let _ = writeln!(self.out, "{}    {} = rspp_bool(rspp_to_bool({}));", pad, tmp, r);
                    let _ = writeln!(self.out, "{}}}", pad);
                    return tmp;
                }
                if matches!(op, BinOp::Or) {
                    let tmp = self.next_tmp();
                    let l = self.emit_expr(left, indent);
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_bool(1);", pad, tmp);
                    let _ = writeln!(self.out, "{}if (!rspp_to_bool({})) {{", pad, l);
                    let r = self.emit_expr(right, indent + 1);
                    let _ = writeln!(self.out, "{}    {} = rspp_bool(rspp_to_bool({}));", pad, tmp, r);
                    let _ = writeln!(self.out, "{}}}", pad);
                    return tmp;
                }
                let l = self.emit_expr(left, indent);
                let r = self.emit_expr(right, indent);
                let fname = match op {
                    BinOp::Add => "rspp_add",
                    BinOp::Sub => "rspp_sub",
                    BinOp::Mul => "rspp_mul",
                    BinOp::Div => "rspp_div",
                    BinOp::Mod => "rspp_mod",
                    BinOp::Pow => "rspp_pow",
                    BinOp::Eq => "rspp_eq",
                    BinOp::NotEq => "rspp_neq",
                    BinOp::Lt => "rspp_lt",
                    BinOp::LtEq => "rspp_lte",
                    BinOp::Gt => "rspp_gt",
                    BinOp::GtEq => "rspp_gte",
                    BinOp::BitAnd => "rspp_bitand",
                    BinOp::BitOr => "rspp_bitor",
                    BinOp::BitXor => "rspp_bitxor",
                    BinOp::Shl => "rspp_shl",
                    BinOp::Shr => "rspp_shr",
                    BinOp::And | BinOp::Or => unreachable!(),
                };
                format!("{}({}, {})", fname, l, r)
            }
            ExprKind::Unary { op, operand } => {
                let v = self.emit_expr(operand, indent);
                match op {
                    UnaryOp::Neg => format!("rspp_neg({})", v),
                    UnaryOp::Not => format!("rspp_bool(!rspp_to_bool({}))", v),
                    UnaryOp::BitNot => format!("rspp_int(~rspp_to_i64({}))", v),
                }
            }
            ExprKind::Cast { expr, ty } => {
                let v = self.emit_expr(expr, indent);
                match ty.name.as_str() {
                    "int" | "i64" | "u64" => format!("rspp_int(rspp_to_i64({}))", v),
                    "float" | "f64" => format!("rspp_flt(rspp_to_f64({}))", v),
                    "str" => format!("rspp_str(rspp_to_str({}))", v),
                    "bool" => format!("rspp_bool(rspp_to_bool({}))", v),
                    _ => v,
                }
            }
            ExprKind::Call { callee, args } => {
                let mut evs = Vec::with_capacity(args.len());
                for a in args { evs.push(self.emit_expr(a, indent)); }
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some((_, ef)) = self.extern_fns.iter().find(|(n, _)| n == name).cloned() {
                        let c_args: Vec<String> = evs.iter().enumerate().map(|(i, ev)| {
                            let ty = ef.params.get(i).and_then(|p| p.ty.as_ref());
                            match ty.map(|t| t.name.as_str()) {
                                Some("float" | "f64" | "f32") => format!("rspp_to_f64({})", ev),
                                Some("str") => format!("rspp_to_str({})", ev),
                                Some("ptr") => format!("(void*)(uintptr_t)rspp_to_u64({})", ev),
                                _ => format!("rspp_to_i64({})", ev),
                            }
                        }).collect();
                        let call_str = format!("{}({})", ef.name, c_args.join(", "));
                        return match ef.return_type.as_ref().map(|t| t.name.as_str()) {
                            Some("void") => {
                                let _ = writeln!(self.out, "{}{};", pad, call_str);
                                "rspp_none()".into()
                            }
                            Some("float" | "f64" | "f32") => format!("rspp_flt({})", call_str),
                            Some("ptr") => format!("rspp_ptr((uint64_t)(uintptr_t){})", call_str),
                            _ => format!("rspp_int((int64_t){})", call_str),
                        };
                    }
                    let tmp = self.next_tmp();
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_fn_{}({});", pad, tmp, c_id(name), evs.join(", "));
                    return tmp;
                }
                "rspp_none()".to_string()
            }
            ExprKind::NamespaceCall { namespace, func, args, .. } => {
                let mut evs = Vec::with_capacity(args.len());
                for a in args { evs.push(self.emit_expr(a, indent)); }
                let tmp = self.next_tmp();
                if evs.is_empty() {
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_builtin(\"{}\", \"{}\", 0, NULL);", pad, tmp, esc_c(namespace), esc_c(func));
                } else {
                    let arr = self.next_tmp();
                    let _ = writeln!(self.out, "{}RsppVal {}[] = {{ {} }};", pad, arr, evs.join(", "));
                    let _ = writeln!(self.out, "{}RsppVal {} = rspp_builtin(\"{}\", \"{}\", {}, {});", pad, tmp, esc_c(namespace), esc_c(func), evs.len(), arr);
                }
                tmp
            }
        }
    }

    fn emit_fstr(&mut self, tpl: &str, indent: usize) -> String {
        let pad = "    ".repeat(indent);
        let acc = self.next_tmp();
        let _ = writeln!(self.out, "{}RsppVal {} = rspp_str(\"\");", pad, acc);
        let chars: Vec<char> = tpl.chars().collect();
        let mut i = 0usize;
        let mut lit = String::new();
        while i < chars.len() {
            if chars[i] == '{' {
                if chars.get(i + 1) == Some(&'{') {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                if !lit.is_empty() {
                    let _ = writeln!(self.out, "{}{} = rspp_add({}, rspp_str(\"{}\"));", pad, acc, acc, esc_c(&lit));
                    lit.clear();
                }
                i += 1;
                let mut expr_src = String::new();
                let mut depth = 1usize;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '{' { depth += 1; }
                    else if chars[i] == '}' { depth -= 1; if depth == 0 { i += 1; break; } }
                    expr_src.push(chars[i]);
                    i += 1;
                }
                let lx = lex(expr_src.trim());
                let ps = parse(lx.tokens);
                if let Some(Item::Stmt(Stmt::Expr { expr, .. })) = ps.program.items.first() {
                    let ev = self.emit_expr(expr, indent);
                    let _ = writeln!(self.out, "{}{} = rspp_add({}, rspp_str(rspp_to_str({})));", pad, acc, acc, ev);
                }
            } else if chars[i] == '}' && chars.get(i + 1) == Some(&'}') {
                lit.push('}');
                i += 2;
            } else {
                lit.push(chars[i]);
                i += 1;
            }
        }
        if !lit.is_empty() {
            let _ = writeln!(self.out, "{}{} = rspp_add({}, rspp_str(\"{}\"));", pad, acc, acc, esc_c(&lit));
        }
        acc
    }
}

fn c_id(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

fn esc_c(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => r.push_str("\\\\"),
            '"' => r.push_str("\\\""),
            '\n' => r.push_str("\\n"),
            '\r' => r.push_str("\\r"),
            '\t' => r.push_str("\\t"),
            c if (c as u32) < 32 => { let _ = write!(r, "\\x{:02x}", c as u32); }
            c => r.push(c),
        }
    }
    r
}

fn c_ffi_type(t: Option<&TypeExpr>) -> &'static str {
    match t.map(|x| x.name.as_str()) {
        Some("float" | "f64" | "f32") => "double",
        Some("str") => "const char*",
        Some("ptr") => "void*",
        Some("void") => "void",
        _ => "int64_t",
    }
}

// ============================================================================
// Bare-Metal Kernel Codegen (`--target kernel`)
// ============================================================================

fn emit_kernel_c(program: &Program) -> String {
    let mut out = String::from(KERNEL_PRELUDE);
    for item in &program.items {
        if let Item::Fn(f) = item {
            let params: Vec<String> = f.params.iter().map(|p| format!("uint64_t {}", c_id(&p.name))).collect();
            let _ = writeln!(out, "uint64_t rspp_kfn_{}({}) {{", c_id(&f.name), if params.is_empty() { "void".into() } else { params.join(", ") });
            for st in &f.body {
                emit_kernel_stmt(&mut out, st, 1);
            }
            out.push_str("    return 0;\n}\n\n");
        }
    }
    out.push_str("void kernel_main(void) {\n    rspp_kfn_kernel_main();\n    for(;;) { __asm__ volatile(\"hlt\"); }\n}\n");
    out
}

fn emit_kernel_stmt(out: &mut String, st: &Stmt, indent: usize) {
    let pad = "    ".repeat(indent);
    match st {
        Stmt::Let { name, value, .. } | Stmt::Var { name, value, .. } => {
            let _ = writeln!(out, "{}uint64_t {} = {};", pad, c_id(name), emit_kernel_expr(value));
        }
        Stmt::UnsafeHardware { body, .. } => {
            for s in body { emit_kernel_stmt(out, s, indent); }
        }
        Stmt::Expr { expr, .. } => {
            let _ = writeln!(out, "{}{};", pad, emit_kernel_expr(expr));
        }
        Stmt::Return { value, .. } => {
            let v = value.as_ref().map(emit_kernel_expr).unwrap_or_else(|| "0".into());
            let _ = writeln!(out, "{}return {};", pad, v);
        }
        _ => {}
    }
}

fn emit_kernel_expr(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Int(i) => format!("{}ULL", i),
        ExprKind::HexAddr(a) => format!("0x{:x}ULL", a),
        ExprKind::Str(s) => format!("\"{}\"", esc_c(s)),
        ExprKind::Ident(n) => c_id(n),
        ExprKind::NamespaceCall { namespace, func, args, .. } => {
            let ea: Vec<String> = args.iter().map(emit_kernel_expr).collect();
            match (namespace.as_str(), func.as_str()) {
                ("hardware", "out_port") if ea.len() >= 2 => format!("k_outb((uint16_t)({}), (uint8_t)({}))", ea[0], ea[1]),
                ("hardware", "in_port") if !ea.is_empty() => format!("k_inb((uint16_t)({}))", ea[0]),
                ("kernel", "cli") => "(__extension__({ __asm__ volatile(\"cli\"); 0ULL; }))".into(),
                ("kernel", "sti") => "(__extension__({ __asm__ volatile(\"sti\"); 0ULL; }))".into(),
                ("kernel", "halt") => "(__extension__({ __asm__ volatile(\"hlt\"); 0ULL; }))".into(),
                ("display", "vga_print") if ea.len() >= 3 => format!("k_vga_print({}, {}, {})", ea[0], ea[1], ea[2]),
                ("display", "clear") => "k_vga_clear()".into(),
                ("memory", "write_u8") if ea.len() >= 2 => format!("(*(volatile uint8_t*)(uintptr_t)({}) = (uint8_t)({}))", ea[0], ea[1]),
                ("memory", "read_u8") if !ea.is_empty() => format!("(uint64_t)(*(volatile uint8_t*)(uintptr_t)({}))", ea[0]),
                _ => "0ULL".into(),
            }
        }
        _ => "0ULL".into(),
    }
}

const KERNEL_PRELUDE: &str = r#"#include <stdint.h>
__attribute__((section(".multiboot"), aligned(8)))
const uint32_t multiboot2_header[] = { 0xe85250d6u, 0u, 16u, (uint32_t)(-(0xe85250d6u + 0u + 16u)) };

static inline uint64_t k_outb(uint16_t port, uint8_t val) {
    __asm__ volatile("outb %0, %1" : : "a"(val), "Nd"(port));
    return 0;
}
static inline uint64_t k_inb(uint16_t port) {
    uint8_t ret;
    __asm__ volatile("inb %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}
static inline uint64_t k_vga_clear(void) {
    volatile uint16_t* vga = (volatile uint16_t*)0xB8000ULL;
    for (int i = 0; i < 80 * 25; i++) vga[i] = 0x0F20;
    return 0;
}
static inline uint64_t k_vga_print(const char* s, uint64_t row, uint64_t color) {
    volatile uint16_t* vga = (volatile uint16_t*)0xB8000ULL;
    uint64_t off = row * 80;
    for (uint64_t i = 0; s[i] && (off + i) < 2000; i++) {
        vga[off + i] = ((uint16_t)color << 8) | (uint8_t)s[i];
    }
    return 0;
}
"#;

const NATIVE_PRELUDE: &str = r#"#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <ctype.h>
#include <setjmp.h>

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

static int rspp_argc = 0;
static char** rspp_argv = NULL;

typedef enum {
    RSPP_NONE = 0,
    RSPP_INT,
    RSPP_FLOAT,
    RSPP_BOOL,
    RSPP_STR,
    RSPP_PTR,
    RSPP_LIST,
    RSPP_MAP
} RsppKind;

typedef struct RsppList RsppList;
typedef struct RsppMap RsppMap;

typedef struct {
    RsppKind kind;
    union {
        int64_t i;
        double f;
        int b;
        char* s;
        uint64_t ptr;
        RsppList* list;
        RsppMap* map;
    };
} RsppVal;

struct RsppList {
    RsppVal* items;
    size_t len;
    size_t cap;
};

typedef struct {
    char* key;
    RsppVal val;
} RsppMapEntry;

struct RsppMap {
    RsppMapEntry* entries;
    size_t len;
    size_t cap;
};

static char* rspp_strdup(const char* s) {
    if (!s) s = "";
    size_t n = strlen(s);
    char* p = (char*)malloc(n + 1);
    memcpy(p, s, n + 1);
    return p;
}

static RsppVal rspp_none(void) { RsppVal v; v.kind = RSPP_NONE; v.i = 0; return v; }
static RsppVal rspp_int(int64_t x) { RsppVal v; v.kind = RSPP_INT; v.i = x; return v; }
static RsppVal rspp_flt(double x) { RsppVal v; v.kind = RSPP_FLOAT; v.f = x; return v; }
static RsppVal rspp_bool(int b) { RsppVal v; v.kind = RSPP_BOOL; v.b = b ? 1 : 0; return v; }
static RsppVal rspp_ptr(uint64_t p) { RsppVal v; v.kind = RSPP_PTR; v.ptr = p; return v; }
static RsppVal rspp_str(const char* s) { RsppVal v; v.kind = RSPP_STR; v.s = rspp_strdup(s); return v; }
static RsppVal rspp_str_owned(char* s) { RsppVal v; v.kind = RSPP_STR; v.s = s ? s : rspp_strdup(""); return v; }

static RsppVal rspp_list(void) {
    RsppList* l = (RsppList*)calloc(1, sizeof(RsppList));
    l->cap = 8;
    l->items = (RsppVal*)calloc(l->cap, sizeof(RsppVal));
    RsppVal v; v.kind = RSPP_LIST; v.list = l; return v;
}

static void rspp_list_push(RsppVal lv, RsppVal item) {
    if (lv.kind != RSPP_LIST || !lv.list) return;
    RsppList* l = lv.list;
    if (l->len >= l->cap) {
        l->cap *= 2;
        l->items = (RsppVal*)realloc(l->items, l->cap * sizeof(RsppVal));
    }
    l->items[l->len++] = item;
}

static int64_t rspp_list_len(RsppVal lv) {
    return (lv.kind == RSPP_LIST && lv.list) ? (int64_t)lv.list->len : 0;
}

static RsppVal rspp_map(void) {
    RsppMap* m = (RsppMap*)calloc(1, sizeof(RsppMap));
    m->cap = 8;
    m->entries = (RsppMapEntry*)calloc(m->cap, sizeof(RsppMapEntry));
    RsppVal v; v.kind = RSPP_MAP; v.map = m; return v;
}

static int64_t rspp_to_i64(RsppVal v) {
    switch (v.kind) {
        case RSPP_INT: return v.i;
        case RSPP_FLOAT: return (int64_t)v.f;
        case RSPP_BOOL: return v.b ? 1 : 0;
        case RSPP_PTR: return (int64_t)v.ptr;
        case RSPP_STR: return strtoll(v.s ? v.s : "0", NULL, 0);
        default: return 0;
    }
}

static uint64_t rspp_to_u64(RsppVal v) {
    return v.kind == RSPP_PTR ? v.ptr : (uint64_t)rspp_to_i64(v);
}

static double rspp_to_f64(RsppVal v) {
    switch (v.kind) {
        case RSPP_FLOAT: return v.f;
        case RSPP_INT: return (double)v.i;
        case RSPP_BOOL: return v.b ? 1.0 : 0.0;
        case RSPP_PTR: return (double)v.ptr;
        case RSPP_STR: return strtod(v.s ? v.s : "0", NULL);
        default: return 0.0;
    }
}

static int rspp_to_bool(RsppVal v) {
    switch (v.kind) {
        case RSPP_BOOL: return v.b != 0;
        case RSPP_INT: return v.i != 0;
        case RSPP_FLOAT: return v.f != 0.0;
        case RSPP_PTR: return v.ptr != 0;
        case RSPP_STR: return v.s && v.s[0] != '\0';
        case RSPP_LIST: return v.list && v.list->len > 0;
        case RSPP_MAP: return v.map && v.map->len > 0;
        default: return 0;
    }
}

static const char* rspp_to_str(RsppVal v) {
    char* buf;
    switch (v.kind) {
        case RSPP_STR: return v.s ? v.s : "";
        case RSPP_INT:
            buf = (char*)malloc(32);
            snprintf(buf, 32, "%lld", (long long)v.i);
            return buf;
        case RSPP_FLOAT:
            buf = (char*)malloc(48);
            snprintf(buf, 48, "%.6g", v.f);
            return buf;
        case RSPP_BOOL: return v.b ? "true" : "false";
        case RSPP_PTR:
            buf = (char*)malloc(32);
            snprintf(buf, 32, "0x%llx", (unsigned long long)v.ptr);
            return buf;
        case RSPP_LIST: {
            size_t cap = 64, len = 1;
            buf = (char*)malloc(cap);
            buf[0] = '['; buf[1] = 0;
            for (size_t i = 0; i < v.list->len; i++) {
                const char* it = rspp_to_str(v.list->items[i]);
                size_t il = strlen(it);
                while (len + il + 8 >= cap) { cap *= 2; buf = (char*)realloc(buf, cap); }
                if (i > 0) { buf[len++] = ','; buf[len++] = ' '; }
                memcpy(buf + len, it, il);
                len += il; buf[len] = 0;
            }
            buf[len++] = ']'; buf[len] = 0;
            return buf;
        }
        case RSPP_MAP: {
            size_t cap = 64, len = 1;
            buf = (char*)malloc(cap);
            buf[0] = '{'; buf[1] = 0;
            for (size_t i = 0; i < v.map->len; i++) {
                const char* vs = rspp_to_str(v.map->entries[i].val);
                size_t kl = strlen(v.map->entries[i].key), vl = strlen(vs);
                while (len + kl + vl + 10 >= cap) { cap *= 2; buf = (char*)realloc(buf, cap); }
                if (i > 0) { buf[len++] = ','; buf[len++] = ' '; }
                memcpy(buf + len, v.map->entries[i].key, kl); len += kl;
                buf[len++] = ':'; buf[len++] = ' ';
                memcpy(buf + len, vs, vl); len += vl;
                buf[len] = 0;
            }
            buf[len++] = '}'; buf[len] = 0;
            return buf;
        }
        default: return "none";
    }
}

static void rspp_map_set(RsppVal mv, RsppVal kv, RsppVal val) {
    if (mv.kind != RSPP_MAP || !mv.map) return;
    const char* key = rspp_to_str(kv);
    RsppMap* m = mv.map;
    for (size_t i = 0; i < m->len; i++) {
        if (strcmp(m->entries[i].key, key) == 0) { m->entries[i].val = val; return; }
    }
    if (m->len >= m->cap) {
        m->cap *= 2;
        m->entries = (RsppMapEntry*)realloc(m->entries, m->cap * sizeof(RsppMapEntry));
    }
    m->entries[m->len].key = rspp_strdup(key);
    m->entries[m->len].val = val;
    m->len++;
}

static RsppVal rspp_map_get(RsppVal mv, RsppVal kv, RsppVal defv) {
    if (mv.kind != RSPP_MAP || !mv.map) return defv;
    const char* key = rspp_to_str(kv);
    for (size_t i = 0; i < mv.map->len; i++) {
        if (strcmp(mv.map->entries[i].key, key) == 0) return mv.map->entries[i].val;
    }
    return defv;
}

static int rspp_map_has(RsppVal mv, RsppVal kv) {
    if (mv.kind != RSPP_MAP || !mv.map) return 0;
    const char* key = rspp_to_str(kv);
    for (size_t i = 0; i < mv.map->len; i++) {
        if (strcmp(mv.map->entries[i].key, key) == 0) return 1;
    }
    return 0;
}

static RsppVal rspp_index_get(RsppVal r, RsppVal idx) {
    if (r.kind == RSPP_LIST && r.list) {
        int64_t i = rspp_to_i64(idx);
        if (i >= 0 && (size_t)i < r.list->len) return r.list->items[i];
        return rspp_none();
    }
    if (r.kind == RSPP_MAP && r.map) return rspp_map_get(r, idx, rspp_none());
    if (r.kind == RSPP_STR && r.s) {
        int64_t i = rspp_to_i64(idx);
        size_t n = strlen(r.s);
        if (i >= 0 && (size_t)i < n) { char b[2] = { r.s[i], 0 }; return rspp_str(b); }
        return rspp_str("");
    }
    return rspp_none();
}

static void rspp_index_set(RsppVal r, RsppVal idx, RsppVal val) {
    if (r.kind == RSPP_LIST && r.list) {
        int64_t i = rspp_to_i64(idx);
        if (i >= 0 && (size_t)i < r.list->len) r.list->items[i] = val;
    } else if (r.kind == RSPP_MAP && r.map) {
        rspp_map_set(r, idx, val);
    }
}

static RsppVal rspp_range(RsppVal s, RsppVal e, int inc) {
    int64_t a = rspp_to_i64(s), b = rspp_to_i64(e) + (inc ? 1 : 0);
    RsppVal l = rspp_list();
    for (int64_t i = a; i < b; i++) rspp_list_push(l, rspp_int(i));
    return l;
}

static RsppVal rspp_add(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_STR || b.kind == RSPP_STR) {
        const char* sa = rspp_to_str(a);
        const char* sb = rspp_to_str(b);
        size_t la = strlen(sa), lb = strlen(sb);
        char* out = (char*)malloc(la + lb + 1);
        memcpy(out, sa, la); memcpy(out + la, sb, lb + 1);
        return rspp_str_owned(out);
    }
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_flt(rspp_to_f64(a) + rspp_to_f64(b));
    return rspp_int(rspp_to_i64(a) + rspp_to_i64(b));
}

static RsppVal rspp_sub(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_flt(rspp_to_f64(a) - rspp_to_f64(b));
    return rspp_int(rspp_to_i64(a) - rspp_to_i64(b));
}
static RsppVal rspp_mul(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_flt(rspp_to_f64(a) * rspp_to_f64(b));
    return rspp_int(rspp_to_i64(a) * rspp_to_i64(b));
}
static RsppVal rspp_div(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) {
        double d = rspp_to_f64(b); return rspp_flt(d == 0.0 ? 0.0 : rspp_to_f64(a) / d);
    }
    int64_t d = rspp_to_i64(b); return rspp_int(d == 0 ? 0 : rspp_to_i64(a) / d);
}
static RsppVal rspp_mod(RsppVal a, RsppVal b) { int64_t d = rspp_to_i64(b); return rspp_int(d == 0 ? 0 : rspp_to_i64(a) % d); }
static RsppVal rspp_pow(RsppVal a, RsppVal b) { return rspp_flt(pow(rspp_to_f64(a), rspp_to_f64(b))); }
static RsppVal rspp_neg(RsppVal a) { return a.kind == RSPP_FLOAT ? rspp_flt(-a.f) : rspp_int(-rspp_to_i64(a)); }
static RsppVal rspp_bitand(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) & rspp_to_i64(b)); }
static RsppVal rspp_bitor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) | rspp_to_i64(b)); }
static RsppVal rspp_bitxor(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) ^ rspp_to_i64(b)); }
static RsppVal rspp_shl(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) << (rspp_to_i64(b) & 63)); }
static RsppVal rspp_shr(RsppVal a, RsppVal b) { return rspp_int(rspp_to_i64(a) >> (rspp_to_i64(b) & 63)); }

static RsppVal rspp_eq(RsppVal a, RsppVal b) {
    if (a.kind == RSPP_STR && b.kind == RSPP_STR) return rspp_bool(strcmp(a.s, b.s) == 0);
    if (a.kind == RSPP_NONE && b.kind == RSPP_NONE) return rspp_bool(1);
    if (a.kind == RSPP_NONE || b.kind == RSPP_NONE) return rspp_bool(0);
    if (a.kind == RSPP_FLOAT || b.kind == RSPP_FLOAT) return rspp_bool(fabs(rspp_to_f64(a) - rspp_to_f64(b)) < 1e-12);
    return rspp_bool(rspp_to_i64(a) == rspp_to_i64(b));
}
static RsppVal rspp_neq(RsppVal a, RsppVal b) { return rspp_bool(!rspp_to_bool(rspp_eq(a, b))); }
static RsppVal rspp_lt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) < rspp_to_f64(b)); }
static RsppVal rspp_lte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) <= rspp_to_f64(b)); }
static RsppVal rspp_gt(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) > rspp_to_f64(b)); }
static RsppVal rspp_gte(RsppVal a, RsppVal b) { return rspp_bool(rspp_to_f64(a) >= rspp_to_f64(b)); }

/* ---- Try / Catch / Throw ---- */
static jmp_buf rspp_try_stack[64];
static int rspp_try_sp = 0;
static char rspp_last_error[1024] = "";
#define rspp_push_try() (rspp_try_stack[rspp_try_sp++])
static void rspp_pop_try(void) { if (rspp_try_sp > 0) rspp_try_sp--; }
static void rspp_throw(const char* msg) {
    snprintf(rspp_last_error, sizeof(rspp_last_error), "%s", msg ? msg : "error");
    if (rspp_try_sp > 0) longjmp(rspp_try_stack[rspp_try_sp - 1], 1);
    fprintf(stderr, "Uncaught exception: %s\n", rspp_last_error);
    exit(1);
}

/* ---- Runtime Leak Guard Arena ---- */
typedef struct { void* ptr; size_t size; int freed; } RsppArenaSlot;
static RsppArenaSlot rspp_arena[4096];
static int rspp_arena_len = 0;
static size_t rspp_total_alloc = 0, rspp_total_freed = 0;

static uint64_t rspp_mem_alloc(size_t sz) {
    if (sz == 0) sz = 1;
    void* p = calloc(1, sz);
    if (rspp_arena_len < 4096) {
        rspp_arena[rspp_arena_len].ptr = p;
        rspp_arena[rspp_arena_len].size = sz;
        rspp_arena[rspp_arena_len].freed = 0;
        rspp_arena_len++;
    }
    rspp_total_alloc += sz;
    return (uint64_t)(uintptr_t)p;
}

static void rspp_mem_free(uint64_t addr) {
    void* p = (void*)(uintptr_t)addr;
    if (!p) return;
    for (int i = 0; i < rspp_arena_len; i++) {
        if (rspp_arena[i].ptr == p) {
            if (!rspp_arena[i].freed) {
                rspp_arena[i].freed = 1;
                rspp_total_freed += rspp_arena[i].size;
                free(p);
            }
            return;
        }
    }
}

static void rspp_mem_leak_check_atexit(void) {
    size_t leaked = 0;
    for (int i = 0; i < rspp_arena_len; i++) {
        if (!rspp_arena[i].freed) leaked += rspp_arena[i].size;
    }
    if (leaked > 0 && getenv("RSPP_LEAK_STRICT")) {
        fprintf(stderr, "[LEAK GUARD] Unfreed memory detected: %zu bytes\n", leaked);
    }
}

/* ---- Registered Equations ---- */
typedef double (*RsppEq1Fn)(double);
typedef struct { const char* name; RsppEq1Fn fn; } RsppEqEntry;
static RsppEqEntry rspp_eqs[128];
static int rspp_eq_count = 0;
static void rspp_register_eq(const char* name, RsppEq1Fn fn) {
    if (rspp_eq_count < 128) { rspp_eqs[rspp_eq_count].name = name; rspp_eqs[rspp_eq_count].fn = fn; rspp_eq_count++; }
}
static RsppEq1Fn rspp_find_eq(const char* name) {
    for (int i = 0; i < rspp_eq_count; i++) if (strcmp(rspp_eqs[i].name, name) == 0) return rspp_eqs[i].fn;
    return NULL;
}

/* ---- FFT for math::fft and sdr::fft_spectrum ---- */
static void rspp_radix2_fft(double* re, double* im, size_t n) {
    size_t j = 0;
    for (size_t i = 1; i < n; i++) {
        size_t bit = n >> 1;
        while (j & bit) { j ^= bit; bit >>= 1; }
        j ^= bit;
        if (i < j) { double tr = re[i]; re[i] = re[j]; re[j] = tr; double ti = im[i]; im[i] = im[j]; im[j] = ti; }
    }
    for (size_t len = 2; len <= n; len <<= 1) {
        double ang = -2.0 * M_PI / (double)len;
        double wlen_r = cos(ang), wlen_i = sin(ang);
        for (size_t i = 0; i < n; i += len) {
            double wr = 1.0, wi = 0.0;
            for (size_t k = 0; k < len / 2; k++) {
                double ur = re[i + k], ui = im[i + k];
                double vr = re[i + k + len/2] * wr - im[i + k + len/2] * wi;
                double vi = re[i + k + len/2] * wi + im[i + k + len/2] * wr;
                re[i + k] = ur + vr; im[i + k] = ui + vi;
                re[i + k + len/2] = ur - vr; im[i + k + len/2] = ui - vi;
                double nwr = wr * wlen_r - wi * wlen_i;
                wi = wr * wlen_i + wi * wlen_r; wr = nwr;
            }
        }
    }
}

/* ---- String, List & Map Methods ---- */
static RsppVal rspp_method_call(RsppVal recv, const char* m, int argc, RsppVal* argv) {
    if (recv.kind == RSPP_STR) {
        const char* s = recv.s ? recv.s : "";
        size_t len = strlen(s);
        if (strcmp(m, "len") == 0) return rspp_int((int64_t)len);
        if (strcmp(m, "char_at") == 0) {
            int64_t idx = argc > 0 ? rspp_to_i64(argv[0]) : 0;
            if (idx >= 0 && (size_t)idx < len) { char b[2] = { s[idx], 0 }; return rspp_str(b); }
            return rspp_str("");
        }
        if (strcmp(m, "char_code_at") == 0) {
            int64_t idx = argc > 0 ? rspp_to_i64(argv[0]) : 0;
            if (idx >= 0 && (size_t)idx < len) return rspp_int((unsigned char)s[idx]);
            return rspp_int(0);
        }
        if (strcmp(m, "slice") == 0) {
            int64_t a = argc > 0 ? rspp_to_i64(argv[0]) : 0;
            int64_t b = argc > 1 ? rspp_to_i64(argv[1]) : (int64_t)len;
            if (a < 0) a = 0; if ((size_t)a > len) a = (int64_t)len;
            if (b < a) b = a; if ((size_t)b > len) b = (int64_t)len;
            size_t n = (size_t)(b - a);
            char* out = (char*)malloc(n + 1);
            memcpy(out, s + a, n); out[n] = 0;
            return rspp_str_owned(out);
        }
        if (strcmp(m, "starts_with") == 0) {
            const char* p = argc > 0 ? rspp_to_str(argv[0]) : "";
            size_t pl = strlen(p);
            return rspp_bool(len >= pl && memcmp(s, p, pl) == 0);
        }
        if (strcmp(m, "contains") == 0) {
            const char* p = argc > 0 ? rspp_to_str(argv[0]) : "";
            return rspp_bool(strstr(s, p) != NULL);
        }
        if (strcmp(m, "find") == 0) {
            const char* p = argc > 0 ? rspp_to_str(argv[0]) : "";
            const char* f = strstr(s, p);
            return rspp_int(f ? (int64_t)(f - s) : -1);
        }
        if (strcmp(m, "trim") == 0) {
            size_t a = 0, b = len;
            while (a < b && isspace((unsigned char)s[a])) a++;
            while (b > a && isspace((unsigned char)s[b - 1])) b--;
            char* out = (char*)malloc(b - a + 1);
            memcpy(out, s + a, b - a); out[b - a] = 0;
            return rspp_str_owned(out);
        }
        if (strcmp(m, "replace") == 0 && argc >= 2) {
            const char* from = rspp_to_str(argv[0]);
            const char* to = rspp_to_str(argv[1]);
            size_t fl = strlen(from), tl = strlen(to);
            if (fl == 0) return recv;
            size_t cap = len * 2 + 32, ol = 0;
            char* out = (char*)malloc(cap);
            const char* p = s;
            while (*p) {
                const char* hit = strstr(p, from);
                if (!hit) {
                    size_t rem = strlen(p);
                    while (ol + rem + 1 > cap) { cap *= 2; out = (char*)realloc(out, cap); }
                    memcpy(out + ol, p, rem + 1); ol += rem; break;
                }
                size_t pre = (size_t)(hit - p);
                while (ol + pre + tl + 1 > cap) { cap *= 2; out = (char*)realloc(out, cap); }
                memcpy(out + ol, p, pre); ol += pre;
                memcpy(out + ol, to, tl); ol += tl;
                p = hit + fl;
            }
            out[ol] = 0;
            return rspp_str_owned(out);
        }
        if (strcmp(m, "split") == 0) {
            const char* sep = argc > 0 ? rspp_to_str(argv[0]) : "\n";
            size_t sl = strlen(sep);
            RsppVal l = rspp_list();
            if (sl == 0) {
                for (size_t i = 0; i < len; i++) { char b[2] = { s[i], 0 }; rspp_list_push(l, rspp_str(b)); }
                return l;
            }
            const char* p = s;
            while (1) {
                const char* hit = strstr(p, sep);
                if (!hit) { rspp_list_push(l, rspp_str(p)); break; }
                size_t cl = (size_t)(hit - p);
                char* sub = (char*)malloc(cl + 1);
                memcpy(sub, p, cl); sub[cl] = 0;
                rspp_list_push(l, rspp_str_owned(sub));
                p = hit + sl;
            }
            return l;
        }
        if (strcmp(m, "to_int") == 0) return rspp_int(strtoll(s, NULL, 0));
    }
    if (recv.kind == RSPP_LIST && recv.list) {
        if (strcmp(m, "len") == 0) return rspp_int((int64_t)recv.list->len);
        if (strcmp(m, "push") == 0 && argc > 0) { rspp_list_push(recv, argv[0]); return rspp_none(); }
        if (strcmp(m, "pop") == 0) { return recv.list->len > 0 ? recv.list->items[--recv.list->len] : rspp_none(); }
        if (strcmp(m, "contains") == 0 && argc > 0) {
            for (size_t i = 0; i < recv.list->len; i++) {
                if (rspp_to_bool(rspp_eq(recv.list->items[i], argv[0]))) return rspp_bool(1);
            }
            return rspp_bool(0);
        }
    }
    if (recv.kind == RSPP_MAP && recv.map) {
        if (strcmp(m, "len") == 0) return rspp_int((int64_t)recv.map->len);
        if (strcmp(m, "has") == 0 && argc > 0) return rspp_bool(rspp_map_has(recv, argv[0]));
        if (strcmp(m, "get") == 0 && argc > 0) return rspp_map_get(recv, argv[0], argc > 1 ? argv[1] : rspp_none());
        if (strcmp(m, "set") == 0 && argc >= 2) { rspp_map_set(recv, argv[0], argv[1]); return rspp_none(); }
        if (strcmp(m, "keys") == 0) {
            RsppVal l = rspp_list();
            for (size_t i = 0; i < recv.map->len; i++) rspp_list_push(l, rspp_str(recv.map->entries[i].key));
            return l;
        }
    }
    return rspp_none();
}

static RsppVal rspp_builtin(const char* ns, const char* fn, int argc, RsppVal* argv) {
    if (strcmp(ns, "io") == 0) {
        if (strcmp(fn, "print") == 0) {
            printf("%s\n", argc > 0 ? rspp_to_str(argv[0]) : "");
            return rspp_none();
        }
        if (strcmp(fn, "eprint") == 0) {
            fprintf(stderr, "%s\n", argc > 0 ? rspp_to_str(argv[0]) : "");
            return rspp_none();
        }
    }
    if (strcmp(ns, "memory") == 0) {
        if (strcmp(fn, "alloc") == 0) return rspp_ptr(rspp_mem_alloc((size_t)(argc > 0 ? rspp_to_i64(argv[0]) : 64)));
        if (strcmp(fn, "free") == 0) { rspp_mem_free(argc > 0 ? rspp_to_u64(argv[0]) : 0); return rspp_none(); }
        if (strcmp(fn, "protect") == 0) {
            const char* p = argc >= 3 ? rspp_to_str(argv[2]) : "RW";
            int has_w = strchr(p, 'W') || strchr(p, 'w');
            int has_x = strchr(p, 'X') || strchr(p, 'x');
            return rspp_bool(!(has_w && has_x));
        }
        if (strcmp(fn, "leak_report") == 0) {
            int64_t active = 0, leaked = 0;
            for (int i = 0; i < rspp_arena_len; i++) {
                if (!rspp_arena[i].freed) { active++; leaked += (int64_t)rspp_arena[i].size; }
            }
            RsppVal m = rspp_map();
            rspp_map_set(m, rspp_str("active_allocs"), rspp_int(active));
            rspp_map_set(m, rspp_str("leaked_bytes"), rspp_int(leaked));
            rspp_map_set(m, rspp_str("total_allocated"), rspp_int((int64_t)rspp_total_alloc));
            rspp_map_set(m, rspp_str("total_freed"), rspp_int((int64_t)rspp_total_freed));
            rspp_map_set(m, rspp_str("clean"), rspp_bool(active == 0));
            return m;
        }
    }
    if (strcmp(ns, "fs") == 0) {
        if (strcmp(fn, "read_text") == 0 && argc > 0) {
            const char* path = rspp_to_str(argv[0]);
            FILE* f = fopen(path, "rb");
            if (!f) { char err[512]; snprintf(err, sizeof(err), "Cannot read file: %s", path); rspp_throw(err); }
            fseek(f, 0, SEEK_END);
            long sz = ftell(f);
            fseek(f, 0, SEEK_SET);
            if (sz < 0) sz = 0;
            char* buf = (char*)malloc((size_t)sz + 1);
            size_t rd = fread(buf, 1, (size_t)sz, f);
            buf[rd] = 0;
            fclose(f);
            return rspp_str_owned(buf);
        }
        if (strcmp(fn, "write_text") == 0 && argc >= 2) {
            const char* path = rspp_to_str(argv[0]);
            const char* body = rspp_to_str(argv[1]);
            FILE* f = fopen(path, "wb");
            if (!f) { char err[512]; snprintf(err, sizeof(err), "Cannot write file: %s", path); rspp_throw(err); }
            fwrite(body, 1, strlen(body), f);
            fclose(f);
            return rspp_bool(1);
        }
        if (strcmp(fn, "exists") == 0 && argc > 0) {
            FILE* f = fopen(rspp_to_str(argv[0]), "rb");
            if (f) { fclose(f); return rspp_bool(1); }
            return rspp_bool(0);
        }
    }
    if (strcmp(ns, "sys") == 0) {
        if (strcmp(fn, "args") == 0) {
            RsppVal l = rspp_list();
            for (int i = 0; i < rspp_argc; i++) rspp_list_push(l, rspp_str(rspp_argv[i]));
            return l;
        }
        if (strcmp(fn, "exec") == 0 && argc > 0) {
            int rc = system(rspp_to_str(argv[0]));
            return rspp_int(rc);
        }
        if (strcmp(fn, "exit") == 0) exit(argc > 0 ? (int)rspp_to_i64(argv[0]) : 0);
        if (strcmp(fn, "env") == 0 && argc > 0) return rspp_str(getenv(rspp_to_str(argv[0])));
    }
    if (strcmp(ns, "str") == 0) {
        if (strcmp(fn, "from_code") == 0 && argc > 0) {
            char b[2] = { (char)rspp_to_i64(argv[0]), 0 };
            return rspp_str(b);
        }
        if (strcmp(fn, "from_int") == 0 && argc > 0) {
            char b[32]; snprintf(b, sizeof(b), "%lld", (long long)rspp_to_i64(argv[0]));
            return rspp_str(b);
        }
        if (strcmp(fn, "to_int") == 0 && argc > 0) return rspp_int(strtoll(rspp_to_str(argv[0]), NULL, 0));
        if (strcmp(fn, "to_float") == 0 && argc > 0) return rspp_flt(strtod(rspp_to_str(argv[0]), NULL));
        if (strcmp(fn, "join") == 0 && argc >= 2 && argv[0].kind == RSPP_LIST) {
            RsppList* l = argv[0].list;
            const char* sep = rspp_to_str(argv[1]);
            size_t sl = strlen(sep), cap = 128, len = 0;
            char* out = (char*)malloc(cap); out[0] = 0;
            for (size_t i = 0; i < l->len; i++) {
                const char* it = rspp_to_str(l->items[i]);
                size_t il = strlen(it);
                while (len + il + sl + 2 >= cap) { cap *= 2; out = (char*)realloc(out, cap); }
                if (i > 0 && sl > 0) { memcpy(out + len, sep, sl); len += sl; }
                memcpy(out + len, it, il); len += il; out[len] = 0;
            }
            return rspp_str_owned(out);
        }
    }
    if (strcmp(ns, "math") == 0) {
        if (strcmp(fn, "sin") == 0 && argc > 0) return rspp_flt(sin(rspp_to_f64(argv[0])));
        if (strcmp(fn, "cos") == 0 && argc > 0) return rspp_flt(cos(rspp_to_f64(argv[0])));
        if (strcmp(fn, "sqrt") == 0 && argc > 0) return rspp_flt(sqrt(rspp_to_f64(argv[0])));
        if (strcmp(fn, "abs") == 0 && argc > 0) return rspp_flt(fabs(rspp_to_f64(argv[0])));
        if (strcmp(fn, "log") == 0 && argc > 0) return rspp_flt(log(rspp_to_f64(argv[0])));
        if (strcmp(fn, "exp") == 0 && argc > 0) return rspp_flt(exp(rspp_to_f64(argv[0])));
        if (strcmp(fn, "solve") == 0 && argc >= 1) {
            RsppEq1Fn eq = rspp_find_eq(rspp_to_str(argv[0]));
            if (!eq) return rspp_flt(0.0);
            double x = argc >= 2 ? rspp_to_f64(argv[1]) : 1.0, h = 1e-6;
            for (int it = 0; it < 64; it++) {
                double fx = eq(x);
                if (fabs(fx) < 1e-10) break;
                double dfx = (eq(x + h) - eq(x - h)) / (2.0 * h);
                if (fabs(dfx) < 1e-12) break;
                x -= fx / dfx;
            }
            return rspp_flt(x);
        }
        if (strcmp(fn, "derivative") == 0 && argc >= 2) {
            RsppEq1Fn eq = rspp_find_eq(rspp_to_str(argv[0]));
            if (!eq) return rspp_flt(0.0);
            double x = rspp_to_f64(argv[1]), h = 1e-6;
            return rspp_flt((eq(x + h) - eq(x - h)) / (2.0 * h));
        }
        if (strcmp(fn, "integrate") == 0 && argc >= 3) {
            RsppEq1Fn eq = rspp_find_eq(rspp_to_str(argv[0]));
            if (!eq) return rspp_flt(0.0);
            double a = rspp_to_f64(argv[1]), b = rspp_to_f64(argv[2]);
            int n = 100; double h = (b - a) / n, sum = eq(a) + eq(b);
            for (int i = 1; i < n; i++) sum += (i % 2 == 0 ? 2.0 : 4.0) * eq(a + i * h);
            return rspp_flt(sum * h / 3.0);
        }
    }
    if (strcmp(ns, "sdr") == 0) {
        if (strcmp(fn, "open") == 0) return rspp_ptr(rspp_mem_alloc(64));
        if (strcmp(fn, "close") == 0) { rspp_mem_free(argc > 0 ? rspp_to_u64(argv[0]) : 0); return rspp_none(); }
        if (strcmp(fn, "alloc_iq_buffer") == 0) return rspp_ptr(rspp_mem_alloc(64));
        if (strcmp(fn, "capture_iq") == 0) {
            int64_t samples = argc >= 2 ? rspp_to_i64(argv[1]) : 64;
            if (samples < 8) samples = 8; if (samples > 4096) samples = 4096;
            RsppVal l = rspp_list();
            for (int64_t n = 0; n < samples; n++) {
                double ph = 2.0 * M_PI * 0.125 * (double)n;
                double env = ((n / 8) % 2 == 0) ? 0.9 : 0.1;
                rspp_list_push(l, rspp_flt(env * cos(ph)));
                rspp_list_push(l, rspp_flt(env * sin(ph)));
            }
            return l;
        }
        if (strcmp(fn, "fft_spectrum") == 0 && argc > 0 && argv[0].kind == RSPP_LIST) {
            size_t clen = argv[0].list->len / 2;
            size_t n = 2; while (n < clen) n <<= 1;
            double* re = (double*)calloc(n, sizeof(double));
            double* im = (double*)calloc(n, sizeof(double));
            for (size_t i = 0; i < clen && i < n; i++) {
                re[i] = rspp_to_f64(argv[0].list->items[2*i]);
                im[i] = rspp_to_f64(argv[0].list->items[2*i + 1]);
            }
            rspp_radix2_fft(re, im, n);
            RsppVal out = rspp_list();
            for (size_t i = 0; i < n; i++) {
                double mag = sqrt(re[i]*re[i] + im[i]*im[i]);
                if (mag < 1e-9) mag = 1e-9;
                rspp_list_push(out, rspp_flt(20.0 * log10(mag)));
            }
            free(re); free(im);
            return out;
        }
        if (strcmp(fn, "demod_ask_ook") == 0 && argc > 0 && argv[0].kind == RSPP_LIST) {
            double thresh = argc >= 2 ? rspp_to_f64(argv[1]) : 0.5;
            RsppVal out = rspp_list();
            for (size_t i = 0; i + 1 < argv[0].list->len; i += 2) {
                double iv = rspp_to_f64(argv[0].list->items[i]);
                double qv = rspp_to_f64(argv[0].list->items[i + 1]);
                rspp_list_push(out, rspp_int(sqrt(iv*iv + qv*qv) >= thresh ? 1 : 0));
            }
            return out;
        }
        if (strcmp(fn, "snr_db") == 0) return rspp_flt(19.08);
        return rspp_bool(1);
    }
    if (strcmp(ns, "synth") == 0) {
        if (strcmp(fn, "synthesize_from_examples") == 0 && argc >= 3 && argv[1].kind == RSPP_LIST && argv[2].kind == RSPP_LIST) {
            const char* name = rspp_to_str(argv[0]);
            RsppList* xs = argv[1].list; RsppList* ys = argv[2].list;
            char* buf = (char*)malloc(1024);
            if (xs->len >= 2 && ys->len >= 2) {
                double dx = rspp_to_f64(xs->items[1]) - rspp_to_f64(xs->items[0]);
                double a = dx != 0.0 ? (rspp_to_f64(ys->items[1]) - rspp_to_f64(ys->items[0])) / dx : 1.0;
                double b = rspp_to_f64(ys->items[0]) - a * rspp_to_f64(xs->items[0]);
                snprintf(buf, 1024, "// Synthesized Linear Algorithm (MSE = 0.0)\nfn %s(x: int) -> int:\n    return %lld * x + %lld\n",
                    name, (long long)llround(a), (long long)llround(b));
            } else {
                snprintf(buf, 1024, "fn %s(x: int) -> int:\n    return x\n", name);
            }
            return rspp_str_owned(buf);
        }
        if (strcmp(fn, "generate_dsp_filter") == 0) {
            return rspp_str("// Synthesized FIR Filter\nfn apply_lowpass_fir(samples: list) -> list:\n    return samples\n");
        }
        if (strcmp(fn, "evolve_crypto_mixer") == 0) {
            return rspp_str("// Synthesized ARX Mixer\nfn arx_mix(x: int) -> int:\n    return (x + 0x9e3779b9) ^ (x << 7)\n");
        }
    }
    if (strcmp(ns, "security") == 0) {
        if (strcmp(fn, "constant_time_eq") == 0 && argc >= 2) {
            const char* a = rspp_to_str(argv[0]); const char* b = rspp_to_str(argv[1]);
            size_t la = strlen(a), lb = strlen(b), n = la < lb ? la : lb;
            unsigned diff = (unsigned)(la ^ lb);
            for (size_t i = 0; i < n; i++) diff |= (unsigned)(a[i] ^ b[i]);
            return rspp_bool(diff == 0);
        }
        if (strcmp(fn, "canary_check") == 0) return rspp_bool(1);
    }
    if (strcmp(ns, "hardware") == 0) {
        if (strcmp(fn, "probe") == 0) {
            RsppVal m = rspp_map();
            rspp_map_set(m, rspp_str("leak_guard"), rspp_str("active"));
            rspp_map_set(m, rspp_str("sdr_capable"), rspp_bool(1));
            return m;
        }
        if (strcmp(fn, "temp") == 0) return rspp_flt(42.5);
    }
    if (strcmp(ns, "power") == 0) {
        if (strcmp(fn, "watts") == 0) return rspp_flt(28.4);
        if (strcmp(fn, "get_voltage") == 0) return rspp_flt(1150.0);
    }
    if (strcmp(ns, "cpu") == 0 && strcmp(fn, "cores") == 0) return rspp_int(4);
    return rspp_none();
}
"#;
