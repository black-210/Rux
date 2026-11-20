use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};

use rspp_core::check_source;
use rspp_core::math_diag::Severity;
use rspp_core::stdlib_spec::{BUILTINS, NAMESPACES};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut docs: HashMap<String, String> = HashMap::new();

    while let Some(msg) = read_lsp_message(&mut reader) {
        let method = json_get_str(&msg, "method").unwrap_or_default();
        let id_raw = json_get_raw(&msg, "id");

        match method.as_str() {
            "initialize" => {
                if let Some(id) = id_raw {
                    let caps = r#"{"capabilities":{"textDocumentSync":1,"completionProvider":{"triggerCharacters":[":","."]},"hoverProvider":true,"codeActionProvider":true}}"#;
                    send_response(&mut writer, &id, caps);
                }
            }
            "shutdown" => {
                if let Some(id) = id_raw {
                    send_response(&mut writer, &id, "null");
                }
            }
            "exit" => break,
            "textDocument/didOpen" | "textDocument/didChange" => {
                if let Some(uri) = extract_uri(&msg) {
                    if let Some(text) = extract_text(&msg) {
                        docs.insert(uri.clone(), text.clone());
                        publish_diagnostics(&mut writer, &uri, &text);
                    }
                }
            }
            "textDocument/completion" => {
                if let Some(id) = id_raw {
                    let uri = extract_uri(&msg).unwrap_or_default();
                    let line = json_get_int(&msg, "line").unwrap_or(0) as usize;
                    let col = json_get_int(&msg, "character").unwrap_or(0) as usize;
                    let text = docs.get(&uri).map(|s| s.as_str()).unwrap_or("");
                    let items = compute_completions(text, line, col);
                    send_response(&mut writer, &id, &items);
                }
            }
            "textDocument/hover" => {
                if let Some(id) = id_raw {
                    let uri = extract_uri(&msg).unwrap_or_default();
                    let line = json_get_int(&msg, "line").unwrap_or(0) as usize;
                    let col = json_get_int(&msg, "character").unwrap_or(0) as usize;
                    let text = docs.get(&uri).map(|s| s.as_str()).unwrap_or("");
                    let hover = compute_hover(text, line, col);
                    send_response(&mut writer, &id, &hover);
                }
            }
            "textDocument/codeAction" => {
                if let Some(id) = id_raw {
                    let uri = extract_uri(&msg).unwrap_or_default();
                    let line = json_get_int(&msg, "line").unwrap_or(0) as usize;
                    let text = docs.get(&uri).map(|s| s.as_str()).unwrap_or("");
                    let actions = compute_code_actions(&uri, text, line);
                    send_response(&mut writer, &id, &actions);
                }
            }
            _ => {
                if let Some(id) = id_raw {
                    send_response(&mut writer, &id, "null");
                }
            }
        }
    }
}

fn read_lsp_message(reader: &mut BufReader<io::StdinLock<'_>>) -> Option<String> {
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n == 0 { return None; }
        let t = line.trim();
        if t.is_empty() { break; }
        if let Some(rest) = t.to_ascii_lowercase().strip_prefix("content-length:") {
            content_len = rest.trim().parse::<usize>().unwrap_or(0);
        }
    }
    if content_len == 0 { return None; }
    let mut buf = vec![0u8; content_len];
    reader.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn send_lsp(writer: &mut io::StdoutLock<'_>, json: &str) {
    let header = format!("Content-Length: {}\r\n\r\n", json.len());
    let _ = writer.write_all(header.as_bytes());
    let _ = writer.write_all(json.as_bytes());
    let _ = writer.flush();
}

fn send_response(writer: &mut io::StdoutLock<'_>, id: &str, result_json: &str) {
    let msg = format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}", id, result_json);
    send_lsp(writer, &msg);
}

fn publish_diagnostics(writer: &mut io::StdoutLock<'_>, uri: &str, text: &str) {
    let (_, diags) = check_source(text);
    let mut items = Vec::new();
    for d in &diags {
        let sev = match d.severity {
            Severity::Warning => 2,
            Severity::Hint => 4,
            _ => 1,
        };
        let sl = d.span.line.saturating_sub(1);
        let sc = d.span.col.saturating_sub(1);
        let el = d.span.end_line.saturating_sub(1).max(sl);
        let ec = d.span.end_col.saturating_sub(1).max(sc + 1);
        let full_msg = format!(
            "[{}] {} | {} | Math: {} | Fix: {}",
            d.code, d.message, d.message_ar, d.math_analysis, d.solution
        );
        items.push(format!(
            "{{\"range\":{{\"start\":{{\"line\":{},\"character\":{}}},\"end\":{{\"line\":{},\"character\":{}}}}},\"severity\":{},\"code\":\"{}\",\"source\":\"rust++\",\"message\":\"{}\"}}",
            sl, sc, el, ec, sev, esc_json(&d.code), esc_json(&full_msg)
        ));
    }
    let notif = format!(
        "{{\"jsonrpc\":\"2.0\",\"method\":\"textDocument/publishDiagnostics\",\"params\":{{\"uri\":\"{}\",\"diagnostics\":[{}]}}}}",
        esc_json(uri),
        items.join(",")
    );
    send_lsp(writer, &notif);
}

fn compute_completions(text: &str, line: usize, col: usize) -> String {
    let line_str = text.lines().nth(line).unwrap_or("");
    let prefix: String = line_str.chars().take(col).collect();
    let mut items = Vec::new();

    if let Some(idx) = prefix.rfind("::") {
        let before = &prefix[..idx];
        let ns: String = before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>().chars().rev().collect();
        for b in BUILTINS.iter().filter(|b| b.namespace == ns) {
            items.push(format!(
                "{{\"label\":\"{}\",\"kind\":3,\"detail\":\"{}\",\"documentation\":\"{} ({})\"}}",
                esc_json(b.name), esc_json(b.signature), esc_json(b.summary), esc_json(b.summary_ar)
            ));
        }
    } else {
        for ns in NAMESPACES {
            items.push(format!("{{\"label\":\"{}\",\"kind\":9,\"detail\":\"rust++ namespace {}::\"}}", ns, ns));
        }
        for kw in ["fn", "equation", "algorithm", "let", "var", "defer", "try", "catch", "throw", "unsafe_hardware", "struct", "enum", "import", "return"] {
            items.push(format!("{{\"label\":\"{}\",\"kind\":14,\"detail\":\"keyword\"}}", kw));
        }
    }
    format!("[{}]", items.join(","))
}

fn compute_hover(text: &str, line: usize, col: usize) -> String {
    let line_str = text.lines().nth(line).unwrap_or("");
    let chars: Vec<char> = line_str.chars().collect();
    if chars.is_empty() { return "null".into(); }
    let mut c = col.min(chars.len().saturating_sub(1));
    while c > 0 && (chars[c].is_ascii_alphanumeric() || chars[c] == '_' || chars[c] == ':') { c -= 1; }
    if !chars[c].is_ascii_alphanumeric() && chars[c] != '_' && c + 1 < chars.len() { c += 1; }
    let word: String = chars[c..].iter().take_while(|ch| ch.is_ascii_alphanumeric() || **ch == '_' || **ch == ':').collect();
    if let Some((ns, fn_name)) = word.split_once("::") {
        if let Some(b) = BUILTINS.iter().find(|b| b.namespace == ns && b.name == fn_name) {
            let doc = format!("```rustpp\n{}\n```\n{}\n\n{}", b.signature, b.summary, b.summary_ar);
            return format!("{{\"contents\":{{\"kind\":\"markdown\",\"value\":\"{}\"}}}}", esc_json(&doc));
        }
    }
    "null".into()
}

fn compute_code_actions(uri: &str, text: &str, line: usize) -> String {
    let (_, diags) = check_source(text);
    let target_line = line + 1;
    let mut actions = Vec::new();
    for d in diags {
        if d.span.line.abs_diff(target_line) <= 1 {
            for fix in d.fixes {
                let mut edits = Vec::new();
                for ed in fix.edits {
                    edits.push(format!(
                        "{{\"range\":{{\"start\":{{\"line\":{},\"character\":{}}},\"end\":{{\"line\":{},\"character\":{}}}}},\"newText\":\"{}\"}}",
                        ed.span.line.saturating_sub(1),
                        ed.span.col.saturating_sub(1),
                        ed.span.end_line.saturating_sub(1),
                        ed.span.end_col.saturating_sub(1),
                        esc_json(&ed.replacement)
                    ));
                }
                actions.push(format!(
                    "{{\"title\":\"[{}] {}\",\"kind\":\"quickfix\",\"edit\":{{\"changes\":{{\"{}\":[{}]}}}}}}",
                    esc_json(&d.code),
                    esc_json(&fix.title),
                    esc_json(uri),
                    edits.join(",")
                ));
            }
        }
    }
    format!("[{}]", actions.join(","))
}

fn extract_uri(msg: &str) -> Option<String> { json_get_str(msg, "uri") }

fn extract_text(msg: &str) -> Option<String> { json_get_str(msg, "text") }

fn json_get_str(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let idx = json.find(&pat)?;
    let after = &json[idx + pat.len()..];
    let colon = after.find(':')?;
    let val = after[colon + 1..].trim_start();
    if !val.starts_with('"') { return None; }
    let mut out = String::new();
    let mut chars = val[1..].chars();
    while let Some(c) = chars.next() {
        if c == '"' { return Some(out); }
        if c == '\\' {
            match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => out.push(other),
            }
        } else {
            out.push(c);
        }
    }
    None
}

fn json_get_int(json: &str, key: &str) -> Option<i64> {
    let pat = format!("\"{}\"", key);
    let idx = json.rfind(&pat)?;
    let after = &json[idx + pat.len()..];
    let colon = after.find(':')?;
    let num: String = after[colon + 1..].trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    num.parse().ok()
}

fn json_get_raw(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let idx = json.find(&pat)?;
    let after = &json[idx + pat.len()..];
    let colon = after.find(':')?;
    let val = after[colon + 1..].trim_start();
    if val.starts_with('"') {
        let s = json_get_str(json, key)?;
        return Some(format!("\"{}\"", esc_json(&s)));
    }
    let raw: String = val.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    if raw.is_empty() { None } else { Some(raw) }
}

fn esc_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 32 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}
