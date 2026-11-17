/* memaccess.h — the 0xEB peek / 0xEC poke commands: which addresses they may
 * touch, and the handlers themselves.
 *
 * No firmware dependencies, so test_memaccess.c can include this on the host
 * with a fake memory (define MEMACCESS_TEST_BACKEND and supply mem_read /
 * mem_write first).
 *
 * Why a whitelist: a read of an unmapped address on the peripheral buses can
 * bus-fault, and a fault in this firmware hangs the keyboard until it is
 * unplugged. Flash, SRAM and the register blocks listed in the AT32F405 SDK
 * (patch/at32f405_sdk/cmsis/at32f402_405.h) are safe to read; everything else
 * is refused.
 *
 * Not covered, on purpose: the USB OTG block (0x50000000, reading its FIFO
 * window consumes data) and the DBGMCU block.
 */
#ifndef MEMACCESS_H
#define MEMACCESS_H

#include <stdint.h>

typedef enum {
    MEM_NONE = 0,
    MEM_FLASH,    /* 0x08000000, 256 KB — readable, never writable here */
    MEM_SRAM,     /* 0x20000000,  96 KB */
    MEM_PERIPH,   /* register blocks — read side effects are the caller's problem */
    MEM_SCS,      /* ARM system control space (SysTick, NVIC, SCB) — read only */
} mem_kind_t;

#define MEM_FLASH_BASE  0x08000000u
#define MEM_FLASH_END   0x08040000u
#define MEM_SRAM_BASE   0x20000000u
#define MEM_SRAM_END    0x20018000u
#define MEM_SCS_BASE    0xE000E000u
#define MEM_SCS_END     0xE000F000u

/* [start, end) windows over the 1 KB register slots the SDK lists, with
 * neighbouring slots merged. A slot the SDK does not list is not here, even
 * when it sits between two that are. */
static const uint32_t mem_periph_windows[][2] = {
    /* APB1 */
    {0x40000000u, 0x40000C00u},  /* TMR2 TMR3 TMR4 */
    {0x40001000u, 0x40001800u},  /* TMR6 TMR7 */
    {0x40001C00u, 0x40002400u},  /* TMR13 TMR14 */
    {0x40002800u, 0x40003400u},  /* ERTC WWDT WDT */
    {0x40003800u, 0x40004000u},  /* SPI2 SPI3 */
    {0x40004400u, 0x40006000u},  /* USART2-5 I2C1-3 */
    {0x40006400u, 0x40006800u},  /* CAN1 */
    {0x40007000u, 0x40007400u},  /* PWC */
    {0x40007800u, 0x40008000u},  /* UART7 UART8 */
    /* APB2 */
    {0x40010000u, 0x40010400u},  /* TMR1 */
    {0x40011000u, 0x40011800u},  /* USART1 USART6 */
    {0x40012000u, 0x40012400u},  /* ADC1 ADCCOM */
    {0x40013000u, 0x40013400u},  /* SPI1 */
    {0x40013800u, 0x40014C00u},  /* SCFG EXINT TMR9 TMR10 TMR11 */
    {0x40015000u, 0x40015400u},  /* I2SF5 */
    {0x40017400u, 0x40017800u},  /* ACC */
    /* AHB1 */
    {0x40020000u, 0x40021000u},  /* GPIOA-D */
    {0x40021400u, 0x40021800u},  /* GPIOF */
    {0x40023000u, 0x40023400u},  /* CRC */
    {0x40023800u, 0x40024000u},  /* CRM, FLASH registers */
    {0x40026000u, 0x40026800u},  /* DMA1 DMA2 (and their DMAMUX) */
};

/* Classify [addr, addr+len). The whole range must sit in one region, and the
 * sum must not wrap. */
static inline mem_kind_t mem_classify(uint32_t addr, uint32_t len) {
    if (len == 0)
        return MEM_NONE;
    uint32_t end = addr + len;
    if (end <= addr)  /* wrapped (end == 0 also lands here) */
        return MEM_NONE;

    if (addr >= MEM_FLASH_BASE && end <= MEM_FLASH_END)
        return MEM_FLASH;
    if (addr >= MEM_SRAM_BASE && end <= MEM_SRAM_END)
        return MEM_SRAM;
    if (addr >= MEM_SCS_BASE && end <= MEM_SCS_END)
        return MEM_SCS;
    for (unsigned i = 0; i < sizeof mem_periph_windows / sizeof mem_periph_windows[0]; i++) {
        if (addr >= mem_periph_windows[i][0] && end <= mem_periph_windows[i][1])
            return MEM_PERIPH;
    }
    return MEM_NONE;
}

/* Access width in bytes: 1, 2 or 4. */
static inline int mem_width_valid(uint32_t width) {
    return width == 1 || width == 2 || width == 4;
}

static inline int mem_aligned(uint32_t addr, uint32_t width) {
    return (addr & (width - 1)) == 0;
}

/* ── Command handlers: peek (0xEB) / poke (0xEC) ───────────────────────
 *
 * Read and write memory over the vendor channel, for debugging without an SWD
 * probe. Which addresses are allowed is decided by mem_classify above.
 *
 * Both reply with the same header, written over the request in the command
 * buffer (host sees resp[N] = buf[N+2]):
 *   resp[1] status, resp[2] length (peek) or width (poke), resp[3] nonce echo,
 *   resp[4..7] address echo, resp[8..] data.
 * The nonce is the host's own byte. It lets the host reject a stale response
 * left over from an earlier command, which for a debugging tool is worse than
 * an error.
 *
 * Byte 9 of the buffer (payload byte 6) is never a field. The host transport
 * writes its Bit7 checksum there, over whatever the payload put in it; the
 * first version of this protocol kept a nonce in that byte and got the
 * checksum back as the nonce, and a poke value would have been corrupted the
 * same way.
 *
 * PEEK request: buf[3..6] address LE, buf[7] length (1..48), buf[8] width
 *   (1, 2 or 4; length must be a multiple of it), buf[9] checksum slot,
 *   buf[10] nonce. Reads are made `width` bytes at a time, so a 32-bit
 *   register is read whole.
 * POKE request: buf[3..6] address LE, buf[7] width, buf[8] nonce, buf[9]
 *   checksum slot, buf[10..13] value LE, buf[14..17] the tag "POKE". SRAM and
 *   registers only. The reply carries the SRAM value read back; for registers
 *   it is 0, because reading some registers has side effects.
 */
#define MEM_PEEK_MAX           48
#define MEM_STATUS_OK          0
#define MEM_STATUS_BAD_RANGE   1   /* not an address this command may touch */
#define MEM_STATUS_BAD_ARGS    2   /* length 0 or > 48, bad width, length not a multiple of width */
#define MEM_STATUS_UNALIGNED   3
#define MEM_STATUS_NOT_ARMED   4   /* poke without the "POKE" tag */
#define MEM_STATUS_READ_ONLY   5   /* flash or system control space */

static uint32_t get_u32_le(volatile uint8_t *p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static void put_u32_le(volatile uint8_t *p, uint32_t v) {
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
    p[2] = (uint8_t)(v >> 16);
    p[3] = (uint8_t)(v >> 24);
}

#ifndef MEMACCESS_TEST_BACKEND
static uint32_t mem_read(uint32_t addr, uint32_t width) {
    switch (width) {
    case 1:  return *(volatile uint8_t *)addr;
    case 2:  return *(volatile uint16_t *)addr;
    default: return *(volatile uint32_t *)addr;
    }
}

static void mem_write(uint32_t addr, uint32_t width, uint32_t value) {
    switch (width) {
    case 1:  *(volatile uint8_t *)addr = (uint8_t)value;   break;
    case 2:  *(volatile uint16_t *)addr = (uint16_t)value; break;
    default: *(volatile uint32_t *)addr = value;           break;
    }
}
#endif /* MEMACCESS_TEST_BACKEND: test_memaccess.c supplies its own mem_read/mem_write */

static int mem_handle_peek(volatile uint8_t *buf) {
    uint32_t addr  = get_u32_le(&buf[3]);
    uint32_t len   = buf[7];
    uint32_t width = buf[8];
    uint8_t  nonce = buf[10];
    uint8_t  status = MEM_STATUS_OK;
    uint8_t  data[MEM_PEEK_MAX];

    if (len == 0 || len > MEM_PEEK_MAX || !mem_width_valid(width) || len % width)
        status = MEM_STATUS_BAD_ARGS;
    else if (!mem_aligned(addr, width))
        status = MEM_STATUS_UNALIGNED;
    else if (mem_classify(addr, len) == MEM_NONE)
        status = MEM_STATUS_BAD_RANGE;

    if (status != MEM_STATUS_OK)
        len = 0;

    /* Read everything before touching the buffer: the reply overwrites the
     * request, and the address may be the command buffer itself. */
    for (uint32_t i = 0; i < len; i += width) {
        uint32_t v = mem_read(addr + i, width);
        for (uint32_t b = 0; b < width; b++)
            data[i + b] = (uint8_t)(v >> (8 * b));
    }

    buf[3] = status;
    buf[4] = (uint8_t)len;
    buf[5] = nonce;
    put_u32_le(&buf[6], addr);
    for (uint32_t i = 0; i < len; i++)
        buf[10 + i] = data[i];

    buf[0] = 0;  /* mark consumed */
    return 1;
}

static int mem_handle_poke(volatile uint8_t *buf) {
    uint32_t addr  = get_u32_le(&buf[3]);
    uint32_t width = buf[7];
    uint8_t  nonce = buf[8];
    uint32_t value = get_u32_le(&buf[10]);
    int armed = buf[14] == 'P' && buf[15] == 'O' && buf[16] == 'K' && buf[17] == 'E';
    uint32_t readback = 0;
    uint8_t  status = MEM_STATUS_OK;

    if (!armed) {
        status = MEM_STATUS_NOT_ARMED;
    } else if (!mem_width_valid(width)) {
        status = MEM_STATUS_BAD_ARGS;
    } else if (!mem_aligned(addr, width)) {
        status = MEM_STATUS_UNALIGNED;
    } else {
        switch (mem_classify(addr, width)) {
        case MEM_SRAM:
            mem_write(addr, width, value);
            readback = mem_read(addr, width);
            break;
        case MEM_PERIPH:
            mem_write(addr, width, value);
            break;
        case MEM_FLASH:
        case MEM_SCS:
            status = MEM_STATUS_READ_ONLY;
            break;
        default:
            status = MEM_STATUS_BAD_RANGE;
            break;
        }
    }

    buf[3] = status;
    buf[4] = (status == MEM_STATUS_OK) ? (uint8_t)width : 0;
    buf[5] = nonce;
    put_u32_le(&buf[6], addr);
    put_u32_le(&buf[10], readback);

    buf[0] = 0;  /* mark consumed */
    return 1;
}

#endif /* MEMACCESS_H */
