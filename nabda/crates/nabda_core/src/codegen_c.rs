use std::collections::HashSet;
use std::fmt::Write as _;
use std::process::Command;

use crate::ast::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTarget {
    Native,
    Kernel,
}

pub struct CodegenOutput {
    pub c_source: String,
    pub link_libs: Vec<String>,
}

pub fn generate_c(program: &Program, target: BuildTarget) -> CodegenOutput {
    let mut gen = CCodegen::new(target);
    gen.emit_program(program);
    CodegenOutput {
        c_source: gen.out,
        link_libs: gen.link_libs,
    }
}

pub fn compile_native_binary(
    program: &Program,
    c_path: &std::path::Path,
    out_path: &std::path::Path,
    target: BuildTarget,
) -> Result<CodegenOutput, String> {
    let cg = generate_c(program, target);
    std::fs::write(c_path, &cg.c_source).map_err(|e| format!("Failed writing C output: {}", e))?;

    let mut cmd = Command::new("gcc");
    cmd.arg("-O2").arg("-std=c11");
    if target == BuildTarget::Kernel {
        cmd.args(["-ffreestanding", "-nostdlib", "-c", "-o"])
            .arg(out_path)
            .arg(c_path);
    } else {
        cmd.arg("-o").arg(out_path).arg(c_path);
        for lib in &cg.link_libs {
            cmd.arg(format!("-l{}", lib));
        }
    }

    let output = cmd.output().map_err(|e| format!("Failed invoking gcc: {}", e))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(c_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gcc compilation failed:\n{}", stderr));
    }
    Ok(cg)
}

struct CCodegen {
    out: String,
    target: BuildTarget,
    link_libs: Vec<String>,
    declared_vars: HashSet<String>,
    var_types: std::collections::HashMap<String, &'static str>,
}

impl CCodegen {
    fn new(target: BuildTarget) -> Self {
        Self {
            out: String::new(),
            target,
            link_libs: Vec::new(),
            declared_vars: HashSet::new(),
            var_types: std::collections::HashMap::new(),
        }
    }

    fn emit_program(&mut self, program: &Program) {
        if self.target == BuildTarget::Kernel {
            self.emit_kernel_prelude();
        } else {
            self.emit_native_prelude();
        }

        // Collect attributes & extern FFI declarations
        for item in &program.items {
            if let Item::ExternFn(ef) = item {
                for attr in &ef.attrs {
                    if attr.name == "link" {
                        for lib in &attr.args {
                            if !self.link_libs.contains(lib) {
                                self.link_libs.push(lib.clone());
                            }
                        }
                    }
                }
                let ret = ef
                    .return_type
                    .as_ref()
                    .map(|t| map_type(&t.name))
                    .unwrap_or("int64_t");
                let params: Vec<String> = ef
                    .params
                    .iter()
                    .map(|p| {
                        let ty = p.ty.as_ref().map(|t| map_type(&t.name)).unwrap_or("int64_t");
                        format!("{} {}", ty, p.name)
                    })
                    .collect();
                let _ = writeln!(self.out, "extern {} {}({});", ret, ef.name, params.join(", "));
            }
        }

        // Structs & Enums
        for item in &program.items {
            match item {
                Item::Struct(s) => {
                    let _ = writeln!(self.out, "typedef struct {{");
                    for f in &s.fields {
                        let _ = writeln!(self.out, "    {} {};", map_type(&f.ty.name), f.name);
                    }
                    let _ = writeln!(self.out, "}} {};\n", s.name);
                }
                Item::Enum(e) => {
                    let _ = writeln!(self.out, "typedef enum {{");
                    for v in &e.variants {
                        let _ = writeln!(self.out, "    {}_{},", e.name, v);
                    }
                    let _ = writeln!(self.out, "}} {};\n", e.name);
                }
                Item::Const(c) => {
                    let ty = c.ty.as_ref().map(|t| map_type(&t.name)).unwrap_or("int64_t");
                    let val = self.emit_expr(&c.value);
                    let _ = writeln!(self.out, "static const {} {} = {};", ty, c.name, val);
                }
                _ => {}
            }
        }

        // Forward declare functions
        for item in &program.items {
            if let Item::Fn(f) = item {
                for attr in &f.attrs {
                    if attr.name == "link" {
                        for lib in &attr.args {
                            if !self.link_libs.contains(lib) {
                                self.link_libs.push(lib.clone());
                            }
                        }
                    }
                }
                if f.name == "main" {
                    continue;
                }
                let ret = f
                    .return_type
                    .as_ref()
                    .map(|t| map_type(&t.name))
                    .unwrap_or("int64_t");
                let params: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| {
                        let ty = p.ty.as_ref().map(|t| map_type(&t.name)).unwrap_or("int64_t");
                        format!("{} {}", ty, p.name)
                    })
                    .collect();
                let _ = writeln!(self.out, "{} {}({});", ret, f.name, params.join(", "));
            }
        }
        let _ = writeln!(self.out);

        // Function bodies
        let mut has_main = false;
        let mut top_stmts = Vec::new();
        for item in &program.items {
            match item {
                Item::Fn(f) => {
                    if f.name == "main" {
                        has_main = true;
                        self.emit_main_fn(f);
                    } else {
                        self.emit_fn(f);
                    }
                }
                Item::Stmt(s) => top_stmts.push(s),
                _ => {}
            }
        }

        if !has_main {
            let entry_name = if self.target == BuildTarget::Kernel { "kernel_main" } else { "main" };
            let _ = writeln!(self.out, "int {}(void) {{", entry_name);
            self.declared_vars.clear();
            self.var_types.clear();
            for st in top_stmts {
                self.emit_stmt(st, 1);
            }
            let _ = writeln!(self.out, "    return 0;\n}}");
        }
    }

    fn emit_native_prelude(&mut self) {
        self.out.push_str(
            r#"/* Generated by Nabda Compiler (nabdac) — Standalone Native C11 Target */
#define _POSIX_C_SOURCE 199309L
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <time.h>

static inline const char* nb_str_cat(const char* a, const char* b) {
    size_t la = strlen(a), lb = strlen(b);
    char* r = (char*)malloc(la + lb + 1);
    memcpy(r, a, la);
    memcpy(r + la, b, lb + 1);
    return r;
}
static inline const char* nb_fmt_i64(int64_t v) {
    char* b = (char*)malloc(32);
    snprintf(b, 32, "%lld", (long long)v);
    return b;
}
static inline const char* nb_fmt_u64(uint64_t v) {
    char* b = (char*)malloc(32);
    snprintf(b, 32, "0x%llX", (unsigned long long)v);
    return b;
}
static inline const char* nb_fmt_f64(double v) {
    char* b = (char*)malloc(32);
    snprintf(b, 32, "%.3f", v);
    return b;
}
static inline uint64_t nb_alloc(int64_t sz) { return (uint64_t)(uintptr_t)malloc((size_t)sz); }
static inline uint64_t nb_alloc_zeroed(int64_t sz) { return (uint64_t)(uintptr_t)calloc(1, (size_t)sz); }
static inline void nb_free(uint64_t addr) { free((void*)(uintptr_t)addr); }
static inline uint64_t nb_read_u8(uint64_t a) { return *(volatile uint8_t*)(uintptr_t)a; }
static inline uint64_t nb_read_u16(uint64_t a) { return *(volatile uint16_t*)(uintptr_t)a; }
static inline uint64_t nb_read_u32(uint64_t a) { return *(volatile uint32_t*)(uintptr_t)a; }
static inline uint64_t nb_read_u64(uint64_t a) { return *(volatile uint64_t*)(uintptr_t)a; }
static inline void nb_write_u8(uint64_t a, int64_t v) { *(volatile uint8_t*)(uintptr_t)a = (uint8_t)v; }
static inline void nb_write_u16(uint64_t a, int64_t v) { *(volatile uint16_t*)(uintptr_t)a = (uint16_t)v; }
static inline void nb_write_u32(uint64_t a, int64_t v) { *(volatile uint32_t*)(uintptr_t)a = (uint32_t)v; }
static inline void nb_write_u64(uint64_t a, int64_t v) { *(volatile uint64_t*)(uintptr_t)a = (uint64_t)v; }
static inline void nb_zero_secure(uint64_t a, int64_t len) {
    volatile uint8_t* p = (volatile uint8_t*)(uintptr_t)a;
    for (int64_t i = 0; i < len; i++) p[i] = 0;
}
static inline int64_t nb_ct_eq(const char* a, const char* b) {
    size_t la = strlen(a), lb = strlen(b);
    if (la != lb) return 0;
    volatile uint8_t diff = 0;
    for (size_t i = 0; i < la; i++) diff |= (uint8_t)a[i] ^ (uint8_t)b[i];
    return diff == 0;
}
static inline void nb_sleep_ns(uint64_t ns) {
    struct timespec ts = { (time_t)(ns / 1000000000ULL), (long)(ns % 1000000000ULL) };
    nanosleep(&ts, NULL);
}
static double nb_vcore_mv = 1180.0;
static double nb_cpu_freq_hz[16] = { 3600000000.0 };
static int64_t nb_fan_pct = 45;
static inline int64_t nb_set_volt(const char* rail, double mv) { (void)rail; nb_vcore_mv = mv; return 1; }
static inline double nb_get_volt(const char* rail) { (void)rail; return nb_vcore_mv; }
static inline int64_t nb_set_cpu_freq(int64_t c, double hz) { if (c >= 0 && c < 16) nb_cpu_freq_hz[c] = hz; return 1; }
static inline double nb_get_cpu_freq(int64_t c) { return (c >= 0 && c < 16) ? nb_cpu_freq_hz[c] : 3600000000.0; }
static inline int64_t nb_set_fan(int64_t id, int64_t pct) { (void)id; nb_fan_pct = pct; return 1; }
static inline int64_t nb_get_rpm(int64_t id) { (void)id; return 600 + nb_fan_pct * 24; }
static inline const char* nb_sha256_stub(const char* s) { (void)s; return "e3b0c44298fc1c149afbf4c8996fb924"; }

"#,
        );
    }

    fn emit_kernel_prelude(&mut self) {
        self.out.push_str(
            r#"/* Generated by Nabda Compiler (nabdac) — Bare-Metal x86_64 Kernel Target (-ffreestanding -nostdlib) */
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

__attribute__((section(".multiboot"), used))
static const uint32_t nb_multiboot2_header[] = { 0xE85250D6U, 0U, 16U, (uint32_t)(-(0xE85250D6U + 16U)) };

static inline void nb_outb(uint16_t port, uint8_t val) {
    __asm__ volatile ("outb %0, %1" : : "a"(val), "Nd"(port));
}
static inline uint8_t nb_inb(uint16_t port) {
    uint8_t ret;
    __asm__ volatile ("inb %1, %0" : "=a"(ret) : "Nd"(port));
    return ret;
}
static inline uint64_t nb_rdtsc(void) {
    uint32_t lo, hi;
    __asm__ volatile ("rdtsc" : "=a"(lo), "=d"(hi));
    return ((uint64_t)hi << 32) | lo;
}
static inline uint64_t nb_rdmsr(uint32_t msr) {
    uint32_t lo, hi;
    __asm__ volatile ("rdmsr" : "=a"(lo), "=d"(hi) : "c"(msr));
    return ((uint64_t)hi << 32) | lo;
}
static inline void nb_wrmsr(uint32_t msr, uint64_t val) {
    __asm__ volatile ("wrmsr" : : "c"(msr), "a"((uint32_t)val), "d"((uint32_t)(val >> 32)));
}
static inline void nb_cli(void) { __asm__ volatile ("cli"); }
static inline void nb_sti(void) { __asm__ volatile ("sti"); }
static inline void nb_hlt(void) { __asm__ volatile ("hlt"); }
static inline void nb_vga_print(const char* s) {
    volatile uint16_t* vga = (volatile uint16_t*)0xB8000;
    static size_t cursor = 0;
    while (*s) { vga[cursor++] = (uint16_t)(0x0F00 | (uint8_t)*s++); }
}
static inline void nb_write_u16(uint64_t a, int64_t v) { *(volatile uint16_t*)(uintptr_t)a = (uint16_t)v; }
static inline void nb_write_u32(uint64_t a, int64_t v) { *(volatile uint32_t*)(uintptr_t)a = (uint32_t)v; }
static inline uint64_t nb_read_u32(uint64_t a) { return *(volatile uint32_t*)(uintptr_t)a; }

"#,
        );
    }

    fn emit_main_fn(&mut self, f: &FnDef) {
        let entry_name = if self.target == BuildTarget::Kernel { "kernel_main" } else { "main" };
        let _ = writeln!(self.out, "int {}(void) {{", entry_name);
        self.declared_vars.clear();
        self.var_types.clear();
        for st in &f.body {
            self.emit_stmt(st, 1);
        }
        let _ = writeln!(self.out, "    return 0;\n}}\n");
    }

    fn emit_fn(&mut self, f: &FnDef) {
        let ret = f
            .return_type
            .as_ref()
            .map(|t| map_type(&t.name))
            .unwrap_or("int64_t");
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                let ty = p.ty.as_ref().map(|t| map_type(&t.name)).unwrap_or("int64_t");
                format!("{} {}", ty, p.name)
            })
            .collect();
        let _ = writeln!(self.out, "{} {}({}) {{", ret, f.name, params.join(", "));
        self.declared_vars.clear();
        self.var_types.clear();
        for p in &f.params {
            let ty = p.ty.as_ref().map(|t| map_type(&t.name)).unwrap_or("int64_t");
            self.declared_vars.insert(p.name.clone());
            self.var_types.insert(p.name.clone(), ty);
        }
        for st in &f.body {
            self.emit_stmt(st, 1);
        }
        if ret != "void" && !matches!(f.body.last(), Some(Stmt::Return { .. })) {
            let _ = writeln!(self.out, "    return 0;");
        }
        let _ = writeln!(self.out, "}}\n");
    }

    fn emit_stmt(&mut self, stmt: &Stmt, indent: usize) {
        let pad = "    ".repeat(indent);
        match stmt {
            Stmt::Let { name, ty, value, .. } | Stmt::Var { name, ty, value, .. } => {
                let c_ty = ty
                    .as_ref()
                    .map(|t| map_type(&t.name))
                    .unwrap_or_else(|| self.infer_expr_type(value));
                let val_c = self.emit_expr(value);
                self.var_types.insert(name.clone(), c_ty);
                if self.declared_vars.insert(name.clone()) {
                    let _ = writeln!(self.out, "{}{} {} = {};", pad, c_ty, name, val_c);
                } else {
                    let _ = writeln!(self.out, "{}{} = {};", pad, name, val_c);
                }
            }
            Stmt::Assign { target, value, .. } => {
                let t = self.emit_expr(target);
                let v = self.emit_expr(value);
                let _ = writeln!(self.out, "{}{} = {};", pad, t, v);
            }
            Stmt::CompoundAssign { target, op, value, .. } => {
                let t = self.emit_expr(target);
                let v = self.emit_expr(value);
                let op_str = match op {
                    BinOp::Add => "+=",
                    BinOp::Sub => "-=",
                    BinOp::Mul => "*=",
                    BinOp::Div => "/=",
                    _ => "=",
                };
                let _ = writeln!(self.out, "{}{} {} {};", pad, t, op_str, v);
            }
            Stmt::Expr { expr, .. } => {
                let e = self.emit_expr(expr);
                let _ = writeln!(self.out, "{}{};", pad, e);
            }
            Stmt::If { branches, else_branch, .. } => {
                for (i, (cond, body)) in branches.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "else if" };
                    let c = self.emit_expr(cond);
                    let _ = writeln!(self.out, "{}{} ({}) {{", pad, kw, c);
                    for s in body {
                        self.emit_stmt(s, indent + 1);
                    }
                    let _ = writeln!(self.out, "{}}}", pad);
                }
                if let Some(eb) = else_branch {
                    let _ = writeln!(self.out, "{}else {{", pad);
                    for s in eb {
                        self.emit_stmt(s, indent + 1);
                    }
                    let _ = writeln!(self.out, "{}}}", pad);
                }
            }
            Stmt::For { var, iter, body, .. } => {
                if let ExprKind::Range { start, end, inclusive } = &iter.kind {
                    let s = self.emit_expr(start);
                    let e = self.emit_expr(end);
                    let cmp = if *inclusive { "<=" } else { "<" };
                    let _ = writeln!(self.out, "{}for (int64_t {} = {}; {} {} {}; {}++) {{", pad, var, s, var, cmp, e, var);
                    for st in body {
                        self.emit_stmt(st, indent + 1);
                    }
                    let _ = writeln!(self.out, "{}}}", pad);
                }
            }
            Stmt::While { cond, body, .. } => {
                let c = self.emit_expr(cond);
                let _ = writeln!(self.out, "{}while ({}) {{", pad, c);
                for st in body {
                    self.emit_stmt(st, indent + 1);
                }
                let _ = writeln!(self.out, "{}}}", pad);
            }
            Stmt::Loop { body, .. } => {
                let _ = writeln!(self.out, "{}while (1) {{", pad);
                for st in body {
                    self.emit_stmt(st, indent + 1);
                }
                let _ = writeln!(self.out, "{}}}", pad);
            }
            Stmt::Match { subject, arms, .. } => {
                let subj = self.emit_expr(subject);
                for (i, arm) in arms.iter().enumerate() {
                    let is_wild = matches!(&arm.pattern.kind, ExprKind::Ident(n) if n == "_");
                    if is_wild {
                        let _ = writeln!(self.out, "{}else {{", pad);
                    } else {
                        let kw = if i == 0 { "if" } else { "else if" };
                        let pat = self.emit_expr(&arm.pattern);
                        let _ = writeln!(self.out, "{}{} (({}) == ({})) {{", pad, kw, subj, pat);
                    }
                    for st in &arm.body {
                        self.emit_stmt(st, indent + 1);
                    }
                    let _ = writeln!(self.out, "{}}}", pad);
                }
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    let vc = self.emit_expr(v);
                    let _ = writeln!(self.out, "{}return {};", pad, vc);
                } else {
                    let _ = writeln!(self.out, "{}return;", pad);
                }
            }
            Stmt::Break(_) => {
                let _ = writeln!(self.out, "{}break;", pad);
            }
            Stmt::Continue(_) => {
                let _ = writeln!(self.out, "{}continue;", pad);
            }
            Stmt::UnsafeHardware { body, .. } => {
                let _ = writeln!(self.out, "{}/* unsafe_hardware block */ {{", pad);
                for st in body {
                    self.emit_stmt(st, indent + 1);
                }
                let _ = writeln!(self.out, "{}}}", pad);
            }
        }
    }

    fn emit_expr(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(i) => format!("{}LL", i),
            ExprKind::HexAddr(a) => format!("0x{:X}ULL", a),
            ExprKind::Float(f) => format!("{:.6}", f),
            ExprKind::Volt(mv) => format!("{:.3}", mv),
            ExprKind::Freq(hz) => format!("{:.1}", hz),
            ExprKind::Duration(ns) => format!("{}ULL", ns),
            ExprKind::Str(s) => format!("\"{}\"", escape_c_str(s)),
            ExprKind::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            ExprKind::NoneLit => "0".to_string(),
            ExprKind::Ident(name) => name.clone(),
            ExprKind::NamespacePath { namespace, member, .. } => format!("\"{}::{}\"", namespace, member),
            ExprKind::NamespaceCall { namespace, func, args, .. } => {
                let c_args: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                match (namespace.as_str(), func.as_str()) {
                    ("io", "println") => {
                        if self.target == BuildTarget::Kernel {
                            format!("nb_vga_print({})", c_args.first().cloned().unwrap_or_else(|| "\"\"".into()))
                        } else if let Some(first) = args.first() {
                            match self.infer_expr_type(first) {
                                "const char*" => format!("printf(\"%s\\n\", {})", c_args[0]),
                                "double" => format!("printf(\"%.3f\\n\", (double)({}))", c_args[0]),
                                "uint64_t" => format!("printf(\"0x%llX\\n\", (unsigned long long)({}))", c_args[0]),
                                _ => format!("printf(\"%lld\\n\", (long long)({}))", c_args[0]),
                            }
                        } else {
                            "printf(\"\\n\")".into()
                        }
                    }
                    ("memory", "alloc") => format!("nb_alloc({})", c_args.join(", ")),
                    ("memory", "alloc_zeroed") => format!("nb_alloc_zeroed({})", c_args.join(", ")),
                    ("memory", "free") => format!("nb_free({})", c_args.join(", ")),
                    ("memory", "read_u8") => format!("nb_read_u8({})", c_args.join(", ")),
                    ("memory", "read_u16") => format!("nb_read_u16({})", c_args.join(", ")),
                    ("memory", "read_u32") => format!("nb_read_u32({})", c_args.join(", ")),
                    ("memory", "read_u64") => format!("nb_read_u64({})", c_args.join(", ")),
                    ("memory", "write_u8") => format!("nb_write_u8({})", c_args.join(", ")),
                    ("memory", "write_u16") => format!("nb_write_u16({})", c_args.join(", ")),
                    ("memory", "write_u32") => format!("nb_write_u32({})", c_args.join(", ")),
                    ("memory", "write_u64") => format!("nb_write_u64({})", c_args.join(", ")),
                    ("memory", "map_mmio") => c_args.first().cloned().unwrap_or_else(|| "0".into()),
                    ("memory", "zero_secure") => format!("nb_zero_secure({})", c_args.join(", ")),
                    ("hardware", "port_out_u8") if self.target == BuildTarget::Kernel => format!("nb_outb({})", c_args.join(", ")),
                    ("hardware", "port_in_u8") if self.target == BuildTarget::Kernel => format!("nb_inb({})", c_args.join(", ")),
                    ("hardware", "fan_set_speed") => format!("nb_set_fan({})", c_args.join(", ")),
                    ("hardware", "fan_get_rpm") => format!("nb_get_rpm({})", c_args.join(", ")),
                    ("power", "set_voltage") => format!("nb_set_volt({})", c_args.join(", ")),
                    ("power", "get_voltage") => format!("nb_get_volt({})", c_args.join(", ")),
                    ("cpu", "set_freq") if self.target == BuildTarget::Native => format!("nb_set_cpu_freq({})", c_args.join(", ")),
                    ("cpu", "get_freq") if self.target == BuildTarget::Native => format!("nb_get_cpu_freq({})", c_args.join(", ")),
                    ("cpu", "read_tsc") if self.target == BuildTarget::Kernel => "nb_rdtsc()".into(),
                    ("cpu", "read_msr") if self.target == BuildTarget::Kernel => format!("nb_rdmsr({})", c_args.join(", ")),
                    ("cpu", "write_msr") if self.target == BuildTarget::Kernel => format!("nb_wrmsr({})", c_args.join(", ")),
                    ("cpu", "disable_interrupts") if self.target == BuildTarget::Kernel => "nb_cli()".into(),
                    ("cpu", "enable_interrupts") if self.target == BuildTarget::Kernel => "nb_sti()".into(),
                    ("cpu", "halt") if self.target == BuildTarget::Kernel => "nb_hlt()".into(),
                    ("time", "sleep") if self.target == BuildTarget::Native => format!("nb_sleep_ns({})", c_args.join(", ")),
                    ("security", "constant_time_eq") if self.target == BuildTarget::Native => format!("nb_ct_eq({})", c_args.join(", ")),
                    ("security", "hash_sha256") if self.target == BuildTarget::Native => format!("nb_sha256_stub({})", c_args.join(", ")),
                    _ => format!("((int64_t)0 /* {}::{} */)", namespace, func),
                }
            }
            ExprKind::Call { callee, args } => {
                let fn_name = self.emit_expr(callee);
                let c_args: Vec<String> = args.iter().map(|a| self.emit_expr(a)).collect();
                format!("{}({})", fn_name, c_args.join(", "))
            }
            ExprKind::Field { receiver, field } => format!("{}.{}", self.emit_expr(receiver), field),
            ExprKind::Index { receiver, index } => format!("{}[{}]", self.emit_expr(receiver), self.emit_expr(index)),
            ExprKind::Binary { left, op, right } => {
                if matches!(op, BinOp::Add)
                    && self.target == BuildTarget::Native
                    && (self.infer_expr_type(left) == "const char*" || self.infer_expr_type(right) == "const char*")
                {
                    let l_str = self.emit_as_c_str(left);
                    let r_str = self.emit_as_c_str(right);
                    return format!("nb_str_cat({}, {})", l_str, r_str);
                }
                let op_s = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Mod => "%",
                    BinOp::Eq => "==",
                    BinOp::NotEq => "!=",
                    BinOp::Lt => "<",
                    BinOp::LtEq => "<=",
                    BinOp::Gt => ">",
                    BinOp::GtEq => ">=",
                    BinOp::And => "&&",
                    BinOp::Or => "||",
                    BinOp::BitAnd => "&",
                    BinOp::BitOr => "|",
                    BinOp::BitXor => "^",
                    BinOp::Shl => "<<",
                    BinOp::Shr => ">>",
                };
                format!("({} {} {})", self.emit_expr(left), op_s, self.emit_expr(right))
            }
            ExprKind::Unary { op, operand } => {
                let op_s = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                };
                format!("({}{})", op_s, self.emit_expr(operand))
            }
            ExprKind::Cast { expr, ty } => format!("(({})({}))", map_type(&ty.name), self.emit_expr(expr)),
            _ => "0".into(),
        }
    }

    fn emit_as_c_str(&self, expr: &Expr) -> String {
        let val = self.emit_expr(expr);
        match self.infer_expr_type(expr) {
            "const char*" => val,
            "double" => format!("nb_fmt_f64((double)({}))", val),
            "uint64_t" => format!("nb_fmt_u64((uint64_t)({}))", val),
            _ => format!("nb_fmt_i64((int64_t)({}))", val),
        }
    }

    fn infer_expr_type(&self, expr: &Expr) -> &'static str {
        match &expr.kind {
            ExprKind::Str(_) => "const char*",
            ExprKind::Ident(name) => self.var_types.get(name).copied().unwrap_or("int64_t"),
            ExprKind::Float(_) | ExprKind::Volt(_) | ExprKind::Freq(_) => "double",
            ExprKind::Bool(_) => "bool",
            ExprKind::HexAddr(_) | ExprKind::Duration(_) => "uint64_t",
            ExprKind::Binary { left, op, right } => {
                if matches!(op, BinOp::Add)
                    && (self.infer_expr_type(left) == "const char*" || self.infer_expr_type(right) == "const char*")
                {
                    "const char*"
                } else if self.infer_expr_type(left) == "double" || self.infer_expr_type(right) == "double" {
                    "double"
                } else {
                    "int64_t"
                }
            }
            ExprKind::NamespaceCall { namespace, func, .. } => match (namespace.as_str(), func.as_str()) {
                ("memory", "alloc" | "alloc_zeroed" | "alloc_aligned" | "map_mmio") => "uint64_t",
                ("power", "get_voltage") | ("cpu", "get_freq") => "double",
                ("security", "hash_sha256" | "hmac_sha256") => "const char*",
                _ => "int64_t",
            },
            _ => "int64_t",
        }
    }
}

fn map_type(t: &str) -> &'static str {
    match t {
        "int" | "i64" => "int64_t",
        "i32" => "int32_t",
        "u8" => "uint8_t",
        "u16" => "uint16_t",
        "u32" => "uint32_t",
        "u64" | "addr" | "ptr" | "time" => "uint64_t",
        "float" | "f64" | "volt" | "freq" => "double",
        "bool" => "bool",
        "str" => "const char*",
        "void" => "void",
        _ => "int64_t",
    }
}

fn escape_c_str(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
