use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    SecurityAlert,
    HardwareSafety,
    Hint,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARNING",
            Severity::SecurityAlert => "SECURITY ALERT",
            Severity::HardwareSafety => "HARDWARE SAFETY",
            Severity::Hint => "HINT",
        }
    }

    pub fn label_ar(&self) -> &'static str {
        match self {
            Severity::Error => "خطأ",
            Severity::Warning => "تحذير",
            Severity::SecurityAlert => "تنبيه أمني",
            Severity::HardwareSafety => "سلامة العتاد",
            Severity::Hint => "تلميح",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagLang {
    En,
    Ar,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub span: Span,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoFix {
    pub title: String,
    pub title_ar: String,
    pub edits: Vec<TextEdit>,
    pub is_preferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    pub message_ar: String,
    pub why: String,
    pub why_ar: String,
    pub solution: String,
    pub solution_ar: String,
    pub fixes: Vec<AutoFix>,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, span: Span, msg: impl Into<String>) -> Self {
        let m = msg.into();
        Self {
            code: code.into(),
            severity: Severity::Error,
            span,
            message_ar: m.clone(),
            message: m,
            why: String::new(),
            why_ar: String::new(),
            solution: String::new(),
            solution_ar: String::new(),
            fixes: Vec::new(),
        }
    }

    pub fn with_ar_message(mut self, ar: impl Into<String>) -> Self {
        self.message_ar = ar.into();
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_why(mut self, en: impl Into<String>, ar: impl Into<String>) -> Self {
        self.why = en.into();
        self.why_ar = ar.into();
        self
    }

    pub fn with_solution(mut self, en: impl Into<String>, ar: impl Into<String>) -> Self {
        self.solution = en.into();
        self.solution_ar = ar.into();
        self
    }

    pub fn with_fix(mut self, fix: AutoFix) -> Self {
        self.fixes.push(fix);
        self
    }

    pub fn render(&self, filename: &str, source: &str, lang: DiagLang, color: bool) -> String {
        let mut out = String::new();
        let (red, yellow, cyan, green, bold, reset) = if color {
            ("\x1b[31m", "\x1b[33m", "\x1b[36m", "\x1b[32m", "\x1b[1m", "\x1b[0m")
        } else {
            ("", "", "", "", "", "")
        };

        let sev_color = match self.severity {
            Severity::Error | Severity::SecurityAlert | Severity::HardwareSafety => red,
            Severity::Warning => yellow,
            Severity::Hint => cyan,
        };

        let header = match lang {
            DiagLang::En => format!("[{}] {}: {}", self.code, self.severity.label(), self.message),
            DiagLang::Ar => format!("[{}] {}: {}", self.code, self.severity.label_ar(), self.message_ar),
            DiagLang::Both => format!(
                "[{}] {} ({}): {}\n    {}",
                self.code,
                self.severity.label(),
                self.severity.label_ar(),
                self.message,
                self.message_ar
            ),
        };
        out.push_str(&format!("{}{}{}{}\n", bold, sev_color, header, reset));
        out.push_str(&format!("  --> {}:{}:{}\n", filename, self.span.line, self.span.col));

        let lines: Vec<&str> = source.lines().collect();
        if self.span.line >= 1 && self.span.line <= lines.len() {
            let line_str = lines[self.span.line - 1];
            let line_num = format!("{:>4}", self.span.line);
            out.push_str(&format!("     |\n{} | {}\n     | ", line_num, line_str));
            let col_pad = self.span.col.saturating_sub(1);
            let width = if self.span.end_line == self.span.line {
                self.span.end_col.saturating_sub(self.span.col).max(1)
            } else {
                line_str.chars().count().saturating_sub(col_pad).max(1)
            };
            for _ in 0..col_pad {
                out.push(' ');
            }
            out.push_str(sev_color);
            for _ in 0..width {
                out.push('^');
            }
            out.push_str(reset);
            out.push('\n');
        }

        if !self.why.is_empty() || !self.why_ar.is_empty() {
            match lang {
                DiagLang::En => out.push_str(&format!("  {}Why it happened:{} {}\n", cyan, reset, self.why)),
                DiagLang::Ar => out.push_str(&format!("  {}كيف صار الخطأ:{} {}\n", cyan, reset, self.why_ar)),
                DiagLang::Both => {
                    out.push_str(&format!("  {}Why it happened:{} {}\n", cyan, reset, self.why));
                    out.push_str(&format!("  {}كيف صار الخطأ:{} {}\n", cyan, reset, self.why_ar));
                }
            }
        }

        if !self.solution.is_empty() || !self.solution_ar.is_empty() {
            match lang {
                DiagLang::En => out.push_str(&format!("  {}How to fix:{} {}\n", green, reset, self.solution)),
                DiagLang::Ar => out.push_str(&format!("  {}الحل:{} {}\n", green, reset, self.solution_ar)),
                DiagLang::Both => {
                    out.push_str(&format!("  {}How to fix:{} {}\n", green, reset, self.solution));
                    out.push_str(&format!("  {}الحل:{} {}\n", green, reset, self.solution_ar));
                }
            }
        }

        for fix in &self.fixes {
            match lang {
                DiagLang::En => out.push_str(&format!("  {}Auto-fix available:{} {}\n", yellow, reset, fix.title)),
                DiagLang::Ar => out.push_str(&format!("  {}إصلاح تلقائي متاح:{} {}\n", yellow, reset, fix.title_ar)),
                DiagLang::Both => out.push_str(&format!(
                    "  {}Auto-fix available (إصلاح تلقائي):{} {} | {}\n",
                    yellow, reset, fix.title, fix.title_ar
                )),
            }
        }

        out
    }
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

pub fn closest_match<'a>(needle: &str, candidates: &[&'a str], max_dist: usize) -> Option<&'a str> {
    let mut best: Option<(&'a str, usize)> = None;
    for &cand in candidates {
        let dist = levenshtein(needle, cand);
        if dist <= max_dist {
            if let Some((_, best_dist)) = best {
                if dist < best_dist {
                    best = Some((cand, dist));
                }
            } else {
                best = Some((cand, dist));
            }
        }
    }
    best.map(|(s, _)| s)
}
