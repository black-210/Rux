# Learn `rust++` (`.rspp`) in One Day

`rust++` combines Python-like indentation simplicity, direct hardware/kernel/cybersecurity control, native Software-Defined Radio (SDR), direct mathematical equations, self-synthesizing algorithms, automatic Memory Leak Guard protection, and a mathematical error-recovery engine — all compiled by a **self-hosting compiler** written in `rust++` itself.

## 1. Hello World, Variables, & F-Strings (Hour 1)
```rustpp
fn main():
    let name = "rust++"     // immutable
    var freq = 433.92MHz    // mutable
    io::print(f"Booting {name} at {freq} Hz")
```

## 2. Direct Mathematical Equations & Solvers (Hour 2)
Write equations directly with `^` / `**` and implicit multiplication (`5x`):
```rustpp
equation quad(x) = x^2 - 5x + 6

fn main():
    let root = math::solve("quad", 4.0)          // Newton-Raphson -> 3.0
    let slope = math::derivative("quad", 4.0)    // Central diff -> 3.0
    let area = math::integrate("quad", 0.0, 3.0) // Simpson rule -> 4.5
    io::print(f"root={root}, slope={slope}, area={area}")
```

## 3. Built-in Memory Leak Protection System (Hour 3)
`StaticLeakGuard` proves every allocation (`memory::alloc`, `sdr::open`, `gpu::alloc_vram`) has a valid free/defer/return path, and the runtime arena tracks live bytes (`memory::leak_report()`):
```rustpp
fn safe_work():
    let buf = memory::alloc(1024)
    defer memory::free(buf)
    io::print(memory::leak_report())
```
Forgetting `memory::free` triggers `LG001` with an automatic `defer memory::free(...)` fix via `rspp fix`.

## 4. Native SDR (Software-Defined Radio) (Hour 4)
Built-in units (`Msps`, `ksps`, `sps`, `dBm`, `GHz`, `MHz`, `kHz`, `Hz`, `rad`, `deg`) and native Cooley-Tukey FFT + FM/AM/ASK demodulators:
```rustpp
fn main():
    let dev = sdr::open("rtlsdr")
    defer sdr::close(dev)
    sdr::set_center_freq(dev, 433.92MHz)
    sdr::set_sample_rate(dev, 2.4Msps)
    let iq = sdr::capture_iq(dev, 64)
    let spectrum = sdr::fft_spectrum(iq)
    let bits = sdr::demod_ask_ook(iq, 0.5)
```

## 5. Algorithms That Generate Other Algorithms (Hour 5)
```rustpp
fn main():
    let code = synth::synthesize_from_examples("predict", [1, 2, 3, 4], [4, 7, 10, 13])
    io::print(code) // emits fn predict(x: int) -> int: return 3 * x + 1
```

## 6. Hardware, Bare-Metal Kernels & Cybersecurity (Hour 6)
- `power::set_voltage("vcore", 1.25V)` (checked via Interval Arithmetic `[500mV..1550mV]`)
- `hardware::fan_speed(75)`, `hardware::gpio_write(17, true)`, `hardware::pwm_pulse(18, 0.5, 25kHz)`
- `memory::protect(ptr, 4096, "RX")` (enforces W^X)
- `security::constant_time_eq(a, b)`, `security::sha256(msg)`
- Bare-metal x86_64 kernel target: `rspp build examples/05_baremetal_kernel.rspp --target kernel`
