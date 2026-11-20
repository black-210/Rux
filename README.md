# Nabda (نبضة) Programming Language

**Nabda** is a standalone systems, hardware, OS kernel, and cybersecurity programming language **written in Rust**. It combines Rust's memory/hardware safety and bare-metal power with an ultra-clean, Python-inspired syntax that can be learned in a single day.

## Key Capabilities

- **Standalone Rust-Built Compiler & VM (`nabda` / `nabdac`)**: Lexer, parser, type & safety analyzer, iterative auto-fixer, virtual/host runtime, native C11 executable compiler, and freestanding x86_64 Multiboot2 kernel compiler.
- **Direct Hardware, Power & Pulse Control**:
  - Built-in namespaces: `memory::`, `hardware::`, `power::`, `cpu::`, `gpu::`, `display::`, `time::`, `kernel::`, `security::`, `io::`, `fs::`, `net::`, `ffi::`, `math::`
  - First-class physical unit literals: `1.15V`, `950mV`, `4.2GHz`, `25kHz`, `10ms`, `50us`, `250ns`
  - Compile-time voltage & fan safety guardrails + `unsafe_hardware:` explicit privilege blocks
- **OS Kernel & Bare-Metal Ready**: `nabda build file.nb --target kernel` emits freestanding `-ffreestanding -nostdlib` x86_64 code with Multiboot2 header, inline `inb`/`outb`, `rdmsr`/`wrmsr`, `cli`/`sti`/`hlt`, and `0xB8000` VGA MMIO.
- **Cybersecurity & Hardware Security Built In**: W^X memory protection checks, constant-time secret comparison enforcement, SHA-256, HMAC-SHA256, CSPRNG, deep packet inspection, firewall rules, and TPM PCR attestation.
- **Instant Error Explanation & Auto-Fix (`nabda fix`)**: Every error explains **What happened**, **Why it happened**, **How to fix it** (in English and Arabic), and can be automatically repaired via `nabda fix` or VS Code Quick Fix (`Ctrl+.`).
- **Full VS Code Support (`nabda-lsp` + `editors/vscode-nabda`)**: Smart `::` autocomplete, hover docs, parameter signature help, go-to-definition, document outline, live diagnostics, and 1-click Quick Fixes.
- **External Libraries & Open-Source Ecosystem**: `nabda.toml` package manager (`nabda pkg`), `@link("lib")` + `extern "C"` FFI, and `nabda import-c <header.h>` automatic C binding generator.

## Quickstart

```bash
# Build compiler, CLI, and LSP server
cargo build --release

# Run tests
cargo test --workspace

# Run an example
./target/release/nabda run examples/02_hardware_power_fan.nb

# Inspect errors & auto-fix them
./target/release/nabda check examples/07_autofix_demo.nb
./target/release/nabda fix examples/07_autofix_demo.nb --dry-run

# Compile to a standalone native binary
./target/release/nabda build examples/01_hello_one_day.nb -o hello_nabda

# Compile to a bare-metal OS kernel object
./target/release/nabda build examples/03_os_kernel_boot.nb --target kernel

# Learn in 1 day
./target/release/nabda learn
```
