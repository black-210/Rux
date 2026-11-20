use std::collections::HashMap;

use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    SecurityAlert,
    HardwareSafety,
    MemoryLeak,
    Hint,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARNING",
            Severity::SecurityAlert => "SECURITY ALERT",
            Severity::HardwareSafety => "HARDWARE SAFETY",
            Severity::MemoryLeak => "LEAK GUARD",
            Severity::Hint => "HINT",
        }
    }

    pub fn label_ar(&self) -> &'static str {
        match self {
            Severity::Error => "خطأ",
            Severity::Warning => "تحذير",
            Severity::SecurityAlert => "تنبيه أمني",
            Severity::HardwareSafety => "سلامة العتاد",
            Severity::MemoryLeak => "حماية الذاكرة",
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

#[derive(Debug, Clone, PartialEq)]
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
    pub math_analysis: String,
    pub confidence: f64,
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
            math_analysis: String::new(),
            confidence: 1.0,
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

    pub fn with_math(mut self, analysis: impl Into<String>, confidence: f64) -> Self {
        self.math_analysis = analysis.into();
        self.confidence = confidence;
        self
    }

    pub fn with_fix(mut self, fix: AutoFix) -> Self {
        self.fixes.push(fix);
        self
    }

    pub fn render(&self, filename: &str, source: &str, lang: DiagLang, color: bool) -> String {
        let mut out = String::new();
        let (red, yellow, cyan, green, magenta, bold, reset) = if color {
            ("\x1b[31m", "\x1b[33m", "\x1b[36m", "\x1b[32m", "\x1b[35m", "\x1b[1m", "\x1b[0m")
        } else {
            ("", "", "", "", "", "", "")
        };

        let sev_color = match self.severity {
            Severity::Error | Severity::SecurityAlert | Severity::HardwareSafety | Severity::MemoryLeak => red,
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
                DiagLang::En => out.push_str(&format!("  {}Why:{} {}\n", cyan, reset, self.why)),
                DiagLang::Ar => out.push_str(&format!("  {}السبب:{} {}\n", cyan, reset, self.why_ar)),
                DiagLang::Both => {
                    out.push_str(&format!("  {}Why:{} {}\n", cyan, reset, self.why));
                    out.push_str(&format!("  {}كيف صار الخطأ:{} {}\n", cyan, reset, self.why_ar));
                }
            }
        }

        if !self.math_analysis.is_empty() {
            out.push_str(&format!(
                "  {}Math Engine (P={:.1}%):{} {}\n",
                magenta,
                self.confidence * 100.0,
                reset,
                self.math_analysis
            ));
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
                DiagLang::En => out.push_str(&format!("  {}Auto-fix:{} {}\n", yellow, reset, fix.title)),
                DiagLang::Ar => out.push_str(&format!("  {}إصلاح تلقائي:{} {}\n", yellow, reset, fix.title_ar)),
                DiagLang::Both => out.push_str(&format!(
                    "  {}Auto-fix (إصلاح تلقائي):{} {} | {}\n",
                    yellow, reset, fix.title, fix.title_ar
                )),
            }
        }
        out
    }
}

/// Optimal String Alignment (Damerau-Levenshtein) distance with adjacent transposition cost = 1.
pub fn damerau_levenshtein(a: &str, b: &str) -> usize {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let m = ac.len();
    let n = bc.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
            let mut best = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && ac[i - 1] == bc[j - 2] && ac[i - 2] == bc[j - 1] {
                best = best.min(dp[i - 2][j - 2] + 1);
            }
            dp[i][j] = best;
        }
    }
    dp[m][n]
}

/// Character bigram cosine similarity in [0.0, 1.0] for robust vector-space error recovery.
pub fn ngram_cosine_similarity(a: &str, b: &str) -> f64 {
    fn bigrams(s: &str) -> HashMap<(char, char), f64> {
        let padded = format!("^{}$", s.to_ascii_lowercase());
        let chars: Vec<char> = padded.chars().collect();
        let mut map = HashMap::new();
        for w in chars.windows(2) {
            *map.entry((w[0], w[1])).or_insert(0.0) += 1.0;
        }
        map
    }
    let va = bigrams(a);
    let vb = bigrams(b);
    if va.is_empty() || vb.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    for (k, wa) in &va {
        if let Some(wb) = vb.get(k) {
            dot += wa * wb;
        }
    }
    let norm_a: f64 = va.values().map(|v| v * v).sum::<f64>().sqrt();
    let norm_b: f64 = vb.values().map(|v| v * v).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[derive(Debug, Clone)]
pub struct RankedCandidate<'a> {
    pub candidate: &'a str,
    pub edit_dist: usize,
    pub cosine_sim: f64,
    pub posterior_prob: f64,
}

/// Bayesian posterior ranking combining exponential edit-distance likelihood and n-gram cosine similarity.
pub fn bayesian_best_match<'a>(needle: &str, candidates: &[&'a str]) -> Option<RankedCandidate<'a>> {
    if candidates.is_empty() || needle.is_empty() {
        return None;
    }
    let mut scored: Vec<(usize, f64, &'a str, usize, f64)> = Vec::new();
    let mut total_unnorm = 0.0;

    for (idx, &cand) in candidates.iter().enumerate() {
        let dist = damerau_levenshtein(needle, cand);
        let cos = ngram_cosine_similarity(needle, cand);
        let max_len = needle.chars().count().max(cand.chars().count()).max(1) as f64;
        let dl_score = (1.0 - (dist as f64 / max_len)).max(0.0);
        // Log-linear Bayesian likelihood exp(3.5 * dl_score + 2.5 * cos)
        let weight = (3.5 * dl_score + 2.5 * cos).exp();
        total_unnorm += weight;
        scored.push((idx, weight, cand, dist, cos));
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let best = scored.first()?;
    let posterior = if total_unnorm > 0.0 {
        (best.1 / total_unnorm).max(best.4 * 0.85).min(0.999)
    } else {
        0.0
    };

    if best.3 <= 3 || best.4 >= 0.45 {
        Some(RankedCandidate {
            candidate: best.2,
            edit_dist: best.3,
            cosine_sim: best.4,
            posterior_prob: posterior,
        })
    } else {
        None
    }
}

/// Mathematical Interval Arithmetic [lo, hi] for compile-time bounds, voltage, and div-by-zero proof.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub lo: f64,
    pub hi: f64,
}

impl Interval {
    pub fn point(v: f64) -> Self {
        Self { lo: v, hi: v }
    }

    pub fn range(lo: f64, hi: f64) -> Self {
        Self { lo: lo.min(hi), hi: lo.max(hi) }
    }

    pub fn add(self, other: Self) -> Self {
        Self { lo: self.lo + other.lo, hi: self.hi + other.hi }
    }

    pub fn sub(self, other: Self) -> Self {
        Self { lo: self.lo - other.hi, hi: self.hi - other.lo }
    }

    pub fn mul(self, other: Self) -> Self {
        let p = [self.lo * other.lo, self.lo * other.hi, self.hi * other.lo, self.hi * other.hi];
        Self {
            lo: p.iter().copied().fold(f64::INFINITY, f64::min),
            hi: p.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        }
    }

    pub fn contains(&self, v: f64) -> bool {
        v >= self.lo && v <= self.hi
    }

    pub fn is_strictly_zero(&self) -> bool {
        self.lo == 0.0 && self.hi == 0.0
    }
}
