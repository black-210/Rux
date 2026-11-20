# Learn Nabda (نبضة) in One Day

Nabda gives you **Rust-level memory, hardware, and security safety** with **Python-level readability**—no semicolons, no complex lifetimes, and direct first-class namespaces for hardware, OS kernels, and cybersecurity.

---

## Hour 1: Variables, Types & Functions
- `let` creates an immutable (read-only) binding.
- `var` creates a mutable variable.
- `fn name(arg: type) -> return_type:` defines a function.

```nabda
fn main():
    let name = "Nabda"
    var count = 0
    count += 1
    io::println("Hello " + name)
```

## Hour 2: Physical Units & Power / Voltage / Clock Control
Nabda natively understands **voltage (`V`, `mV`)**, **frequency (`GHz`, `MHz`, `kHz`, `Hz`)**, and **pulse/time (`ns`, `us`, `ms`, `s`)**:

```nabda
power::set_voltage("VCORE", 1.15V)
cpu::set_freq(0, 4.2GHz)
hardware::fan_set_speed(0, 75)
hardware::pwm_pulse(12, 25kHz, 70, 20ms)
```

If you accidentally enter a dangerous voltage like `1.95V`, Nabda catches it **at compile time**, explains the silicon damage risk, and offers an automatic fix!

## Hour 3: Direct Memory, MMIO & OS Kernels
```nabda
unsafe_hardware:
    kernel::init_gdt()
    kernel::init_idt()
    kernel::init_paging()
    let vga = memory::map_mmio(0xB8000, 4096)
    memory::write_u16(vga, 0x0F4E)
```

Compile directly to a freestanding x86_64 kernel object:
```bash
nabda build my_kernel.nb --target kernel
```

## Hour 4: GPU & Display Framebuffer
```nabda
gpu::set_clock(0, 1950MHz)
gpu::dispatch_compute(0, "shader_main", 256)
display::set_mode(0, 1920, 1080, 144)
display::fill_rect(0, 0, 200, 100, 0x00FFAA)
display::present()
```

## Hour 5: Cybersecurity & Hardware Security
Nabda audits your code automatically:
- Rejects `RWX` memory pages (W^X enforcement)
- Detects timing-unsafe `==` on secrets/tokens/hashes and suggests `security::constant_time_eq`
- Provides built-in SHA-256, HMAC-SHA256, CSPRNG, packet inspection, firewall rules, and TPM PCR attestation

## Hour 6: External Open-Source Libraries & VS Code
- Manage open-source projects with `nabda pkg init`, `nabda pkg add <pkg>`, `nabda pkg install`
- Auto-generate FFI bindings from any C/Rust `.h` header: `nabda import-c header.h mylib -o bindings.nb`
- Open `editors/vscode-nabda` in VS Code for smart `::` autocomplete, bilingual error explanations, and 1-click Quick Fix lightbulbs!
