/* Host-side test for memaccess.h: `make test` in this directory. */
#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* Fake memory behind mem_read / mem_write: 96 KB SRAM, 256 KB flash, and one
 * canned value for every register. Every access is counted so a test can tell
 * that a refused command touched nothing. */
#define MEMACCESS_TEST_BACKEND
static uint8_t fake_sram[0x18000];
static uint8_t fake_flash[0x40000];
static uint32_t fake_reg = 0xDEADBEEFu;
static unsigned n_reads, n_writes;
static uint32_t last_write_addr, last_write_val, last_write_width;

static uint8_t *fake_bytes(uint32_t addr) {
    if (addr >= 0x20000000u && addr < 0x20018000u) return &fake_sram[addr - 0x20000000u];
    if (addr >= 0x08000000u && addr < 0x08040000u) return &fake_flash[addr - 0x08000000u];
    return NULL;
}

static uint32_t mem_read(uint32_t addr, uint32_t width) {
    n_reads++;
    uint8_t *p = fake_bytes(addr);
    uint32_t v = 0;
    if (p) {
        for (uint32_t i = 0; i < width; i++) v |= (uint32_t)p[i] << (8 * i);
        return v;
    }
    return width == 4 ? fake_reg : fake_reg & ((1u << (8 * width)) - 1);
}

static void mem_write(uint32_t addr, uint32_t width, uint32_t value) {
    n_writes++;
    last_write_addr = addr; last_write_val = value; last_write_width = width;
    uint8_t *p = fake_bytes(addr);
    if (p) for (uint32_t i = 0; i < width; i++) p[i] = (uint8_t)(value >> (8 * i));
}

#include "memaccess.h"

static int failures;

#define CHECK(cond) do { \
    if (!(cond)) { printf("FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond); failures++; } \
} while (0)

#define KIND(addr, len, want) do { \
    mem_kind_t got_ = mem_classify((addr), (len)); \
    if (got_ != (want)) { \
        printf("FAIL %s:%d  classify(%#x, %u) = %d, want %d\n", \
               __FILE__, __LINE__, (unsigned)(addr), (unsigned)(len), got_, (want)); \
        failures++; \
    } \
} while (0)


/* ── Handler tests ──────────────────────────────────────────────────────
 * The command buffer lives inside fake SRAM at the firmware's real address,
 * so a peek of the buffer itself can be tested. resp(n) is what the host reads:
 * resp[N] = buf[N+2]. */
#define CMD_BUF_ADDR 0x200082DCu
#define BUF  (&fake_sram[CMD_BUF_ADDR - 0x20000000u])
#define resp(n) (BUF[(n) + 2])

static void put_le32(uint8_t *p, uint32_t v) {
    p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8); p[2] = (uint8_t)(v >> 16); p[3] = (uint8_t)(v >> 24);
}

static void reset_counters(void) { n_reads = n_writes = 0; }

/* Stand-in for the host transport's Bit7 checksum, which lands in the byte
 * after the request's first six payload bytes: buf[9]. Any value must be
 * harmless, so tests write a different one each time. */
static uint8_t checksum_slot = 0x83;

static void send_peek(uint32_t addr, uint8_t len, uint8_t width, uint8_t nonce) {
    memset(BUF, 0, 64);
    BUF[0] = 1;            /* "command pending", as the firmware's SET_REPORT handler leaves it */
    BUF[2] = 0xEB;
    put_le32(&BUF[3], addr);
    BUF[7] = len; BUF[8] = width;
    BUF[9] = checksum_slot++;
    BUF[10] = nonce;
}

static void send_poke(uint32_t addr, uint8_t width, uint32_t value, uint8_t nonce, const char *tag) {
    memset(BUF, 0, 64);
    BUF[0] = 1;
    BUF[2] = 0xEC;
    put_le32(&BUF[3], addr);
    BUF[7] = width;
    BUF[8] = nonce;
    BUF[9] = checksum_slot++;
    put_le32(&BUF[10], value);
    memcpy(&BUF[14], tag, 4);
}

static void expect_header(const char *what, uint8_t status, uint8_t len_or_width, uint8_t nonce, uint32_t addr, int line) {
    if (resp(1) != status || resp(2) != len_or_width || resp(3) != nonce ||
        resp(4) != (uint8_t)addr || resp(5) != (uint8_t)(addr >> 8) ||
        resp(6) != (uint8_t)(addr >> 16) || resp(7) != (uint8_t)(addr >> 24)) {
        printf("FAIL line %d (%s): header status=%u len/width=%u nonce=%#x addr=%02x%02x%02x%02x, want %u %u %#x %#x\n",
               line, what, resp(1), resp(2), resp(3), resp(7), resp(6), resp(5), resp(4),
               status, len_or_width, nonce, (unsigned)addr);
        failures++;
    }
}
#define HEADER(what, st, lw, nonce, addr) expect_header(what, st, lw, nonce, addr, __LINE__)

static void test_handlers(void) {
    /* peek: SRAM bytes, with the layout the driver will parse */
    for (int i = 0; i < 32; i++) fake_sram[0x100 + i] = (uint8_t)(0xA0 + i);
    send_peek(0x20000100u, 16, 1, 0x5A);
    CHECK(mem_handle_peek(BUF) == 1);
    CHECK(BUF[0] == 0);  /* consumed */
    HEADER("peek sram", MEM_STATUS_OK, 16, 0x5A, 0x20000100u);
    CHECK(memcmp((const void *)&resp(8), &fake_sram[0x100], 16) == 0);

    /* peek: a 32-bit access comes back little-endian */
    fake_sram[0x200] = 0x78; fake_sram[0x201] = 0x56; fake_sram[0x202] = 0x34; fake_sram[0x203] = 0x12;
    send_peek(0x20000200u, 4, 4, 1);
    mem_handle_peek(BUF);
    HEADER("peek word", MEM_STATUS_OK, 4, 1, 0x20000200u);
    CHECK(resp(8) == 0x78 && resp(9) == 0x56 && resp(10) == 0x34 && resp(11) == 0x12);

    /* peek: a register, read whole, and as a halfword */
    send_peek(0x40020410u, 4, 4, 2);
    mem_handle_peek(BUF);
    HEADER("peek reg", MEM_STATUS_OK, 4, 2, 0x40020410u);
    CHECK(resp(8) == 0xEF && resp(9) == 0xBE && resp(10) == 0xAD && resp(11) == 0xDE);
    send_peek(0x40020410u, 2, 2, 3);
    mem_handle_peek(BUF);
    CHECK(resp(2) == 2 && resp(8) == 0xEF && resp(9) == 0xBE);

    /* peek: the largest request, and flash */
    fake_flash[0x5200] = 0x42;
    send_peek(0x08005200u, MEM_PEEK_MAX, 1, 4);
    mem_handle_peek(BUF);
    HEADER("peek flash", MEM_STATUS_OK, MEM_PEEK_MAX, 4, 0x08005200u);
    CHECK(resp(8) == 0x42);

    /* peek of the command buffer itself must show the request as it was sent,
     * not the reply being written over it */
    send_peek(CMD_BUF_ADDR, 16, 1, 9);
    uint8_t before[16];
    memcpy(before, BUF, 16);
    mem_handle_peek(BUF);
    HEADER("peek self", MEM_STATUS_OK, 16, 9, CMD_BUF_ADDR);
    CHECK(memcmp((const void *)&resp(8), before, 16) == 0);

    /* peek: every refusal, each with length 0 and no memory access */
    struct { uint32_t addr; uint8_t len, width, status; const char *why; } bad[] = {
        {0x20000100u, 0,  1, MEM_STATUS_BAD_ARGS,  "length 0"},
        {0x20000100u, 49, 1, MEM_STATUS_BAD_ARGS,  "length 49"},
        {0x20000100u, 4,  3, MEM_STATUS_BAD_ARGS,  "width 3"},
        {0x20000100u, 4,  0, MEM_STATUS_BAD_ARGS,  "width 0"},
        {0x20000100u, 6,  4, MEM_STATUS_BAD_ARGS,  "length not a multiple of width"},
        {0x20000101u, 4,  4, MEM_STATUS_UNALIGNED, "unaligned word"},
        {0x20000101u, 2,  2, MEM_STATUS_UNALIGNED, "unaligned halfword"},
        {0x50000000u, 4,  4, MEM_STATUS_BAD_RANGE, "USB OTG"},
        {0x40000FFCu, 4,  4, MEM_STATUS_BAD_RANGE, "register gap"},
        {0x20017FFCu, 8,  4, MEM_STATUS_BAD_RANGE, "runs off SRAM"},
        {0xFFFFFFFCu, 4,  4, MEM_STATUS_BAD_RANGE, "wraps to zero"},
        {0x00000000u, 4,  4, MEM_STATUS_BAD_RANGE, "null"},
    };
    for (unsigned i = 0; i < sizeof bad / sizeof bad[0]; i++) {
        reset_counters();
        send_peek(bad[i].addr, bad[i].len, bad[i].width, (uint8_t)(0x40 + i));
        CHECK(mem_handle_peek(BUF) == 1);
        HEADER(bad[i].why, bad[i].status, 0, (uint8_t)(0x40 + i), bad[i].addr);
        if (n_reads != 0) { printf("FAIL peek '%s' still read memory\n", bad[i].why); failures++; }
    }

    /* poke: SRAM, all three widths, reads back what it wrote */
    reset_counters();
    send_poke(0x20000300u, 4, 0xCAFEBABEu, 7, "POKE");
    CHECK(mem_handle_poke(BUF) == 1);
    CHECK(BUF[0] == 0);
    HEADER("poke word", MEM_STATUS_OK, 4, 7, 0x20000300u);
    CHECK(resp(8) == 0xBE && resp(9) == 0xBA && resp(10) == 0xFE && resp(11) == 0xCA);
    CHECK(fake_sram[0x300] == 0xBE && fake_sram[0x303] == 0xCA);
    send_poke(0x20000310u, 2, 0x1234u, 8, "POKE");
    mem_handle_poke(BUF);
    HEADER("poke half", MEM_STATUS_OK, 2, 8, 0x20000310u);
    CHECK(resp(8) == 0x34 && resp(9) == 0x12);
    send_poke(0x20000320u, 1, 0x1FFu, 9, "POKE");   /* value is truncated to the width */
    mem_handle_poke(BUF);
    HEADER("poke byte", MEM_STATUS_OK, 1, 9, 0x20000320u);
    CHECK(resp(8) == 0xFF && resp(9) == 0 && fake_sram[0x321] == 0);

    /* poke: a register is written but not read back */
    reset_counters();
    send_poke(0x40020418u, 4, 0x00002000u, 10, "POKE");   /* GPIOB SCR */
    mem_handle_poke(BUF);
    HEADER("poke reg", MEM_STATUS_OK, 4, 10, 0x40020418u);
    CHECK(n_writes == 1 && n_reads == 0);
    CHECK(last_write_addr == 0x40020418u && last_write_val == 0x2000u && last_write_width == 4);
    CHECK(resp(8) == 0 && resp(9) == 0 && resp(10) == 0 && resp(11) == 0);


    /* Shared vectors: the exact request payloads the Rust driver builds (see
     * peek_request_matches_the_firmware_layout and poke_request_matches_the_
     * firmware_layout in monsgeek-keyboard/src/memory.rs), with the transport's
     * Bit7 checksum in payload byte 6 as it arrives on the wire. If either side's
     * layout changes, both tests fail. The checksums are real: 255 - sum of the
     * first seven frame bytes, command byte included. */
    {
        /* frame [EB 00 01 00 20 10 01 <ck> 5A]; ck = 255 - (0xEB+0x01+0x20+0x10+0x01 & 0xFF) */
        static const uint8_t peek_vec[8] = {0x00, 0x01, 0x00, 0x20, 16, 1, 0x00, 0x5A};
        memset(BUF, 0, 64);
        BUF[0] = 1; BUF[2] = 0xEB;
        memcpy(&BUF[3], peek_vec, sizeof peek_vec);
        BUF[9] = (uint8_t)(255 - ((0xEB + 0x00 + 0x01 + 0x00 + 0x20 + 0x10 + 0x01) & 0xFF));
        mem_handle_peek(BUF);
        HEADER("rust peek vector", MEM_STATUS_OK, 16, 0x5A, 0x20000100u);

        /* a value whose second byte sits where the old layout was clobbered */
        static const uint8_t poke_vec[15] = {0x18, 0x04, 0x02, 0x40, 4, 9, 0x00,
                                             0x00, 0x20, 0x00, 0x00, 'P', 'O', 'K', 'E'};
        reset_counters();
        memset(BUF, 0, 64);
        BUF[0] = 1; BUF[2] = 0xEC;
        memcpy(&BUF[3], poke_vec, sizeof poke_vec);
        BUF[9] = (uint8_t)(255 - ((0xEC + 0x18 + 0x04 + 0x02 + 0x40 + 0x04 + 0x09) & 0xFF));
        mem_handle_poke(BUF);
        HEADER("rust poke vector", MEM_STATUS_OK, 4, 9, 0x40020418u);
        CHECK(n_writes == 1 && last_write_addr == 0x40020418u &&
              last_write_val == 0x2000u && last_write_width == 4);
    }

    /* The checksum slot must never matter: same request, different bytes there. */
    for (int ck = 0; ck < 256; ck += 51) {
        send_peek(0x20000100u, 8, 1, 0x33);
        BUF[9] = (uint8_t)ck;
        mem_handle_peek(BUF);
        HEADER("peek ignores checksum slot", MEM_STATUS_OK, 8, 0x33, 0x20000100u);
        send_poke(0x20000340u, 4, 0xA1B2C3D4u, 0x34, "POKE");
        BUF[9] = (uint8_t)ck;
        mem_handle_poke(BUF);
        HEADER("poke ignores checksum slot", MEM_STATUS_OK, 4, 0x34, 0x20000340u);
        CHECK(resp(8) == 0xD4 && resp(9) == 0xC3 && resp(10) == 0xB2 && resp(11) == 0xA1);
    }

    /* poke: every refusal writes nothing */
    struct { uint32_t addr; uint8_t width; const char *tag; uint8_t status; const char *why; } nope[] = {
        {0x20000300u, 4, "poke", MEM_STATUS_NOT_ARMED, "lower-case tag"},
        {0x20000300u, 4, "\0\0\0\0", MEM_STATUS_NOT_ARMED, "no tag"},
        {0x20000300u, 3, "POKE", MEM_STATUS_BAD_ARGS,  "width 3"},
        {0x20000302u, 4, "POKE", MEM_STATUS_UNALIGNED, "unaligned"},
        {0x08005200u, 4, "POKE", MEM_STATUS_READ_ONLY, "flash"},
        {0xE000ED28u, 4, "POKE", MEM_STATUS_READ_ONLY, "system control space"},
        {0x50000000u, 4, "POKE", MEM_STATUS_BAD_RANGE, "USB OTG"},
        {0x20018000u, 4, "POKE", MEM_STATUS_BAD_RANGE, "past SRAM"},
        {0x40000FFCu, 4, "POKE", MEM_STATUS_BAD_RANGE, "register gap"},
    };
    for (unsigned i = 0; i < sizeof nope / sizeof nope[0]; i++) {
        reset_counters();
        send_poke(nope[i].addr, nope[i].width, 0x11223344u, (uint8_t)(0x60 + i), nope[i].tag);
        if (nope[i].tag[0] == '\0') memset(&BUF[14], 0, 4);
        CHECK(mem_handle_poke(BUF) == 1);
        HEADER(nope[i].why, nope[i].status, 0, (uint8_t)(0x60 + i), nope[i].addr);
        if (n_writes != 0) { printf("FAIL poke '%s' wrote memory\n", nope[i].why); failures++; }
    }
}

int main(void) {
    /* SRAM: first and last byte, and a word that straddles the end */
    KIND(0x20000000u, 1, MEM_SRAM);
    KIND(0x20017FFFu, 1, MEM_SRAM);
    KIND(0x20017FFCu, 4, MEM_SRAM);
    KIND(0x20017FFDu, 4, MEM_NONE);
    KIND(0x20018000u, 1, MEM_NONE);
    KIND(0x2000045Cu, 80, MEM_SRAM);   /* g_kbd_state, whole struct */
    KIND(0x20007470u, 230, MEM_SRAM);  /* g_fw_config, whole struct */

    /* Flash */
    KIND(0x08000000u, 4, MEM_FLASH);
    KIND(0x08005200u, 48, MEM_FLASH);
    KIND(0x0803FFFFu, 1, MEM_FLASH);
    KIND(0x0803FFFFu, 2, MEM_NONE);
    KIND(0x08040000u, 1, MEM_NONE);
    KIND(0x07FFFFFFu, 2, MEM_NONE);

    /* Registers the firmware itself uses */
    KIND(0x40020410u, 4, MEM_PERIPH);  /* GPIOB IDR */
    KIND(0x40020810u, 4, MEM_PERIPH);  /* GPIOC IDR */
    KIND(0x40003C0Cu, 4, MEM_PERIPH);  /* SPI3 DT */
    KIND(0x4000380Cu, 4, MEM_PERIPH);  /* SPI2 DT */
    KIND(0x40026008u, 4, MEM_PERIPH);  /* DMA1 channel 1 */
    KIND(0x40026104u, 4, MEM_PERIPH);  /* DMA1MUX channel 1 */
    KIND(0x40023800u, 4, MEM_PERIPH);  /* CRM */
    KIND(0x40007000u, 4, MEM_PERIPH);  /* PWC */
    KIND(0x40012000u, 4, MEM_PERIPH);  /* ADC1 */
    KIND(0x40000400u, 4, MEM_PERIPH);  /* TMR3 */

    /* Gaps between register slots, and ranges that leave a window */
    KIND(0x40000FFCu, 4, MEM_NONE);
    KIND(0x40006000u, 4, MEM_NONE);
    KIND(0x40000BFCu, 8, MEM_NONE);    /* runs off the end of TMR2-4 */
    KIND(0x40003FFCu, 8, MEM_NONE);    /* SPI3 into the gap */
    KIND(0x40020FFCu, 8, MEM_NONE);    /* GPIOD into the gap */

    /* Outside everything: USB OTG FIFO window, null, vector table alias */
    KIND(0x50000000u, 4, MEM_NONE);
    KIND(0x50001000u, 4, MEM_NONE);
    KIND(0x00000000u, 4, MEM_NONE);
    KIND(0x40030000u, 4, MEM_NONE);
    KIND(0xE0042000u, 4, MEM_NONE);    /* DBGMCU */

    /* ARM system control space */
    KIND(0xE000ED28u, 4, MEM_SCS);     /* SCB CFSR */
    KIND(0xE000E010u, 4, MEM_SCS);     /* SysTick CTRL */
    KIND(0xE000EFFCu, 4, MEM_SCS);
    KIND(0xE000F000u, 4, MEM_NONE);
    KIND(0xE000DFFCu, 8, MEM_NONE);

    /* Degenerate lengths and wrap-around */
    KIND(0x20000000u, 0, MEM_NONE);
    KIND(0xFFFFFFFCu, 8, MEM_NONE);
    KIND(0xFFFFFFFCu, 4, MEM_NONE);    /* end wraps to exactly 0 */
    KIND(0xFFFFFFFFu, 1, MEM_NONE);

    /* Width and alignment */
    CHECK(!mem_width_valid(0));
    CHECK(mem_width_valid(1) && mem_width_valid(2) && mem_width_valid(4));
    CHECK(!mem_width_valid(3) && !mem_width_valid(8));
    CHECK(mem_aligned(0x20000001u, 1));
    CHECK(!mem_aligned(0x20000001u, 2));
    CHECK(mem_aligned(0x20000002u, 2));
    CHECK(!mem_aligned(0x20000002u, 4));
    CHECK(mem_aligned(0x20000004u, 4));

    /* The window table itself: sorted, disjoint, non-empty, 1 KB aligned */
    unsigned n = sizeof mem_periph_windows / sizeof mem_periph_windows[0];
    for (unsigned i = 0; i < n; i++) {
        CHECK(mem_periph_windows[i][0] < mem_periph_windows[i][1]);
        CHECK((mem_periph_windows[i][0] & 0x3FF) == 0);
        CHECK((mem_periph_windows[i][1] & 0x3FF) == 0);
        if (i > 0)
            CHECK(mem_periph_windows[i - 1][1] < mem_periph_windows[i][0]);  /* strict: touching windows should be merged */
    }

    test_handlers();

    if (failures) {
        printf("%d check(s) failed\n", failures);
        return 1;
    }
    printf("memaccess: all checks passed (%u peripheral windows)\n", n);
    return 0;
}
