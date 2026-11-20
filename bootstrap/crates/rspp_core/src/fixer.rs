use crate::analyzer::analyze;
use crate::lexer::lex;
use crate::math_diag::{Diagnostic, TextEdit};
use crate::parser::parse;

pub struct FixOutcome {
    pub fixed_source: String,
    pub applied_fixes: Vec<String>,
    pub remaining_diagnostics: Vec<Diagnostic>,
}

/// Applies non-overlapping preferred mathematical & leak-guard AutoFixes iteratively until convergence.
pub fn auto_fix(source: &str, max_passes: usize) -> FixOutcome {
    let mut current = source.to_string();
    let mut applied_fixes = Vec::new();

    for _ in 0..max_passes {
        let mut all_diags = Vec::new();
        let lx = lex(&current);
        all_diags.extend(lx.diagnostics);
        let ps = parse(lx.tokens);
        all_diags.extend(ps.diagnostics);
        let an = analyze(&ps.program, &current);
        all_diags.extend(an.diagnostics);

        let mut edits: Vec<(TextEdit, String)> = Vec::new();
        for d in &all_diags {
            if let Some(fix) = d.fixes.iter().find(|f| f.is_preferred).or_else(|| d.fixes.first()) {
                for ed in &fix.edits {
                    if ed.span.start <= ed.span.end && ed.span.end <= current.len() {
                        edits.push((ed.clone(), format!("[{}] {}", d.code, fix.title)));
                    }
                }
            }
        }

        if edits.is_empty() {
            return FixOutcome {
                fixed_source: current,
                applied_fixes,
                remaining_diagnostics: all_diags,
            };
        }

        // Apply from end to start without overlap
        edits.sort_by(|a, b| b.0.span.start.cmp(&a.0.span.start).then(b.0.span.end.cmp(&a.0.span.end)));
        let mut last_start = usize::MAX;
        let mut changed = false;
        for (ed, label) in edits {
            if ed.span.end <= last_start {
                current.replace_range(ed.span.start..ed.span.end, &ed.replacement);
                last_start = ed.span.start;
                applied_fixes.push(label);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let lx = lex(&current);
    let mut rem = lx.diagnostics;
    let ps = parse(lx.tokens);
    rem.extend(ps.diagnostics);
    let an = analyze(&ps.program, &current);
    rem.extend(an.diagnostics);

    FixOutcome {
        fixed_source: current,
        applied_fixes,
        remaining_diagnostics: rem,
    }
}
