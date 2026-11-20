use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

use crate::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Addr(u64),
    Float(f64),
    Volt(f64),     // millivolts
    Freq(f64),     // Hz
    Duration(u64), // nanoseconds
    Bool(bool),
    Str(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    None,
}

impl Value {
    pub fn display(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Addr(a) => format!("0x{:X}", a),
            Value::Float(f) => format!("{:.4}", f).trim_end_matches('0').trim_end_matches('.').to_string(),
            Value::Volt(mv) => {
                if *mv >= 1000.0 {
                    format!("{:.3}V", mv / 1000.0)
                } else {
                    format!("{:.1}mV", mv)
                }
            }
            Value::Freq(hz) => {
                if *hz >= 1_000_000_000.0 {
                    format!("{:.2}GHz", hz / 1_000_000_000.0)
                } else if *hz >= 1_000_000.0 {
                    format!("{:.1}MHz", hz / 1_000_000.0)
                } else if *hz >= 1_000.0 {
                    format!("{:.1}kHz", hz / 1_000.0)
                } else {
                    format!("{:.0}Hz", hz)
                }
            }
            Value::Duration(ns) => {
                if *ns >= 1_000_000_000 {
                    format!("{}s", ns / 1_000_000_000)
                } else if *ns >= 1_000_000 {
                    format!("{}ms", ns / 1_000_000)
                } else if *ns >= 1_000 {
                    format!("{}us", ns / 1_000)
                } else {
                    format!("{}ns", ns)
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => s.clone(),
            Value::List(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.display()).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Map(entries) => {
                let inner: Vec<String> = entries.iter().map(|(k, v)| format!("{}: {}", k, v.display())).collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Struct { name, fields } => {
                let inner: Vec<String> = fields.iter().map(|(k, v)| format!("{}: {}", k, v.display())).collect();
                format!("{} {{ {} }}", name, inner.join(", "))
            }
            Value::None => "none".to_string(),
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Addr(a) => *a != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::None => false,
            _ => true,
        }
    }

    pub fn as_i64(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Addr(a) => *a as i64,
            Value::Float(f) => *f as i64,
            Value::Volt(mv) => *mv as i64,
            Value::Freq(hz) => *hz as i64,
            Value::Duration(ns) => *ns as i64,
            Value::Bool(b) => if *b { 1 } else { 0 },
            _ => 0,
        }
    }

    pub fn as_u64(&self) -> u64 {
        match self {
            Value::Addr(a) => *a,
            Value::Int(i) => *i as u64,
            Value::Duration(ns) => *ns,
            _ => self.as_i64() as u64,
        }
    }

    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Float(f) => *f,
            Value::Int(i) => *i as f64,
            Value::Volt(mv) => *mv,
            Value::Freq(hz) => *hz,
            Value::Duration(ns) => *ns as f64,
            Value::Addr(a) => *a as f64,
            _ => 0.0,
        }
    }
}

enum Flow {
    None(Value),
    Return(Value),
    Break,
    Continue,
}

#[derive(Debug, Clone)]
pub struct MemRegion {
    pub base: u64,
    pub data: Vec<u8>,
    pub perm: String,
    pub freed: bool,
}

#[derive(Debug, Clone)]
pub struct HardwareState {
    pub rails_mv: HashMap<String, f64>,
    pub rail_amps: HashMap<String, f64>,
    pub cpu_freqs_hz: Vec<f64>,
    pub fan_duty: Vec<i64>,
    pub gpu_clocks_hz: Vec<f64>,
    pub gpu_vram_hz: Vec<f64>,
    pub gpu_power_w: Vec<f64>,
    pub gpio_modes: HashMap<i64, String>,
    pub gpio_levels: HashMap<i64, i64>,
    pub io_ports: HashMap<u16, u32>,
    pub msrs: HashMap<u32, u64>,
    pub power_limit_w: f64,
    pub display_mode: (i64, i64, i64),
    pub display_brightness: i64,
    pub framebuffer_pixels: usize,
    pub kernel_tasks: Vec<(i64, String, i64)>,
    pub firewall_rules: Vec<(String, String, i64)>,
    pub tpm_pcrs: HashMap<i64, String>,
    pub aslr_enabled: bool,
    pub dep_nx_enabled: bool,
    pub stack_canary_enabled: bool,
    pub interrupts_enabled: bool,
    pub gdt_ready: bool,
    pub idt_ready: bool,
    pub paging_ready: bool,
}

impl Default for HardwareState {
    fn default() -> Self {
        let mut rails_mv = HashMap::new();
        rails_mv.insert("VCORE".into(), 1180.0);
        rails_mv.insert("VDDQ".into(), 1200.0);
        rails_mv.insert("GPU_CORE".into(), 980.0);
        rails_mv.insert("SOC_IO".into(), 1050.0);

        let mut rail_amps = HashMap::new();
        rail_amps.insert("VCORE".into(), 38.5);
        rail_amps.insert("VDDQ".into(), 6.2);
        rail_amps.insert("GPU_CORE".into(), 45.0);
        rail_amps.insert("SOC_IO".into(), 4.1);

        let mut tpm_pcrs = HashMap::new();
        tpm_pcrs.insert(0, "0000000000000000000000000000000000000000000000000000000000000000".into());

        Self {
            rails_mv,
            rail_amps,
            cpu_freqs_hz: vec![3_600_000_000.0; 8],
            fan_duty: vec![45, 40, 50],
            gpu_clocks_hz: vec![1_650_000_000.0],
            gpu_vram_hz: vec![7_000_000_000.0],
            gpu_power_w: vec![180.0],
            gpio_modes: HashMap::new(),
            gpio_levels: HashMap::new(),
            io_ports: HashMap::new(),
            msrs: HashMap::new(),
            power_limit_w: 95.0,
            display_mode: (1920, 1080, 60),
            display_brightness: 85,
            framebuffer_pixels: 0,
            kernel_tasks: Vec::new(),
            firewall_rules: Vec::new(),
            tpm_pcrs,
            aslr_enabled: true,
            dep_nx_enabled: true,
            stack_canary_enabled: true,
            interrupts_enabled: true,
            gdt_ready: false,
            idt_ready: false,
            paging_ready: false,
        }
    }
}

pub struct RuntimeResult {
    pub output: String,
    pub exit_value: Value,
    pub hardware: HardwareState,
}

pub fn execute(program: &Program) -> Result<RuntimeResult, String> {
    let mut vm = Vm::new();
    vm.load_program(program);
    let val = vm.run(program)?;
    Ok(RuntimeResult {
        output: vm.stdout,
        exit_value: val,
        hardware: vm.hw,
    })
}

pub struct Vm {
    fns: HashMap<String, FnDef>,
    globals: HashMap<String, Value>,
    scopes: Vec<HashMap<String, Value>>,
    next_addr: u64,
    regions: Vec<MemRegion>,
    pub hw: HardwareState,
    pub stdout: String,
    start_instant: Instant,
    in_unsafe_hardware: bool,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            fns: HashMap::new(),
            globals: HashMap::new(),
            scopes: vec![HashMap::new()],
            next_addr: 0x1000_0000,
            regions: Vec::new(),
            hw: HardwareState::default(),
            stdout: String::new(),
            start_instant: Instant::now(),
            in_unsafe_hardware: false,
        }
    }

    pub fn load_program(&mut self, program: &Program) {
        for item in &program.items {
            if let Item::Fn(f) = item {
                self.fns.insert(f.name.clone(), f.clone());
            }
        }
    }

    pub fn run(&mut self, program: &Program) -> Result<Value, String> {
        let mut last = Value::None;
        for item in &program.items {
            match item {
                Item::Const(c) => {
                    let v = self.eval_expr(&c.value)?;
                    self.globals.insert(c.name.clone(), v);
                }
                Item::Stmt(s) => {
                    if let Flow::Return(v) = self.exec_stmt(s)? {
                        return Ok(v);
                    }
                }
                _ => {}
            }
        }
        if let Some(main_fn) = self.fns.get("main").cloned() {
            last = self.call_fn(&main_fn, vec![])?;
        }
        Ok(last)
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn set_var(&mut self, name: &str, val: Value) {
        if let Some(sc) = self.scopes.last_mut() {
            sc.insert(name.to_string(), val);
        }
    }

    fn update_var(&mut self, name: &str, val: Value) -> bool {
        for sc in self.scopes.iter_mut().rev() {
            if sc.contains_key(name) {
                sc.insert(name.to_string(), val);
                return true;
            }
        }
        if self.globals.contains_key(name) {
            self.globals.insert(name.to_string(), val);
            return true;
        }
        false
    }

    fn get_var(&self, name: &str) -> Option<Value> {
        for sc in self.scopes.iter().rev() {
            if let Some(v) = sc.get(name) {
                return Some(v.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    fn call_fn(&mut self, f: &FnDef, args: Vec<Value>) -> Result<Value, String> {
        self.push_scope();
        for (idx, param) in f.params.iter().enumerate() {
            let v = args.get(idx).cloned().unwrap_or(Value::None);
            self.set_var(&param.name, v);
        }
        let mut res = Value::None;
        for st in &f.body {
            match self.exec_stmt(st)? {
                Flow::Return(v) => {
                    res = v;
                    break;
                }
                Flow::None(v) => res = v,
                Flow::Break | Flow::Continue => break,
            }
        }
        self.pop_scope();
        Ok(res)
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Flow, String> {
        self.push_scope();
        let mut last = Value::None;
        for st in stmts {
            match self.exec_stmt(st)? {
                Flow::None(v) => last = v,
                other => {
                    self.pop_scope();
                    return Ok(other);
                }
            }
        }
        self.pop_scope();
        Ok(Flow::None(last))
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Flow, String> {
        match stmt {
            Stmt::Let { name, value, .. } | Stmt::Var { name, value, .. } => {
                let v = self.eval_expr(value)?;
                self.set_var(name, v);
                Ok(Flow::None(Value::None))
            }
            Stmt::Assign { target, value, .. } => {
                let v = self.eval_expr(value)?;
                if let ExprKind::Ident(name) = &target.kind {
                    self.update_var(name, v);
                }
                Ok(Flow::None(Value::None))
            }
            Stmt::CompoundAssign { target, op, value, .. } => {
                let rhs = self.eval_expr(value)?;
                if let ExprKind::Ident(name) = &target.kind {
                    let cur = self.get_var(name).unwrap_or(Value::Int(0));
                    let new_val = apply_binop(*op, cur, rhs)?;
                    self.update_var(name, new_val);
                }
                Ok(Flow::None(Value::None))
            }
            Stmt::Expr { expr, .. } => {
                let v = self.eval_expr(expr)?;
                Ok(Flow::None(v))
            }
            Stmt::If { branches, else_branch, .. } => {
                for (cond, body) in branches {
                    if self.eval_expr(cond)?.as_bool() {
                        return self.exec_block(body);
                    }
                }
                if let Some(eb) = else_branch {
                    return self.exec_block(eb);
                }
                Ok(Flow::None(Value::None))
            }
            Stmt::For { var, iter, body, .. } => {
                let list_val = self.eval_expr(iter)?;
                let items = match list_val {
                    Value::List(l) => l,
                    _ => vec![],
                };
                for item in items {
                    self.push_scope();
                    self.set_var(var, item);
                    let mut should_break = false;
                    for st in body {
                        match self.exec_stmt(st)? {
                            Flow::None(_) => {}
                            Flow::Continue => break,
                            Flow::Break => {
                                should_break = true;
                                break;
                            }
                            Flow::Return(v) => {
                                self.pop_scope();
                                return Ok(Flow::Return(v));
                            }
                        }
                    }
                    self.pop_scope();
                    if should_break {
                        break;
                    }
                }
                Ok(Flow::None(Value::None))
            }
            Stmt::While { cond, body, .. } => {
                let mut guard = 0usize;
                while self.eval_expr(cond)?.as_bool() {
                    guard += 1;
                    if guard > 1_000_000 {
                        return Err("While loop exceeded 1,000,000 iterations safety guard".into());
                    }
                    match self.exec_block(body)? {
                        Flow::None(_) | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::None(Value::None))
            }
            Stmt::Loop { body, .. } => {
                let mut guard = 0usize;
                loop {
                    guard += 1;
                    if guard > 1_000_000 {
                        return Err("Infinite loop safety guard reached (1,000,000 iterations)".into());
                    }
                    match self.exec_block(body)? {
                        Flow::None(_) | Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                    }
                }
                Ok(Flow::None(Value::None))
            }
            Stmt::Match { subject, arms, .. } => {
                let subj = self.eval_expr(subject)?;
                for arm in arms {
                    let matched = match &arm.pattern.kind {
                        ExprKind::Ident(id) if id == "_" => true,
                        _ => {
                            let pat = self.eval_expr(&arm.pattern)?;
                            pat == subj || pat.display() == subj.display()
                        }
                    };
                    if matched {
                        return self.exec_block(&arm.body);
                    }
                }
                Ok(Flow::None(Value::None))
            }
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::None,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Break(_) => Ok(Flow::Break),
            Stmt::Continue(_) => Ok(Flow::Continue),
            Stmt::UnsafeHardware { body, .. } => {
                let prev = self.in_unsafe_hardware;
                self.in_unsafe_hardware = true;
                let flow = self.exec_block(body)?;
                self.in_unsafe_hardware = prev;
                Ok(flow)
            }
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value, String> {
        match &expr.kind {
            ExprKind::Int(v) => Ok(Value::Int(*v)),
            ExprKind::HexAddr(v) => Ok(Value::Addr(*v)),
            ExprKind::Float(v) => Ok(Value::Float(*v)),
            ExprKind::Volt(mv) => Ok(Value::Volt(*mv)),
            ExprKind::Freq(hz) => Ok(Value::Freq(*hz)),
            ExprKind::Duration(ns) => Ok(Value::Duration(*ns)),
            ExprKind::Str(s) => Ok(Value::Str(s.clone())),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::NoneLit => Ok(Value::None),
            ExprKind::Ident(name) => self
                .get_var(name)
                .ok_or_else(|| format!("Runtime error: undefined variable `{}`", name)),
            ExprKind::NamespacePath { namespace, member, .. } => Ok(match (namespace.as_str(), member.as_str()) {
                ("power", "VCORE" | "VDDQ" | "GPU_CORE" | "SOC_IO") => Value::Str(member.clone()),
                ("memory", "READ" | "WRITE" | "EXEC" | "NO_EXEC") => Value::Str(member.clone()),
                ("hardware", "OUTPUT" | "INPUT") => Value::Str(member.clone()),
                ("hardware", "HIGH") => Value::Int(1),
                ("hardware", "LOW") => Value::Int(0),
                _ => Value::Str(format!("{}::{}", namespace, member)),
            }),
            ExprKind::NamespaceCall { namespace, func, args, .. } => {
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args {
                    eval_args.push(self.eval_expr(a)?);
                }
                self.call_builtin(namespace, func, eval_args)
            }
            ExprKind::Call { callee, args } => {
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args {
                    eval_args.push(self.eval_expr(a)?);
                }
                if let ExprKind::Ident(fname) = &callee.kind {
                    if let Some(f) = self.fns.get(fname).cloned() {
                        return self.call_fn(&f, eval_args);
                    }
                    if fname == "println" || fname == "print" {
                        let line = eval_args.iter().map(|v| v.display()).collect::<Vec<_>>().join(" ");
                        if fname == "println" {
                            println!("{}", line);
                            self.stdout.push_str(&line);
                            self.stdout.push('\n');
                        } else {
                            print!("{}", line);
                            self.stdout.push_str(&line);
                        }
                        return Ok(Value::None);
                    }
                    if fname == "len" {
                        return Ok(match eval_args.first() {
                            Some(Value::List(l)) => Value::Int(l.len() as i64),
                            Some(Value::Str(s)) => Value::Int(s.len() as i64),
                            Some(Value::Map(m)) => Value::Int(m.len() as i64),
                            _ => Value::Int(0),
                        });
                    }
                    return Err(format!("Unknown function `{}`", fname));
                }
                Err("Callee is not callable".into())
            }
            ExprKind::MethodCall { receiver, method, args } => {
                let recv = self.eval_expr(receiver)?;
                let mut eval_args = Vec::new();
                for a in args {
                    eval_args.push(self.eval_expr(a)?);
                }
                match (recv, method.as_str()) {
                    (Value::List(l), "len") => Ok(Value::Int(l.len() as i64)),
                    (Value::Str(s), "len") => Ok(Value::Int(s.len() as i64)),
                    (Value::Str(s), "contains") => {
                        let sub = eval_args.first().map(|v| v.display()).unwrap_or_default();
                        Ok(Value::Bool(s.contains(&sub)))
                    }
                    (Value::Str(s), "upper") => Ok(Value::Str(s.to_uppercase())),
                    (Value::Str(s), "lower") => Ok(Value::Str(s.to_lowercase())),
                    _ => Ok(Value::None),
                }
            }
            ExprKind::Field { receiver, field } => {
                let recv = self.eval_expr(receiver)?;
                match recv {
                    Value::Struct { fields, .. } => Ok(fields.get(field).cloned().unwrap_or(Value::None)),
                    Value::Map(entries) => Ok(entries
                        .into_iter()
                        .find(|(k, _)| k == field)
                        .map(|(_, v)| v)
                        .unwrap_or(Value::None)),
                    _ => Ok(Value::None),
                }
            }
            ExprKind::Index { receiver, index } => {
                let recv = self.eval_expr(receiver)?;
                let idx = self.eval_expr(index)?;
                match recv {
                    Value::List(items) => {
                        let i = idx.as_i64() as usize;
                        Ok(items.get(i).cloned().unwrap_or(Value::None))
                    }
                    Value::Map(entries) => {
                        let key = idx.display();
                        Ok(entries.into_iter().find(|(k, _)| k == &key).map(|(_, v)| v).unwrap_or(Value::None))
                    }
                    _ => Ok(Value::None),
                }
            }
            ExprKind::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.eval_expr(it)?);
                }
                Ok(Value::List(out))
            }
            ExprKind::Map(entries) => {
                let mut out = Vec::with_capacity(entries.len());
                for (k, v) in entries {
                    out.push((self.eval_expr(k)?.display(), self.eval_expr(v)?));
                }
                Ok(Value::Map(out))
            }
            ExprKind::StructLit { name, fields } => {
                let mut map = HashMap::new();
                for (k, v) in fields {
                    map.insert(k.clone(), self.eval_expr(v)?);
                }
                Ok(Value::Struct {
                    name: name.clone(),
                    fields: map,
                })
            }
            ExprKind::Binary { left, op, right } => {
                if matches!(op, BinOp::And) {
                    let l = self.eval_expr(left)?;
                    if !l.as_bool() {
                        return Ok(Value::Bool(false));
                    }
                    let r = self.eval_expr(right)?;
                    return Ok(Value::Bool(r.as_bool()));
                }
                if matches!(op, BinOp::Or) {
                    let l = self.eval_expr(left)?;
                    if l.as_bool() {
                        return Ok(Value::Bool(true));
                    }
                    let r = self.eval_expr(right)?;
                    return Ok(Value::Bool(r.as_bool()));
                }
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                apply_binop(*op, l, r)
            }
            ExprKind::Unary { op, operand } => {
                let v = self.eval_expr(operand)?;
                Ok(match op {
                    UnaryOp::Neg => match v {
                        Value::Int(i) => Value::Int(-i),
                        Value::Float(f) => Value::Float(-f),
                        Value::Volt(mv) => Value::Volt(-mv),
                        _ => Value::Int(-v.as_i64()),
                    },
                    UnaryOp::Not => Value::Bool(!v.as_bool()),
                    UnaryOp::BitNot => Value::Int(!v.as_i64()),
                })
            }
            ExprKind::Range { start, end, inclusive } => {
                let s = self.eval_expr(start)?.as_i64();
                let e = self.eval_expr(end)?.as_i64();
                let mut items = Vec::new();
                if *inclusive {
                    for i in s..=e {
                        items.push(Value::Int(i));
                    }
                } else {
                    for i in s..e {
                        items.push(Value::Int(i));
                    }
                }
                Ok(Value::List(items))
            }
            ExprKind::Cast { expr, ty } => {
                let v = self.eval_expr(expr)?;
                Ok(match ty.name.as_str() {
                    "int" | "i64" | "u32" | "u64" | "u16" | "u8" => Value::Int(v.as_i64()),
                    "addr" => Value::Addr(v.as_u64()),
                    "float" | "f64" => Value::Float(v.as_f64()),
                    "str" => Value::Str(v.display()),
                    "bool" => Value::Bool(v.as_bool()),
                    _ => v,
                })
            }
        }
    }

    fn alloc_region(&mut self, size: usize, zeroed: bool, base_override: Option<u64>) -> u64 {
        let base = base_override.unwrap_or_else(|| {
            let b = self.next_addr;
            let aligned = ((size as u64 + 4095) / 4096).max(1) * 4096;
            self.next_addr += aligned;
            b
        });
        let data = if zeroed { vec![0u8; size] } else { vec![0xAAu8; size] };
        self.regions.push(MemRegion {
            base,
            data,
            perm: "RW".to_string(),
            freed: false,
        });
        base
    }

    fn find_region_mut(&mut self, addr: u64) -> Option<(&mut MemRegion, usize)> {
        for r in &mut self.regions {
            if !r.freed && addr >= r.base && addr < r.base + r.data.len() as u64 {
                let off = (addr - r.base) as usize;
                return Some((r, off));
            }
        }
        None
    }

    fn call_builtin(&mut self, ns: &str, func: &str, args: Vec<Value>) -> Result<Value, String> {
        match (ns, func) {
            // ---- io:: ----
            ("io", "println") => {
                let s = args.iter().map(|v| v.display()).collect::<Vec<_>>().join(" ");
                println!("{}", s);
                self.stdout.push_str(&s);
                self.stdout.push('\n');
                Ok(Value::None)
            }
            ("io", "print") => {
                let s = args.iter().map(|v| v.display()).collect::<Vec<_>>().join(" ");
                print!("{}", s);
                self.stdout.push_str(&s);
                Ok(Value::None)
            }
            ("io", "hex") => {
                let n = args.first().map(|v| v.as_u64()).unwrap_or(0);
                Ok(Value::Str(format!("0x{:X}", n)))
            }
            ("io", "bin") => {
                let n = args.first().map(|v| v.as_u64()).unwrap_or(0);
                Ok(Value::Str(format!("0b{:b}", n)))
            }

            // ---- memory:: ----
            ("memory", "alloc") => {
                let size = args.first().map(|v| v.as_i64().max(1) as usize).unwrap_or(64);
                Ok(Value::Addr(self.alloc_region(size, false, None)))
            }
            ("memory", "alloc_zeroed") => {
                let size = args.first().map(|v| v.as_i64().max(1) as usize).unwrap_or(64);
                Ok(Value::Addr(self.alloc_region(size, true, None)))
            }
            ("memory", "alloc_aligned") => {
                let size = args.first().map(|v| v.as_i64().max(1) as usize).unwrap_or(4096);
                let align = args.get(1).map(|v| v.as_u64().max(1)).unwrap_or(4096);
                let rem = self.next_addr % align;
                if rem != 0 {
                    self.next_addr += align - rem;
                }
                Ok(Value::Addr(self.alloc_region(size, true, None)))
            }
            ("memory", "free") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                for r in &mut self.regions {
                    if r.base == addr {
                        if r.freed {
                            return Err(format!("Double-free detected at address 0x{:X}", addr));
                        }
                        r.freed = true;
                        return Ok(Value::None);
                    }
                }
                Ok(Value::None)
            }
            ("memory", "map_mmio") => {
                let phys = args.first().map(|v| v.as_u64()).unwrap_or(0xB8000);
                let size = args.get(1).map(|v| v.as_i64().max(1) as usize).unwrap_or(4096);
                Ok(Value::Addr(self.alloc_region(size, true, Some(phys))))
            }
            ("memory", "write_u8") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let val = args.get(1).map(|v| v.as_i64() as u8).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    r.data[off] = val;
                }
                Ok(Value::None)
            }
            ("memory", "write_u16") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let val = args.get(1).map(|v| v.as_i64() as u16).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    let bytes = val.to_le_bytes();
                    for (i, b) in bytes.iter().enumerate() {
                        if off + i < r.data.len() {
                            r.data[off + i] = *b;
                        }
                    }
                }
                Ok(Value::None)
            }
            ("memory", "write_u32") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let val = args.get(1).map(|v| v.as_i64() as u32).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    let bytes = val.to_le_bytes();
                    for (i, b) in bytes.iter().enumerate() {
                        if off + i < r.data.len() {
                            r.data[off + i] = *b;
                        }
                    }
                }
                Ok(Value::None)
            }
            ("memory", "write_u64") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let val = args.get(1).map(|v| v.as_u64()).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    let bytes = val.to_le_bytes();
                    for (i, b) in bytes.iter().enumerate() {
                        if off + i < r.data.len() {
                            r.data[off + i] = *b;
                        }
                    }
                }
                Ok(Value::None)
            }
            ("memory", "read_u8") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    return Ok(Value::Int(r.data[off] as i64));
                }
                Ok(Value::Int(0))
            }
            ("memory", "read_u16") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    let b0 = *r.data.get(off).unwrap_or(&0);
                    let b1 = *r.data.get(off + 1).unwrap_or(&0);
                    return Ok(Value::Int(u16::from_le_bytes([b0, b1]) as i64));
                }
                Ok(Value::Int(0))
            }
            ("memory", "read_u32") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    let mut buf = [0u8; 4];
                    for i in 0..4 {
                        buf[i] = *r.data.get(off + i).unwrap_or(&0);
                    }
                    return Ok(Value::Int(u32::from_le_bytes(buf) as i64));
                }
                Ok(Value::Int(0))
            }
            ("memory", "read_u64") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    let mut buf = [0u8; 8];
                    for i in 0..8 {
                        buf[i] = *r.data.get(off + i).unwrap_or(&0);
                    }
                    return Ok(Value::Int(u64::from_le_bytes(buf) as i64));
                }
                Ok(Value::Int(0))
            }
            ("memory", "protect") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let perm = args.get(2).map(|v| v.display()).unwrap_or_else(|| "RW".into());
                if perm.to_ascii_uppercase().contains("RWX") && !self.in_unsafe_hardware {
                    return Err("Security policy rejected RWX page permission outside unsafe_hardware".into());
                }
                if let Some((r, _)) = self.find_region_mut(addr) {
                    r.perm = perm;
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(false))
            }
            ("memory", "zero_secure") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let len = args.get(1).map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                if let Some((r, off)) = self.find_region_mut(addr) {
                    for i in 0..len {
                        if off + i < r.data.len() {
                            r.data[off + i] = 0;
                        }
                    }
                }
                Ok(Value::None)
            }
            ("memory", "copy") => {
                let dst = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let src = args.get(1).map(|v| v.as_u64()).unwrap_or(0);
                let len = args.get(2).map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                let mut tmp = vec![0u8; len];
                if let Some((r, off)) = self.find_region_mut(src) {
                    for i in 0..len {
                        tmp[i] = *r.data.get(off + i).unwrap_or(&0);
                    }
                }
                if let Some((r, off)) = self.find_region_mut(dst) {
                    for i in 0..len {
                        if off + i < r.data.len() {
                            r.data[off + i] = tmp[i];
                        }
                    }
                }
                Ok(Value::None)
            }
            ("memory", "stats") => {
                let active = self.regions.iter().filter(|r| !r.freed).count() as i64;
                let bytes: usize = self.regions.iter().filter(|r| !r.freed).map(|r| r.data.len()).sum();
                Ok(Value::Map(vec![
                    ("active_regions".into(), Value::Int(active)),
                    ("allocated_bytes".into(), Value::Int(bytes as i64)),
                ]))
            }

            // ---- hardware:: ----
            ("hardware", "probe") => {
                let cpu_model = read_linux_cpu_model();
                Ok(Value::Map(vec![
                    ("cpu".into(), Value::Str(cpu_model)),
                    ("cores".into(), Value::Int(self.hw.cpu_freqs_hz.len() as i64)),
                    ("fans".into(), Value::Int(self.hw.fan_duty.len() as i64)),
                    ("gpus".into(), Value::Int(self.hw.gpu_clocks_hz.len() as i64)),
                    ("vcore_mv".into(), Value::Volt(*self.hw.rails_mv.get("VCORE").unwrap_or(&1180.0))),
                ]))
            }
            ("hardware", "pci_scan") => Ok(Value::List(vec![
                Value::Str("00:00.0 Host bridge [8086:4660]".into()),
                Value::Str("00:02.0 VGA controller [10DE:2684]".into()),
                Value::Str("00:14.0 USB xHCI controller [8086:51ED]".into()),
                Value::Str("00:1F.3 Audio/NVMe bus [8086:51C8]".into()),
            ])),
            ("hardware", "pci_read") => Ok(Value::Int(0x8086_4660)),
            ("hardware", "port_in_u8") => {
                let port = args.first().map(|v| v.as_u64() as u16).unwrap_or(0);
                Ok(Value::Int(*self.hw.io_ports.get(&port).unwrap_or(&0) as i64))
            }
            ("hardware", "port_out_u8") => {
                let port = args.first().map(|v| v.as_u64() as u16).unwrap_or(0);
                let val = args.get(1).map(|v| v.as_u64() as u32).unwrap_or(0);
                self.hw.io_ports.insert(port, val);
                Ok(Value::None)
            }
            ("hardware", "gpio_mode") => {
                let pin = args.first().map(|v| v.as_i64()).unwrap_or(0);
                let mode = args.get(1).map(|v| v.display()).unwrap_or_else(|| "OUTPUT".into());
                self.hw.gpio_modes.insert(pin, mode);
                Ok(Value::None)
            }
            ("hardware", "gpio_write") => {
                let pin = args.first().map(|v| v.as_i64()).unwrap_or(0);
                let level = args.get(1).map(|v| v.as_i64()).unwrap_or(0);
                self.hw.gpio_levels.insert(pin, level);
                Ok(Value::None)
            }
            ("hardware", "gpio_read") => {
                let pin = args.first().map(|v| v.as_i64()).unwrap_or(0);
                Ok(Value::Int(*self.hw.gpio_levels.get(&pin).unwrap_or(&0)))
            }
            ("hardware", "pwm_pulse") => {
                let pin = args.first().map(|v| v.as_i64()).unwrap_or(0);
                let duty = args.get(2).map(|v| v.as_i64()).unwrap_or(50);
                self.hw.gpio_levels.insert(pin, if duty > 0 { 1 } else { 0 });
                Ok(Value::None)
            }
            ("hardware", "fan_list") => {
                let list = self
                    .hw
                    .fan_duty
                    .iter()
                    .enumerate()
                    .map(|(i, d)| Value::Str(format!("fan#{}: {}% ({} RPM)", i, d, 600 + d * 24)))
                    .collect();
                Ok(Value::List(list))
            }
            ("hardware", "fan_set_speed") => {
                let id = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                let pct = args.get(1).map(|v| v.as_i64().clamp(0, 100)).unwrap_or(50);
                if id < self.hw.fan_duty.len() {
                    self.hw.fan_duty[id] = pct;
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(false))
            }
            ("hardware", "fan_get_rpm") => {
                let id = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                let d = *self.hw.fan_duty.get(id).unwrap_or(&40);
                Ok(Value::Int(600 + d * 24))
            }
            ("hardware", "sensor_temp") => {
                let zone = args.first().map(|v| v.display()).unwrap_or_else(|| "cpu".into());
                let base = match zone.as_str() {
                    "gpu" => 52.5,
                    "vrm" => 44.0,
                    "nvme" => 39.5,
                    _ => 48.0,
                };
                let avg_fan = self.hw.fan_duty.first().copied().unwrap_or(45) as f64;
                Ok(Value::Float(base - (avg_fan - 45.0) * 0.12))
            }

            // ---- power:: ----
            ("power", "rails") => {
                let mut keys: Vec<String> = self.hw.rails_mv.keys().cloned().collect();
                keys.sort();
                Ok(Value::List(keys.into_iter().map(Value::Str).collect()))
            }
            ("power", "get_voltage") => {
                let rail = args.first().map(|v| v.display()).unwrap_or_else(|| "VCORE".into());
                Ok(Value::Volt(*self.hw.rails_mv.get(&rail).unwrap_or(&1200.0)))
            }
            ("power", "set_voltage") => {
                let rail = args.first().map(|v| v.display()).unwrap_or_else(|| "VCORE".into());
                let mv = args.get(1).map(|v| v.as_f64()).unwrap_or(1150.0);
                if (mv > 1500.0 || mv < 500.0) && !self.in_unsafe_hardware {
                    return Err(format!("Hardware safety blocked unsafe voltage {:.1}mV on {}", mv, rail));
                }
                self.hw.rails_mv.insert(rail, mv);
                Ok(Value::Bool(true))
            }
            ("power", "get_current") => {
                let rail = args.first().map(|v| v.display()).unwrap_or_else(|| "VCORE".into());
                Ok(Value::Float(*self.hw.rail_amps.get(&rail).unwrap_or(&10.0)))
            }
            ("power", "get_watts") => {
                let rail = args.first().map(|v| v.display()).unwrap_or_else(|| "VCORE".into());
                let v = *self.hw.rails_mv.get(&rail).unwrap_or(&1200.0) / 1000.0;
                let a = *self.hw.rail_amps.get(&rail).unwrap_or(&10.0);
                Ok(Value::Float(v * a))
            }
            ("power", "set_power_limit") => {
                let w = args.first().map(|v| v.as_f64()).unwrap_or(65.0);
                self.hw.power_limit_w = w;
                Ok(Value::Bool(true))
            }
            ("power", "pulse_rail") => {
                let rail = args.first().map(|v| v.display()).unwrap_or_else(|| "SOC_IO".into());
                let mv = args.get(1).map(|v| v.as_f64()).unwrap_or(800.0);
                let _prev = *self.hw.rails_mv.get(&rail).unwrap_or(&1050.0);
                // Pulse transiently then restore
                let _ = mv;
                Ok(Value::Bool(true))
            }
            ("power", "sleep_state") => Ok(Value::None),

            // ---- cpu:: ----
            ("cpu", "info") => Ok(Value::Map(vec![
                ("model".into(), Value::Str(read_linux_cpu_model())),
                ("arch".into(), Value::Str(std::env::consts::ARCH.into())),
                ("cores".into(), Value::Int(self.hw.cpu_freqs_hz.len() as i64)),
            ])),
            ("cpu", "cores") => Ok(Value::Int(self.hw.cpu_freqs_hz.len() as i64)),
            ("cpu", "get_freq") => {
                let core = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                Ok(Value::Freq(*self.hw.cpu_freqs_hz.get(core).unwrap_or(&3_600_000_000.0)))
            }
            ("cpu", "set_freq") => {
                let core = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                let hz = args.get(1).map(|v| v.as_f64()).unwrap_or(3_600_000_000.0);
                if core < self.hw.cpu_freqs_hz.len() {
                    self.hw.cpu_freqs_hz[core] = hz;
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(false))
            }
            ("cpu", "read_msr") => {
                let msr = args.first().map(|v| v.as_u64() as u32).unwrap_or(0);
                Ok(Value::Int(*self.hw.msrs.get(&msr).unwrap_or(&0x2400) as i64))
            }
            ("cpu", "write_msr") => {
                let msr = args.first().map(|v| v.as_u64() as u32).unwrap_or(0);
                let val = args.get(1).map(|v| v.as_u64()).unwrap_or(0);
                self.hw.msrs.insert(msr, val);
                Ok(Value::None)
            }
            ("cpu", "read_tsc") => Ok(Value::Int(self.start_instant.elapsed().as_nanos() as i64 * 3)),
            ("cpu", "enable_interrupts") => {
                self.hw.interrupts_enabled = true;
                Ok(Value::None)
            }
            ("cpu", "disable_interrupts") => {
                self.hw.interrupts_enabled = false;
                Ok(Value::None)
            }
            ("cpu", "halt" | "cache_flush") => Ok(Value::None),

            // ---- gpu:: ----
            ("gpu", "list") => Ok(Value::List(vec![Value::Str("GPU#0 Nabda Virtual RTX/Compute (8192 MB VRAM)".into())])),
            ("gpu", "set_clock") => {
                let id = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                let hz = args.get(1).map(|v| v.as_f64()).unwrap_or(1_800_000_000.0);
                if id < self.hw.gpu_clocks_hz.len() {
                    self.hw.gpu_clocks_hz[id] = hz;
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(false))
            }
            ("gpu", "set_vram_clock") => {
                let id = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                let hz = args.get(1).map(|v| v.as_f64()).unwrap_or(7_000_000_000.0);
                if id < self.hw.gpu_vram_hz.len() {
                    self.hw.gpu_vram_hz[id] = hz;
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(false))
            }
            ("gpu", "set_power_limit") => {
                let id = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0);
                let w = args.get(1).map(|v| v.as_f64()).unwrap_or(200.0);
                if id < self.hw.gpu_power_w.len() {
                    self.hw.gpu_power_w[id] = w;
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(false))
            }
            ("gpu", "get_temp") => Ok(Value::Float(53.0)),
            ("gpu", "alloc_vram") => {
                let bytes = args.get(1).map(|v| v.as_i64().max(1) as usize).unwrap_or(4096);
                Ok(Value::Addr(self.alloc_region(bytes, true, None)))
            }
            ("gpu", "dispatch_compute") => Ok(Value::Bool(true)),

            // ---- display:: ----
            ("display", "list") => {
                let (w, h, hz) = self.hw.display_mode;
                Ok(Value::List(vec![Value::Str(format!("Display#0 {}x{}@{}Hz (brightness {}%)", w, h, hz, self.hw.display_brightness))]))
            }
            ("display", "set_mode") => {
                let w = args.get(1).map(|v| v.as_i64()).unwrap_or(1920);
                let h = args.get(2).map(|v| v.as_i64()).unwrap_or(1080);
                let hz = args.get(3).map(|v| v.as_i64()).unwrap_or(60);
                self.hw.display_mode = (w, h, hz);
                Ok(Value::Bool(true))
            }
            ("display", "set_brightness") => {
                let b = args.get(1).map(|v| v.as_i64().clamp(0, 100)).unwrap_or(80);
                self.hw.display_brightness = b;
                Ok(Value::Bool(true))
            }
            ("display", "draw_pixel") => {
                self.hw.framebuffer_pixels += 1;
                Ok(Value::None)
            }
            ("display", "fill_rect") => {
                let w = args.get(2).map(|v| v.as_i64().max(0) as usize).unwrap_or(1);
                let h = args.get(3).map(|v| v.as_i64().max(0) as usize).unwrap_or(1);
                self.hw.framebuffer_pixels += w * h;
                Ok(Value::None)
            }
            ("display", "clear") => {
                self.hw.framebuffer_pixels = 0;
                Ok(Value::None)
            }
            ("display", "present") => Ok(Value::None),

            // ---- time:: ----
            ("time", "now_ns") => Ok(Value::Int(self.start_instant.elapsed().as_nanos() as i64)),
            ("time", "now_ms") => Ok(Value::Int(self.start_instant.elapsed().as_millis() as i64)),
            ("time", "sleep") => {
                let ns = args.first().map(|v| v.as_u64()).unwrap_or(0);
                // Cap real sleep in tests/VM at 50ms so scripts stay snappy
                std::thread::sleep(Duration::from_nanos(ns.min(50_000_000)));
                Ok(Value::None)
            }
            ("time", "spin_wait_ns") => {
                let ns = args.first().map(|v| v.as_u64()).unwrap_or(0).min(1_000_000);
                let t = Instant::now();
                while t.elapsed().as_nanos() < ns as u128 {}
                Ok(Value::None)
            }
            ("time", "pulse_train") => {
                let count = args.first().map(|v| v.as_i64().max(0)).unwrap_or(1);
                let high = args.get(1).map(|v| v.as_u64()).unwrap_or(1000);
                let low = args.get(2).map(|v| v.as_u64()).unwrap_or(1000);
                Ok(Value::Map(vec![
                    ("pulses".into(), Value::Int(count)),
                    ("period_ns".into(), Value::Int((high + low) as i64)),
                ]))
            }
            ("time", "watchdog_start" | "watchdog_kick") => Ok(Value::None),

            // ---- kernel:: ----
            ("kernel", "boot_info") => Ok(Value::Map(vec![
                ("kernel_base".into(), Value::Addr(0xFFFF_8000_0010_0000)),
                ("ring".into(), Value::Int(0)),
                ("paging".into(), Value::Str("4-level PML4 + NX".into())),
            ])),
            ("kernel", "log") => {
                let lvl = args.first().map(|v| v.display()).unwrap_or_else(|| "INFO".into());
                let msg = args.get(1).map(|v| v.display()).unwrap_or_default();
                let line = format!("[KERNEL:{}] {}", lvl, msg);
                println!("{}", line);
                self.stdout.push_str(&line);
                self.stdout.push('\n');
                Ok(Value::None)
            }
            ("kernel", "panic") => {
                let msg = args.first().map(|v| v.display()).unwrap_or_else(|| "panic".into());
                Err(format!("KERNEL PANIC: {}", msg))
            }
            ("kernel", "init_gdt") => {
                self.hw.gdt_ready = true;
                Ok(Value::Bool(true))
            }
            ("kernel", "init_idt") => {
                self.hw.idt_ready = true;
                Ok(Value::Bool(true))
            }
            ("kernel", "init_paging") => {
                self.hw.paging_ready = true;
                Ok(Value::Bool(true))
            }
            ("kernel", "create_task") => {
                let name = args.first().map(|v| v.display()).unwrap_or_else(|| "task".into());
                let prio = args.get(1).map(|v| v.as_i64()).unwrap_or(0);
                let pid = (self.hw.kernel_tasks.len() + 1) as i64;
                self.hw.kernel_tasks.push((pid, name, prio));
                Ok(Value::Int(pid))
            }
            ("kernel", "ring_level") => Ok(Value::Int(0)),

            // ---- security:: ----
            ("security", "hash_sha256") => {
                let data = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Value::Str(sha256_hex(data.as_bytes())))
            }
            ("security", "hmac_sha256") => {
                let key = args.first().map(|v| v.display()).unwrap_or_default();
                let data = args.get(1).map(|v| v.display()).unwrap_or_default();
                Ok(Value::Str(hmac_sha256_hex(key.as_bytes(), data.as_bytes())))
            }
            ("security", "csprng_bytes") => {
                let len = args.first().map(|v| v.as_i64().clamp(1, 256) as usize).unwrap_or(16);
                Ok(Value::Str(csprng_hex(len)))
            }
            ("security", "constant_time_eq") => {
                let a = args.first().map(|v| v.display()).unwrap_or_default();
                let b = args.get(1).map(|v| v.display()).unwrap_or_default();
                Ok(Value::Bool(constant_time_equal(a.as_bytes(), b.as_bytes())))
            }
            ("security", "enable_aslr") => {
                self.hw.aslr_enabled = true;
                Ok(Value::Bool(true))
            }
            ("security", "enable_dep_nx") => {
                self.hw.dep_nx_enabled = true;
                Ok(Value::Bool(true))
            }
            ("security", "enable_stack_canary") => {
                self.hw.stack_canary_enabled = true;
                Ok(Value::Bool(true))
            }
            ("security", "audit_memory_region") => {
                let addr = args.first().map(|v| v.as_u64()).unwrap_or(0);
                let region = self.regions.iter().find(|r| addr >= r.base && addr < r.base + r.data.len() as u64);
                let (wx_safe, status) = match region {
                    Some(r) if r.perm.to_ascii_uppercase().contains("RWX") => (false, "CRITICAL_RWX_PAGE"),
                    Some(r) if r.freed => (false, "USE_AFTER_FREE"),
                    Some(_) => (true, "SECURE_WX_ENFORCED"),
                    None if addr == 0 => (false, "NULL_PAGE_UNSAFE"),
                    None => (true, "UNTRACKED_MMIO"),
                };
                Ok(Value::Map(vec![
                    ("wx_safe".into(), Value::Bool(wx_safe)),
                    ("status".into(), Value::Str(status.into())),
                ]))
            }
            ("security", "inspect_packet") => {
                let hex_str = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(inspect_packet_hex(&hex_str))
            }
            ("security", "firewall_rule") => {
                let action = args.first().map(|v| v.display()).unwrap_or_else(|| "DROP".into());
                let proto = args.get(1).map(|v| v.display()).unwrap_or_else(|| "TCP".into());
                let port = args.get(2).map(|v| v.as_i64()).unwrap_or(0);
                self.hw.firewall_rules.push((action, proto, port));
                Ok(Value::Bool(true))
            }
            ("security", "tpm_attest") => {
                let pcr = args.first().map(|v| v.as_i64()).unwrap_or(0);
                let measurement = args.get(1).map(|v| v.display()).unwrap_or_default();
                let prev = self.hw.tpm_pcrs.get(&pcr).cloned().unwrap_or_else(|| "0".repeat(64));
                let combined = format!("{}{}", prev, measurement);
                let next = sha256_hex(combined.as_bytes());
                self.hw.tpm_pcrs.insert(pcr, next.clone());
                Ok(Value::Str(next))
            }
            ("security", "verify_firmware_sig") => {
                let h = args.first().map(|v| v.display()).unwrap_or_default();
                let sig = args.get(1).map(|v| v.display()).unwrap_or_default();
                let pubk = args.get(2).map(|v| v.display()).unwrap_or_default();
                let expected = hmac_sha256_hex(pubk.as_bytes(), h.as_bytes());
                Ok(Value::Bool(constant_time_equal(expected.as_bytes(), sig.as_bytes())))
            }

            // ---- fs:: / net:: / ffi:: / math:: ----
            ("fs", "read_text") => {
                let path = args.first().map(|v| v.display()).unwrap_or_default();
                let content = fs::read_to_string(&path).map_err(|e| format!("fs::read_text error: {}", e))?;
                Ok(Value::Str(content))
            }
            ("fs", "write_text") => {
                let path = args.first().map(|v| v.display()).unwrap_or_default();
                let content = args.get(1).map(|v| v.display()).unwrap_or_default();
                fs::write(&path, content).map_err(|e| format!("fs::write_text error: {}", e))?;
                Ok(Value::Bool(true))
            }
            ("fs", "exists") => {
                let path = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Value::Bool(std::path::Path::new(&path).exists()))
            }
            ("net", "interfaces") => Ok(Value::List(vec![
                Value::Str("lo (127.0.0.1) UP".into()),
                Value::Str("eth0 (10.0.2.15) UP".into()),
            ])),
            ("net", "ping") => {
                let host = args.first().map(|v| v.display()).unwrap_or_else(|| "127.0.0.1".into());
                Ok(Value::Map(vec![
                    ("host".into(), Value::Str(host)),
                    ("rtt_ms".into(), Value::Float(0.24)),
                    ("reachable".into(), Value::Bool(true)),
                ]))
            }
            ("ffi", "load_lib") => Ok(Value::Int(1)),
            ("ffi", "sizeof") => {
                let t = args.first().map(|v| v.display()).unwrap_or_default();
                let sz = match t.as_str() {
                    "u8" | "i8" | "bool" => 1,
                    "u16" | "i16" => 2,
                    "u32" | "i32" | "f32" => 4,
                    _ => 8,
                };
                Ok(Value::Int(sz))
            }
            ("math", "min") => {
                let a = args.first().map(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get(1).map(|v| v.as_f64()).unwrap_or(0.0);
                Ok(Value::Float(a.min(b)))
            }
            ("math", "max") => {
                let a = args.first().map(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get(1).map(|v| v.as_f64()).unwrap_or(0.0);
                Ok(Value::Float(a.max(b)))
            }
            ("math", "clamp") => {
                let x = args.first().map(|v| v.as_f64()).unwrap_or(0.0);
                let lo = args.get(1).map(|v| v.as_f64()).unwrap_or(0.0);
                let hi = args.get(2).map(|v| v.as_f64()).unwrap_or(100.0);
                Ok(Value::Float(x.clamp(lo, hi)))
            }
            _ => Err(format!("Unsupported builtin call `{}::{}`", ns, func)),
        }
    }
}

fn apply_binop(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
    match op {
        BinOp::Add => match (&l, &r) {
            (Value::Str(a), b) => Ok(Value::Str(format!("{}{}", a, b.display()))),
            (a, Value::Str(b)) => Ok(Value::Str(format!("{}{}", a.display(), b))),
            (Value::Volt(a), Value::Volt(b)) => Ok(Value::Volt(a + b)),
            (Value::Freq(a), Value::Freq(b)) => Ok(Value::Freq(a + b)),
            (Value::Duration(a), Value::Duration(b)) => Ok(Value::Duration(a + b)),
            (Value::Float(_), _) | (_, Value::Float(_)) => Ok(Value::Float(l.as_f64() + r.as_f64())),
            (Value::Addr(a), _) => Ok(Value::Addr(a.wrapping_add(r.as_u64()))),
            _ => Ok(Value::Int(l.as_i64().wrapping_add(r.as_i64()))),
        },
        BinOp::Sub => match (&l, &r) {
            (Value::Volt(a), Value::Volt(b)) => Ok(Value::Volt(a - b)),
            (Value::Freq(a), Value::Freq(b)) => Ok(Value::Freq(a - b)),
            (Value::Duration(a), Value::Duration(b)) => Ok(Value::Duration(a.saturating_sub(*b))),
            (Value::Float(_), _) | (_, Value::Float(_)) => Ok(Value::Float(l.as_f64() - r.as_f64())),
            _ => Ok(Value::Int(l.as_i64().wrapping_sub(r.as_i64()))),
        },
        BinOp::Mul => match (&l, &r) {
            (Value::Float(_), _) | (_, Value::Float(_)) => Ok(Value::Float(l.as_f64() * r.as_f64())),
            _ => Ok(Value::Int(l.as_i64().wrapping_mul(r.as_i64()))),
        },
        BinOp::Div => {
            let rv = r.as_f64();
            if rv == 0.0 {
                return Err("Division by zero".into());
            }
            match (&l, &r) {
                (Value::Float(_), _) | (_, Value::Float(_)) => Ok(Value::Float(l.as_f64() / rv)),
                _ => Ok(Value::Int(l.as_i64() / r.as_i64())),
            }
        }
        BinOp::Mod => {
            let ri = r.as_i64();
            if ri == 0 {
                return Err("Modulo by zero".into());
            }
            Ok(Value::Int(l.as_i64() % ri))
        }
        BinOp::Eq => Ok(Value::Bool(l == r || l.display() == r.display())),
        BinOp::NotEq => Ok(Value::Bool(l != r && l.display() != r.display())),
        BinOp::Lt => Ok(Value::Bool(l.as_f64() < r.as_f64())),
        BinOp::LtEq => Ok(Value::Bool(l.as_f64() <= r.as_f64())),
        BinOp::Gt => Ok(Value::Bool(l.as_f64() > r.as_f64())),
        BinOp::GtEq => Ok(Value::Bool(l.as_f64() >= r.as_f64())),
        BinOp::BitAnd => Ok(Value::Int(l.as_i64() & r.as_i64())),
        BinOp::BitOr => Ok(Value::Int(l.as_i64() | r.as_i64())),
        BinOp::BitXor => Ok(Value::Int(l.as_i64() ^ r.as_i64())),
        BinOp::Shl => Ok(Value::Int(l.as_i64() << (r.as_i64() & 63))),
        BinOp::Shr => Ok(Value::Int(l.as_i64() >> (r.as_i64() & 63))),
        BinOp::And => Ok(Value::Bool(l.as_bool() && r.as_bool())),
        BinOp::Or => Ok(Value::Bool(l.as_bool() || r.as_bool())),
    }
}

fn read_linux_cpu_model() -> String {
    if let Ok(info) = fs::read_to_string("/proc/cpuinfo") {
        for line in info.lines() {
            if let Some(rest) = line.strip_prefix("model name") {
                if let Some((_, val)) = rest.split_once(':') {
                    return val.trim().to_string();
                }
            }
        }
    }
    format!("Nabda Virtual CPU ({})", std::env::consts::ARCH)
}

pub fn constant_time_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn csprng_hex(len: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; len];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    } else {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED_CAFE);
        let mut x = seed;
        for b in &mut buf {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = (x & 0xFF) as u8;
        }
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Pure-Rust FIPS 180-4 SHA-256 implementation
pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    sha256_bytes(data).iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let digest = sha256_bytes(key);
        k_block[..32].copy_from_slice(&digest);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }
    let mut o_pad = [0x5cu8; 64];
    let mut i_pad = [0x36u8; 64];
    for i in 0..64 {
        o_pad[i] ^= k_block[i];
        i_pad[i] ^= k_block[i];
    }
    let mut inner = i_pad.to_vec();
    inner.extend_from_slice(data);
    let inner_hash = sha256_bytes(&inner);

    let mut outer = o_pad.to_vec();
    outer.extend_from_slice(&inner_hash);
    sha256_hex(&outer)
}

fn inspect_packet_hex(hex_str: &str) -> Value {
    let clean: String = hex_str.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let bytes: Vec<u8> = clean
        .as_bytes()
        .chunks(2)
        .filter_map(|c| {
            if c.len() == 2 {
                u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()
            } else {
                None
            }
        })
        .collect();

    if bytes.len() < 20 {
        return Value::Map(vec![
            ("valid".into(), Value::Bool(false)),
            ("anomaly".into(), Value::Str("TRUNCATED_HEADER".into())),
            ("length".into(), Value::Int(bytes.len() as i64)),
        ]);
    }

    let version = bytes[0] >> 4;
    let proto_num = bytes[9];
    let proto = match proto_num {
        6 => "TCP",
        17 => "UDP",
        1 => "ICMP",
        _ => "OTHER",
    };
    let src_ip = format!("{}.{}.{}.{}", bytes[12], bytes[13], bytes[14], bytes[15]);
    let dst_ip = format!("{}.{}.{}.{}", bytes[16], bytes[17], bytes[18], bytes[19]);
    let mut anomaly = "NONE";
    let mut dst_port = 0i64;
    if proto == "TCP" && bytes.len() >= 34 {
        dst_port = u16::from_be_bytes([bytes[22], bytes[23]]) as i64;
        let flags = bytes[33];
        if flags == 0 {
            anomaly = "NULL_SCAN_DETECTED";
        } else if (flags & 0x29) == 0x29 {
            anomaly = "XMAS_SCAN_DETECTED";
        } else if (flags & 0x02) != 0 && (flags & 0x10) == 0 {
            anomaly = "SYN_PROBE";
        }
    }

    Value::Map(vec![
        ("valid".into(), Value::Bool(version == 4)),
        ("proto".into(), Value::Str(proto.into())),
        ("src_ip".into(), Value::Str(src_ip)),
        ("dst_ip".into(), Value::Str(dst_ip)),
        ("dst_port".into(), Value::Int(dst_port)),
        ("anomaly".into(), Value::Str(anomaly.into())),
    ])
}
