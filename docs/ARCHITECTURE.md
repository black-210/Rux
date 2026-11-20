# Nabda Architecture

## 1. Crate Structure
- `nabda_core`: Pure-Rust (`std`-only) compiler library with zero external dependencies:
  - `lexer.rs` / `token.rs`: Indentation-sensitive + optional-brace lexer with first-class physical units (`V`, `mV`, `GHz`, `MHz`, `kHz`, `Hz`, `ns`, `us`, `ms`, `s`), hex/binary literals, and `::` namespaces.
  - `parser.rs` / `ast.rs`: Recursive-descent + Pratt expression parser with error recovery and instant keyword/punctuation auto-fixes.
  - `analyzer.rs`: Type inference, `let` vs `var` immutability checks, leak warnings, `unsafe_hardware:` enforcement, voltage ceiling guardrails, and cybersecurity static checks (`RWX` W^X ban, timing-safe comparison enforcement).
  - `diagnostic.rs` & `fixer.rs`: Bilingual (English + Arabic) error reporting ("What happened + Why it happened + How to fix it") and multi-pass `TextEdit` auto-fixer.
  - `vm.rs`: Safe sandboxed + host-probing hardware/OS/security runtime with pure-Rust SHA-256, HMAC-SHA256, packet inspector, MMIO/RAM region tracker, and TPM PCR engine.
  - `codegen_c.rs`: Emits standalone C11 executables (via `gcc`) or freestanding `-ffreestanding -nostdlib` x86_64 Multiboot2 OS kernels with inline ASM `inb`/`outb`/`rdmsr`/`wrmsr`/`cli`/`sti`/`hlt`.
  - `pkg.rs`: `nabda.toml` + `nabda.lock` package manager & `import-c` header-to-FFI generator.
- `nabda_cli`: Provides both `nabda` and `nabdac` binaries.
- `nabda_lsp`: Pure-Rust JSON-RPC 2.0 Language Server over `stdin`/`stdout`.
- `editors/vscode-nabda`: Zero-dependency VS Code extension integrating `nabda-lsp`.
