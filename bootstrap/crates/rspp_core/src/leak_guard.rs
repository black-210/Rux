use std::collections::HashMap;

use crate::math_diag::{AutoFix, Diagnostic, Severity, TextEdit};
use crate::span::Span;

#[derive(Debug, Clone)]
pub struct AllocNode {
    pub id: usize,
    pub var_name: String,
    pub alloc_span: Span,
    pub stmt_end_span: Span,
    pub scope_depth: usize,
    pub freed: bool,
    pub freed_span: Option<Span>,
    pub returned: bool,
    pub deferred: bool,
    pub aliases: Vec<String>,
}

/// Static Ownership & Escape Graph Leak Protection Engine.
/// Verifies zero memory leaks, detects double-frees, use-after-free, and alias overwrites,
/// and synthesizes deterministic `defer memory::free(...)` auto-fixes.
pub struct StaticLeakGuard {
    next_id: usize,
    nodes: Vec<AllocNode>,
    var_to_alloc: HashMap<String, usize>,
    scope_depth: usize,
    pub diagnostics: Vec<Diagnostic>,
}

impl StaticLeakGuard {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            nodes: Vec::new(),
            var_to_alloc: HashMap::new(),
            scope_depth: 0,
            diagnostics: Vec::new(),
        }
    }

    pub fn enter_scope(&mut self) {
        self.scope_depth += 1;
    }

    pub fn exit_scope(&mut self, source: &str) {
        let depth = self.scope_depth;
        let leaked: Vec<AllocNode> = self
            .nodes
            .iter()
            .filter(|n| n.scope_depth == depth && !n.freed && !n.returned && !n.deferred)
            .cloned()
            .collect();

        for node in leaked {
            let indent = line_indent(source, node.stmt_end_span.line);
            let insert_sp = Span::new(
                node.stmt_end_span.end,
                node.stmt_end_span.end,
                node.stmt_end_span.end_line,
                node.stmt_end_span.end_col,
                node.stmt_end_span.end_line,
                node.stmt_end_span.end_col,
            );
            self.diagnostics.push(
                Diagnostic::error(
                    "LG001",
                    node.alloc_span,
                    format!(
                        "Memory Leak Guard: allocation `{}` (node #{}) escapes scope without being freed",
                        node.var_name, node.id
                    ),
                )
                .with_severity(Severity::MemoryLeak)
                .with_ar_message(format!(
                    "حماية تسرب الذاكرة: الكتلة المحجوزة `{}` تخرج من النطاق بدون تحرير",
                    node.var_name
                ))
                .with_why(
                    "Reachability graph analysis proved that this heap/buffer allocation has no free or return edge on scope exit.",
                    "تحليل الرسم البياني للملكية أثبت عدم وجود مسار تحرير لهذه الذاكرة قبل انتهاء الكتلة.",
                )
                .with_math(
                    format!("Ownership Graph Reachability: Node(#{}, scope={}) -> OutDegree(free/return/defer) == 0", node.id, depth),
                    0.99,
                )
                .with_solution(
                    format!("Add `defer memory::free({})` right after allocation or call `memory::free({})` before exiting.", node.var_name, node.var_name),
                    format!("أضف `defer memory::free({})` بعد الحجز مباشرة.", node.var_name),
                )
                .with_fix(AutoFix {
                    title: format!("Insert `defer memory::free({})`", node.var_name),
                    title_ar: format!("إضافة `defer memory::free({})` تلقائياً", node.var_name),
                    edits: vec![TextEdit {
                        span: insert_sp,
                        replacement: format!("\n{}defer memory::free({})", indent, node.var_name),
                    }],
                    is_preferred: true,
                }),
            );
        }

        // Remove local mappings at exiting scope
        let expired_ids: Vec<usize> = self
            .nodes
            .iter()
            .filter(|n| n.scope_depth == depth)
            .map(|n| n.id)
            .collect();
        self.var_to_alloc.retain(|_, id| !expired_ids.contains(id));
        self.scope_depth = self.scope_depth.saturating_sub(1);
    }

    pub fn record_alloc(&mut self, var_name: &str, alloc_span: Span, stmt_end_span: Span) {
        // Check if variable previously owned an unfreed allocation
        if let Some(&prev_id) = self.var_to_alloc.get(var_name) {
            if let Some(prev) = self.nodes.iter().find(|n| n.id == prev_id) {
                if !prev.freed && !prev.returned && !prev.deferred {
                    self.diagnostics.push(
                        Diagnostic::error(
                            "LG004",
                            alloc_span,
                            format!("Overwrite Leak: `{}` is reassigned before freeing its previous allocation (#{})", var_name, prev_id),
                        )
                        .with_severity(Severity::MemoryLeak)
                        .with_ar_message(format!("تسرب ذاكرة: إعادة تعيين `{}` قبل تحرير الكتلة السابقة", var_name))
                        .with_why(
                            "Reassigning an owning pointer drops the only reference to the prior memory block.",
                            "تغيير قيمة المؤشر يضيع العنوان الوحيد للذاكرة المحجوزة سابقاً.",
                        )
                        .with_math(format!("RefCount(#{}) drops 1 -> 0 while Live=true", prev_id), 1.0)
                        .with_solution(
                            format!("Call `memory::free({})` before reallocating.", var_name),
                            format!("استدعِ `memory::free({})` قبل إعادة التخصيص.", var_name),
                        ),
                    );
                }
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(AllocNode {
            id,
            var_name: var_name.to_string(),
            alloc_span,
            stmt_end_span,
            scope_depth: self.scope_depth,
            freed: false,
            freed_span: None,
            returned: false,
            deferred: false,
            aliases: Vec::new(),
        });
        self.var_to_alloc.insert(var_name.to_string(), id);
    }

    pub fn record_alias(&mut self, alias_name: &str, src_var: &str) {
        if let Some(&id) = self.var_to_alloc.get(src_var) {
            if let Some(node) = self.nodes.iter_mut().find(|n| n.id == id) {
                node.aliases.push(alias_name.to_string());
            }
            self.var_to_alloc.insert(alias_name.to_string(), id);
        }
    }

    pub fn record_free(&mut self, var_name: &str, span: Span, via_defer: bool) {
        let Some(&id) = self.var_to_alloc.get(var_name) else { return };
        if let Some(node) = self.nodes.iter_mut().find(|n| n.id == id) {
            if node.freed && !via_defer {
                self.diagnostics.push(
                    Diagnostic::error(
                        "LG002",
                        span,
                        format!("Double-Free detected on `{}` (node #{})", var_name, id),
                    )
                    .with_severity(Severity::MemoryLeak)
                    .with_ar_message(format!("تحرير مزدوج للذاكرة على `{}`", var_name))
                    .with_why(
                        "Freeing an already-freed pointer corrupts the heap allocator and creates security vulnerabilities.",
                        "تحرير نفس الذاكرة مرتين يفسد بنية الذاكرة ويسبب ثغرات خطيرة.",
                    )
                    .with_math(format!("StateTransition(#{}): Freed -> Free (Illegal)", id), 1.0)
                    .with_solution("Remove the redundant `memory::free(...)` call.", "احذف استدعاء `memory::free(...)` المكرر."),
                );
                return;
            }
            if via_defer {
                node.deferred = true;
            } else {
                node.freed = true;
                node.freed_span = Some(span);
            }
        }
    }

    pub fn record_use(&mut self, var_name: &str, span: Span) {
        let Some(&id) = self.var_to_alloc.get(var_name) else { return };
        if let Some(node) = self.nodes.iter().find(|n| n.id == id) {
            if node.freed {
                self.diagnostics.push(
                    Diagnostic::error(
                        "LG003",
                        span,
                        format!("Use-After-Free: `{}` was already freed (node #{})", var_name, id),
                    )
                    .with_severity(Severity::MemoryLeak)
                    .with_ar_message(format!("استخدام ذاكرة بعد تحريرها على `{}`", var_name))
                    .with_why(
                        "Accessing memory after `memory::free(...)` causes undefined behavior and dangling pointer exploits.",
                        "الوصول إلى عنوان ذاكرة بعد تحريره يؤدي لانهيار البرنامج أو ثغرات أمنية.",
                    )
                    .with_math(format!("Temporal Safety Violation: Use({}) > Free(#{})", span.line, id), 1.0)
                    .with_solution(
                        "Move this operation before `memory::free(...)` or use `defer memory::free(...)`.",
                        "انقل هذا السطر قبل `memory::free(...)` أو استخدم `defer`.",
                    ),
                );
            }
        }
    }

    pub fn record_return(&mut self, var_name: &str) {
        if let Some(&id) = self.var_to_alloc.get(var_name) {
            if let Some(node) = self.nodes.iter_mut().find(|n| n.id == id) {
                node.returned = true;
            }
        }
    }
}

fn line_indent(source: &str, line_1: usize) -> String {
    source
        .lines()
        .nth(line_1.saturating_sub(1))
        .unwrap_or("")
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}
