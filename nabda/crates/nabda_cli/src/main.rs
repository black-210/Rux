use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;

use nabda_core::codegen_c::{compile_native_binary, generate_c, BuildTarget};
use nabda_core::diagnostic::DiagLang;
use nabda_core::{check_source, fixer, has_blocking_errors, pkg, run_source, tutorial};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return;
    }

    let lang = parse_lang_flag(&args);
    let cmd = args[1].as_str();

    match cmd {
        "run" => cmd_run(&args[2..], lang),
        "check" => cmd_check(&args[2..], lang),
        "fix" => cmd_fix(&args[2..], lang),
        "build" => cmd_build(&args[2..], lang),
        "probe" => cmd_probe(),
        "repl" => cmd_repl(lang),
        "learn" => cmd_learn(&args[2..]),
        "pkg" => cmd_pkg(&args[2..]),
        "import-c" => cmd_import_c(&args[2..]),
        "version" | "--version" | "-V" => {
            println!("Nabda Compiler (nabda / nabdac) v0.1.0");
            println!("Target: {} | Safety: Hardware + W^X + Auto-Fix", env::consts::ARCH);
        }
        "help" | "--help" | "-h" => print_help(),
        path if path.ends_with(".nb") => cmd_run(&args[1..], lang),
        other => {
            eprintln!("Unknown command `{}`. Run `nabda help` for usage.", other);
            process::exit(1);
        }
    }
}

fn parse_lang_flag(args: &[String]) -> DiagLang {
    for i in 0..args.len() {
        if args[i] == "--lang" {
            if let Some(v) = args.get(i + 1) {
                return match v.as_str() {
                    "en" => DiagLang::En,
                    "ar" => DiagLang::Ar,
                    _ => DiagLang::Both,
                };
            }
        }
    }
    DiagLang::Both
}

fn first_positional(args: &[String]) -> Option<&str> {
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--lang" || a == "-o" || a == "--target" {
            skip_next = true;
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        return Some(a.as_str());
    }
    None
}

fn read_source(file: &str) -> String {
    match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open `{}`: {}", file, e);
            process::exit(1);
        }
    }
}

fn cmd_run(args: &[String], lang: DiagLang) {
    let Some(file) = first_positional(args) else {
        eprintln!("Usage: nabda run <file.nb> [--auto-fix]");
        process::exit(1);
    };
    let auto_fix = args.iter().any(|a| a == "--auto-fix");
    let mut src = read_source(file);

    if auto_fix {
        let summary = fixer::auto_fix_source(&src);
        if !summary.applied_fixes.is_empty() {
            println!("Auto-fixed {} issue(s) before execution:", summary.applied_fixes.len());
            for f in &summary.applied_fixes {
                println!("  * {}", f);
            }
            src = summary.fixed_source;
        }
    }

    match run_source(&src) {
        Ok(_) => {}
        Err(diags) => {
            for d in &diags {
                eprint!("{}", d.render(file, &src, lang, true));
            }
            let fixable = diags.iter().filter(|d| !d.fixes.is_empty()).count();
            if fixable > 0 {
                eprintln!(
                    "\nTip: {} issue(s) can be fixed automatically! Run: nabda fix {}",
                    fixable, file
                );
            }
            process::exit(1);
        }
    }
}

fn cmd_check(args: &[String], lang: DiagLang) {
    let Some(file) = first_positional(args) else {
        eprintln!("Usage: nabda check <file.nb>");
        process::exit(1);
    };
    let src = read_source(file);
    let (_, diags, _) = check_source(&src);

    if diags.is_empty() {
        println!("OK: `{}` passed all syntax, type, hardware-safety, and cybersecurity checks.", file);
        return;
    }

    for d in &diags {
        eprint!("{}", d.render(file, &src, lang, true));
    }

    let fixable = diags.iter().filter(|d| !d.fixes.is_empty()).count();
    if fixable > 0 {
        eprintln!("\nAuto-fix available for {} diagnostic(s). Run: nabda fix {}", fixable, file);
    }

    if has_blocking_errors(&diags) {
        process::exit(1);
    }
}

fn cmd_fix(args: &[String], lang: DiagLang) {
    let Some(file) = first_positional(args) else {
        eprintln!("Usage: nabda fix <file.nb> [--dry-run]");
        process::exit(1);
    };
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let src = read_source(file);
    let summary = fixer::auto_fix_source(&src);

    if summary.applied_fixes.is_empty() {
        println!("No automatic fixes needed for `{}`.", file);
    } else {
        println!("Applied {} auto-fix(es) to `{}`:", summary.applied_fixes.len(), file);
        for f in &summary.applied_fixes {
            println!("  [FIXED] {}", f);
        }
        if dry_run {
            println!("\n--- Fixed Preview ---\n{}", summary.fixed_source);
        } else if let Err(e) = fs::write(file, &summary.fixed_source) {
            eprintln!("Failed writing fixed file: {}", e);
            process::exit(1);
        }
    }

    if !summary.remaining_diagnostics.is_empty() {
        println!("\nRemaining diagnostics:");
        for d in &summary.remaining_diagnostics {
            eprint!("{}", d.render(file, &summary.fixed_source, lang, true));
        }
        if has_blocking_errors(&summary.remaining_diagnostics) {
            process::exit(1);
        }
    }
}

fn cmd_build(args: &[String], lang: DiagLang) {
    let Some(file) = first_positional(args) else {
        eprintln!("Usage: nabda build <file.nb> [-o <output>] [--target native|kernel] [--emit-c]");
        process::exit(1);
    };
    let mut out_name: Option<String> = None;
    let mut target = BuildTarget::Native;
    let mut emit_c_only = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                if let Some(v) = args.get(i + 1) {
                    out_name = Some(v.clone());
                    i += 1;
                }
            }
            "--target" => {
                if let Some(v) = args.get(i + 1) {
                    if v == "kernel" {
                        target = BuildTarget::Kernel;
                    }
                    i += 1;
                }
            }
            "--emit-c" => emit_c_only = true,
            _ => {}
        }
        i += 1;
    }

    let src = read_source(file);
    let (program, diags, _) = check_source(&src);
    if has_blocking_errors(&diags) {
        for d in &diags {
            eprint!("{}", d.render(file, &src, lang, true));
        }
        process::exit(1);
    }

    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("out");
    let default_out = if target == BuildTarget::Kernel {
        format!("{}_kernel.o", stem)
    } else {
        stem.to_string()
    };
    let out_path = Path::new(out_name.as_deref().unwrap_or(&default_out)).to_path_buf();
    let c_path = Path::new(&format!("{}.nabda.c", stem)).to_path_buf();

    if emit_c_only {
        let cg = generate_c(&program, target);
        if let Err(e) = fs::write(&c_path, &cg.c_source) {
            eprintln!("Failed writing C file: {}", e);
            process::exit(1);
        }
        println!("Generated C source: {}", c_path.display());
        return;
    }

    match compile_native_binary(&program, &c_path, &out_path, target) {
        Ok(cg) => {
            let _ = fs::remove_file(&c_path);
            println!(
                "Built {:?} target -> {} (linked libs: {:?})",
                target,
                out_path.display(),
                cg.link_libs
            );
        }
        Err(e) => {
            eprintln!("Build failed: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_probe() {
    let probe_src = r#"
fn main():
    io::println("=== Nabda Hardware & Security Probe ===")
    io::println("Hardware: " + hardware::probe())
    io::println("Power Rails: " + power::rails())
    io::println("VCORE: " + power::get_voltage("VCORE") + " (" + power::get_watts("VCORE") + " W)")
    io::println("Fans: " + hardware::fan_list())
    io::println("GPUs: " + gpu::list())
    io::println("Displays: " + display::list())
    io::println("PCI Bus: " + hardware::pci_scan())
    io::println("NICs: " + net::interfaces())
"#;
    if let Err(e) = run_source(probe_src) {
        for d in e {
            eprintln!("{}", d.message);
        }
    }
}

fn cmd_repl(lang: DiagLang) {
    println!("Nabda Interactive REPL v0.1.0 — Learn & test hardware/security expressions in real time.");
    println!("Type `:help` or `:quit` to exit.\n");
    let stdin = io::stdin();
    let mut accumulated = String::new();
    loop {
        print!("nabda> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed == ":quit" || trimmed == ":exit" {
            break;
        }
        if trimmed == ":help" {
            println!("Commands: :quit, :clear, or enter Nabda statements like `power::get_voltage(\"VCORE\")`");
            continue;
        }
        if trimmed == ":clear" {
            accumulated.clear();
            println!("State cleared.");
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let candidate = if trimmed.starts_with("let ") || trimmed.starts_with("var ") || trimmed.starts_with("fn ") {
            format!("{}\n{}", accumulated, line)
        } else {
            format!("{}\nio::println({})\n", accumulated, trimmed)
        };
        match run_source(&candidate) {
            Ok(_) => {
                if trimmed.starts_with("let ") || trimmed.starts_with("var ") || trimmed.starts_with("fn ") {
                    accumulated.push_str(&line);
                }
            }
            Err(diags) => {
                for d in diags {
                    eprint!("{}", d.render("<repl>", &candidate, lang, true));
                }
            }
        }
    }
}

fn cmd_learn(args: &[String]) {
    let pick: Option<usize> = args.first().and_then(|s| s.parse().ok());
    for lesson in tutorial::LESSONS {
        if let Some(id) = pick {
            if lesson.id != id {
                continue;
            }
        }
        println!("\n============================================================");
        println!("{} | {}", lesson.title, lesson.title_ar);
        println!("------------------------------------------------------------");
        println!("{}", lesson.summary);
        println!("\nCode:\n{}", lesson.code);
        if pick.is_some() {
            println!("--- Running Lesson {} Output ---", lesson.id);
            let _ = run_source(lesson.code);
        }
    }
    if pick.is_none() {
        println!("\nRun `nabda learn <1..5>` to execute any lesson live!");
    }
}

fn cmd_pkg(args: &[String]) {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let sub = args.first().map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my_nabda_project");
            match pkg::init_project(&cwd, name) {
                Ok(m) => println!("{}", m),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        "add" => {
            let Some(name) = args.get(1) else {
                eprintln!("Usage: nabda pkg add <package_name> [git_or_path]");
                process::exit(1);
            };
            let src = args.get(2).map(|s| s.as_str());
            match pkg::add_dependency(&cwd, name, src) {
                Ok(m) => println!("{}", m),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            }
        }
        "install" => match pkg::install_dependencies(&cwd) {
            Ok(m) => println!("{}", m),
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        },
        "search" | "list" => {
            println!("Open-Source Nabda & C/Rust FFI Package Catalog:");
            for p in pkg::OPEN_SOURCE_CATALOG {
                println!("  {:<20} [{}] — {}", p.name, p.category, p.description);
                println!("      source: {}", p.source);
            }
        }
        other => {
            eprintln!("Unknown pkg subcommand `{}`. Try init, add, install, list.", other);
            process::exit(1);
        }
    }
}

fn cmd_import_c(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: nabda import-c <header.h> <lib_name> [-o bindings.nb]");
        process::exit(1);
    }
    let header = Path::new(&args[0]);
    let lib = &args[1];
    match pkg::import_c_header(header, lib) {
        Ok(bindings) => {
            if let Some(pos) = args.iter().position(|a| a == "-o") {
                if let Some(out) = args.get(pos + 1) {
                    if let Err(e) = fs::write(out, &bindings) {
                        eprintln!("Failed writing {}: {}", out, e);
                        process::exit(1);
                    }
                    println!("Generated FFI bindings -> {}", out);
                    return;
                }
            }
            println!("{}", bindings);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        r#"Nabda (نبضة) Programming Language Toolchain v0.1.0
A standalone Rust-powered systems, hardware, OS kernel, and cybersecurity language.

USAGE:
  nabda run <file.nb> [--auto-fix] [--lang en|ar|both]    Run a Nabda program
  nabda check <file.nb> [--lang en|ar|both]              Analyze types, hardware & security
  nabda fix <file.nb> [--dry-run]                        Auto-fix errors & security issues
  nabda build <file.nb> [-o out] [--target native|kernel] Compile to native binary or OS kernel
  nabda probe                                            Inspect hardware, power rails, GPU & NICs
  nabda repl                                             Interactive shell
  nabda learn [1..5]                                     Learn Nabda in 1 day
  nabda pkg <init|add|install|list>                      Manage open-source packages (nabda.toml)
  nabda import-c <header.h> <lib> [-o out.nb]            Generate FFI bindings from C headers
"#
    );
}
