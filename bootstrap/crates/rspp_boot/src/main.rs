use std::fs;
use std::path::{Path, PathBuf};

use rspp_core::codegen_c::{compile_to_binary, emit_c, load_program_with_imports, CompileTarget};
use rspp_core::fixer::auto_fix;
use rspp_core::math_diag::{DiagLang, Severity};
use rspp_core::{check_file, check_source, pkg, tutorial, vm};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        return;
    }

    let mut lang = DiagLang::Both;
    let mut target = CompileTarget::Native;
    let mut output_path: Option<PathBuf> = None;
    let mut emit_c_only = false;
    let mut positional: Vec<String> = Vec::new();
    let mut pass_args: Vec<String> = Vec::new();

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--lang" if i + 1 < args.len() => {
                lang = match args[i + 1].as_str() {
                    "ar" => DiagLang::Ar,
                    "en" => DiagLang::En,
                    _ => DiagLang::Both,
                };
                i += 2;
            }
            "--target" if i + 1 < args.len() => {
                target = if args[i + 1] == "kernel" {
                    CompileTarget::Kernel
                } else {
                    CompileTarget::Native
                };
                i += 2;
            }
            "-o" | "--output" if i + 1 < args.len() => {
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--emit-c" => {
                emit_c_only = true;
                i += 1;
            }
            "--" => {
                pass_args.extend(args[i + 1..].iter().cloned());
                break;
            }
            other => {
                positional.push(other.to_string());
                i += 1;
            }
        }
    }

    let Some(cmd) = positional.first().map(|s| s.as_str()) else {
        print_usage();
        return;
    };

    match cmd {
        "check" => {
            let Some(file) = positional.get(1) else {
                eprintln!("Usage: rspp check <file.rspp>");
                std::process::exit(1);
            };
            cmd_check(Path::new(file), lang);
        }
        "fix" => {
            let Some(file) = positional.get(1) else {
                eprintln!("Usage: rspp fix <file.rspp>");
                std::process::exit(1);
            };
            cmd_fix(Path::new(file), lang);
        }
        "run" => {
            let Some(file) = positional.get(1) else {
                eprintln!("Usage: rspp run <file.rspp> [-- args...]");
                std::process::exit(1);
            };
            let mut run_args = vec![file.clone()];
            run_args.extend(pass_args);
            cmd_run(Path::new(file), run_args, lang);
        }
        "build" => {
            let Some(file) = positional.get(1) else {
                eprintln!("Usage: rspp build <file.rspp> [-o out] [--target native|kernel]");
                std::process::exit(1);
            };
            cmd_build(Path::new(file), output_path.as_deref(), target, emit_c_only, lang);
        }
        "init" => {
            let name = positional.get(1).map(|s| s.as_str()).unwrap_or("my_rspp_app");
            match pkg::init_project(Path::new("."), name) {
                Ok(()) => println!("Initialized rust++ project `{}` (rustpp.toml + src/main.rspp)", name),
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        "add" => {
            let Some(spec) = positional.get(1) else {
                eprintln!("Usage: rspp add <package|git-url>");
                std::process::exit(1);
            };
            match pkg::add_dependency(Path::new("."), spec) {
                Ok(msg) => println!("{}", msg),
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        "import-c" => {
            let Some(hdr) = positional.get(1) else {
                eprintln!("Usage: rspp import-c <header.h>");
                std::process::exit(1);
            };
            match pkg::import_c_header(Path::new(hdr)) {
                Ok(bindings) => println!("{}", bindings),
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        "probe" => {
            let src = "fn main():\n    io::print(hardware::probe())\n    io::print(memory::leak_report())\n";
            let (prog, _) = check_source(src);
            let mut machine = vm::Vm::new();
            let _ = machine.run(&prog);
        }
        "learn" => {
            let l = match lang {
                DiagLang::Ar => "ar",
                _ => "en",
            };
            println!("{}", tutorial::learn_in_one_day_guide(l));
        }
        "version" | "--version" | "-V" => {
            println!("rust++ (rspp) 0.2.0 — Self-Hosting Hardware, Kernel, SDR & Math Compiler");
        }
        other => {
            if other.ends_with(".rspp") && Path::new(other).exists() {
                cmd_run(Path::new(other), vec![other.to_string()], lang);
            } else {
                eprintln!("Unknown command `{}`", other);
                print_usage();
                std::process::exit(1);
            }
        }
    }
}

fn print_usage() {
    println!(
        "rust++ (.rspp) Compiler & Hardware/SDR Toolchain v0.2.0\n\
         Usage:\n  \
           rspp check <file.rspp>             Diagnose code with math + Leak Guard engine\n  \
           rspp fix <file.rspp>               Auto-repair syntax, typos & memory leaks\n  \
           rspp run <file.rspp>               Execute immediately on rust++ VM\n  \
           rspp build <file.rspp> [-o bin]    Compile to native binary or `--target kernel`\n  \
           rspp init [name]                   Initialize rustpp.toml + src/main.rspp\n  \
           rspp add <pkg|git-url>             Install open-source external library\n  \
           rspp import-c <header.h>           Generate FFI bindings from C header\n  \
           rspp probe                         Inspect host CPU/SDR hardware capabilities\n  \
           rspp learn [--lang ar|en]          Learn rust++ in one day guide"
    );
}

fn cmd_check(path: &Path, lang: DiagLang) {
    match check_file(path) {
        Ok((_, src, diags)) => {
            let fname = path.display().to_string();
            for d in &diags {
                eprintln!("{}", d.render(&fname, &src, lang, true));
            }
            let has_err = diags.iter().any(|d| !matches!(d.severity, Severity::Warning | Severity::Hint));
            if diags.is_empty() {
                println!("OK: `{}` passed all Leak Guard, Math & Hardware Safety checks (0 issues).", fname);
            } else if has_err {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_fix(path: &Path, lang: DiagLang) {
    let Ok(src) = fs::read_to_string(path) else {
        eprintln!("Cannot read {}", path.display());
        std::process::exit(1);
    };
    let res = auto_fix(&src, 6);
    if res.applied_fixes.is_empty() {
        println!("No auto-fixes needed for {}", path.display());
    } else {
        let _ = fs::write(path, &res.fixed_source);
        println!("Applied {} mathematical / leak-guard fixes to {}:", res.applied_fixes.len(), path.display());
        for f in &res.applied_fixes {
            println!("  + {}", f);
        }
    }
    let fname = path.display().to_string();
    for d in &res.remaining_diagnostics {
        eprintln!("{}", d.render(&fname, &res.fixed_source, lang, true));
    }
}

fn cmd_run(path: &Path, args: Vec<String>, lang: DiagLang) {
    match load_program_with_imports(path) {
        Ok((prog, src, diags)) => {
            if !diags.is_empty() {
                let fname = path.display().to_string();
                for d in &diags {
                    eprintln!("{}", d.render(&fname, &src, lang, true));
                }
                std::process::exit(1);
            }
            let mut machine = vm::Vm::new().with_args(args);
            if let Err(e) = machine.run(&prog) {
                eprintln!("Runtime Error: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_build(path: &Path, out_opt: Option<&Path>, target: CompileTarget, emit_c_only: bool, lang: DiagLang) {
    match load_program_with_imports(path) {
        Ok((prog, src, diags)) => {
            if !diags.is_empty() {
                let fname = path.display().to_string();
                for d in &diags {
                    eprintln!("{}", d.render(&fname, &src, lang, true));
                }
                std::process::exit(1);
            }
            let out_bin = out_opt
                .map(PathBuf::from)
                .unwrap_or_else(|| path.with_extension(if target == CompileTarget::Kernel { "kernel.o" } else { "out" }));
            if emit_c_only {
                let c = emit_c(&prog, target);
                let c_path = out_bin.with_extension("c");
                if let Err(e) = fs::write(&c_path, c) {
                    eprintln!("Error writing {}: {}", c_path.display(), e);
                    std::process::exit(1);
                }
                println!("Emitted C11: {}", c_path.display());
                return;
            }
            match compile_to_binary(&prog, target, &out_bin, &[]) {
                Ok(()) => println!("Built {}: {}", if target == CompileTarget::Kernel { "kernel object" } else { "native binary" }, out_bin.display()),
                Err(e) => {
                    eprintln!("Build failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
