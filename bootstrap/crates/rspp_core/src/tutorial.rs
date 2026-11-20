pub fn learn_in_one_day_guide(lang: &str) -> &'static str {
    if lang == "ar" {
        r#"===========================================================
         تعلّم لغة rust++ في يوم واحد (.rspp)
===========================================================

[1] البداية السريعة (30 دقيقة):
    fn main():
        let name = "rust++"
        var x = 10
        x += 5
        io::print(f"مرحباً من {name}, x={x}")

[2] المعادلات الرياضية المباشرة (ساعة واحدة):
    equation quad(x) = x^2 - 5x + 6
    let r = math::solve("quad", 4.0)
    io::print(f"Root: {r}, d/dx: {math::derivative('quad', 4.0)}")

[3] حماية الذاكرة التلقائية من التسرب (ساعة واحدة):
    let buf = memory::alloc(256)
    defer memory::free(buf)
    io::print(memory::leak_report())

[4] الراديو المعرف بالبرمجيات SDR (ساعة واحدة):
    let dev = sdr::open("sim")
    defer sdr::close(dev)
    sdr::set_center_freq(dev, 433.92MHz)
    sdr::set_sample_rate(dev, 2.4Msps)
    let iq = sdr::capture_iq(dev, 64)
    let spec = sdr::fft_spectrum(iq)

[5] خوارزمية تولد خوارزميات (ساعة واحدة):
    let code = synth::synthesize_from_examples("triple_plus_one", [1, 2, 3, 4], [4, 7, 10, 13])
    io::print(code)
"#
    } else {
        r#"===========================================================
           LEARN rust++ IN ONE DAY (.rspp)
===========================================================

[Hour 1] Core Syntax & Strings:
    fn main():
        let name = "rust++"
        var count = 10
        count += 5
        io::print(f"Hello {name}, count={count}")

[Hour 2] Direct Mathematical Equations & Solvers:
    equation poly(x) = x^2 - 5x + 6
    let root = math::solve("poly", 4.0)
    let slope = math::derivative("poly", 4.0)
    let area = math::integrate("poly", 0.0, 3.0)

[Hour 3] Hardware Control & Leak Guard:
    power::set_voltage("vcore", 1.25V)
    hardware::fan_speed(65)
    let buf = memory::alloc(512)
    defer memory::free(buf)
    unsafe_hardware:
        hardware::out_port(0x3F8, 65)

[Hour 4] Native SDR (Software-Defined Radio):
    let dev = sdr::open("sim")
    defer sdr::close(dev)
    sdr::set_center_freq(dev, 433.92MHz)
    sdr::set_sample_rate(dev, 2.4Msps)
    let iq = sdr::capture_iq(dev, 64)
    let bits = sdr::demod_ask_ook(iq, 0.5)

[Hour 5] Algorithms Generating Other Algorithms:
    let generated = synth::synthesize_from_examples("predict", [1, 2, 3, 4], [4, 7, 10, 13])
    let filter_code = synth::generate_dsp_filter("lowpass", 120.0kHz, 2.4Msps, 15)
"#
    }
}
