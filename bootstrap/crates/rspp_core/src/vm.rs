use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::PI;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ast::*;

#[derive(Debug, Clone)]
pub enum Val {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Ptr(u64),
    List(Rc<RefCell<Vec<Val>>>),
    Map(Rc<RefCell<BTreeMap<String, Val>>>),
    None,
}

impl Val {
    pub fn as_f64(&self) -> f64 {
        match self {
            Val::Int(i) => *i as f64,
            Val::Float(f) => *f,
            Val::Ptr(p) => *p as f64,
            Val::Bool(b) => if *b { 1.0 } else { 0.0 },
            Val::Str(s) => s.trim().parse::<f64>().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn as_i64(&self) -> i64 {
        match self {
            Val::Int(i) => *i,
            Val::Float(f) => *f as i64,
            Val::Ptr(p) => *p as i64,
            Val::Bool(b) => if *b { 1 } else { 0 },
            Val::Str(s) => parse_int_str(s),
            _ => 0,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Int(i) => *i != 0,
            Val::Float(f) => *f != 0.0,
            Val::Str(s) => !s.is_empty(),
            Val::Ptr(p) => *p != 0,
            Val::List(l) => !l.borrow().is_empty(),
            Val::Map(m) => !m.borrow().is_empty(),
            Val::None => false,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Val::Int(i) => i.to_string(),
            Val::Float(f) => {
                if (f.fract()).abs() < 1e-12 && f.abs() < 1e15 {
                    format!("{:.1}", f)
                } else {
                    format!("{}", f)
                }
            }
            Val::Bool(b) => b.to_string(),
            Val::Str(s) => s.clone(),
            Val::Ptr(p) => format!("0x{:x}", p),
            Val::List(l) => {
                let items: Vec<String> = l.borrow().iter().map(|v| v.repr()).collect();
                format!("[{}]", items.join(", "))
            }
            Val::Map(m) => {
                let items: Vec<String> = m.borrow().iter().map(|(k, v)| format!("{}: {}", k, v.repr())).collect();
                format!("{{{}}}", items.join(", "))
            }
            Val::None => "none".to_string(),
        }
    }

    pub fn repr(&self) -> String {
        match self {
            Val::Str(s) => format!("\"{}\"", s),
            _ => self.display(),
        }
    }
}

fn parse_int_str(s: &str) -> i64 {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).unwrap_or(0)
    } else if let Some(bin) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        i64::from_str_radix(bin, 2).unwrap_or(0)
    } else {
        t.parse::<i64>().unwrap_or(0)
    }
}

enum Flow {
    Normal(Val),
    Return(Val),
    Break,
    Continue,
    Throw(Val),
}

#[derive(Debug, Clone)]
struct LiveAlloc {
    size: usize,
    freed: bool,
}

#[derive(Debug, Clone)]
struct SdrDeviceState {
    #[allow(dead_code)]
    driver: String,
    center_freq_hz: f64,
    sample_rate: f64,
    gain_db: f64,
    bandwidth_hz: f64,
}

pub struct Vm {
    scopes: Vec<HashMap<String, Val>>,
    functions: HashMap<String, FnDef>,
    equations: HashMap<String, EquationDef>,
    algorithms: HashMap<String, AlgorithmDef>,
    next_ptr: u64,
    allocs: HashMap<u64, LiveAlloc>,
    total_allocated: usize,
    total_freed: usize,
    sdr_devs: HashMap<u64, SdrDeviceState>,
    voltages: HashMap<String, f64>,
    fan_percent: i64,
    cli_args: Vec<String>,
    pub output: Vec<String>,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            equations: HashMap::new(),
            algorithms: HashMap::new(),
            next_ptr: 0x10000,
            allocs: HashMap::new(),
            total_allocated: 0,
            total_freed: 0,
            sdr_devs: HashMap::new(),
            voltages: HashMap::from([("vcore".into(), 1150.0), ("vram".into(), 1350.0)]),
            fan_percent: 45,
            cli_args: Vec::new(),
            output: Vec::new(),
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.cli_args = args;
        self
    }

    pub fn run(&mut self, program: &Program) -> Result<Val, String> {
        for item in &program.items {
            match item {
                Item::Fn(f) => { self.functions.insert(f.name.clone(), f.clone()); }
                Item::Equation(eq) => { self.equations.insert(eq.name.clone(), eq.clone()); }
                Item::Algorithm(alg) => { self.algorithms.insert(alg.name.clone(), alg.clone()); }
                _ => {}
            }
        }
        for item in &program.items {
            match item {
                Item::Const(c) => {
                    let v = self.eval_expr(&c.value)?;
                    self.set_var(&c.name, v);
                }
                Item::Stmt(s) => {
                    match self.exec_stmt(s)? {
                        Flow::Return(v) => return Ok(v),
                        Flow::Throw(v) => return Err(format!("Uncaught exception: {}", v.display())),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if let Some(main_fn) = self.functions.get("main").cloned() {
            let res = self.call_fn(&main_fn, Vec::new())?;
            return Ok(res);
        }
        Ok(Val::None)
    }

    fn push_scope(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop_scope(&mut self) { self.scopes.pop(); }

    fn set_var(&mut self, name: &str, val: Val) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), val);
        }
    }

    fn assign_var(&mut self, name: &str, val: Val) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), val);
                return;
            }
        }
        self.set_var(name, val);
    }

    fn get_var(&self, name: &str) -> Option<Val> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Flow, String> {
        self.push_scope();
        let mut defers: Vec<Stmt> = Vec::new();
        let mut result = Flow::Normal(Val::None);

        for s in stmts {
            if let Stmt::Defer { stmt, .. } = s {
                defers.push((**stmt).clone());
                continue;
            }
            match self.exec_stmt(s)? {
                Flow::Normal(_) => {}
                other => {
                    result = other;
                    break;
                }
            }
        }

        for d in defers.into_iter().rev() {
            let _ = self.exec_stmt(&d)?;
        }
        self.pop_scope();
        Ok(result)
    }

    fn exec_stmt(&mut self, stmt: &Stmt) -> Result<Flow, String> {
        match stmt {
            Stmt::Let { name, value, .. } | Stmt::Var { name, value, .. } => {
                let v = self.eval_expr(value)?;
                self.set_var(name, v);
                Ok(Flow::Normal(Val::None))
            }
            Stmt::Assign { target, value, .. } => {
                let v = self.eval_expr(value)?;
                self.assign_target(target, v)?;
                Ok(Flow::Normal(Val::None))
            }
            Stmt::CompoundAssign { target, op, value, .. } => {
                let rhs = self.eval_expr(value)?;
                let cur = self.eval_expr(target)?;
                let next = self.apply_binop(*op, cur, rhs)?;
                self.assign_target(target, next)?;
                Ok(Flow::Normal(Val::None))
            }
            Stmt::Expr { expr, .. } => {
                let v = self.eval_expr(expr)?;
                Ok(Flow::Normal(v))
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
                Ok(Flow::Normal(Val::None))
            }
            Stmt::For { var, iter, body, .. } => {
                let it = self.eval_expr(iter)?;
                let items = match it {
                    Val::List(l) => l.borrow().clone(),
                    _ => Vec::new(),
                };
                for item in items {
                    self.push_scope();
                    self.set_var(var, item);
                    let res = self.exec_block(body)?;
                    self.pop_scope();
                    match res {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Throw(v) => return Ok(Flow::Throw(v)),
                        Flow::Normal(_) => {}
                    }
                }
                Ok(Flow::Normal(Val::None))
            }
            Stmt::While { cond, body, .. } => {
                while self.eval_expr(cond)?.as_bool() {
                    match self.exec_block(body)? {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Throw(v) => return Ok(Flow::Throw(v)),
                        Flow::Normal(_) => {}
                    }
                }
                Ok(Flow::Normal(Val::None))
            }
            Stmt::Loop { body, .. } => {
                loop {
                    match self.exec_block(body)? {
                        Flow::Break => break,
                        Flow::Continue => continue,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Throw(v) => return Ok(Flow::Throw(v)),
                        Flow::Normal(_) => {}
                    }
                }
                Ok(Flow::Normal(Val::None))
            }
            Stmt::Match { subject, arms, .. } => {
                let sub = self.eval_expr(subject)?;
                for arm in arms {
                    let is_wildcard = matches!(&arm.pattern.kind, ExprKind::Ident(n) if n == "_");
                    if is_wildcard {
                        return self.exec_block(&arm.body);
                    }
                    let pat = self.eval_expr(&arm.pattern)?;
                    if sub.display() == pat.display() {
                        return self.exec_block(&arm.body);
                    }
                }
                Ok(Flow::Normal(Val::None))
            }
            Stmt::Return { value, .. } => {
                let v = match value {
                    Some(e) => self.eval_expr(e)?,
                    None => Val::None,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Break(_) => Ok(Flow::Break),
            Stmt::Continue(_) => Ok(Flow::Continue),
            Stmt::UnsafeHardware { body, .. } => self.exec_block(body),
            Stmt::Defer { .. } => Ok(Flow::Normal(Val::None)),
            Stmt::TryCatch { try_body, err_var, catch_body, .. } => {
                match self.exec_block(try_body) {
                    Ok(Flow::Throw(err_val)) => {
                        self.push_scope();
                        self.set_var(err_var, err_val);
                        let r = self.exec_block(catch_body);
                        self.pop_scope();
                        r
                    }
                    Err(msg) => {
                        self.push_scope();
                        self.set_var(err_var, Val::Str(msg));
                        let r = self.exec_block(catch_body);
                        self.pop_scope();
                        r
                    }
                    other => other,
                }
            }
            Stmt::Throw { value, .. } => {
                let v = self.eval_expr(value)?;
                Ok(Flow::Throw(v))
            }
        }
    }

    fn assign_target(&mut self, target: &Expr, val: Val) -> Result<(), String> {
        match &target.kind {
            ExprKind::Ident(name) => {
                self.assign_var(name, val);
                Ok(())
            }
            ExprKind::Index { receiver, index } => {
                let r = self.eval_expr(receiver)?;
                let idx = self.eval_expr(index)?;
                match r {
                    Val::List(l) => {
                        let i = idx.as_i64() as usize;
                        let mut b = l.borrow_mut();
                        if i < b.len() { b[i] = val; }
                        Ok(())
                    }
                    Val::Map(m) => {
                        m.borrow_mut().insert(idx.display(), val);
                        Ok(())
                    }
                    _ => Err("Index assignment on non-container".into()),
                }
            }
            _ => Err("Invalid assignment target".into()),
        }
    }

    fn call_fn(&mut self, f: &FnDef, args: Vec<Val>) -> Result<Val, String> {
        self.push_scope();
        for (i, p) in f.params.iter().enumerate() {
            self.set_var(&p.name, args.get(i).cloned().unwrap_or(Val::None));
        }
        let flow = self.exec_block(&f.body)?;
        self.pop_scope();
        match flow {
            Flow::Return(v) | Flow::Normal(v) => Ok(v),
            Flow::Throw(v) => Err(v.display()),
            _ => Ok(Val::None),
        }
    }

    fn call_equation(&mut self, eq: &EquationDef, args: Vec<Val>) -> Result<Val, String> {
        self.push_scope();
        for (i, p) in eq.params.iter().enumerate() {
            self.set_var(p, Val::Float(args.get(i).map(|a| a.as_f64()).unwrap_or(0.0)));
        }
        let res = self.eval_expr(&eq.expr)?;
        self.pop_scope();
        Ok(Val::Float(res.as_f64()))
    }

    fn call_algorithm(&mut self, alg: &AlgorithmDef, args: Vec<Val>) -> Result<Val, String> {
        self.push_scope();
        for (i, p) in alg.params.iter().enumerate() {
            self.set_var(&p.name, args.get(i).cloned().unwrap_or(Val::None));
        }
        let flow = self.exec_block(&alg.body)?;
        self.pop_scope();
        match flow {
            Flow::Return(v) | Flow::Normal(v) => Ok(v),
            Flow::Throw(v) => Err(v.display()),
            _ => Ok(Val::None),
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> Result<Val, String> {
        match &expr.kind {
            ExprKind::Int(i) => Ok(Val::Int(*i)),
            ExprKind::HexAddr(a) => Ok(Val::Ptr(*a)),
            ExprKind::Float(f) | ExprKind::Volt(f) | ExprKind::Freq(f)
            | ExprKind::SampleRate(f) | ExprKind::Dbm(f) | ExprKind::Angle(f) => Ok(Val::Float(*f)),
            ExprKind::Duration(ns) => Ok(Val::Int(*ns as i64)),
            ExprKind::Str(s) => Ok(Val::Str(s.clone())),
            ExprKind::FStr(tpl) => self.eval_fstr(tpl),
            ExprKind::Bool(b) => Ok(Val::Bool(*b)),
            ExprKind::NoneLit => Ok(Val::None),
            ExprKind::Ident(n) => self.get_var(n).ok_or_else(|| format!("Undefined variable `{}`", n)),
            ExprKind::List(items) => {
                let mut v = Vec::with_capacity(items.len());
                for it in items { v.push(self.eval_expr(it)?); }
                Ok(Val::List(Rc::new(RefCell::new(v))))
            }
            ExprKind::Map(entries) => {
                let mut m = BTreeMap::new();
                for (k, v) in entries {
                    let ks = self.eval_expr(k)?.display();
                    let vs = self.eval_expr(v)?;
                    m.insert(ks, vs);
                }
                Ok(Val::Map(Rc::new(RefCell::new(m))))
            }
            ExprKind::StructLit { name: _, fields } => {
                let mut m = BTreeMap::new();
                for (k, v) in fields { m.insert(k.clone(), self.eval_expr(v)?); }
                Ok(Val::Map(Rc::new(RefCell::new(m))))
            }
            ExprKind::Range { start, end, inclusive } => {
                let s = self.eval_expr(start)?.as_i64();
                let e = self.eval_expr(end)?.as_i64();
                let last = if *inclusive { e + 1 } else { e };
                let v: Vec<Val> = (s..last).map(Val::Int).collect();
                Ok(Val::List(Rc::new(RefCell::new(v))))
            }
            ExprKind::Index { receiver, index } => {
                let r = self.eval_expr(receiver)?;
                let idx = self.eval_expr(index)?;
                match r {
                    Val::List(l) => {
                        let i = idx.as_i64();
                        let b = l.borrow();
                        if i >= 0 && (i as usize) < b.len() { Ok(b[i as usize].clone()) } else { Ok(Val::None) }
                    }
                    Val::Map(m) => Ok(m.borrow().get(&idx.display()).cloned().unwrap_or(Val::None)),
                    Val::Str(s) => {
                        let i = idx.as_i64() as usize;
                        Ok(s.chars().nth(i).map(|c| Val::Str(c.to_string())).unwrap_or(Val::None))
                    }
                    _ => Ok(Val::None),
                }
            }
            ExprKind::Field { receiver, field } => {
                let r = self.eval_expr(receiver)?;
                match r {
                    Val::Map(m) => Ok(m.borrow().get(field).cloned().unwrap_or(Val::None)),
                    _ => Ok(Val::None),
                }
            }
            ExprKind::MethodCall { receiver, method, args } => {
                let r = self.eval_expr(receiver)?;
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args { eval_args.push(self.eval_expr(a)?); }
                self.eval_method(r, method, eval_args)
            }
            ExprKind::Binary { left, op, right } => {
                if matches!(op, BinOp::And) {
                    let l = self.eval_expr(left)?;
                    if !l.as_bool() { return Ok(Val::Bool(false)); }
                    let r = self.eval_expr(right)?;
                    return Ok(Val::Bool(r.as_bool()));
                }
                if matches!(op, BinOp::Or) {
                    let l = self.eval_expr(left)?;
                    if l.as_bool() { return Ok(Val::Bool(true)); }
                    let r = self.eval_expr(right)?;
                    return Ok(Val::Bool(r.as_bool()));
                }
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                self.apply_binop(*op, l, r)
            }
            ExprKind::Unary { op, operand } => {
                let v = self.eval_expr(operand)?;
                match op {
                    UnaryOp::Neg => match v {
                        Val::Int(i) => Ok(Val::Int(-i)),
                        Val::Float(f) => Ok(Val::Float(-f)),
                        _ => Ok(Val::Float(-v.as_f64())),
                    },
                    UnaryOp::Not => Ok(Val::Bool(!v.as_bool())),
                    UnaryOp::BitNot => Ok(Val::Int(!v.as_i64())),
                }
            }
            ExprKind::Cast { expr, ty } => {
                let v = self.eval_expr(expr)?;
                match ty.name.as_str() {
                    "int" | "i64" | "u64" => Ok(Val::Int(v.as_i64())),
                    "float" | "f64" => Ok(Val::Float(v.as_f64())),
                    "str" => Ok(Val::Str(v.display())),
                    "bool" => Ok(Val::Bool(v.as_bool())),
                    _ => Ok(v),
                }
            }
            ExprKind::Call { callee, args } => {
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args { eval_args.push(self.eval_expr(a)?); }
                if let ExprKind::Ident(name) = &callee.kind {
                    if let Some(f) = self.functions.get(name).cloned() {
                        return self.call_fn(&f, eval_args);
                    }
                    if let Some(eq) = self.equations.get(name).cloned() {
                        return self.call_equation(&eq, eval_args);
                    }
                    if let Some(alg) = self.algorithms.get(name).cloned() {
                        return self.call_algorithm(&alg, eval_args);
                    }
                }
                Err("Unknown function call".into())
            }
            ExprKind::NamespacePath { namespace, member, .. } => Ok(Val::Str(format!("{}::{}", namespace, member))),
            ExprKind::NamespaceCall { namespace, func, args, .. } => {
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args { eval_args.push(self.eval_expr(a)?); }
                self.call_builtin(namespace, func, eval_args)
            }
        }
    }

    fn eval_fstr(&mut self, tpl: &str) -> Result<Val, String> {
        let mut out = String::new();
        let chars: Vec<char> = tpl.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                if chars.get(i + 1) == Some(&'{') {
                    out.push('{');
                    i += 2;
                    continue;
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
                let lx = crate::lexer::lex(expr_src.trim());
                let ps = crate::parser::parse(lx.tokens);
                if let Some(Item::Stmt(Stmt::Expr { expr, .. })) = ps.program.items.first() {
                    let val = self.eval_expr(expr)?;
                    out.push_str(&val.display());
                } else if let Some(val) = self.get_var(expr_src.trim()) {
                    out.push_str(&val.display());
                }
            } else if chars[i] == '}' && chars.get(i + 1) == Some(&'}') {
                out.push('}');
                i += 2;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        Ok(Val::Str(out))
    }

    fn eval_method(&self, receiver: Val, method: &str, args: Vec<Val>) -> Result<Val, String> {
        match receiver {
            Val::Str(s) => match method {
                "len" => Ok(Val::Int(s.len() as i64)),
                "char_at" => {
                    let idx = args.first().map(|a| a.as_i64() as usize).unwrap_or(0);
                    Ok(s.as_bytes().get(idx).map(|b| Val::Str((*b as char).to_string())).unwrap_or(Val::Str(String::new())))
                }
                "char_code_at" => {
                    let idx = args.first().map(|a| a.as_i64() as usize).unwrap_or(0);
                    Ok(Val::Int(s.as_bytes().get(idx).copied().unwrap_or(0) as i64))
                }
                "slice" => {
                    let a = args.first().map(|v| v.as_i64().max(0) as usize).unwrap_or(0).min(s.len());
                    let b = args.get(1).map(|v| v.as_i64().max(0) as usize).unwrap_or(s.len()).min(s.len());
                    if a <= b { Ok(Val::Str(s[a..b].to_string())) } else { Ok(Val::Str(String::new())) }
                }
                "starts_with" => Ok(Val::Bool(s.starts_with(&args.first().map(|v| v.display()).unwrap_or_default()))),
                "ends_with" => Ok(Val::Bool(s.ends_with(&args.first().map(|v| v.display()).unwrap_or_default()))),
                "contains" => Ok(Val::Bool(s.contains(&args.first().map(|v| v.display()).unwrap_or_default()))),
                "find" => {
                    let pat = args.first().map(|v| v.display()).unwrap_or_default();
                    Ok(Val::Int(s.find(&pat).map(|i| i as i64).unwrap_or(-1)))
                }
                "replace" => {
                    let from = args.first().map(|v| v.display()).unwrap_or_default();
                    let to = args.get(1).map(|v| v.display()).unwrap_or_default();
                    Ok(Val::Str(s.replace(&from, &to)))
                }
                "trim" => Ok(Val::Str(s.trim().to_string())),
                "upper" => Ok(Val::Str(s.to_uppercase())),
                "lower" => Ok(Val::Str(s.to_lowercase())),
                "to_int" => Ok(Val::Int(parse_int_str(&s))),
                "split" => {
                    let sep = args.first().map(|v| v.display()).unwrap_or_default();
                    let parts = if sep.is_empty() {
                        s.chars().map(|c| Val::Str(c.to_string())).collect()
                    } else {
                        s.split(&sep).map(|p| Val::Str(p.to_string())).collect()
                    };
                    Ok(Val::List(Rc::new(RefCell::new(parts))))
                }
                _ => Ok(Val::None),
            },
            Val::List(l) => match method {
                "len" => Ok(Val::Int(l.borrow().len() as i64)),
                "push" => {
                    if let Some(v) = args.into_iter().next() { l.borrow_mut().push(v); }
                    Ok(Val::None)
                }
                "pop" => Ok(l.borrow_mut().pop().unwrap_or(Val::None)),
                "contains" => {
                    let needle = args.first().map(|v| v.display()).unwrap_or_default();
                    Ok(Val::Bool(l.borrow().iter().any(|x| x.display() == needle)))
                }
                "clear" => { l.borrow_mut().clear(); Ok(Val::None) }
                _ => Ok(Val::None),
            },
            Val::Map(m) => match method {
                "len" => Ok(Val::Int(m.borrow().len() as i64)),
                "has" => {
                    let k = args.first().map(|v| v.display()).unwrap_or_default();
                    Ok(Val::Bool(m.borrow().contains_key(&k)))
                }
                "get" => {
                    let k = args.first().map(|v| v.display()).unwrap_or_default();
                    Ok(m.borrow().get(&k).cloned().unwrap_or_else(|| args.get(1).cloned().unwrap_or(Val::None)))
                }
                "set" => {
                    if let (Some(k), Some(v)) = (args.first(), args.get(1)) {
                        m.borrow_mut().insert(k.display(), v.clone());
                    }
                    Ok(Val::None)
                }
                "keys" => {
                    let ks: Vec<Val> = m.borrow().keys().map(|k| Val::Str(k.clone())).collect();
                    Ok(Val::List(Rc::new(RefCell::new(ks))))
                }
                _ => Ok(Val::None),
            },
            _ => Ok(Val::None),
        }
    }

    fn apply_binop(&self, op: BinOp, l: Val, r: Val) -> Result<Val, String> {
        match op {
            BinOp::Add => match (&l, &r) {
                (Val::Str(a), b) => Ok(Val::Str(format!("{}{}", a, b.display()))),
                (a, Val::Str(b)) => Ok(Val::Str(format!("{}{}", a.display(), b))),
                (Val::Float(_), _) | (_, Val::Float(_)) => Ok(Val::Float(l.as_f64() + r.as_f64())),
                _ => Ok(Val::Int(l.as_i64() + r.as_i64())),
            },
            BinOp::Sub => match (&l, &r) {
                (Val::Float(_), _) | (_, Val::Float(_)) => Ok(Val::Float(l.as_f64() - r.as_f64())),
                _ => Ok(Val::Int(l.as_i64() - r.as_i64())),
            },
            BinOp::Mul => match (&l, &r) {
                (Val::Float(_), _) | (_, Val::Float(_)) => Ok(Val::Float(l.as_f64() * r.as_f64())),
                _ => Ok(Val::Int(l.as_i64() * r.as_i64())),
            },
            BinOp::Div => {
                let d = r.as_f64();
                if d == 0.0 { return Err("Division by zero".into()); }
                if matches!(l, Val::Float(_)) || matches!(r, Val::Float(_)) {
                    Ok(Val::Float(l.as_f64() / d))
                } else {
                    Ok(Val::Int(l.as_i64() / r.as_i64()))
                }
            }
            BinOp::Mod => {
                let d = r.as_i64();
                if d == 0 { return Err("Modulo by zero".into()); }
                Ok(Val::Int(l.as_i64() % d))
            }
            BinOp::Pow => Ok(Val::Float(l.as_f64().powf(r.as_f64()))),
            BinOp::Eq => Ok(Val::Bool(match (&l, &r) {
                (Val::Str(a), Val::Str(b)) => a == b,
                (Val::Bool(a), Val::Bool(b)) => a == b,
                _ => (l.as_f64() - r.as_f64()).abs() < 1e-12 && std::mem::discriminant(&l) != std::mem::discriminant(&Val::None),
            })),
            BinOp::NotEq => {
                let eq = self.apply_binop(BinOp::Eq, l, r)?;
                Ok(Val::Bool(!eq.as_bool()))
            }
            BinOp::Lt => Ok(Val::Bool(l.as_f64() < r.as_f64())),
            BinOp::LtEq => Ok(Val::Bool(l.as_f64() <= r.as_f64())),
            BinOp::Gt => Ok(Val::Bool(l.as_f64() > r.as_f64())),
            BinOp::GtEq => Ok(Val::Bool(l.as_f64() >= r.as_f64())),
            BinOp::And => Ok(Val::Bool(l.as_bool() && r.as_bool())),
            BinOp::Or => Ok(Val::Bool(l.as_bool() || r.as_bool())),
            BinOp::BitAnd => Ok(Val::Int(l.as_i64() & r.as_i64())),
            BinOp::BitOr => Ok(Val::Int(l.as_i64() | r.as_i64())),
            BinOp::BitXor => Ok(Val::Int(l.as_i64() ^ r.as_i64())),
            BinOp::Shl => Ok(Val::Int(l.as_i64() << (r.as_i64() & 63))),
            BinOp::Shr => Ok(Val::Int(l.as_i64() >> (r.as_i64() & 63))),
        }
    }

    fn eval_eq_at(&mut self, eq_name: &str, x: f64) -> Result<f64, String> {
        if let Some(eq) = self.equations.get(eq_name).cloned() {
            let v = self.call_equation(&eq, vec![Val::Float(x)])?;
            return Ok(v.as_f64());
        }
        if let Some(f) = self.functions.get(eq_name).cloned() {
            let v = self.call_fn(&f, vec![Val::Float(x)])?;
            return Ok(v.as_f64());
        }
        Err(format!("Unknown equation/function `{}`", eq_name))
    }

    fn call_builtin(&mut self, ns: &str, func: &str, args: Vec<Val>) -> Result<Val, String> {
        match (ns, func) {
            ("io", "print") => {
                let s = args.first().map(|v| v.display()).unwrap_or_default();
                println!("{}", s);
                self.output.push(s);
                Ok(Val::None)
            }
            ("io", "eprint") => {
                let s = args.first().map(|v| v.display()).unwrap_or_default();
                eprintln!("{}", s);
                Ok(Val::None)
            }
            ("memory", "alloc") | ("gpu", "alloc_vram") | ("sdr", "alloc_iq_buffer") => {
                let sz = args.first().map(|v| v.as_i64().max(1) as usize).unwrap_or(64);
                let ptr = self.next_ptr;
                self.next_ptr += ((sz as u64 + 15) & !15).max(16);
                self.allocs.insert(ptr, LiveAlloc { size: sz, freed: false });
                self.total_allocated += sz;
                Ok(Val::Ptr(ptr))
            }
            ("memory", "free") | ("sdr", "close") => {
                let ptr = args.first().map(|v| v.as_i64() as u64).unwrap_or(0);
                if let Some(a) = self.allocs.get_mut(&ptr) {
                    if !a.freed {
                        a.freed = true;
                        self.total_freed += a.size;
                    }
                }
                self.sdr_devs.remove(&ptr);
                Ok(Val::None)
            }
            ("memory", "protect") => {
                let perms = args.get(2).map(|v| v.display()).unwrap_or_default();
                let u = perms.to_ascii_uppercase();
                Ok(Val::Bool(!(u.contains('W') && u.contains('X'))))
            }
            ("memory", "leak_report") => {
                let active_count = self.allocs.values().filter(|a| !a.freed).count() as i64;
                let active_bytes: usize = self.allocs.values().filter(|a| !a.freed).map(|a| a.size).sum();
                let mut m = BTreeMap::new();
                m.insert("active_allocs".into(), Val::Int(active_count));
                m.insert("leaked_bytes".into(), Val::Int(active_bytes as i64));
                m.insert("total_allocated".into(), Val::Int(self.total_allocated as i64));
                m.insert("total_freed".into(), Val::Int(self.total_freed as i64));
                m.insert("clean".into(), Val::Bool(active_count == 0));
                Ok(Val::Map(Rc::new(RefCell::new(m))))
            }
            ("hardware", "probe") => {
                let mut m = BTreeMap::new();
                m.insert("cores".into(), Val::Int(std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(4)));
                m.insert("arch".into(), Val::Str(std::env::consts::ARCH.to_string()));
                m.insert("os".into(), Val::Str(std::env::consts::OS.to_string()));
                m.insert("sdr_capable".into(), Val::Bool(true));
                m.insert("leak_guard".into(), Val::Str("active".into()));
                Ok(Val::Map(Rc::new(RefCell::new(m))))
            }
            ("hardware", "fan_speed") => {
                self.fan_percent = args.first().map(|v| v.as_i64().clamp(0, 100)).unwrap_or(50);
                Ok(Val::None)
            }
            ("hardware", "temp") => Ok(Val::Float(42.5)),
            ("hardware", "gpio_write") | ("hardware", "pwm_pulse") | ("hardware", "out_port") => Ok(Val::None),
            ("hardware", "in_port") => Ok(Val::Int(0)),
            ("power", "set_voltage") => {
                let rail = args.first().map(|v| v.display()).unwrap_or_else(|| "vcore".into());
                let mv = args.get(1).map(|v| v.as_f64()).unwrap_or(1150.0);
                self.voltages.insert(rail, mv.clamp(500.0, 1550.0));
                Ok(Val::None)
            }
            ("power", "get_voltage") => {
                let rail = args.first().map(|v| v.display()).unwrap_or_else(|| "vcore".into());
                Ok(Val::Float(*self.voltages.get(&rail).unwrap_or(&1150.0)))
            }
            ("power", "watts") => Ok(Val::Float(28.4)),
            ("power", "profile") => Ok(Val::None),
            ("cpu", "cores") => Ok(Val::Int(std::thread::available_parallelism().map(|n| n.get() as i64).unwrap_or(4))),
            ("cpu", "set_freq") | ("cpu", "pause") | ("cpu", "wrmsr") => Ok(Val::None),
            ("cpu", "rdmsr") => Ok(Val::Int(0)),
            ("gpu", "info") => {
                let mut m = BTreeMap::new();
                m.insert("backend".into(), Val::Str("OpenCL/Vulkan-Ready".into()));
                m.insert("vram_mb".into(), Val::Int(8192));
                Ok(Val::Map(Rc::new(RefCell::new(m))))
            }
            ("gpu", "dispatch") => Ok(Val::Bool(true)),
            ("gpu", "set_clock") => Ok(Val::None),
            ("display", "resolution") => {
                let mut m = BTreeMap::new();
                m.insert("width".into(), Val::Int(1920));
                m.insert("height".into(), Val::Int(1080));
                Ok(Val::Map(Rc::new(RefCell::new(m))))
            }
            ("display", "set_brightness") | ("display", "vga_print") | ("display", "clear") => Ok(Val::None),
            ("time", "now_ns") => {
                let ns = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as i64).unwrap_or(0);
                Ok(Val::Int(ns))
            }
            ("time", "sleep") | ("time", "pulse_wait") => Ok(Val::None),
            ("kernel", "log") => {
                let s = args.first().map(|v| v.display()).unwrap_or_default();
                self.output.push(format!("[KERNEL] {}", s));
                Ok(Val::None)
            }
            ("kernel", "cli") | ("kernel", "sti") | ("kernel", "halt") => Ok(Val::None),
            ("kernel", "panic") => Err(format!("KERNEL PANIC: {}", args.first().map(|v| v.display()).unwrap_or_default())),
            ("security", "constant_time_eq") => {
                let a = args.first().map(|v| v.display()).unwrap_or_default().into_bytes();
                let b = args.get(1).map(|v| v.display()).unwrap_or_default().into_bytes();
                let mut diff = a.len() ^ b.len();
                let n = a.len().min(b.len());
                for i in 0..n { diff |= (a[i] ^ b[i]) as usize; }
                Ok(Val::Bool(diff == 0))
            }
            ("security", "sha256") => {
                let s = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Val::Str(sha256_hex(s.as_bytes())))
            }
            ("security", "canary_check") => Ok(Val::Bool(true)),
            ("security", "zeroize") => Ok(Val::None),
            // Math & Equations
            ("math", "solve") => {
                let name = args.first().map(|v| v.display()).unwrap_or_default();
                let mut x = args.get(1).map(|v| v.as_f64()).unwrap_or(1.0);
                let h = 1e-6;
                for _ in 0..64 {
                    let fx = self.eval_eq_at(&name, x)?;
                    if fx.abs() < 1e-10 { break; }
                    let dfx = (self.eval_eq_at(&name, x + h)? - self.eval_eq_at(&name, x - h)?) / (2.0 * h);
                    if dfx.abs() < 1e-12 { break; }
                    x -= fx / dfx;
                }
                Ok(Val::Float(x))
            }
            ("math", "derivative") => {
                let name = args.first().map(|v| v.display()).unwrap_or_default();
                let x = args.get(1).map(|v| v.as_f64()).unwrap_or(0.0);
                let h = 1e-6;
                let df = (self.eval_eq_at(&name, x + h)? - self.eval_eq_at(&name, x - h)?) / (2.0 * h);
                Ok(Val::Float(df))
            }
            ("math", "integrate") => {
                let name = args.first().map(|v| v.display()).unwrap_or_default();
                let a = args.get(1).map(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get(2).map(|v| v.as_f64()).unwrap_or(1.0);
                let n = 100usize;
                let h = (b - a) / n as f64;
                let mut sum = self.eval_eq_at(&name, a)? + self.eval_eq_at(&name, b)?;
                for i in 1..n {
                    let coeff = if i % 2 == 0 { 2.0 } else { 4.0 };
                    sum += coeff * self.eval_eq_at(&name, a + i as f64 * h)?;
                }
                Ok(Val::Float(sum * h / 3.0))
            }
            ("math", "sin") => Ok(Val::Float(args.first().map(|v| v.as_f64()).unwrap_or(0.0).sin())),
            ("math", "cos") => Ok(Val::Float(args.first().map(|v| v.as_f64()).unwrap_or(0.0).cos())),
            ("math", "sqrt") => Ok(Val::Float(args.first().map(|v| v.as_f64()).unwrap_or(0.0).sqrt())),
            ("math", "abs") => Ok(Val::Float(args.first().map(|v| v.as_f64()).unwrap_or(0.0).abs())),
            ("math", "log") => Ok(Val::Float(args.first().map(|v| v.as_f64()).unwrap_or(1.0).ln())),
            ("math", "exp") => Ok(Val::Float(args.first().map(|v| v.as_f64()).unwrap_or(0.0).exp())),
            ("math", "fft") => {
                let vals = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let mags = compute_fft_magnitudes(&vals);
                Ok(Val::List(Rc::new(RefCell::new(mags.into_iter().map(Val::Float).collect()))))
            }
            // Native SDR DSP Engine
            ("sdr", "open") => {
                let driver = args.first().map(|v| v.display()).unwrap_or_else(|| "sim".into());
                let ptr = self.next_ptr;
                self.next_ptr += 64;
                self.allocs.insert(ptr, LiveAlloc { size: 64, freed: false });
                self.total_allocated += 64;
                self.sdr_devs.insert(ptr, SdrDeviceState {
                    driver,
                    center_freq_hz: 433.92e6,
                    sample_rate: 2.4e6,
                    gain_db: 20.0,
                    bandwidth_hz: 1.5e6,
                });
                Ok(Val::Ptr(ptr))
            }
            ("sdr", "set_center_freq") => {
                let dev = args.first().map(|v| v.as_i64() as u64).unwrap_or(0);
                let f = args.get(1).map(|v| v.as_f64()).unwrap_or(433.92e6);
                if let Some(s) = self.sdr_devs.get_mut(&dev) { s.center_freq_hz = f; }
                Ok(Val::None)
            }
            ("sdr", "set_sample_rate") => {
                let dev = args.first().map(|v| v.as_i64() as u64).unwrap_or(0);
                let r = args.get(1).map(|v| v.as_f64()).unwrap_or(2.4e6);
                if let Some(s) = self.sdr_devs.get_mut(&dev) { s.sample_rate = r; }
                Ok(Val::None)
            }
            ("sdr", "set_gain") => {
                let dev = args.first().map(|v| v.as_i64() as u64).unwrap_or(0);
                let g = args.get(1).map(|v| v.as_f64()).unwrap_or(20.0);
                if let Some(s) = self.sdr_devs.get_mut(&dev) { s.gain_db = g; }
                Ok(Val::None)
            }
            ("sdr", "set_bandwidth") => {
                let dev = args.first().map(|v| v.as_i64() as u64).unwrap_or(0);
                let bw = args.get(1).map(|v| v.as_f64()).unwrap_or(1.5e6);
                if let Some(s) = self.sdr_devs.get_mut(&dev) { s.bandwidth_hz = bw; }
                Ok(Val::None)
            }
            ("sdr", "capture_iq") => {
                let dev = args.first().map(|v| v.as_i64() as u64).unwrap_or(0);
                let samples = args.get(1).map(|v| v.as_i64().clamp(8, 8192) as usize).unwrap_or(64);
                let sr = self.sdr_devs.get(&dev).map(|s| s.sample_rate).unwrap_or(2.4e6);
                let tone_hz = sr * 0.125;
                let mut iq = Vec::with_capacity(samples * 2);
                for n in 0..samples {
                    let t = n as f64 / sr;
                    let phase = 2.0 * PI * tone_hz * t;
                    let env = if (n / 8) % 2 == 0 { 0.9 } else { 0.1 };
                    iq.push(Val::Float(env * phase.cos()));
                    iq.push(Val::Float(env * phase.sin()));
                }
                Ok(Val::List(Rc::new(RefCell::new(iq))))
            }
            ("sdr", "transmit_iq") => Ok(Val::Bool(true)),
            ("sdr", "fft_spectrum") => {
                let iq = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let spec = compute_iq_fft_db(&iq);
                Ok(Val::List(Rc::new(RefCell::new(spec.into_iter().map(Val::Float).collect()))))
            }
            ("sdr", "demod_fm") => {
                let iq = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let mut audio = Vec::new();
                for pair in iq.chunks(2).collect::<Vec<_>>().windows(2) {
                    let (i0, q0) = (pair[0][0], *pair[0].get(1).unwrap_or(&0.0));
                    let (i1, q1) = (pair[1][0], *pair[1].get(1).unwrap_or(&0.0));
                    let angle = (i0 * q1 - q0 * i1).atan2(i0 * i1 + q0 * q1);
                    audio.push(Val::Float(angle));
                }
                Ok(Val::List(Rc::new(RefCell::new(audio))))
            }
            ("sdr", "demod_am") => {
                let iq = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let env: Vec<Val> = iq.chunks(2).map(|c| {
                    let i = c[0];
                    let q = *c.get(1).unwrap_or(&0.0);
                    Val::Float((i * i + q * q).sqrt())
                }).collect();
                Ok(Val::List(Rc::new(RefCell::new(env))))
            }
            ("sdr", "demod_ask_ook") => {
                let iq = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let thresh = args.get(1).map(|v| v.as_f64()).unwrap_or(0.5);
                let bits: Vec<Val> = iq.chunks(2).map(|c| {
                    let i = c[0];
                    let q = *c.get(1).unwrap_or(&0.0);
                    Val::Int(if (i * i + q * q).sqrt() >= thresh { 1 } else { 0 })
                }).collect();
                Ok(Val::List(Rc::new(RefCell::new(bits))))
            }
            ("sdr", "modulate_bpsk") => {
                let bits = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_i64()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let sps = args.get(1).map(|v| v.as_i64().max(1) as usize).unwrap_or(4);
                let mut iq = Vec::new();
                for b in bits {
                    let amp = if b != 0 { 1.0 } else { -1.0 };
                    for _ in 0..sps {
                        iq.push(Val::Float(amp));
                        iq.push(Val::Float(0.0));
                    }
                }
                Ok(Val::List(Rc::new(RefCell::new(iq))))
            }
            ("sdr", "snr_db") => {
                let iq = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let mut pows: Vec<f64> = iq.chunks(2).map(|c| {
                    let i = c[0];
                    let q = *c.get(1).unwrap_or(&0.0);
                    i * i + q * q
                }).collect();
                if pows.is_empty() { return Ok(Val::Float(0.0)); }
                pows.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let noise = pows[pows.len() / 4].max(1e-9);
                let sig = *pows.last().unwrap_or(&1.0);
                Ok(Val::Float(10.0 * (sig / noise).log10()))
            }
            // Algorithm-generating Algorithms (`synth::`)
            ("synth", "synthesize_from_examples") => {
                let fn_name = args.first().map(|v| v.display()).unwrap_or_else(|| "synthesized".into());
                let xs: Vec<f64> = match args.get(1) {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect(),
                    _ => Vec::new(),
                };
                let ys: Vec<f64> = match args.get(2) {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.as_f64()).collect(),
                    _ => Vec::new(),
                };
                Ok(Val::Str(synthesize_rule_from_examples(&fn_name, &xs, &ys)))
            }
            ("synth", "generate_dsp_filter") => {
                let kind = args.first().map(|v| v.display()).unwrap_or_else(|| "lowpass".into());
                let cutoff = args.get(1).map(|v| v.as_f64()).unwrap_or(100_000.0);
                let sr = args.get(2).map(|v| v.as_f64()).unwrap_or(2_400_000.0);
                let taps = args.get(3).map(|v| v.as_i64().clamp(3, 63) as usize).unwrap_or(11);
                Ok(Val::Str(synthesize_fir_filter(&kind, cutoff, sr, taps)))
            }
            ("synth", "evolve_crypto_mixer") => {
                let name = args.first().map(|v| v.display()).unwrap_or_else(|| "arx_mix".into());
                let rounds = args.get(1).map(|v| v.as_i64().clamp(1, 16) as usize).unwrap_or(4);
                let seed = args.get(2).map(|v| v.as_i64() as u64).unwrap_or(0x9E37_79B9);
                Ok(Val::Str(synthesize_arx_mixer(&name, rounds, seed)))
            }
            ("synth", "generate_fsm") => {
                let name = args.first().map(|v| v.display()).unwrap_or_else(|| "hw_fsm".into());
                let states: Vec<String> = match args.get(1) {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.display()).collect(),
                    _ => vec!["IDLE".into(), "ACTIVE".into(), "DONE".into()],
                };
                Ok(Val::Str(synthesize_fsm(&name, &states)))
            }
            ("synth", "generate_sort") => {
                let name = args.first().map(|v| v.display()).unwrap_or_else(|| "auto_sort".into());
                Ok(Val::Str(synthesize_sort(&name)))
            }
            // fs / sys / str
            ("fs", "read_text") => {
                let path = args.first().map(|v| v.display()).unwrap_or_default();
                let text = std::fs::read_to_string(&path).map_err(|e| format!("fs::read_text failed: {}", e))?;
                Ok(Val::Str(text))
            }
            ("fs", "write_text") => {
                let path = args.first().map(|v| v.display()).unwrap_or_default();
                let body = args.get(1).map(|v| v.display()).unwrap_or_default();
                if let Some(p) = Path::new(&path).parent() {
                    if !p.as_os_str().is_empty() { let _ = std::fs::create_dir_all(p); }
                }
                std::fs::write(&path, body).map_err(|e| format!("fs::write_text failed: {}", e))?;
                Ok(Val::Bool(true))
            }
            ("fs", "exists") => {
                let path = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Val::Bool(Path::new(&path).exists()))
            }
            ("sys", "args") => {
                Ok(Val::List(Rc::new(RefCell::new(self.cli_args.iter().cloned().map(Val::Str).collect()))))
            }
            ("sys", "exec") => {
                let cmd = args.first().map(|v| v.display()).unwrap_or_default();
                let st = Command::new("sh").arg("-c").arg(&cmd).status().map_err(|e| e.to_string())?;
                Ok(Val::Int(st.code().unwrap_or(1) as i64))
            }
            ("sys", "exit") => {
                let code = args.first().map(|v| v.as_i64() as i32).unwrap_or(0);
                std::process::exit(code);
            }
            ("sys", "env") => {
                let k = args.first().map(|v| v.display()).unwrap_or_default();
                Ok(Val::Str(std::env::var(k).unwrap_or_default()))
            }
            ("str", "from_code") => {
                let code = args.first().map(|v| v.as_i64() as u32).unwrap_or(0);
                let ch = char::from_u32(code).unwrap_or('\0');
                Ok(Val::Str(ch.to_string()))
            }
            ("str", "from_int") => Ok(Val::Str(args.first().map(|v| v.as_i64()).unwrap_or(0).to_string())),
            ("str", "to_int") => Ok(Val::Int(parse_int_str(&args.first().map(|v| v.display()).unwrap_or_default()))),
            ("str", "to_float") => Ok(Val::Float(args.first().map(|v| v.as_f64()).unwrap_or(0.0))),
            ("str", "join") => {
                let items: Vec<String> = match args.first() {
                    Some(Val::List(l)) => l.borrow().iter().map(|v| v.display()).collect(),
                    _ => Vec::new(),
                };
                let sep = args.get(1).map(|v| v.display()).unwrap_or_default();
                Ok(Val::Str(items.join(&sep)))
            }
            _ => Ok(Val::None),
        }
    }
}

pub fn synthesize_rule_from_examples(fn_name: &str, xs: &[f64], ys: &[f64]) -> String {
    let n = xs.len().min(ys.len());
    if n >= 2 {
        // 1) Try linear exact fit: y = a*x + b
        let dx = xs[1] - xs[0];
        if dx.abs() > 1e-9 {
            let a = (ys[1] - ys[0]) / dx;
            let b = ys[0] - a * xs[0];
            let linear_ok = (0..n).all(|i| ((a * xs[i] + b) - ys[i]).abs() < 1e-6);
            if linear_ok {
                let ai = a.round() as i64;
                let bi = b.round() as i64;
                if (a - ai as f64).abs() < 1e-9 && (b - bi as f64).abs() < 1e-9 {
                    let sign = if bi >= 0 { format!("+ {}", bi) } else { format!("- {}", -bi) };
                    return format!("// Synthesized Linear Algorithm (MSE = 0.0)\nfn {}(x: int) -> int:\n    return {} * x {}\n", fn_name, ai, sign);
                }
                return format!("// Synthesized Linear Algorithm (MSE = 0.0)\nfn {}(x: float) -> float:\n    return {} * x + {}\n", fn_name, a, b);
            }
        }
        // 2) Try quadratic fit using 3 points (Lagrange / Vandermonde)
        if n >= 3 {
            let (x0, x1, x2) = (xs[0], xs[1], xs[2]);
            let (y0, y1, y2) = (ys[0], ys[1], ys[2]);
            let denom = (x0 - x1) * (x0 - x2) * (x1 - x2);
            if denom.abs() > 1e-9 {
                let a = (x2 * (y1 - y0) + x1 * (y0 - y2) + x0 * (y2 - y1)) / denom;
                let b = (x2 * x2 * (y0 - y1) + x1 * x1 * (y2 - y0) + x0 * x0 * (y1 - y2)) / denom;
                let c = (x1 * x2 * (x1 - x2) * y0 + x2 * x0 * (x2 - x0) * y1 + x0 * x1 * (x0 - x1) * y2) / denom;
                let quad_ok = (0..n).all(|i| ((a * xs[i] * xs[i] + b * xs[i] + c) - ys[i]).abs() < 1e-5);
                if quad_ok {
                    return format!(
                        "// Synthesized Quadratic Algorithm (MSE = 0.0)\nequation {}_eq(x) = {:.4}*x^2 + {:.4}*x + {:.4}\nfn {}(x: float) -> float:\n    return {}_eq(x)\n",
                        fn_name, a, b, c, fn_name, fn_name
                    );
                }
            }
        }
    }
    format!("// Synthesized Lookup-Fallback Algorithm\nfn {}(x: int) -> int:\n    return x\n", fn_name)
}

pub fn synthesize_fir_filter(kind: &str, cutoff_hz: f64, sr_hz: f64, taps: usize) -> String {
    let fc = (cutoff_hz / sr_hz.max(1.0)).clamp(0.01, 0.49);
    let m = taps.max(3) | 1;
    let half = (m - 1) as f64 / 2.0;
    let mut coeffs = Vec::with_capacity(m);
    let mut sum = 0.0;
    for i in 0..m {
        let n = i as f64 - half;
        let sinc = if n.abs() < 1e-9 { 2.0 * fc } else { (2.0 * PI * fc * n).sin() / (PI * n) };
        let win = 0.54 - 0.46 * (2.0 * PI * i as f64 / (m - 1) as f64).cos();
        let w = sinc * win;
        coeffs.push(w);
        sum += w;
    }
    for c in &mut coeffs { *c /= sum.max(1e-9); }
    let coeff_strs: Vec<String> = coeffs.iter().map(|c| format!("{:.6}", c)).collect();
    format!(
        "// Synthesized {} Windowed-Sinc FIR Filter ({} taps)\nfn apply_{}_fir(samples: list) -> list:\n    let h = [{}]\n    var out = []\n    let n = samples.len()\n    var i = 0\n    while i < n:\n        var acc = 0.0\n        var k = 0\n        while k < {}:\n            if i >= k:\n                acc = acc + samples[i - k] * h[k]\n            k += 1\n        out.push(acc)\n        i += 1\n    return out\n",
        kind, m, kind, coeff_strs.join(", "), m
    )
}

pub fn synthesize_arx_mixer(name: &str, rounds: usize, seed: u64) -> String {
    let mut s = format!("// Synthesized {}-Round ARX Cryptographic Mixer\nfn {}(x: int) -> int:\n    var v = x ^ 0x{:x}\n", rounds, name, seed & 0xFFFF_FFFF);
    for r in 0..rounds {
        let rot = ((r * 7 + 5) % 29) + 1;
        let k = ((seed.wrapping_mul((r as u64) + 3)) ^ 0xA5A5_5A5A) & 0x7FFF_FFFF;
        s.push_str(&format!("    v = (v + 0x{:x}) ^ ((v << {}) & 0x7FFFFFFF)\n", k, rot));
    }
    s.push_str("    return v\n");
    s
}

pub fn synthesize_fsm(name: &str, states: &[String]) -> String {
    let mut s = format!("// Synthesized Hardware State Machine\nenum {}State:\n", name);
    for st in states { s.push_str(&format!("    {}\n", st)); }
    s.push_str(&format!("fn {}_step(state: str) -> str:\n    match state:\n", name));
    for i in 0..states.len() {
        let next = &states[(i + 1) % states.len()];
        s.push_str(&format!("        case \"{}\" => return \"{}\"\n", states[i], next));
    }
    s.push_str(&format!("        case _ => return \"{}\"\n", states.first().cloned().unwrap_or_else(|| "IDLE".into())));
    s
}

pub fn synthesize_sort(name: &str) -> String {
    format!(
        "// Synthesized In-Place Insertion Sort\nfn {}(arr: list) -> list:\n    var i = 1\n    while i < arr.len():\n        let key = arr[i]\n        var j = i - 1\n        while j >= 0 and arr[j] > key:\n            arr[j + 1] = arr[j]\n            j -= 1\n        arr[j + 1] = key\n        i += 1\n    return arr\n",
        name
    )
}

pub fn compute_fft_magnitudes(signal: &[f64]) -> Vec<f64> {
    let n = signal.len().next_power_of_two().max(2);
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    for (i, &v) in signal.iter().enumerate() { re[i] = v; }
    radix2_fft(&mut re, &mut im);
    (0..n).map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt()).collect()
}

pub fn compute_iq_fft_db(iq: &[f64]) -> Vec<f64> {
    let complex_len = (iq.len() / 2).next_power_of_two().max(2);
    let mut re = vec![0.0; complex_len];
    let mut im = vec![0.0; complex_len];
    for (i, chunk) in iq.chunks(2).enumerate() {
        if i >= complex_len { break; }
        re[i] = chunk[0];
        im[i] = *chunk.get(1).unwrap_or(&0.0);
    }
    radix2_fft(&mut re, &mut im);
    (0..complex_len)
        .map(|i| {
            let mag = (re[i] * re[i] + im[i] * im[i]).sqrt().max(1e-9);
            20.0 * mag.log10()
        })
        .collect()
}

fn radix2_fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        let wlen_r = ang.cos();
        let wlen_i = ang.sin();
        for i in (0..n).step_by(len) {
            let mut wr = 1.0;
            let mut wi = 0.0;
            for k in 0..(len / 2) {
                let u_r = re[i + k];
                let u_i = im[i + k];
                let v_r = re[i + k + len / 2] * wr - im[i + k + len / 2] * wi;
                let v_i = re[i + k + len / 2] * wi + im[i + k + len / 2] * wr;
                re[i + k] = u_r + v_r;
                im[i + k] = u_i + v_i;
                re[i + k + len / 2] = u_r - v_r;
                im[i + k + len / 2] = u_i - v_i;
                let nwr = wr * wlen_r - wi * wlen_i;
                wi = wr * wlen_i + wi * wlen_r;
                wr = nwr;
            }
        }
        len <<= 1;
    }
}

fn sha256_hex(data: &[u8]) -> String {
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
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while (msg.len() % 64) != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(temp1); d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }
        let vals = [a, b, c, d, e, f, g, hh];
        for i in 0..8 { h[i] = h[i].wrapping_add(vals[i]); }
    }
    h.iter().map(|v| format!("{:08x}", v)).collect()
}
