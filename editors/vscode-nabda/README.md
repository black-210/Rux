# Nabda (نبضة) VS Code Extension

Full IDE support for the Nabda systems, hardware, OS kernel, and cybersecurity programming language:

- **Smart Autocomplete**: Type `memory::`, `hardware::`, `power::`, `cpu::`, `gpu::`, `display::`, `kernel::`, `security::`, `time::`, `ffi::` for instant snippet completions with types and bilingual docs.
- **Instant Error Explanations**: Hover or inspect Problems panel to see *What happened*, *Why it happened*, and *How to fix it* (English + Arabic).
- **1-Click Auto-Fix Lightbulbs (`Ctrl+.`)**: Automatically fixes syntax typos, missing `:`, immutable `let` -> `var`, unsafe over-voltage clamping, W^X `RWX` -> `RX` hardening, and timing-safe comparisons.
- **One-Key Build & Run**: `Ctrl+Alt+N` to run, `Ctrl+Alt+F` to auto-fix the entire file.

## Local Setup

1. Build the compiler & LSP server from the repository root:
   ```bash
   cargo build --release
   ```
2. Open `editors/vscode-nabda` in VS Code and press `F5` (or symlink into `~/.vscode/extensions/vscode-nabda`).
