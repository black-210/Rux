use nabda_core::codegen_c::{generate_c, BuildTarget};
use nabda_core::{check_source, fixer, has_blocking_errors, pkg, run_source};
use std::path::Path;

#[test]
fn test_all_clean_examples_run_successfully() {
    let examples = [
        include_str!("../../../../examples/01_hello_one_day.nb"),
        include_str!("../../../../examples/02_hardware_power_fan.nb"),
        include_str!("../../../../examples/03_os_kernel_boot.nb"),
        include_str!("../../../../examples/04_cybersecurity_audit.nb"),
        include_str!("../../../../examples/05_gpu_display.nb"),
        include_str!("../../../../examples/06_ffi_opensource.nb"),
    ];

    for (idx, src) in examples.iter().enumerate() {
        let (_, diags, _) = check_source(src);
        assert!(
            !has_blocking_errors(&diags),
            "Example {} had blocking diagnostics: {:?}",
            idx + 1,
            diags
        );
        let res = run_source(src).unwrap_or_else(|e| panic!("Example {} failed: {:?}", idx + 1, e));
        assert!(!res.output.is_empty(), "Example {} produced no output", idx + 1);
    }
}

#[test]
fn test_autofix_demo_repairs_cleanly() {
    let broken = include_str!("../../../../examples/07_autofix_demo.nb");
    let summary = fixer::auto_fix_source(broken);
    assert!(summary.applied_fixes.len() >= 5);
    assert!(!has_blocking_errors(&summary.remaining_diagnostics));
    let res = run_source(&summary.fixed_source).expect("Fixed source should run cleanly");
    assert!(res.hardware.rails_mv.contains_key("VCORE"));
}

#[test]
fn test_import_c_header_generates_ffi_bindings() {
    let header_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples/sample_c_lib.h");
    let bindings = pkg::import_c_header(&header_path, "nabdahw").expect("should parse sample C header");
    assert!(bindings.contains("@link(\"nabdahw\")"));
    assert!(bindings.contains("extern \"C\" fn crypto_verify_block"));
    assert!(bindings.contains("extern \"C\" fn hw_read_sensor_mv"));
    assert!(bindings.contains("extern \"C\" fn vga_clear_screen"));
}

#[test]
fn test_kernel_example_generates_freestanding_multiboot_c() {
    let src = include_str!("../../../../examples/03_os_kernel_boot.nb");
    let (prog, _, _) = check_source(src);
    let cg = generate_c(&prog, BuildTarget::Kernel);
    assert!(cg.c_source.contains("nb_multiboot2_header"));
    assert!(cg.c_source.contains("nb_cli()"));
    assert!(cg.c_source.contains("nb_outb("));
}
