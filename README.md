# `rust++` (`.rspp`)

**A self-hosting, hardware-native, SDR-ready systems, kernel, math, and cybersecurity programming language.**

## Core Pillars
- **Self-Hosting Compiler (`/compiler/*.rspp`)**: Written in `rust++` itself; compiles itself cleanly without any runtime dependency on Rust (`scripts/bootstrap_selfhost.sh`).
- **Built-in Memory Leak Protection (`Leak Guard`)**: Static Ownership & Escape Graph (`LG001`–`LG004`) + automatic `defer memory::free(...)` insertion + runtime arena telemetry (`memory::leak_report()`).
- **Mathematical Error-Handling & Auto-Repair**: Damerau-Levenshtein distance, character bigram cosine similarity, Bayesian posterior candidate selection, and Interval Arithmetic `[lo, hi]` hardware/SDR safety proofs (bilingual English + Arabic).
- **Native Software-Defined Radio (`sdr::`)**: Built-in `Msps`/`ksps`/`dBm`/`MHz`/`rad`/`deg` literals, I/Q capture, Radix-2 FFT spectrum, FM/AM/ASK-OOK demodulation, BPSK modulation, and Nyquist interval verification.
- **Direct Mathematical Equations (`equation`, `math::`)**: First-class formulas (`equation f(x) = x^2 - 5x + 6`), implicit multiplication (`2x`), damped Newton-Raphson `math::solve`, `math::derivative`, and `math::integrate`.
- **Self-Synthesizing Algorithms (`algorithm`, `synth::`)**: Symbolic regression (`synth::synthesize_from_examples`), FIR DSP filter synthesis (`synth::generate_dsp_filter`), and ARX cryptographic permutation generation (`synth::evolve_crypto_mixer`).
- **Hardware, Kernel & Cybersecurity**: Direct `memory::`, `hardware::`, `power::`, `cpu::`, `gpu::`, `display::`, `time::`, `kernel::`, `security::`, `ffi::`, `--target kernel` bare-metal x86_64 Multiboot2 codegen, and `rspp import-c <header.h>`.
- **VS Code Support (`/editors/vscode-rustpp`)**: Full TextMate grammar, snippets, and zero-dependency JSON-RPC LSP (`rspp-lsp`) with smart completions, hover docs, Leak Guard diagnostics, and instant Auto-Fix code actions.

## Quick Start
```bash
# 1. Build Stage-0 bootstrap & LSP
cargo build --release --workspace

# 2. Bootstrap self-hosted Stage-1 -> Stage-2 -> Stage-3 fixed-point verification
bash scripts/bootstrap_selfhost.sh

# 3. Run or compile any .rspp program using the self-hosted binary (zero Rust)
./target/selfhost/rspp-stage2 run examples/01_basics_and_equations.rspp
./target/selfhost/rspp-stage2 run examples/02_sdr_radio.rspp
./target/selfhost/rspp-stage2 run examples/03_algorithm_synthesis.rspp

# 4. Diagnose & auto-repair leaks/typos/safety issues
./target/release/rspp check examples/06_math_error_recovery.rspp
./target/release/rspp fix examples/06_math_error_recovery.rspp
```
