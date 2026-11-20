pub mod analyzer;
pub mod ast;
pub mod codegen_c;
pub mod diagnostic;
pub mod fixer;
pub mod lexer;
pub mod parser;
pub mod pkg;
pub mod span;
pub mod stdlib_spec;
pub mod token;
pub mod tutorial;
pub mod vm;

use crate::diagnostic::{Diagnostic, Severity};

pub fn check_source(source: &str) -> (ast::Program, Vec<Diagnostic>, Vec<analyzer::SymbolInfo>) {
    let lex_out = lexer::lex(source);
    let parse_out = parser::parse(lex_out.tokens);
    let analysis = analyzer::analyze(&parse_out.program, source);

    let mut all_diags = Vec::new();
    all_diags.extend(lex_out.diagnostics);
    all_diags.extend(parse_out.diagnostics);
    all_diags.extend(analysis.diagnostics);
    (parse_out.program, all_diags, analysis.symbols)
}

pub fn has_blocking_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        matches!(
            d.severity,
            Severity::Error | Severity::SecurityAlert | Severity::HardwareSafety
        )
    })
}

pub fn run_source(source: &str) -> Result<vm::RuntimeResult, Vec<Diagnostic>> {
    let (program, diags, _) = check_source(source);
    if has_blocking_errors(&diags) {
        return Err(diags);
    }
    vm::execute(&program).map_err(|msg| {
        vec![Diagnostic::error("NB900", span::Span::default(), msg)
            .with_ar_message("خطأ أثناء التنفيذ")
            .with_why("A runtime safety guard or assertion was triggered.", "تم تفعيل حماية وقت التشغيل.")
            .with_solution("Review the runtime message and inputs.", "راجع رسالة الخطأ والقيم المدخلة.")]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_power_and_units() {
        let src = r#"
fn main():
    power::set_voltage("VCORE", 1.15V)
    cpu::set_freq(0, 4.2GHz)
    hardware::fan_set_speed(0, 80)
    time::sleep(1ms)
    io::println(power::get_voltage("VCORE"))
    io::println(hardware::fan_get_rpm(0))
"#;
        let res = run_source(src).expect("should execute cleanly");
        assert!(res.output.contains("1.150V"));
        assert!(res.output.contains("2520"));
    }

    #[test]
    fn test_auto_fix_repairs_errors_and_security() {
        let broken = r#"
fn main()
    let counter = 10
    counter = 20
    power::set_voltage("VCORE", 1.90V)
    let buf = memory::alloc(4096)
    memory::protect(buf, 4096, "RWX")
    let token = "abc"
    let ok = token == "abc"
    memory::free(buf)
"#;
        let diags = fixer::collect_diagnostics(broken);
        assert!(diags.iter().any(|d| d.code == "NB107")); // missing ':'
        assert!(diags.iter().any(|d| d.code == "NB201")); // immutable let mutated
        assert!(diags.iter().any(|d| d.code == "NB402")); // dangerous voltage 1.90V
        assert!(diags.iter().any(|d| d.code == "NB501")); // RWX page
        assert!(diags.iter().any(|d| d.code == "NB502")); // timing unsafe ==

        let summary = fixer::auto_fix_source(broken);
        assert!(summary.applied_fixes.len() >= 5);
        assert!(summary.fixed_source.contains("fn main():"));
        assert!(summary.fixed_source.contains("var counter = 10"));
        assert!(summary.fixed_source.contains("\"RX\""));
        assert!(summary.fixed_source.contains("security::constant_time_eq"));
        assert!(
            !has_blocking_errors(&summary.remaining_diagnostics),
            "remaining diagnostics: {:?}",
            summary.remaining_diagnostics
        );
    }

    #[test]
    fn test_cybersecurity_sha256_and_packet_audit() {
        let src = r#"
fn main():
    let h = security::hash_sha256("abc")
    io::println(h)
    let pkt = security::inspect_packet("4500002800010000400600000a0000010a00000204d2005000000001000000005000000000000000")
    io::println(pkt)
"#;
        let res = run_source(src).expect("cybersecurity script should run");
        // Known FIPS SHA-256("abc")
        assert!(res.output.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert!(res.output.contains("NULL_SCAN_DETECTED"));
    }

    #[test]
    fn test_native_c_and_kernel_codegen() {
        let src = r#"
fn add(a: int, b: int) -> int:
    return a + b

fn main():
    let x = add(7, 35)
    io::println(x)
"#;
        let (prog, diags, _) = check_source(src);
        assert!(!has_blocking_errors(&diags));
        let native_c = codegen_c::generate_c(&prog, codegen_c::BuildTarget::Native);
        assert!(native_c.c_source.contains("int main(void)"));
        assert!(native_c.c_source.contains("add(7LL, 35LL)"));

        let kernel_c = codegen_c::generate_c(&prog, codegen_c::BuildTarget::Kernel);
        assert!(kernel_c.c_source.contains(".multiboot"));
        assert!(kernel_c.c_source.contains("int kernel_main(void)"));
    }
}
