use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use nabda_core::diagnostic::Severity;
use nabda_core::stdlib_spec::{
    find_builtin, find_builtin_const, functions_in_namespace, BUILTIN_CONSTS, NAMESPACES,
};
use nabda_core::token::KEYWORDS;
use nabda_core::{check_source, span::Span};

#[derive(Debug, Clone)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        if let Json::Obj(map) = self {
            map.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else {
            None
        }
    }
    fn as_str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s.as_str()) } else { None }
    }
    fn as_i64(&self) -> Option<i64> {
        if let Json::Num(n) = self { Some(*n as i64) } else { None }
    }
    fn to_string_compact(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => if *b { "true" } else { "false" }.into(),
            Json::Num(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            Json::Str(s) => format!("\"{}\"", escape_json(s)),
            Json::Arr(a) => {
                let parts: Vec<String> = a.iter().map(|v| v.to_string_compact()).collect();
                format!("[{}]", parts.join(","))
            }
            Json::Obj(o) => {
                let parts: Vec<String> = o
                    .iter()
                    .map(|(k, v)| format!("\"{}\":{}", escape_json(k), v.to_string_compact()))
                    .collect();
                format!("{{{}}}", parts.join(","))
            }
        }
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn parse_json(input: &str) -> Option<Json> {
    let bytes = input.as_bytes();
    let mut pos = 0usize;
    parse_val(bytes, &mut pos)
}

fn skip_ws(b: &[u8], pos: &mut usize) {
    while *pos < b.len() && matches!(b[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

fn parse_val(b: &[u8], pos: &mut usize) -> Option<Json> {
    skip_ws(b, pos);
    let ch = *b.get(*pos)?;
    match ch {
        b'n' => {
            *pos += 4;
            Some(Json::Null)
        }
        b't' => {
            *pos += 4;
            Some(Json::Bool(true))
        }
        b'f' => {
            *pos += 5;
            Some(Json::Bool(false))
        }
        b'"' => parse_str(b, pos).map(Json::Str),
        b'[' => {
            *pos += 1;
            let mut arr = Vec::new();
            loop {
                skip_ws(b, pos);
                if b.get(*pos) == Some(&b']') {
                    *pos += 1;
                    break;
                }
                arr.push(parse_val(b, pos)?);
                skip_ws(b, pos);
                if b.get(*pos) == Some(&b',') {
                    *pos += 1;
                } else {
                    if b.get(*pos) == Some(&b']') {
                        *pos += 1;
                    }
                    break;
                }
            }
            Some(Json::Arr(arr))
        }
        b'{' => {
            *pos += 1;
            let mut obj = Vec::new();
            loop {
                skip_ws(b, pos);
                if b.get(*pos) == Some(&b'}') {
                    *pos += 1;
                    break;
                }
                let k = parse_str(b, pos)?;
                skip_ws(b, pos);
                if b.get(*pos) == Some(&b':') {
                    *pos += 1;
                }
                let v = parse_val(b, pos)?;
                obj.push((k, v));
                skip_ws(b, pos);
                if b.get(*pos) == Some(&b',') {
                    *pos += 1;
                } else {
                    if b.get(*pos) == Some(&b'}') {
                        *pos += 1;
                    }
                    break;
                }
            }
            Some(Json::Obj(obj))
        }
        _ => {
            let start = *pos;
            while *pos < b.len() && (b[*pos].is_ascii_digit() || matches!(b[*pos], b'-' | b'+' | b'.' | b'e' | b'E')) {
                *pos += 1;
            }
            let s = std::str::from_utf8(&b[start..*pos]).ok()?;
            s.parse::<f64>().ok().map(Json::Num)
        }
    }
}

fn parse_str(b: &[u8], pos: &mut usize) -> Option<String> {
    if *b.get(*pos)? != b'"' {
        return None;
    }
    *pos += 1;
    let mut out = String::new();
    while *pos < b.len() {
        let c = b[*pos];
        if c == b'"' {
            *pos += 1;
            return Some(out);
        }
        if c == b'\\' {
            *pos += 1;
            let esc = *b.get(*pos)?;
            *pos += 1;
            match esc {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' if *pos + 4 <= b.len() => {
                    if let Ok(hex) = std::str::from_utf8(&b[*pos..*pos + 4]) {
                        if let Ok(code) = u32::from_str_radix(hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        }
                    }
                    *pos += 4;
                }
                other => out.push(other as char),
            }
        } else {
            let rest = std::str::from_utf8(&b[*pos..]).ok()?;
            let ch = rest.chars().next()?;
            out.push(ch);
            *pos += ch.len_utf8();
        }
    }
    Some(out)
}

fn send_msg(out: &mut impl Write, payload: &Json) {
    let body = payload.to_string_compact();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let _ = out.write_all(header.as_bytes());
    let _ = out.write_all(body.as_bytes());
    let _ = out.flush();
}

fn read_lsp_message(reader: &mut impl BufRead) -> Option<String> {
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
            content_length = len_str.trim().parse().unwrap_or(0);
        }
    }
    if content_length == 0 {
        return None;
    }
    let mut buf = vec![0u8; content_length];
    reader.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn span_to_lsp_range(sp: Span) -> Json {
    let start_line = sp.line.saturating_sub(1);
    let start_col = sp.col.saturating_sub(1);
    let end_line = sp.end_line.saturating_sub(1).max(start_line);
    let end_col = sp.end_col.saturating_sub(1);
    Json::Obj(vec![
        (
            "start".into(),
            Json::Obj(vec![
                ("line".into(), Json::Num(start_line as f64)),
                ("character".into(), Json::Num(start_col as f64)),
            ]),
        ),
        (
            "end".into(),
            Json::Obj(vec![
                ("line".into(), Json::Num(end_line as f64)),
                ("character".into(), Json::Num(end_col as f64)),
            ]),
        ),
    ])
}

fn publish_diagnostics(out: &mut impl Write, uri: &str, text: &str) {
    let (_, diags, _) = check_source(text);
    let lsp_diags: Vec<Json> = diags
        .iter()
        .map(|d| {
            let sev = match d.severity {
                Severity::Error | Severity::SecurityAlert | Severity::HardwareSafety => 1,
                Severity::Warning => 2,
                Severity::Hint => 4,
            };
            let full_msg = format!(
                "{}\n{}\nWhy: {}\nFix: {}",
                d.message, d.message_ar, d.why, d.solution
            );
            Json::Obj(vec![
                ("range".into(), span_to_lsp_range(d.span)),
                ("severity".into(), Json::Num(sev as f64)),
                ("code".into(), Json::Str(d.code.clone())),
                ("source".into(), Json::Str("nabda-compiler".into())),
                ("message".into(), Json::Str(full_msg)),
            ])
        })
        .collect();

    let notif = Json::Obj(vec![
        ("jsonrpc".into(), Json::Str("2.0".into())),
        ("method".into(), Json::Str("textDocument/publishDiagnostics".into())),
        (
            "params".into(),
            Json::Obj(vec![
                ("uri".into(), Json::Str(uri.into())),
                ("diagnostics".into(), Json::Arr(lsp_diags)),
            ]),
        ),
    ]);
    send_msg(out, &notif);
}

fn word_at_position(text: &str, line0: usize, char0: usize) -> (String, Option<String>) {
    let line = text.lines().nth(line0).unwrap_or("");
    let chars: Vec<char> = line.chars().collect();
    let idx = char0.min(chars.len());
    let mut start = idx;
    while start > 0 && (chars[start - 1].is_ascii_alphanumeric() || chars[start - 1] == '_' || chars[start - 1] == ':') {
        start -= 1;
    }
    let mut end = idx;
    while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
        end += 1;
    }
    let token: String = chars[start..end].iter().collect();
    let prefix: String = chars[..idx].iter().collect();
    if let Some((ns, member)) = token.split_once("::") {
        (member.to_string(), Some(ns.to_string()))
    } else if let Some(rest) = prefix.rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':').next() {
        if let Some((ns, _)) = rest.split_once("::") {
            (token, Some(ns.to_string()))
        } else {
            (token, None)
        }
    } else {
        (token, None)
    }
}

fn handle_completion(text: &str, line0: usize, char0: usize) -> Json {
    let (_, ns_ctx) = word_at_position(text, line0, char0);
    let mut items = Vec::new();

    if let Some(ns) = ns_ctx {
        for f in functions_in_namespace(&ns) {
            let params_sig = f
                .params
                .iter()
                .map(|(n, t)| format!("{}: {}", n, t))
                .collect::<Vec<_>>()
                .join(", ");
            let snippet_args = f
                .params
                .iter()
                .enumerate()
                .map(|(i, (n, _))| format!("${{{}:{}}}", i + 1, n))
                .collect::<Vec<_>>()
                .join(", ");
            items.push(Json::Obj(vec![
                ("label".into(), Json::Str(f.name.into())),
                ("kind".into(), Json::Num(3.0)), // Function
                ("detail".into(), Json::Str(format!("{}::{}({}) -> {}", ns, f.name, params_sig, f.return_type))),
                ("documentation".into(), Json::Str(format!("{}\n{}\nExample: {}", f.doc_en, f.doc_ar, f.example))),
                ("insertText".into(), Json::Str(format!("{}({})", f.name, snippet_args))),
                ("insertTextFormat".into(), Json::Num(2.0)), // Snippet
            ]));
        }
        for c in BUILTIN_CONSTS.iter().filter(|c| c.namespace == ns) {
            items.push(Json::Obj(vec![
                ("label".into(), Json::Str(c.name.into())),
                ("kind".into(), Json::Num(21.0)), // Constant
                ("detail".into(), Json::Str(c.ty.into())),
                ("documentation".into(), Json::Str(c.doc_en.into())),
            ]));
        }
    } else {
        for &ns in NAMESPACES {
            items.push(Json::Obj(vec![
                ("label".into(), Json::Str(format!("{}::", ns))),
                ("kind".into(), Json::Num(9.0)), // Module
                ("detail".into(), Json::Str(format!("Nabda `{}` hardware/system module", ns))),
            ]));
        }
        for &kw in KEYWORDS {
            items.push(Json::Obj(vec![
                ("label".into(), Json::Str(kw.into())),
                ("kind".into(), Json::Num(14.0)), // Keyword
            ]));
        }
        let (_, _, syms) = check_source(text);
        for s in syms {
            items.push(Json::Obj(vec![
                ("label".into(), Json::Str(s.name)),
                ("kind".into(), Json::Num(if s.kind == "function" { 3.0 } else { 6.0 })),
                ("detail".into(), Json::Str(s.detail)),
            ]));
        }
    }

    Json::Obj(vec![
        ("isIncomplete".into(), Json::Bool(false)),
        ("items".into(), Json::Arr(items)),
    ])
}

fn handle_hover(text: &str, line0: usize, char0: usize) -> Json {
    let (word, ns_opt) = word_at_position(text, line0, char0);
    if let Some(ns) = ns_opt {
        if let Some(spec) = find_builtin(&ns, &word) {
            let sig = spec
                .params
                .iter()
                .map(|(n, t)| format!("{}: {}", n, t))
                .collect::<Vec<_>>()
                .join(", ");
            let md = format!(
                "```nabda\nfn {}::{}({}) -> {}\n```\n{}\n\n{}\n\nExample: `{}`",
                spec.namespace, spec.name, sig, spec.return_type, spec.doc_en, spec.doc_ar, spec.example
            );
            return Json::Obj(vec![(
                "contents".into(),
                Json::Obj(vec![
                    ("kind".into(), Json::Str("markdown".into())),
                    ("value".into(), Json::Str(md)),
                ]),
            )]);
        }
        if let Some(c) = find_builtin_const(&ns, &word) {
            let md = format!("```nabda\nconst {}::{}: {}\n```\n{}", c.namespace, c.name, c.ty, c.doc_en);
            return Json::Obj(vec![(
                "contents".into(),
                Json::Obj(vec![
                    ("kind".into(), Json::Str("markdown".into())),
                    ("value".into(), Json::Str(md)),
                ]),
            )]);
        }
    }
    Json::Null
}

fn handle_code_actions(uri: &str, text: &str, req_line0: usize) -> Json {
    let (_, diags, _) = check_source(text);
    let mut actions = Vec::new();
    for d in diags {
        let diag_line0 = d.span.line.saturating_sub(1);
        if (diag_line0 as isize - req_line0 as isize).abs() > 2 && !d.fixes.is_empty() {
            continue;
        }
        for fix in d.fixes {
            let edits: Vec<Json> = fix
                .edits
                .iter()
                .map(|e| {
                    Json::Obj(vec![
                        ("range".into(), span_to_lsp_range(e.span)),
                        ("newText".into(), Json::Str(e.replacement.clone())),
                    ])
                })
                .collect();
            actions.push(Json::Obj(vec![
                ("title".into(), Json::Str(format!("Nabda Quick Fix: {} ({})", fix.title, fix.title_ar))),
                ("kind".into(), Json::Str("quickfix".into())),
                ("isPreferred".into(), Json::Bool(fix.is_preferred)),
                (
                    "edit".into(),
                    Json::Obj(vec![(
                        "changes".into(),
                        Json::Obj(vec![(uri.into(), Json::Arr(edits))]),
                    )]),
                ),
            ]));
        }
    }
    Json::Arr(actions)
}

fn handle_signature_help(text: &str, line0: usize, char0: usize) -> Json {
    let line = text.lines().nth(line0).unwrap_or("");
    let prefix: String = line.chars().take(char0).collect();
    if let Some(lp) = prefix.rfind('(') {
        let before = &prefix[..lp];
        let call_tok = before
            .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':')
            .next()
            .unwrap_or("");
        if let Some((ns, func)) = call_tok.split_once("::") {
            if let Some(spec) = find_builtin(ns, func) {
                let active_param = prefix[lp + 1..].matches(',').count();
                let params: Vec<Json> = spec
                    .params
                    .iter()
                    .map(|(n, t)| Json::Obj(vec![("label".into(), Json::Str(format!("{}: {}", n, t)))]))
                    .collect();
                let sig_label = format!(
                    "{}::{}({}) -> {}",
                    spec.namespace,
                    spec.name,
                    spec.params.iter().map(|(n, t)| format!("{}: {}", n, t)).collect::<Vec<_>>().join(", "),
                    spec.return_type
                );
                return Json::Obj(vec![
                    (
                        "signatures".into(),
                        Json::Arr(vec![Json::Obj(vec![
                            ("label".into(), Json::Str(sig_label)),
                            ("documentation".into(), Json::Str(spec.doc_en.into())),
                            ("parameters".into(), Json::Arr(params)),
                        ])]),
                    ),
                    ("activeSignature".into(), Json::Num(0.0)),
                    ("activeParameter".into(), Json::Num(active_param as f64)),
                ]);
            }
        }
    }
    Json::Null
}

fn handle_definition(uri: &str, text: &str, line0: usize, char0: usize) -> Json {
    let (word, _) = word_at_position(text, line0, char0);
    let (_, _, syms) = check_source(text);
    if let Some(sym) = syms.into_iter().find(|s| s.name == word) {
        return Json::Obj(vec![
            ("uri".into(), Json::Str(uri.into())),
            ("range".into(), span_to_lsp_range(sym.span)),
        ]);
    }
    Json::Null
}

fn handle_document_symbols(text: &str) -> Json {
    let (_, _, syms) = check_source(text);
    let arr: Vec<Json> = syms
        .into_iter()
        .map(|s| {
            let kind = match s.kind {
                "function" | "extern_fn" => 12,
                "struct" => 23,
                "enum" => 10,
                _ => 13,
            };
            let rng = span_to_lsp_range(s.span);
            Json::Obj(vec![
                ("name".into(), Json::Str(s.name)),
                ("detail".into(), Json::Str(s.detail)),
                ("kind".into(), Json::Num(kind as f64)),
                ("range".into(), rng.clone()),
                ("selectionRange".into(), rng),
            ])
        })
        .collect();
    Json::Arr(arr)
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut out = io::BufWriter::new(stdout.lock());
    let mut docs: HashMap<String, String> = HashMap::new();

    while let Some(raw) = read_lsp_message(&mut reader) {
        let Some(msg) = parse_json(&raw) else { continue };
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params");

        match method {
            "initialize" => {
                let result = Json::Obj(vec![(
                    "capabilities".into(),
                    Json::Obj(vec![
                        ("textDocumentSync".into(), Json::Num(1.0)), // Full sync
                        (
                            "completionProvider".into(),
                            Json::Obj(vec![
                                ("triggerCharacters".into(), Json::Arr(vec![Json::Str(":".into()), Json::Str(".".into())])),
                                ("resolveProvider".into(), Json::Bool(false)),
                            ]),
                        ),
                        ("hoverProvider".into(), Json::Bool(true)),
                        ("codeActionProvider".into(), Json::Bool(true)),
                        ("definitionProvider".into(), Json::Bool(true)),
                        ("documentSymbolProvider".into(), Json::Bool(true)),
                        (
                            "signatureHelpProvider".into(),
                            Json::Obj(vec![(
                                "triggerCharacters".into(),
                                Json::Arr(vec![Json::Str("(".into()), Json::Str(",".into())]),
                            )]),
                        ),
                    ]),
                )]);
                if let Some(rid) = id {
                    let resp = Json::Obj(vec![
                        ("jsonrpc".into(), Json::Str("2.0".into())),
                        ("id".into(), rid),
                        ("result".into(), result),
                    ]);
                    send_msg(&mut out, &resp);
                }
            }
            "shutdown" => {
                if let Some(rid) = id {
                    let resp = Json::Obj(vec![
                        ("jsonrpc".into(), Json::Str("2.0".into())),
                        ("id".into(), rid),
                        ("result".into(), Json::Null),
                    ]);
                    send_msg(&mut out, &resp);
                }
            }
            "exit" => break,
            "textDocument/didOpen" => {
                if let Some(td) = params.and_then(|p| p.get("textDocument")) {
                    if let (Some(uri), Some(text)) = (td.get("uri").and_then(|v| v.as_str()), td.get("text").and_then(|v| v.as_str())) {
                        docs.insert(uri.to_string(), text.to_string());
                        publish_diagnostics(&mut out, uri, text);
                    }
                }
            }
            "textDocument/didChange" => {
                if let Some(p) = params {
                    let uri = p.get("textDocument").and_then(|t| t.get("uri")).and_then(|v| v.as_str());
                    let changes = p.get("contentChanges").and_then(|c| match c {
                        Json::Arr(a) => a.first().and_then(|e| e.get("text")).and_then(|t| t.as_str()),
                        _ => None,
                    });
                    if let (Some(u), Some(txt)) = (uri, changes) {
                        docs.insert(u.to_string(), txt.to_string());
                        publish_diagnostics(&mut out, u, txt);
                    }
                }
            }
            "textDocument/completion" => {
                if let Some(rid) = id {
                    let uri = params.and_then(|p| p.get("textDocument")).and_then(|t| t.get("uri")).and_then(|v| v.as_str()).unwrap_or("");
                    let line0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("line")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let char0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("character")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let text = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                    let res = handle_completion(text, line0, char0);
                    send_msg(
                        &mut out,
                        &Json::Obj(vec![
                            ("jsonrpc".into(), Json::Str("2.0".into())),
                            ("id".into(), rid),
                            ("result".into(), res),
                        ]),
                    );
                }
            }
            "textDocument/hover" => {
                if let Some(rid) = id {
                    let uri = params.and_then(|p| p.get("textDocument")).and_then(|t| t.get("uri")).and_then(|v| v.as_str()).unwrap_or("");
                    let line0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("line")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let char0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("character")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let text = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                    let res = handle_hover(text, line0, char0);
                    send_msg(
                        &mut out,
                        &Json::Obj(vec![
                            ("jsonrpc".into(), Json::Str("2.0".into())),
                            ("id".into(), rid),
                            ("result".into(), res),
                        ]),
                    );
                }
            }
            "textDocument/codeAction" => {
                if let Some(rid) = id {
                    let uri = params.and_then(|p| p.get("textDocument")).and_then(|t| t.get("uri")).and_then(|v| v.as_str()).unwrap_or("");
                    let line0 = params.and_then(|p| p.get("range")).and_then(|r| r.get("start")).and_then(|s| s.get("line")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let text = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                    let res = handle_code_actions(uri, text, line0);
                    send_msg(
                        &mut out,
                        &Json::Obj(vec![
                            ("jsonrpc".into(), Json::Str("2.0".into())),
                            ("id".into(), rid),
                            ("result".into(), res),
                        ]),
                    );
                }
            }
            "textDocument/signatureHelp" => {
                if let Some(rid) = id {
                    let uri = params.and_then(|p| p.get("textDocument")).and_then(|t| t.get("uri")).and_then(|v| v.as_str()).unwrap_or("");
                    let line0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("line")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let char0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("character")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let text = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                    let res = handle_signature_help(text, line0, char0);
                    send_msg(
                        &mut out,
                        &Json::Obj(vec![
                            ("jsonrpc".into(), Json::Str("2.0".into())),
                            ("id".into(), rid),
                            ("result".into(), res),
                        ]),
                    );
                }
            }
            "textDocument/definition" => {
                if let Some(rid) = id {
                    let uri = params.and_then(|p| p.get("textDocument")).and_then(|t| t.get("uri")).and_then(|v| v.as_str()).unwrap_or("");
                    let line0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("line")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let char0 = params.and_then(|p| p.get("position")).and_then(|pos| pos.get("character")).and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                    let text = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                    let res = handle_definition(uri, text, line0, char0);
                    send_msg(
                        &mut out,
                        &Json::Obj(vec![
                            ("jsonrpc".into(), Json::Str("2.0".into())),
                            ("id".into(), rid),
                            ("result".into(), res),
                        ]),
                    );
                }
            }
            "textDocument/documentSymbol" => {
                if let Some(rid) = id {
                    let uri = params.and_then(|p| p.get("textDocument")).and_then(|t| t.get("uri")).and_then(|v| v.as_str()).unwrap_or("");
                    let text = docs.get(uri).map(|s| s.as_str()).unwrap_or("");
                    let res = handle_document_symbols(text);
                    send_msg(
                        &mut out,
                        &Json::Obj(vec![
                            ("jsonrpc".into(), Json::Str("2.0".into())),
                            ("id".into(), rid),
                            ("result".into(), res),
                        ]),
                    );
                }
            }
            _ => {
                if let Some(rid) = id {
                    send_msg(
                        &mut out,
                        &Json::Obj(vec![
                            ("jsonrpc".into(), Json::Str("2.0".into())),
                            ("id".into(), rid),
                            ("result".into(), Json::Null),
                        ]),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsp_completions_and_quickfixes() {
        let src = "fn main():\n    memory::";
        let comp = handle_completion(src, 1, 14);
        let serialized = comp.to_string_compact();
        assert!(serialized.contains("\"alloc\""));
        assert!(serialized.contains("\"zero_secure\""));
        assert!(serialized.contains("\"protect\""));

        let broken = "fn main()\n    let x = 1\n    x = 2\n";
        let actions = handle_code_actions("file:///test.nb", broken, 0);
        let act_json = actions.to_string_compact();
        assert!(act_json.contains("Nabda Quick Fix"));
    }
}
