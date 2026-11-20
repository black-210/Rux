use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub edition: String,
    pub dependencies: Vec<(String, String)>,
    pub c_libs: Vec<String>,
}

pub struct OpenSourcePkg {
    pub name: &'static str,
    pub source: &'static str,
    pub category: &'static str,
    pub description: &'static str,
}

pub const OPEN_SOURCE_CATALOG: &[OpenSourcePkg] = &[
    OpenSourcePkg {
        name: "nabda-kernel-x86",
        source: "git+https://github.com/nabda-lang/kernel-x86.git",
        category: "OS / Kernel",
        description: "Multiboot2, GDT/IDT, APIC, ACPI, NVMe & AHCI bare-metal drivers",
    },
    OpenSourcePkg {
        name: "nabda-crypto-tls",
        source: "git+https://github.com/nabda-lang/crypto-tls.git",
        category: "Cybersecurity",
        description: "Constant-time ChaCha20-Poly1305, Ed25519, X25519, and TLS 1.3",
    },
    OpenSourcePkg {
        name: "nabda-pkt-shield",
        source: "git+https://github.com/nabda-lang/pkt-shield.git",
        category: "Cybersecurity",
        description: "Zero-copy eBPF/XDP firewall, PCAP parser, and IDS rules",
    },
    OpenSourcePkg {
        name: "nabda-gpu-vulkan",
        source: "git+https://github.com/nabda-lang/gpu-vulkan.git",
        category: "GPU / Display",
        description: "Direct Vulkan compute shaders & framebuffer presenter via C FFI",
    },
    OpenSourcePkg {
        name: "nabda-hwmon-vrm",
        source: "git+https://github.com/nabda-lang/hwmon-vrm.git",
        category: "Hardware / Power",
        description: "I2C/SMBus VRM voltage controller, PWM fan curves, and thermal telemetry",
    },
    OpenSourcePkg {
        name: "openssl-ffi",
        source: "system:libssl,libcrypto",
        category: "Open-Source C Library",
        description: "Direct FFI bindings to OpenSSL 3.x shared libraries",
    },
    OpenSourcePkg {
        name: "sdl2-ffi",
        source: "system:SDL2",
        category: "Open-Source C Library",
        description: "Direct FFI bindings to SDL2 windowing, audio, and GPU rendering",
    },
];

pub fn init_project(dir: &Path, name: &str) -> Result<String, String> {
    let manifest_path = dir.join("nabda.toml");
    if manifest_path.exists() {
        return Err("nabda.toml already exists in this directory".into());
    }
    let toml = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2026"
entry = "src/main.nb"

[dependencies]
# Open-source Nabda packages, local paths, or Git repositories:
# nabda-crypto-tls = "git+https://github.com/nabda-lang/crypto-tls.git"

[ffi]
# External open-source C/Rust shared libraries to link automatically:
# libs = ["ssl", "crypto"]
"#,
        name
    );
    fs::write(&manifest_path, toml).map_err(|e| format!("Failed writing nabda.toml: {}", e))?;

    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("Failed creating src/: {}", e))?;
    let main_nb = src_dir.join("main.nb");
    if !main_nb.exists() {
        fs::write(
            &main_nb,
            format!(
                r#"# {} — Nabda Project

fn main():
    io::println("Hello from {} on Nabda!")
    let hw = hardware::probe()
    io::println(hw)
"#,
                name, name
            ),
        )
        .map_err(|e| format!("Failed writing src/main.nb: {}", e))?;
    }
    Ok(format!("Initialized Nabda project `{}` with nabda.toml and src/main.nb", name))
}

pub fn add_dependency(dir: &Path, pkg_name: &str, source_override: Option<&str>) -> Result<String, String> {
    let manifest_path = dir.join("nabda.toml");
    let mut content = if manifest_path.exists() {
        fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?
    } else {
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n".to_string()
    };

    let source = source_override.map(|s| s.to_string()).unwrap_or_else(|| {
        OPEN_SOURCE_CATALOG
            .iter()
            .find(|p| p.name == pkg_name)
            .map(|p| p.source.to_string())
            .unwrap_or_else(|| format!("https://github.com/nabda-lang/{}.git", pkg_name))
    });

    if !content.contains("[dependencies]") {
        content.push_str("\n[dependencies]\n");
    }
    let entry = format!("{} = \"{}\"\n", pkg_name, source);
    if let Some(idx) = content.find("[dependencies]") {
        let insert_at = idx + "[dependencies]".len();
        let after = &content[insert_at..];
        let newline_off = after.find('\n').map(|i| insert_at + i + 1).unwrap_or(content.len());
        content.insert_str(newline_off, &entry);
    }

    fs::write(&manifest_path, &content).map_err(|e| e.to_string())?;
    write_lockfile(dir, &content)?;
    Ok(format!("Added dependency `{}` ({})", pkg_name, source))
}

pub fn install_dependencies(dir: &Path) -> Result<String, String> {
    let manifest_path = dir.join("nabda.toml");
    if !manifest_path.exists() {
        return Err("No nabda.toml found. Run `nabda pkg init <name>` first.".into());
    }
    let content = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let deps_dir = dir.join(".nabda_pkgs");
    fs::create_dir_all(&deps_dir).map_err(|e| e.to_string())?;
    let count = write_lockfile(dir, &content)?;
    Ok(format!("Resolved and locked {} package(s) into nabda.lock", count))
}

fn write_lockfile(dir: &Path, manifest_toml: &str) -> Result<usize, String> {
    let mut lock = String::from("# Auto-generated by Nabda Package Manager\n");
    let mut in_deps = false;
    let mut count = 0usize;
    for line in manifest_toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]";
            continue;
        }
        if in_deps && !t.is_empty() && !t.starts_with('#') {
            if let Some((k, v)) = t.split_once('=') {
                count += 1;
                lock.push_str(&format!("[[package]]\nname = \"{}\"\nsource = {}\n\n", k.trim(), v.trim()));
            }
        }
    }
    fs::write(dir.join("nabda.lock"), lock).map_err(|e| e.to_string())?;
    Ok(count)
}

/// Parses C `.h` header prototypes and generates ready-to-use Nabda `extern "C" fn` bindings!
pub fn import_c_header(header_path: &Path, lib_name: &str) -> Result<String, String> {
    let header = fs::read_to_string(header_path)
        .map_err(|e| format!("Failed reading header `{}`: {}", header_path.display(), e))?;

    let mut out = format!(
        "# Auto-generated Nabda FFI bindings from {}\n# Link against open-source C/Rust library `{}`\n\n",
        header_path.display(),
        lib_name
    );

    let mut count = 0usize;
    for raw_line in header.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("//")
            || line.starts_with("/*")
            || line.starts_with('*')
            || line.starts_with("typedef")
            || line.starts_with("struct")
        {
            continue;
        }
        if let Some(semi) = line.strip_suffix(';') {
            if let Some(lp) = semi.find('(') {
                if let Some(rp) = semi.rfind(')') {
                    let before = semi[..lp].trim();
                    let params_raw = semi[lp + 1..rp].trim();
                    let mut parts: Vec<&str> = before.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let fn_name = parts.pop().unwrap().trim_start_matches('*');
                        let ret_c = parts.join(" ");
                        let ret_nb = map_c_type_to_nb(&ret_c);

                        let mut nb_params = Vec::new();
                        if !params_raw.is_empty() && params_raw != "void" {
                            for (idx, p) in params_raw.split(',').enumerate() {
                                let pt = p.trim();
                                let tokens: Vec<&str> = pt.split_whitespace().collect();
                                if let Some(&last) = tokens.last() {
                                    let clean_name = last.trim_start_matches('*');
                                    let p_name = if tokens.len() > 1 && clean_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                                        clean_name.to_string()
                                    } else {
                                        format!("arg{}", idx)
                                    };
                                    let p_ty = map_c_type_to_nb(pt);
                                    nb_params.push(format!("{}: {}", p_name, p_ty));
                                }
                            }
                        }

                        out.push_str(&format!(
                            "@link(\"{}\")\nextern \"C\" fn {}({}) -> {}\n\n",
                            lib_name,
                            fn_name,
                            nb_params.join(", "),
                            ret_nb
                        ));
                        count += 1;
                    }
                }
            }
        }
    }

    if count == 0 {
        out.push_str("# No C function prototypes matched.\n");
    }
    Ok(out)
}

fn map_c_type_to_nb(c_ty: &str) -> &'static str {
    let t = c_ty.trim();
    if t.contains("char*") || t.contains("char *") {
        "str"
    } else if t.contains('*') {
        "addr"
    } else if t.contains("double") || t.contains("float") {
        "float"
    } else if t.contains("uint64") || t.contains("size_t") {
        "u64"
    } else if t.contains("uint32") {
        "u32"
    } else if t.contains("uint16") {
        "u16"
    } else if t.contains("uint8") {
        "u8"
    } else if t == "void" {
        "void"
    } else {
        "int"
    }
}
