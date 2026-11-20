use crate::diagnostic::{Diagnostic, TextEdit};
use crate::{analyzer, lexer, parser};

#[derive(Debug, Clone)]
pub struct FixSummary {
    pub fixed_source: String,
    pub applied_fixes: Vec<String>,
    pub remaining_diagnostics: Vec<Diagnostic>,
}

pub fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
    let mut sorted = edits.to_vec();
    sorted.sort_by(|a, b| b.span.start.cmp(&a.span.start));

    let mut out = source.to_string();
    let mut last_start = usize::MAX;
    for edit in sorted {
        if edit.span.end <= last_start && edit.span.start <= edit.span.end && edit.span.end <= out.len() {
            out.replace_range(edit.span.start..edit.span.end, &edit.replacement);
            last_start = edit.span.start;
        }
    }
    out
}

pub fn collect_diagnostics(source: &str) -> Vec<Diagnostic> {
    let lex_out = lexer::lex(source);
    let parse_out = parser::parse(lex_out.tokens);
    let analysis = analyzer::analyze(&parse_out.program, source);

    let mut all = Vec::new();
    all.extend(lex_out.diagnostics);
    all.extend(parse_out.diagnostics);
    all.extend(analysis.diagnostics);
    all
}

/// Iteratively applies preferred auto-fixes up to 5 passes so syntax + semantic + security fixes all resolve cleanly.
pub fn auto_fix_source(source: &str) -> FixSummary {
    let mut current = source.to_string();
    let mut applied_fixes = Vec::new();

    for _pass in 0..5 {
        let diags = collect_diagnostics(&current);
        let mut edits = Vec::new();
        let mut pass_titles = Vec::new();

        for d in &diags {
            if let Some(fix) = d.fixes.iter().find(|f| f.is_preferred).or_else(|| d.fixes.first()) {
                if !fix.edits.is_empty() {
                    pass_titles.push(format!("[{}] {}", d.code, fix.title));
                    edits.extend(fix.edits.clone());
                }
            }
        }

        if edits.is_empty() {
            break;
        }

        let next = apply_edits(&current, &edits);
        if next == current {
            break;
        }
        applied_fixes.extend(pass_titles);
        current = next;
    }

    let remaining_diagnostics = collect_diagnostics(&current);
    FixSummary {
        fixed_source: current,
        applied_fixes,
        remaining_diagnostics,
    }
}
