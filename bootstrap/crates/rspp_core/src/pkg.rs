use std::fs;
use std::path::Path;
use std::process::Command;

pub fn init_project(dir: &Path, name: &str) -> Result<(), String> {
    fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    let manifest = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2025\"\nentry = \"src/main.rspp\"\n\n[dependencies]\n\n[ffi]\n# c_libs = [\"m\", \"rtlsdr\"]\n",
        name
    );
    fs::write(dir.join("rustpp.toml"), manifest).map_err(|e| e.to_string())?;
    let hello = "equation energy(m, c) = m * c^2\n\nfn main():\n    let buf = memory::alloc(64)\n    defer memory::free(buf)\n    io::print(f\"Hello from rust++ ({energy(2.0, 3.0)})\")\n    io::print(memory::leak_report())\n";
    let main_path = dir.join("src").join("main.rspp");
    if !main_path.exists() {
        fs::write(main_path, hello).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn add_dependency(dir: &Path, pkg_spec: &str) -> Result<String, String> {
    let manifest_path = dir.join("rustpp.toml");
    let mut content = if manifest_path.exists() {
        fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?
    } else {
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n".to_string()
    };

    if pkg_spec.starts_with("http://") || pkg_spec.starts_with("https://") || pkg_spec.ends_with(".git") {
        let pkg_name = pkg_spec
            .trim_end_matches(".git")
            .rsplit('/')
            .next()
            .unwrap_or("ext_pkg")
            .to_string();
        let pkg_dir = dir.join(".rspp_pkgs").join(&pkg_name);
        if !pkg_dir.exists() {
            fs::create_dir_all(dir.join(".rspp_pkgs")).map_err(|e| e.to_string())?;
            let status = Command::new("git")
                .args(["clone", "--depth", "1", pkg_spec])
                .arg(&pkg_dir)
                .status()
                .map_err(|e| format!("Failed to invoke git: {}", e))?;
            if !status.success() {
                return Err(format!("git clone failed for {}", pkg_spec));
            }
        }
        let line = format!("{} = {{ git = \"{}\" }}\n", pkg_name, pkg_spec);
        content.push_str(&line);
        fs::write(&manifest_path, content).map_err(|e| e.to_string())?;
        return Ok(format!("Added git package `{}` into .rspp_pkgs/{}", pkg_name, pkg_name));
    }

    let (name, ver) = if let Some((n, v)) = pkg_spec.split_once('@') { (n, v) } else { (pkg_spec, "latest") };
    if !content.contains("[dependencies]") {
        content.push_str("\n[dependencies]\n");
    }
    content.push_str(&format!("{} = \"{}\"\n", name, ver));
    fs::write(&manifest_path, content).map_err(|e| e.to_string())?;
    Ok(format!("Registered dependency `{}` ({}) in rustpp.toml", name, ver))
}

/// Generates rust++ `extern "C" fn ...` FFI bindings from a C `.h` header file.
pub fn import_c_header(header_path: &Path) -> Result<String, String> {
    let raw = fs::read_to_string(header_path).map_err(|e| format!("Cannot read {}: {}", header_path.display(), e))?;
    let mut cleaned = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if idx + 1 < bytes.len() && bytes[idx] == b'/' && bytes[idx + 1] == b'*' {
            idx += 2;
            while idx + 1 < bytes.len() && !(bytes[idx] == b'*' && bytes[idx + 1] == b'/') {
                idx += 1;
            }
            idx = (idx + 2).min(bytes.len());
        } else {
            cleaned.push(bytes[idx] as char);
            idx += 1;
        }
    }
    let mut out = format!("// Auto-generated rust++ FFI bindings from {}\n", header_path.display());

    let joined = cleaned
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.starts_with('#') && !l.starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ");

    for decl in joined.split(';') {
        let d = decl.trim();
        if !d.contains('(') || !d.contains(')') { continue; }
        let Some((before_paren, rest)) = d.split_once('(') else { continue };
        let Some((params_str, _)) = rest.rsplit_once(')') else { continue };
        let parts: Vec<&str> = before_paren.split_whitespace().collect();
        if parts.len() < 2 { continue; }
        let fn_name = parts.last().unwrap().trim_start_matches('*');
        let ret_c = parts[..parts.len() - 1].join(" ");
        let ret_rspp = map_c_type(&ret_c);

        let mut rspp_params = Vec::new();
        for (idx, p) in params_str.split(',').enumerate() {
            let pt = p.trim();
            if pt.is_empty() || pt == "void" { continue; }
            let toks: Vec<&str> = pt.split_whitespace().collect();
            if toks.len() == 1 {
                rspp_params.push(format!("arg{}: {}", idx, map_c_type(toks[0])));
            } else {
                let pname = toks.last().unwrap().trim_start_matches('*');
                let pty = map_c_type(&toks[..toks.len() - 1].join(" "));
                rspp_params.push(format!("{}: {}", pname, pty));
            }
        }
        if ret_rspp == "void" {
            out.push_str(&format!("extern \"C\" fn {}({})\n", fn_name, rspp_params.join(", ")));
        } else {
            out.push_str(&format!("extern \"C\" fn {}({}) -> {}\n", fn_name, rspp_params.join(", "), ret_rspp));
        }
    }
    Ok(out)
}

fn map_c_type(c: &str) -> &'static str {
    if c.contains('*') || c.contains("char*") { return "ptr"; }
    if c.contains("double") || c.contains("float") { return "float"; }
    if c.contains("bool") { return "bool"; }
    if c.contains("void") { return "void"; }
    "int"
}
