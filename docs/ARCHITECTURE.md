# `rust++` Architecture & Self-Hosting Design

## 1. Self-Hosting Bootstrap Chain
`rust++` eliminates runtime dependence on Rust by implementing its compiler inside `/compiler/*.rspp`:
- **Stage 0 (`bootstrap/crates/`)**: Rust bootstrap binary (`rspp`) compiles `compiler/main.rspp` (plus its relative `.rspp` modules) into native machine code via C11 + `gcc`.
- **Stage 1 (`target/selfhost/rspp-stage1`)**: Pure native executable compiled by Stage 0, containing zero Rust code.
- **Stage 2 (`target/selfhost/rspp-stage2`)**: `rspp-stage1` compiles `compiler/main.rspp` itself.
- **Stage 3 Fixed-Point Verification**: `rspp-stage2` compiles `compiler/main.rspp` to `rspp-stage3` and asserts `diff -u rspp-stage2.rspp.c rspp-stage3.rspp.c == 0`.

### Compiler Modules (`/compiler/`)
- `lexer.rspp`: Indentation-aware tokenizer (`INDENT`/`DEDENT`), physical/SDR units, implicit math coefficients (`2x`).
- `parser.rspp`: Recursive-descent AST builder (`fn`, `equation`, `algorithm`, `defer`, `try/catch/throw`, `unsafe_hardware`, `|>`, `**`, `^`).
- `math_diag.rspp`: Damerau-Levenshtein OSA distance, character bigram TF-IDF cosine similarity, and Bayesian posterior candidate ranking.
- `leak_guard.rspp`: Static Ownership & Escape Graph analysis emitting `LG001`..`LG004` diagnostics + deterministic `defer` synthesis.
- `codegen_c.rspp`: Tagged `RsppVal` C11 codegen + bare-metal Multiboot2 kernel target + recursive `import "*.rspp"` loader.
- `main.rspp`: Standalone CLI driver (`check`, `build`, `run`, `sdr`, `synth`, `probe`, `version`).

## 2. Mathematical Error Recovery & Safety Engine
1. **Damerau-Levenshtein Distance**: Exact transposition-aware edit distance (`cost = 1` for adjacent swaps).
2. **Bigram Cosine Similarity**: Vector-space similarity over padded character bigrams in `[0.0, 1.0]`.
3. **Bayesian Posterior Ranking**: `P(candidate | typo) ∝ exp(3.5 * dl_norm + 2.5 * cosine)`.
4. **Interval Arithmetic (`[lo, hi]`)**: Compile-time proofs for division-by-zero (`MA401`), core voltage bounds (`HW302`: `500mV..1550mV`), fan PWM duty (`HW303`: `0..100%`), and SDR Nyquist aliasing (`SDR502`: `BW <= Fs`).

## 3. Built-in Memory Leak Protection
- **Compile-Time**: `StaticLeakGuard` assigns every allocation an ownership node ID and checks reachability along `free`, `defer free`, and `return` edges (`LG001` unfreed escape, `LG002` double-free, `LG003` use-after-free, `LG004` overwrite leak).
- **Runtime**: `RsppArenaSlot` tracks every dynamic allocation and exposes `memory::leak_report()`.

## 4. Native SDR, Equations, and Algorithm Synthesis
- **SDR (`sdr::`)**: Radix-2 Cooley-Tukey FFT spectrum (`fft_spectrum`), quadrature FM discriminator, AM envelope detector, ASK/OOK bit decoder, BPSK modulator, SNR estimator.
- **Equations (`equation`)**: First-class equations compiled to dual boxed/unboxed C11 functions registered for `math::solve` (Newton-Raphson), `math::derivative`, and `math::integrate` (Simpson's rule).
- **Meta-Synthesis (`algorithm`, `synth::`)**: Linear/quadratic symbolic regression, windowed-sinc FIR DSP filter generation, ARX cryptographic permutation synthesis, FSM and sorting code generators.
