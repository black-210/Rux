pub struct Lesson {
    pub id: usize,
    pub title: &'static str,
    pub title_ar: &'static str,
    pub summary: &'static str,
    pub code: &'static str,
}

pub const LESSONS: &[Lesson] = &[
    Lesson {
        id: 1,
        title: "Lesson 1: Variables, Types & Functions (10 min)",
        title_ar: "الدرس 1: المتغيرات والدوال (10 دقائق)",
        summary: "Use `let` for safe read-only values, `var` for mutable variables, and `fn name():` for functions — no semicolons needed!",
        code: r#"fn greet(user: str):
    let lang = "Nabda"
    var count = 1
    count += 1
    io::println("Welcome " + user + " to " + lang)

fn main():
    greet("Engineer")
"#,
    },
    Lesson {
        id: 2,
        title: "Lesson 2: Hardware, Voltage, Clocks & Fans (20 min)",
        title_ar: "الدرس 2: التحكم بالكهرباء والفولت والتردد والمراوح",
        summary: "Physical units (`1.15V`, `4.2GHz`, `20ms`) are built directly into the language with safety limits.",
        code: r#"fn main():
    power::set_voltage("VCORE", 1.15V)
    cpu::set_freq(0, 4.2GHz)
    hardware::fan_set_speed(0, 75)
    hardware::pwm_pulse(12, 25kHz, 70, 20ms)
    io::println("VCORE=" + power::get_voltage("VCORE") + " Fan=" + hardware::fan_get_rpm(0))
"#,
    },
    Lesson {
        id: 3,
        title: "Lesson 3: Memory, MMIO & OS Kernel Boot (30 min)",
        title_ar: "الدرس 3: الذاكرة المباشرة وبناء نواة نظام تشغيل",
        summary: "Use `memory::` and `kernel::` with `unsafe_hardware:` only for privileged Ring-0 operations.",
        code: r#"fn main():
    unsafe_hardware:
        kernel::init_gdt()
        kernel::init_idt()
        kernel::init_paging()
        let vga = memory::map_mmio(0xB8000, 4096)
        memory::write_u16(vga, 0x0F4E)
    kernel::log("BOOT", "Nabda Kernel Ready")
"#,
    },
    Lesson {
        id: 4,
        title: "Lesson 4: Cybersecurity & Hardware Attestation (30 min)",
        title_ar: "الدرس 4: الأمن السيبراني وحماية الذاكرة والشبكة",
        summary: "Built-in SHA-256, constant-time comparison, W^X memory protection, packet inspection, and TPM PCR.",
        code: r#"fn main():
    security::enable_aslr()
    security::enable_dep_nx()
    let digest = security::hash_sha256("kernel_v1")
    let pcr0 = security::tpm_attest(0, digest)
    io::println("TPM PCR0 = " + pcr0)
"#,
    },
    Lesson {
        id: 5,
        title: "Lesson 5: GPU Compute, Display & Open-Source FFI (20 min)",
        title_ar: "الدرس 5: كرت الشاشة والمكتبات الخارجية مفتوحة المصدر",
        summary: "Control GPU clocks/VRAM, draw pixels to display framebuffers, and call any C/Rust library via `extern \"C\"`.",
        code: r#"@link("m")
extern "C" fn sqrt(x: float) -> float

fn main():
    gpu::set_clock(0, 1900MHz)
    display::set_mode(0, 1920, 1080, 144)
    display::fill_rect(0, 0, 100, 50, 0x00FF88)
    display::present()
"#,
    },
];
