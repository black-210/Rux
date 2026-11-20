#!/usr/bin/env bash
set -euo pipefail

mkdir -p target/selfhost

echo "=== [Stage 0] Compiling compiler/main.rspp via Rust bootstrap ('rspp') -> target/selfhost/rspp-stage1 ==="
./target/release/rspp build compiler/main.rspp -o target/selfhost/rspp-stage1

echo "=== [Stage 1] Self-Hosting! Compiling compiler/main.rspp using rspp-stage1 (NO Rust) -> target/selfhost/rspp-stage2 ==="
./target/selfhost/rspp-stage1 build compiler/main.rspp -o target/selfhost/rspp-stage2

echo "=== [Stage 2] Fixed-Point Check! Compiling compiler/main.rspp using rspp-stage2 -> target/selfhost/rspp-stage3 ==="
./target/selfhost/rspp-stage2 build compiler/main.rspp -o target/selfhost/rspp-stage3

echo "=== [Stage 3] Comparing generated C11 between Stage 1 and Stage 2 compilers ==="
diff -u target/selfhost/rspp-stage2.rspp.c target/selfhost/rspp-stage3.rspp.c
echo "FIXED POINT VERIFIED: Stage-1 and Stage-2 self-hosted compilers emit bit-identical C11!"

echo "=== [Stage 4] Verifying examples compiled natively by rspp-stage2 (independent of Rust) ==="
./target/selfhost/rspp-stage2 version
./target/selfhost/rspp-stage2 check examples/02_sdr_radio.rspp
./target/selfhost/rspp-stage2 build examples/01_basics_and_equations.rspp -o target/selfhost/ex01
./target/selfhost/ex01
./target/selfhost/rspp-stage2 build examples/02_sdr_radio.rspp -o target/selfhost/ex02
./target/selfhost/ex02
./target/selfhost/rspp-stage2 build examples/03_algorithm_synthesis.rspp -o target/selfhost/ex03
./target/selfhost/ex03
./target/selfhost/rspp-stage2 build examples/05_baremetal_kernel.rspp --target kernel -o target/selfhost/kernel.o
ls -lh target/selfhost/rspp-stage2 target/selfhost/kernel.o
echo "ALL SELF-HOSTING STAGES PASSED!"
