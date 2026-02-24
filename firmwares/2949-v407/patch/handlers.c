/*
 * Firmware patch handlers (C implementation).
 * Part of the MonsGeek M1 V5 TMR patched firmware.
 *
 * Linked against fw_symbols.ld for firmware function/global access.
 * Called from auto-generated stubs in hooks_gen.S.
 *
 * Convention (filter mode):
 *   return 0     = passthrough to original firmware handler
 *   return non-0 = intercepted (original handler skipped)
 */

#include <stdint.h>
#include "fw_v407.h"
#include "hid_desc.h"

/* ── Derived addresses from exported symbols ─────────────────────────── */

/* IF1 Report Descriptor length (from Ghidra RE of hid_class_setup_handler) */
#define IF1_RDESC_LEN  171

/* IF1 HID Descriptor wDescriptorLength field: bytes 7-8 of the 9-byte descriptor */
#define IF1_HDESC_WLEN ((volatile uint8_t *)((uint8_t *)&g_if1_hid_desc + 7))

/* IF1 HID descriptor wDescriptorLength within each config descriptor copy.
 * Config descriptor layout: offset 50-51 = IF1 HID desc bytes 7-8. */
#define CFG_IF1_WLEN_OFF  50
#define CFG_FS_IF1_WLEN  ((volatile uint8_t *)((uint8_t *)&g_cfg_desc_fs + CFG_IF1_WLEN_OFF))
#define CFG_HS_IF1_WLEN  ((volatile uint8_t *)((uint8_t *)&g_cfg_desc_hs + CFG_IF1_WLEN_OFF))
#define CFG_OS_IF1_WLEN  ((volatile uint8_t *)((uint8_t *)&g_cfg_desc_os + CFG_IF1_WLEN_OFF))

/* ── LED buffers (from fw_symbols.ld) ────────────────────────────────── */

#define LED_BUF_SIZE  0x7B0   /* 1968 bytes: 82 LEDs × 24 bytes WS2812 encoding */
#define LED_COUNT     82
#define MATRIX_LEN    96      /* 16 cols × 6 rows; row-major (pos = row*16+col) */


/* ── Battery HID report descriptor (appended to IF1) ─────────────────── */

/* 46 bytes: Battery Strength + Charging status, Feature + Input reports.
 *
 * Feature reports (polled via GET_REPORT):
 *   - Usage Page 0x06 / Usage 0x20 (HID_DC_BATTERYSTRENGTH): triggers
 *     power_supply creation via kernel's report_features().
 *   - Usage Page 0x85 / Usage 0x44 (HID_BAT_CHARGING): charge status.
 *
 * Input reports (pushed on EP 0x82 when charge state changes):
 *   Duplicate usages allow the kernel's hidinput_hid_event() →
 *   hidinput_update_battery() → hidinput_update_battery_charge_status()
 *   chain to fire, which correctly sets POWER_SUPPLY_STATUS_CHARGING
 *   or DISCHARGING.  The Feature-only path (hid_hw_raw_request) bypasses
 *   event processing, so charge status never updates without Input reports.
 *
 * Both share Report ID 7; HID spec allows same ID across report types.
 * Input report data: [0x07, battery_level, charging] — same as Feature. */
static const uint8_t battery_rdesc[] = {
    HID_USAGE_PAGE(HID_USAGE_PAGE_DESKTOP),
    HID_USAGE(HID_USAGE_DESKTOP_KEYBOARD),
    HID_COLLECTION(HID_COLLECTION_APPLICATION),
      HID_REPORT_ID(7)
      /* ── Battery capacity (0-100%) ── */
      HID_USAGE_PAGE(HID_USAGE_PAGE_GENERIC_DEVICE),
      HID_USAGE(HID_USAGE_BATTERY_STRENGTH),
      HID_LOGICAL_MIN(0),
      HID_LOGICAL_MAX_N(100, 2),
      HID_REPORT_SIZE(8),
      HID_REPORT_COUNT(1),
      HID_FEATURE(HID_DATA | HID_VARIABLE | HID_ABSOLUTE),
      HID_USAGE(HID_USAGE_BATTERY_STRENGTH),
      HID_INPUT(HID_DATA | HID_VARIABLE | HID_ABSOLUTE),
      /* ── Charging status (0/1) ── */
      HID_USAGE_PAGE(HID_USAGE_PAGE_BATTERY_SYSTEM),
      HID_USAGE(HID_USAGE_BATTERY_CHARGING),
      HID_LOGICAL_MIN(0),
      HID_LOGICAL_MAX(1),
      HID_REPORT_SIZE(8),
      HID_REPORT_COUNT(1),
      HID_FEATURE(HID_DATA | HID_VARIABLE | HID_ABSOLUTE),
      HID_USAGE(HID_USAGE_BATTERY_CHARGING),
      HID_INPUT(HID_DATA | HID_VARIABLE | HID_ABSOLUTE),
    HID_COLLECTION_END,
};

#define BATTERY_RDESC_LEN  (sizeof(battery_rdesc))     /* 46 */
#define EXTENDED_RDESC_LEN (IF1_RDESC_LEN + BATTERY_RDESC_LEN)  /* 217 */

/* Buffer for extended IF1 descriptor (original 171B + battery 46B).
 * Non-static: address must be visible in ELF for build-time literal pool patch.
 * Placed in .bss → PATCH_SRAM (0x20009800+). */
uint8_t extended_rdesc[EXTENDED_RDESC_LEN];

/* ── Diagnostics (readable via 0xFB patch info) ──────────────────────── */
static struct {
    uint32_t hid_setup_calls;       /* total calls to handle_hid_setup */
    uint32_t hid_setup_intercepts;  /* times we returned 1 (intercepted) */
    uint8_t  last_bmReqType;
    uint8_t  last_bRequest;
    uint16_t last_wValue;
    uint16_t last_wIndex;
    uint16_t last_wLength;
    uint8_t  last_battery_level;
    uint8_t  last_result;           /* 0=passthrough, 1=intercepted */
} diag;

/* ── Debug ring buffer (readable via 0xFD) ───────────────────────────── */

#define LOG_BUF_SIZE 512

static struct {
    uint16_t head;          /* next write position (wraps at LOG_BUF_SIZE) */
    uint16_t count;         /* total bytes written (saturates at LOG_BUF_SIZE) */
    uint8_t  data[LOG_BUF_SIZE];
} log_buf;                  /* 516B in .bss → PATCH_SRAM */

/* Log entry types */
#define LOG_HID_SETUP_ENTRY   0x01  /* 8B payload: setup packet */
#define LOG_HID_SETUP_RESULT  0x02  /* 2B payload: result, battery_level */
#define LOG_VENDOR_CMD_ENTRY  0x03  /* 2B payload: cmd_buf[0], cmd_buf[2] */
#define LOG_USB_CONNECT       0x04  /* 0B payload */
#define LOG_EP0_XFER_START    0x05  /* 6B payload: buf_lo/hi, len, udev_lo/hi, 0 */

/* ── SEGGER RTT (ring buffer in SRAM, read by BMP via SWD) ─────────── */

#define RTT_BUF_SIZE 256

/* RTT Up-Buffer descriptor */
typedef struct {
    const char *name;
    uint8_t    *buf;
    uint32_t    size;
    volatile uint32_t wr_off;   /* firmware advances */
    volatile uint32_t rd_off;   /* BMP advances via SWD */
    uint32_t    flags;          /* 0 = skip if full (non-blocking) */
} rtt_up_buf_t;

/* RTT Control Block — BMP scans SRAM for the magic ID string */
static struct {
    char         id[16];        /* "SEGGER RTT\0\0\0\0\0\0" */
    int32_t      max_up;        /* 1 */
    int32_t      max_down;      /* 0 */
    rtt_up_buf_t up[1];
} rtt_cb;

static uint8_t rtt_buf[RTT_BUF_SIZE];
static const char rtt_channel_name[] = "monsmod";

/* RTT tag definitions for battery monitor */
#define RTT_TAG_ADC_AVG       0x01  /* u16: averaged battery ADC reading */
#define RTT_TAG_BATT_RAW      0x02  /* u8:  battery_raw_level */
#define RTT_TAG_BATT_LEVEL    0x03  /* u8:  battery_level (debounced %) */
#define RTT_TAG_CHARGER       0x04  /* u8:  charger_connected flag */
#define RTT_TAG_DEBOUNCE_CTR  0x05  /* u8:  battery_update_ctr */
#define RTT_TAG_ADC_COUNTER   0x10  /* u32: magnetism engine ADC scan counter */

static void rtt_init(void) {
    /* Zero everything — PATCH_SRAM .bss is NOT zero-initialized */
    uint8_t *p = (uint8_t *)&rtt_cb;
    for (uint32_t i = 0; i < sizeof(rtt_cb); i++)
        p[i] = 0;
    for (uint32_t i = 0; i < RTT_BUF_SIZE; i++)
        rtt_buf[i] = 0;

    /* Set up channel 0 (up only) */
    rtt_cb.up[0].name  = rtt_channel_name;
    rtt_cb.up[0].buf   = rtt_buf;
    rtt_cb.up[0].size  = RTT_BUF_SIZE;
    rtt_cb.up[0].wr_off = 0;
    rtt_cb.up[0].rd_off = 0;
    rtt_cb.up[0].flags  = 0;  /* SEGGER_RTT_MODE_NO_BLOCK_SKIP */
    rtt_cb.max_up   = 1;
    rtt_cb.max_down = 0;

    /* Write magic LAST — prevents BMP finding half-initialized CB.
     * Use volatile to prevent reordering with struct init above. */
    __asm__ volatile ("dsb" ::: "memory");
    const char magic[] = "SEGGER RTT\0\0\0\0\0";
    for (int i = 0; i < 16; i++)
        ((volatile char *)rtt_cb.id)[i] = magic[i];
}

static void rtt_emit(uint8_t tag, uint32_t val) {
    /* Write 5-byte record: [tag:u8] [value:u32 LE] non-blocking. */
    uint32_t wr = rtt_cb.up[0].wr_off;
    uint32_t rd = rtt_cb.up[0].rd_off;

    /* Check available space (circular buffer) */
    uint32_t avail;
    if (wr >= rd)
        avail = RTT_BUF_SIZE - 1 - wr + rd;
    else
        avail = rd - wr - 1;

    if (avail < 5)
        return;  /* drop if buffer full */

    rtt_buf[wr] = tag;
    wr = (wr + 1) % RTT_BUF_SIZE;
    rtt_buf[wr] = (uint8_t)(val & 0xFF);
    wr = (wr + 1) % RTT_BUF_SIZE;
    rtt_buf[wr] = (uint8_t)((val >> 8) & 0xFF);
    wr = (wr + 1) % RTT_BUF_SIZE;
    rtt_buf[wr] = (uint8_t)((val >> 16) & 0xFF);
    wr = (wr + 1) % RTT_BUF_SIZE;
    rtt_buf[wr] = (uint8_t)((val >> 24) & 0xFF);
    wr = (wr + 1) % RTT_BUF_SIZE;

    /* Atomic u32 store — ISR-safe on Cortex-M4 */
    rtt_cb.up[0].wr_off = wr;
}

static void log_entry(uint8_t type, const uint8_t *payload, uint8_t len) {
    /* Write [type] [payload...] into ring buffer */
    uint16_t total = 1 + len;

    /* Write type byte */
    log_buf.data[log_buf.head] = type;
    log_buf.head = (log_buf.head + 1) % LOG_BUF_SIZE;

    /* Write payload */
    for (uint8_t i = 0; i < len; i++) {
        log_buf.data[log_buf.head] = payload[i];
        log_buf.head = (log_buf.head + 1) % LOG_BUF_SIZE;
    }

    /* Saturating count */
    if (log_buf.count <= LOG_BUF_SIZE - total)
        log_buf.count += total;
    else
        log_buf.count = LOG_BUF_SIZE;
}

/* ── Battery monitor "before" hook ─────────────────────────────────── */
/* Called BEFORE battery_level_monitor runs. Emits RTT records with
 * current battery ADC, level, charger state etc. for live observation.
 * battery_level_monitor fires when adc_counter == 2000 (~every few seconds). */

void battery_monitor_before_hook(void) {
    volatile kbd_state_t *kbd = (volatile kbd_state_t *)&g_kbd_state;

    /* Averaged battery ADC: 32-bit value at 0x20000010 */
    volatile uint32_t *adc_avg = (volatile uint32_t *)0x20000010;
    rtt_emit(RTT_TAG_ADC_AVG, *adc_avg & 0xFFFF);

    rtt_emit(RTT_TAG_BATT_RAW, kbd->battery_raw_level);
    rtt_emit(RTT_TAG_BATT_LEVEL, kbd->battery_level);
    rtt_emit(RTT_TAG_CHARGER, kbd->charger_connected);
    rtt_emit(RTT_TAG_DEBOUNCE_CTR, kbd->battery_update_ctr);

    /* ADC scan counter: *(uint32_t *)(0x20004410 + 0xe24) = 0x20005234 */
    volatile uint32_t *adc_ctr = (volatile uint32_t *)0x20005234;
    rtt_emit(RTT_TAG_ADC_COUNTER, *adc_ctr);
}

/* Forward declaration for USB path (GET_REPORT IF2) and handle_patch_info. */
static void fill_patch_info_response(volatile uint8_t *buf);

/* ── HID class setup handler (battery reporting) ─────────────────────── */
/* The stub saves {r0-r3,r12,lr} then does `bl handle_hid_setup`.
 * At the bl, r0 still holds the original first argument (udev) from
 * usb_setup_class_request → hid_class_setup_handler(udev, setup_pkt).
 * NOTE: udev = g_usb_device + 4 (the core_handler passes udev+4 down),
 * i.e. it points to g_usb_device_handle (otg_dev_handle_t). */

int handle_hid_setup(otg_dev_handle_t *udev) {
    uint8_t  bmReqType = udev->setup.bmRequestType;
    uint8_t  bRequest  = udev->setup.bRequest;
    uint16_t wValue    = udev->setup.wValue;
    uint16_t wIndex    = udev->setup.wIndex;
    uint16_t wLength   = udev->setup.wLength;

    diag.hid_setup_calls++;
    diag.last_bmReqType = bmReqType;
    diag.last_bRequest  = bRequest;
    diag.last_wValue    = wValue;
    diag.last_wIndex    = wIndex;
    diag.last_wLength   = wLength;

    /* Log full setup packet */
    log_entry(LOG_HID_SETUP_ENTRY, (const uint8_t *)&udev->setup, 8);

    /* Populate extended_rdesc: original IF1 descriptor + battery descriptor.
     * Runs on every call (idempotent) so the buffer is ready before the
     * original handler reads from it.  The literal pool at 0x0801485c has
     * been patched at build time to point to extended_rdesc, and the length
     * cap at 0x080147fc/08014800 patched from 0xAB to 0xD9, so the original
     * hid_class_setup_handler naturally serves our extended descriptor. */
    memcpy(extended_rdesc, (void *)&g_if1_report_desc, IF1_RDESC_LEN);
    for (int i = 0; i < (int)BATTERY_RDESC_LEN; i++)
        extended_rdesc[IF1_RDESC_LEN + i] = battery_rdesc[i];

    /* Patch wDescriptorLength in all SRAM descriptor copies (idempotent).
     * Must run on EVERY hid_class_setup call — not just IF1 — so that config
     * descriptor copies are patched before the next USB re-enumeration. */
    IF1_HDESC_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    IF1_HDESC_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    CFG_FS_IF1_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    CFG_FS_IF1_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    CFG_HS_IF1_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    CFG_HS_IF1_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    CFG_OS_IF1_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    CFG_OS_IF1_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);

    /* Only intercept GET_REPORT for IF1 battery Feature report.
     * All other requests (GET_DESCRIPTOR, SET_IDLE, etc.) pass through to
     * the original handler, which now reads from our extended_rdesc buffer. */
    if (wIndex == 1 && bmReqType == 0xA1 && bRequest == 0x01) {
        /* GET_REPORT — wValue = (report_type << 8) | report_id
         * Feature report type = 3, Report ID = 7 → wValue = 0x0307 */
        if (wValue == 0x0307) {
            uint8_t bat_level = *(volatile uint8_t *)&g_battery_level;
            uint8_t charging  = *(volatile uint8_t *)&g_charger_connected;

            /* Respond directly via EP0 with capped length.
             * Report format: [ID=7] [battery 0-100] [charging 0/1]
             * Must cap at min(wLength, reportLen) — firmware EP0 state
             * machine hangs if we send more than wLength bytes. */
            static uint8_t bat_report[4] __attribute__((aligned(4)));
            bat_report[0] = 0x07;       /* Report ID 7 */
            bat_report[1] = bat_level;  /* Battery level 0-100 */
            bat_report[2] = charging;   /* 1=charging, 0=discharging */
            uint16_t xfer_len = (wLength < 3) ? wLength : 3;
            usb_ep0_in_xfer_start(udev, bat_report, xfer_len);

            /* Also push an Input report on EP2 so the kernel's event
             * chain fires (hidinput_update_battery_charge_status).
             * The initial Input report from handle_vendor_cmd fires
             * before SET_CONFIGURATION, so EP2 isn't ready yet — this
             * is the reliable path for the first charge status update. */
            volatile uint8_t *ep2_ready = (volatile uint8_t *)0x20000023;
            if (*ep2_ready) {
                static uint8_t bat_input[4] __attribute__((aligned(4)));
                bat_input[0] = 0x07;
                bat_input[1] = bat_level;
                bat_input[2] = charging;
                usb_ep2_in_transmit(bat_input, 3);
            }

            diag.last_battery_level = bat_level;
            diag.last_result = 1;
            diag.hid_setup_intercepts++;

            uint8_t log_payload[2] = { 1, bat_level };
            log_entry(LOG_HID_SETUP_RESULT, log_payload, 2);
            return 1;   /* intercepted — we handled the EP0 response */
        }
    }

    diag.last_result = 0;
    {
        uint8_t log_payload[2] = { 0, 0 };
        log_entry(LOG_HID_SETUP_RESULT, log_payload, 2);
    }
    return 0;   /* passthrough to original handler */
}

/* ── WS2812 encoding for SPI scanout ─────────────────────────────────────
 * Matches firmware ws2812_set_pixel(): each byte expands to 8 SPI bytes;
 * 1 bit → 0xF0 (long high), 0 bit → 0xC0 (short high). MSB first (byte 0 =
 * bit 7). Assumes SPI sends MSB of each byte first. Buffer layout per LED:
 * bytes 0–7 G, 8–15 R, 16–23 B (GRB order for WS2812). */

static void encode_ws2812_byte(volatile uint8_t *p, uint8_t val) {
    p[0] = (val & 0x80) ? 0xF0 : 0xC0;
    p[1] = (val & 0x40) ? 0xF0 : 0xC0;
    p[2] = (val & 0x20) ? 0xF0 : 0xC0;
    p[3] = (val & 0x10) ? 0xF0 : 0xC0;
    p[4] = (val & 0x08) ? 0xF0 : 0xC0;
    p[5] = (val & 0x04) ? 0xF0 : 0xC0;
    p[6] = (val & 0x02) ? 0xF0 : 0xC0;
    p[7] = (val & 0x01) ? 0xF0 : 0xC0;
}

/* ── Patch discovery (0xFB) ──────────────────────────────────────────────
 * Response layout in g_vendor_cmd_buffer (buf = cmd_buf):
 *   buf[3..4] = magic 0xCA 0xFE    → host sees resp[1..2]
 *   buf[5]    = patch version       → resp[3]
 *   buf[6..7] = capabilities LE16   → resp[4..5]
 *   buf[8..15]= name (NUL-padded)   → resp[6..13]
 *   buf[16..] = diagnostics         → resp[14..]
 *
 * (GET_REPORT returns from lp_class_report_buf = cmd_buf+2, so resp[N] = buf[N+2])
 *
 * fill_patch_info_response() is used from both the wired path (handle_vendor_cmd
 * → handle_patch_info) and the USB GET_REPORT interception in handle_hid_setup.
 */
static void fill_patch_info_response(volatile uint8_t *buf) {
    buf[3]  = 0xCA;           /* magic hi */
    buf[4]  = 0xFE;           /* magic lo */
    buf[5]  = 1;              /* patch version */
    buf[6]  = 0x07;           /* capabilities: battery(0) + led_stream(1) + debug_log(2) */
    buf[7]  = 0x00;           /* capabilities hi */
    buf[8]  = 'M';
    buf[9]  = 'O';
    buf[10] = 'N';
    buf[11] = 'S';
    buf[12] = 'M';
    buf[13] = 'O';
    buf[14] = 'D';
    buf[15] = '\0';

    /* Diagnostics: bytes 16-31 */
    buf[16] = (uint8_t)(diag.hid_setup_calls & 0xFF);
    buf[17] = (uint8_t)((diag.hid_setup_calls >> 8) & 0xFF);
    buf[18] = (uint8_t)(diag.hid_setup_intercepts & 0xFF);
    buf[19] = (uint8_t)((diag.hid_setup_intercepts >> 8) & 0xFF);
    buf[20] = diag.last_bmReqType;
    buf[21] = diag.last_bRequest;
    buf[22] = (uint8_t)(diag.last_wValue & 0xFF);
    buf[23] = (uint8_t)(diag.last_wValue >> 8);
    buf[24] = (uint8_t)(diag.last_wIndex & 0xFF);
    buf[25] = (uint8_t)(diag.last_wIndex >> 8);
    buf[26] = (uint8_t)(diag.last_wLength & 0xFF);
    buf[27] = (uint8_t)(diag.last_wLength >> 8);
    buf[28] = diag.last_battery_level;
    buf[29] = diag.last_result;

    /* Raw kbd_state fields for battery debugging (offsets from g_kbd_state) */
    volatile uint8_t *kbd = (volatile uint8_t *)&g_kbd_state;
    buf[30] = kbd[0x40];      /* battery_level */
    buf[31] = kbd[0x41];      /* charger_connected */
    buf[32] = kbd[0x42];      /* charger_debounce_ctr */
    buf[33] = kbd[0x43];      /* battery_update_ctr */
    buf[34] = kbd[0x44];      /* battery_raw_level */
    buf[35] = kbd[0x45];      /* battery_indicator_active */
    /* ADC counter: *(uint32_t *)(0x20004410 + 0xe24) = 0x20005234 */
    volatile uint32_t *adc_ctr = (volatile uint32_t *)0x20005234;
    buf[36] = (uint8_t)(*adc_ctr & 0xFF);
    buf[37] = (uint8_t)((*adc_ctr >> 8) & 0xFF);

    /* Main loop timer counter: g_kbd_state[0..1] — outer loop gates on >= 2 */
    buf[38] = kbd[0x00];      /* timer_counter lo */
    buf[39] = kbd[0x01];      /* timer_counter hi */
    /* Charge status: g_kbd_state + 0x4D (0x200004A9) */
    buf[40] = kbd[0x4D];      /* charge_status (0=none, 1=charging, 2=complete) */
    /* Connection mode: g_kbd_state + 0x04 */
    buf[41] = kbd[0x04];      /* connection_mode */

    /* Averaged ADC value (2 words at 0x20000010, we read first = current average) */
    volatile uint32_t *adc_avg = (volatile uint32_t *)0x20000010;
    uint32_t avg = *adc_avg;
    buf[42] = (uint8_t)(avg & 0xFF);
    buf[43] = (uint8_t)((avg >> 8) & 0xFF);

    /* Raw ADC sample 0 (at 0x20003410 + 0x878 = 0x20003C88) — 16-bit */
    volatile uint16_t *adc_s0 = (volatile uint16_t *)0x20003C88;
    buf[44] = (uint8_t)(*adc_s0 & 0xFF);
    buf[45] = (uint8_t)((*adc_s0 >> 8) & 0xFF);

    /* GPIOC IDR (charger detect pin 13) and GPIOB IDR (charge complete pin 10) */
    volatile uint32_t *gpioc_idr = (volatile uint32_t *)0x40020810;
    volatile uint32_t *gpiob_idr = (volatile uint32_t *)0x40020410;
    uint32_t gc = *gpioc_idr;
    uint32_t gb = *gpiob_idr;
    buf[46] = (uint8_t)(gc & 0xFF);
    buf[47] = (uint8_t)((gc >> 8) & 0xFF);
    buf[48] = (uint8_t)(gb & 0xFF);
    buf[49] = (uint8_t)((gb >> 8) & 0xFF);
}

static int handle_patch_info(volatile uint8_t *buf) {
    fill_patch_info_response(buf);
    buf[0] = 0;   /* mark consumed */
    return 1;
}

/* ── LED streaming (0xFC) ──────────────────────────────────────────────
 *
 * Page 0-6:  Write 18 keys × RGB directly to g_led_frame_buf (WS2812 encoded)
 * Page 0xFF: Commit — copy g_led_frame_buf → g_led_dma_buf for immediate display
 * Page 0xFE: Release — restore built-in LED effect mode
 *
 * On first page write, we set led_effect_mode to 0xFF (invalid) so
 * rgb_led_animate()'s switch falls through without touching the frame buffer.
 * On release, the saved mode is restored.
 *
 * Data layout: buf[3] = page, buf[4..57] = 18×RGB (54 bytes).
 * Host sends row-major indices (page*18 + i), where pos = row*16 + col.
 * Images scale to 16×6 and map pixel (x,y) → pos = y*16+x directly.
 *
 * Uses static_led_pos_tbl from firmware ROM (0x08025031, via fw_symbols.ld).
 * Row-major: static_led_pos_tbl[row*16+col] → WS2812 strip index (0–81).
 * 0xFF = no LED (gap for wide keys / empty slots).  Gaps are part of
 * the rectangular coordinate space — the host is aware and simply gets
 * no visible output at those positions. */
static uint8_t stream_active;
static uint8_t saved_led_effect_mode;

static int handle_led_stream(volatile uint8_t *buf) {
    uint8_t page = buf[3];

    if (page == 0xFF) {
        /* Commit: copy frame buffer to DMA buffer for immediate display */
        memcpy((void *)&g_led_dma_buf, (void *)&g_led_frame_buf, LED_BUF_SIZE);
        buf[0] = 0;
        return 1;
    }

    if (page == 0xFE) {
        /* Release: restore built-in LED effect mode */
        if (stream_active) {
            stream_active = 0;
            ((volatile connection_config_t *)&g_fw_config)->led_effect_mode = saved_led_effect_mode;
        }
        buf[0] = 0;
        return 1;
    }

    if (page < 7) {
        /* First page write: suppress built-in animation */
        if (!stream_active) {
            stream_active = 1;
            saved_led_effect_mode = ((volatile connection_config_t *)&g_fw_config)->led_effect_mode;
            /* 0xFF = invalid mode → rgb_led_animate switch default does nothing */
            ((volatile connection_config_t *)&g_fw_config)->led_effect_mode = 0xFF;
        }

        volatile uint8_t *rgb = &buf[4];
        uint8_t start = page * 18;
        volatile uint8_t *frame = (volatile uint8_t *)&g_led_frame_buf;

        /* Row-major position → physical strip index (0xFF = gap, skip). */
        for (int i = 0; i < 18 && (start + i) < MATRIX_LEN; i++) {
            uint32_t pos = start + i;
            uint8_t strip_idx = static_led_pos_tbl[pos];
            if (strip_idx >= LED_COUNT)
                continue;
            uint8_t r = rgb[i * 3];
            uint8_t g = rgb[i * 3 + 1];
            uint8_t b = rgb[i * 3 + 2];
            volatile uint8_t *p = &frame[strip_idx * 24];
            encode_ws2812_byte(p,      g);   /* GRB order for WS2812 */
            encode_ws2812_byte(p + 8,  r);
            encode_ws2812_byte(p + 16, b);
        }

        buf[0] = 0;
        return 1;
    }

    return 0;  /* unknown page, passthrough */
}

/* ── USB connect init (patches config descriptors before enumeration) ──── */

int handle_usb_connect(void) {
    log_entry(LOG_USB_CONNECT, (const uint8_t *)0, 0);

    /* Initialize RTT control block (re-initializes on each USB plug). */
    rtt_init();

    /* Patch wDescriptorLength to EXTENDED_RDESC_LEN in all SRAM descriptor
     * copies.  Must happen BEFORE enumeration so the config descriptor
     * advertises the extended report descriptor size (171 + 46 battery). */
    IF1_HDESC_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    IF1_HDESC_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    CFG_FS_IF1_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    CFG_FS_IF1_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    CFG_HS_IF1_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    CFG_HS_IF1_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    CFG_OS_IF1_WLEN[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    CFG_OS_IF1_WLEN[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);

    /* Pre-populate extended_rdesc buffer so it's ready if GET_DESCRIPTOR
     * arrives before any hid_setup call. */
    memcpy(extended_rdesc, (void *)&g_if1_report_desc, IF1_RDESC_LEN);
    for (int i = 0; i < (int)BATTERY_RDESC_LEN; i++)
        extended_rdesc[IF1_RDESC_LEN + i] = battery_rdesc[i];

    return 0;   /* passthrough */
}

/* ── Debug log read (0xFD) ─────────────────────────────────────────────
 *
 * Reads pages from the ring buffer.
 *   buf[3] = page number (0-9)
 * Response (host sees resp[N] = buf[N+2]):
 *   buf[3..4] = count (uint16_t LE)   → resp[1..2]
 *   buf[5..6] = head  (uint16_t LE)   → resp[3..4]
 *   buf[7]    = LOG_BUF_SIZE >> 8      → resp[5]
 *   buf[8..63] = 56 bytes of ring data → resp[6..61]
 */
static int handle_log_read(volatile uint8_t *buf) {
    uint8_t page = buf[3];

    /* Header */
    buf[3] = (uint8_t)(log_buf.count & 0xFF);
    buf[4] = (uint8_t)(log_buf.count >> 8);
    buf[5] = (uint8_t)(log_buf.head & 0xFF);
    buf[6] = (uint8_t)(log_buf.head >> 8);
    buf[7] = (uint8_t)(LOG_BUF_SIZE >> 8);  /* 2 → buffer is 512 */

    /* Copy 56 bytes from ring at offset page*56 */
    uint16_t offset = page * 56;
    for (int i = 0; i < 56; i++) {
        uint16_t idx = (offset + i) % LOG_BUF_SIZE;
        buf[8 + i] = (offset + i < LOG_BUF_SIZE) ? log_buf.data[idx] : 0;
    }

    buf[0] = 0;  /* mark consumed */
    return 1;
}

/* ── Vendor command dispatcher ─────────────────────────────────────────── */

int handle_vendor_cmd(void) {
    volatile uint8_t *cmd_buf = (volatile uint8_t *)&g_vendor_cmd_buffer;

    /* ── Battery Input report on charge state change ─────────────── */
    {
        static uint8_t prev_charging;  /* .bss → starts at 0 */

        uint8_t cur_charging = *(volatile uint8_t *)&g_charger_connected;
        if (cur_charging != prev_charging) {
            prev_charging = cur_charging;

            /* Check EP2 ready (not busy) before sending */
            volatile uint8_t *ep2_ready = (volatile uint8_t *)0x20000023;
            if (*ep2_ready) {
                static uint8_t bat_input[4] __attribute__((aligned(4)));
                uint8_t level = *(volatile uint8_t *)&g_battery_level;
                bat_input[0] = 0x07;          /* Report ID 7 */
                bat_input[1] = level;         /* Battery 0-100 */
                bat_input[2] = cur_charging;  /* 1=charging, 0=not */
                usb_ep2_in_transmit(bat_input, 3);
            }
        }
    }

    /* No pending command — cmd_buf[0] is set non-zero by firmware SET_REPORT handler */
    if (cmd_buf[0] == 0)
        return 0;

    /* Log vendor command entry (skip 0xFD to avoid contaminating the log
     * when reading it — each log read would otherwise add 3 bytes) */
    if (cmd_buf[2] != 0xFD) {
        uint8_t log_payload[2] = { cmd_buf[0], cmd_buf[2] };
        log_entry(LOG_VENDOR_CMD_ENTRY, log_payload, 2);
    }

    /* Command byte is at cmd_buf[2] = lp_class_report_buf[0]
     * (SET_REPORT data lands at cmd_buf+2, first byte = command) */
    switch (cmd_buf[2]) {
    case 0xFB:
        return handle_patch_info(cmd_buf);
    case 0xFC:
        return handle_led_stream(cmd_buf);
    case 0xFD:
        return handle_log_read(cmd_buf);
    default:
        return 0;   /* passthrough to original firmware */
    }
}
