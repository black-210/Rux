pub mod analyzer;
pub mod ast;
pub mod codegen_c;
pub mod fixer;
pub mod leak_guard;
pub mod lexer;
pub mod math_diag;
pub mod parser;
pub mod pkg;
pub mod span;
pub mod stdlib_spec;
pub mod token;
pub mod tutorial;
pub mod vm;

use std::path::Path;

pub use codegen_c::CompileTarget;
pub use math_diag::{DiagLang, Diagnostic};

pub fn check_source(source: &str) -> (ast::Program, Vec<Diagnostic>) {
    let lx = lexer::lex(source);
    let mut diags = lx.diagnostics;
    let ps = parser::parse(lx.tokens);
    diags.extend(ps.diagnostics);
    let an = analyzer::analyze(&ps.program, source);
    diags.extend(an.diagnostics);
    (ps.program, diags)
}

pub fn check_file(path: &Path) -> Result<(ast::Program, String, Vec<Diagnostic>), String> {
    let (program, entry_src, mut diags) = codegen_c::load_program_with_imports(path)?;
    let an = analyzer::analyze(&program, &entry_src);
    diags.extend(an.diagnostics);
    Ok((program, entry_src, diags))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_diag::{bayesian_best_match, damerau_levenshtein, ngram_cosine_similarity, Interval};

    #[test]
    fn test_math_diagnostics_algorithms() {
        assert_eq!(damerau_levenshtein("prnit", "print"), 1);
        assert_eq!(damerau_levenshtein("alloc", "aloc"), 1);
        assert!(ngram_cosine_similarity("set_volatge", "set_voltage") > 0.7);
        let best = bayesian_best_match("aloc", &["alloc", "free", "protect"]).unwrap();
        assert_eq!(best.candidate, "alloc");
        assert!(best.posterior_prob > 0.5);

        let a = Interval::range(2.0, 4.0);
        let b = Interval::point(0.0);
        assert!(!a.is_strictly_zero());
        assert!(b.is_strictly_zero());
    }

    #[test]
    fn test_leak_guard_detects_and_fixes_unfreed_alloc() {
        let code = "fn main():\n    let buf = memory::alloc(128)\n    io::print(buf)\n";
        let (_, diags) = check_source(code);
        assert!(diags.iter().any(|d| d.code == "LG001"), "Expected LG001 leak error");
        let fixed = fixer::auto_fix(code, 3);
        assert!(fixed.fixed_source.contains("defer memory::free(buf)"));
        let (_, after) = check_source(&fixed.fixed_source);
        assert!(!after.iter().any(|d| d.code == "LG001"), "Expected zero leaks after auto-fix");
    }

    #[test]
    fn test_equations_and_solver_in_vm() {
        let code = r#"
equation poly(x) = x^2 - 5x + 6
fn main():
    let r = math::solve("poly", 4.0)
    let d = math::derivative("poly", 4.0)
    io::print(r)
    io::print(d)
"#;
        let (prog, diags) = check_source(code);
        assert!(diags.is_empty(), "Unexpected diags: {:?}", diags);
        let mut machine = vm::Vm::new();
        machine.run(&prog).unwrap();
        let r: f64 = machine.output[0].parse().unwrap();
        let d: f64 = machine.output[1].parse().unwrap();
        assert!((r - 3.0).abs() < 1e-4, "Expected root 3.0, got {}", r);
        assert!((d - 3.0).abs() < 1e-4, "Expected derivative 3.0 at x=4, got {}", d);
    }

    #[test]
    fn test_sdr_and_algorithm_synthesis_in_vm() {
        let code = r#"
fn run_sdr() -> int:
    let dev = sdr::open("sim")
    defer sdr::close(dev)
    sdr::set_center_freq(dev, 433.92MHz)
    sdr::set_sample_rate(dev, 2.4Msps)
    let iq = sdr::capture_iq(dev, 32)
    let spec = sdr::fft_spectrum(iq)
    return spec.len()

fn main():
    let bins = run_sdr()
    let gen = synth::synthesize_from_examples("triple_p1", [1, 2, 3], [4, 7, 10])
    io::print(bins)
    io::print(gen)
    io::print(memory::leak_report())
"#;
        let (prog, diags) = check_source(code);
        assert!(diags.is_empty(), "Unexpected diags: {:?}", diags);
        let mut machine = vm::Vm::new();
        machine.run(&prog).unwrap();
        assert_eq!(machine.output[0], "32");
        assert!(machine.output[1].contains("return 3 * x + 1"));
        assert!(machine.output[2].contains("clean: true"));
    }

    #[test]
    fn test_hardware_and_security_diagnostics() {
        let code = r#"
fn main():
    power::set_voltage("vcore", 2.1V)
    memory::protect(0x1000, 4096, "RWX")
"#;
        let (_, diags) = check_source(code);
        assert!(diags.iter().any(|d| d.code == "HW302"));
        assert!(diags.iter().any(|d| d.code == "SEC401"));
    }
}
