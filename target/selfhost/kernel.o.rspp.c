#include <stdint.h>
__attribute__((section(".multiboot"), aligned(8)))
const uint32_t multiboot2_header[] = { 0xe85250d6u, 0u, 16u, (uint32_t)(-(0xe85250d6u + 16u)) };
static inline void k_outb(uint16_t port, uint8_t val) { __asm__ volatile("outb %0, %1" : : "a"(val), "Nd"(port)); }
static inline void k_vga_print(const char* s, uint64_t row, uint64_t color) { volatile uint16_t* vga = (volatile uint16_t*)0xB8000ULL; uint64_t off = row * 80; for (uint64_t i = 0; s[i] && (off + i) < 2000; i++) vga[off + i] = ((uint16_t)color << 8) | (uint8_t)s[i]; }
void kernel_main(void) { k_vga_print("rust++ Self-Hosted Kernel Active", 0, 0x0A); for(;;) __asm__ volatile("hlt"); }
