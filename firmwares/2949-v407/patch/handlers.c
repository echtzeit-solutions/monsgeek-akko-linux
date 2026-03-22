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

/* ── Linker-provided BSS boundaries (from patch.ld) ──────────────────── */

extern uint8_t __patch_bss_start[];
extern uint8_t __patch_bss_end[];

static void zero_patch_bss(void) {
    for (uint8_t *p = __patch_bss_start; p < __patch_bss_end; p++)
        *p = 0;
}

/* ── SRAM addresses (via linker symbols where available) ──────────────── */

#define ADC_BATTERY_AVG   (*(volatile uint32_t *)&g_battery_avg_buf)   /* averaged battery ADC reading */
#define ADC_SCAN_COUNTER  (*(volatile uint32_t *)&g_adc_accumulator)   /* magnetism engine ADC scan counter */
#define ADC_RAW_SAMPLE    (*(volatile uint32_t *)0x20003C88)           /* raw ADC sample 0 (battery channel, no symbol) */

/* ── USB HID request constants ───────────────────────────────────────── */

#define USB_BMREQ_CLASS_IN         0xA1   /* bmRequestType: class, device-to-host, interface */
#define HID_GET_REPORT             0x01   /* bRequest: GET_REPORT */
#define WVALUE_FEATURE_REPORT(id)  ((3 << 8) | (id))  /* wValue for Feature report by ID */

/* ── Derived addresses from exported symbols ─────────────────────────── */

/* IF1 Report Descriptor length (from Ghidra RE of hid_class_setup_handler) */
#define IF1_RDESC_LEN  171

/* wDescriptorLength field within USB descriptors (Ghidra-sourced symbols).
 * Config descriptors: IF1 HID descriptor starts at offset +43, wDescLen at +7 within = +50 total.
 * Standalone IF1 HID desc: wDescLen at +7 from g_if1_hid_desc. */
extern volatile uint8_t g_cfg_desc_fs[];   /* FS config descriptor @ SRAM */
extern volatile uint8_t g_cfg_desc_hs[];   /* HS config descriptor @ SRAM */
extern volatile uint8_t g_cfg_desc_os[];   /* OS config descriptor @ SRAM */
extern volatile uint8_t g_if1_hid_desc[];  /* standalone IF1 HID descriptor @ SRAM */

#define IF1_WDESCLEN_OFF    50  /* offset of IF1 wDescriptorLength within config desc */
#define HID_WDESCLEN_OFF     7  /* offset of wDescriptorLength within HID descriptor */

#define WDESCLEN_FS         (&g_cfg_desc_fs[IF1_WDESCLEN_OFF])
#define WDESCLEN_HS         (&g_cfg_desc_hs[IF1_WDESCLEN_OFF])
#define WDESCLEN_OS         (&g_cfg_desc_os[IF1_WDESCLEN_OFF])
#define WDESCLEN_STANDALONE (&g_if1_hid_desc[HID_WDESCLEN_OFF])

/* ── LED buffers (from fw_symbols.ld) ────────────────────────────────── */

#define LED_BUF_SIZE  0x7B0   /* 1968 bytes: 82 LEDs × 24 bytes WS2812 encoding */
#define LED_COUNT     82
#define MATRIX_LEN    96      /* 16 cols × 6 rows; row-major (pos = row*16+col) */

/* ── On-device animation engine ─────────────────────────────────────── */

#define ANIM_MAX_KF   8
#define ANIM_MAX_DEFS 8

#define ANIM_FLAG_ONE_SHOT  0x01
#define ANIM_FLAG_RAINBOW   0x04

/* Easing IDs (wire format) */
#define EASE_HOLD           0
#define EASE_LINEAR         1
#define EASE_INOUT_QUAD     2
#define EASE_IN_QUAD        3
#define EASE_OUT_QUAD       4
#define EASE_IN_EXPO        5
#define EASE_OUT_EXPO       6

typedef struct {
    uint16_t t_ticks;   /* absolute time in 5ms ticks (200Hz) */
    uint8_t  r, g, b;   /* RGB888 (unpacked from RGB565 on receive) */
    uint8_t  easing;
} anim_keyframe_t;      /* 6 bytes */

typedef struct {
    anim_keyframe_t kf[ANIM_MAX_KF];   /* 48B */
    uint16_t duration_ticks;            /* total cycle length */
    uint16_t elapsed_ticks;             /* current playback position */
    uint8_t  num_kf;                    /* 0 = def unused */
    uint8_t  flags;                     /* bit0: one-shot, bit2: rainbow */
    int8_t   priority;                  /* higher wins key conflicts */
    uint8_t  _pad;
} anim_def_t;                           /* 56 bytes */

typedef struct {
    uint8_t anim_id;        /* 0xFF = no animation, 0-7 = def index */
    uint8_t phase_offset;   /* stagger: value × 8 ticks (40ms granularity) */
} key_anim_t;               /* 2 bytes */

typedef struct {
    uint32_t frame_count;     /* total blend calls (monotonic, for sync) */
    uint8_t  active_count;    /* nonzero defs (fast skip when idle) */
    uint8_t  _pad[3];
} anim_engine_t;              /* 8 bytes */

static anim_def_t   anim_defs[ANIM_MAX_DEFS];   /* 56×8 = 448B */
static key_anim_t   key_table[LED_COUNT];        /* 82×2 = 164B */
static anim_engine_t anim_engine;                /* 8B */
/* Total new BSS: 620B */

/* Per-LED RGB overlay: additive, saturating. 0 = no overlay for that channel.
 * Shared by both anim_tick() and led_overlay_memcpy_and_blend(). */
static uint8_t overlay_buf[LED_COUNT * 3];  /* 82×3 = 246 bytes */
static uint8_t overlay_active;              /* non-zero if any overlay pixel set */


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

/* ── Safe EP2 send (follows stock busy-flag contract) ────────────────── */
/* Check ep2_tx_ready → clear → flush+xfer.  Returns 1 if sent, 0 if busy.
 * Stock usb_ep_report_send uses the same protocol; if we grab the flag
 * first, the stock sender skips that cycle and retries next time. */

static inline int ep2_send_if_ready(void *buf, uint32_t len) {
    volatile hid_report_state_t *rpt =
        (volatile hid_report_state_t *)&g_hid_report_pending_flags;
    if (!rpt->ep2_tx_ready)
        return 0;
    rpt->ep2_tx_ready = 0;
    usb_ep2_in_transmit(buf, len);
    return 1;
}

/* ── Diagnostics (readable via 0xE7 patch info) ──────────────────────── */
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

/* ── Debug ring buffer (readable via 0xE9) ───────────────────────────── */

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

/* RTT Control Block — pinned at PATCH_SRAM origin (0x20009800) via .rtt section.
 * BMP finds it without scanning: monitor rtt ram 0x20009800 0x20009C00 */
typedef struct {
    char         id[16];        /* "SEGGER RTT\0\0\0\0\0\0" */
    int32_t      max_up;        /* 1 */
    int32_t      max_down;      /* 0 */
    rtt_up_buf_t up[1];
} rtt_cb_t;

static rtt_cb_t  __attribute__((section(".rtt"),used)) rtt_cb;
static uint8_t   __attribute__((section(".rtt"),used)) rtt_buf[RTT_BUF_SIZE];
static const char rtt_channel_name[] = "monsmod";

/* RTT tag definitions for battery monitor */
#define RTT_TAG_ADC_AVG       0x01  /* u16: averaged battery ADC reading */
#define RTT_TAG_BATT_RAW      0x02  /* u8:  battery_raw_level */
#define RTT_TAG_BATT_LEVEL    0x03  /* u8:  battery_level (debounced %) */
#define RTT_TAG_CHARGER       0x04  /* u8:  charger_connected flag */
#define RTT_TAG_DEBOUNCE_CTR  0x05  /* u8:  battery_update_ctr */
#define RTT_TAG_ADC_COUNTER   0x10  /* u32: magnetism engine ADC scan counter */

static void rtt_init(void) {
    /* Already initialized?  max_up is set to 1 as the last step below.
     * After zero_patch_bss(), max_up == 0 so this runs once. */
    if (rtt_cb.max_up == 1)
        return;

    /* .bss is zeroed by zero_patch_bss() — no manual zeroing needed. */

    /* Set up channel 0 (up only) */
    rtt_cb.up[0].name  = rtt_channel_name;
    rtt_cb.up[0].buf   = rtt_buf;
    rtt_cb.up[0].size  = RTT_BUF_SIZE;
    rtt_cb.up[0].wr_off = 0;
    rtt_cb.up[0].rd_off = 0;
    rtt_cb.up[0].flags  = 0;  /* SEGGER_RTT_MODE_NO_BLOCK_SKIP */
    rtt_cb.max_down = 0;

    /* Write magic + max_up LAST — prevents BMP finding half-initialized CB
     * and serves as the initialization guard for rtt_emit(). */
    __asm__ volatile ("dsb" ::: "memory");
    const char magic[] = "SEGGER RTT\0\0\0\0\0";
    for (int i = 0; i < 16; i++)
        ((volatile char *)rtt_cb.id)[i] = magic[i];
    rtt_cb.max_up = 1;
}

static void rtt_emit(uint8_t tag, uint32_t val) {
    /* Guard: RTT control block is in PATCH_SRAM which is NOT zero-initialized.
     * In dongle mode, rtt_init() never runs (handle_usb_connect doesn't fire),
     * so wr_off/rd_off contain garbage.  Using garbage as an index into rtt_buf
     * would corrupt random SRAM.  max_up is set to 1 only by rtt_init(). */
    if (rtt_cb.max_up != 1)
        return;

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

/* ── Dongle reports "before" hook ──────────────────────────────────── */
/* Called BEFORE build_dongle_reports runs.
 *
 * Stock firmware bug: hid_report_check_send block 3 zeros the consumer
 * buffer before build_dongle_reports can read it.  Fixed by NOP in
 * hooks.py.  With the NOP, keycode_dispatch case 3 writes consumer data
 * to g_dongle_consumer_buf (0x20000027) and sets bit 0x04.
 * build_dongle_reports reads the actual data for sub=3.  The dongle
 * handles sub=3 → consumer_ready → EP2 natively (rf_tx_handler, with
 * speed gate NOP'd in dongle hooks.py).
 *
 * Auto-release: after build_dongle_reports sends a consumer press
 * (detected by bit 0x04 clearing between cycles while data is non-zero),
 * we zero the buffer and re-set bit 0x04 to force a release report.
 * This handles encoders that don't generate explicit release events.
 *
 * pending_reports_bitmap bit → build_dongle_reports sub type:
 *   0x01→sub=0(mouse)    0x04→sub=3(consumer)  0x08→sub=4(dial)
 *   0x10→sub=5(extra)    0x20→sub=1(keyboard)   0x40→sub=2(NKRO) */

#define RTT_TAG_DONGLE_BITMAP  0x20  /* u8: pending_reports_bitmap snapshot */

void dongle_reports_before_hook(void) {
    rtt_init();  /* idempotent; ensures RTT works in dongle mode */

    if (*(volatile uint8_t *)&g_connection_mode != 5)
        return;

    volatile hid_report_state_t *reports =
        (volatile hid_report_state_t *)&g_hid_report_pending_flags;
    uint8_t bitmap = reports->pending_reports_bitmap;

    /* Consumer auto-release: g_dongle_consumer_buf layout is
     * [report_type:u8] [usage_lo:u8] [usage_hi:u8] at 0x20000027.
     * After build_dongle_reports sends the press (clears bit 0x04),
     * if the usage data is still non-zero, schedule a release. */
    static uint8_t consumer_was_pending;
    volatile uint8_t *cbuf = (volatile uint8_t *)&g_dongle_consumer_buf;

    if (bitmap & 0x04) {
        /* Consumer send pending — remember for next cycle */
        consumer_was_pending = 1;
    } else if (consumer_was_pending) {
        /* Bit 0x04 cleared = build_dongle_reports sent the report.
         * If usage data is non-zero, force a release (zeros). */
        consumer_was_pending = 0;
        if (cbuf[1] != 0 || cbuf[2] != 0) {
            cbuf[1] = 0;
            cbuf[2] = 0;
            reports->pending_reports_bitmap |= 0x04;
        }
    }

    if (bitmap)
        rtt_emit(RTT_TAG_DONGLE_BITMAP, bitmap);
}

/* ── Battery monitor "before" hook ─────────────────────────────────── */
/* Called BEFORE battery_level_monitor runs. Emits RTT records with
 * current battery ADC, level, charger state etc. for live observation.
 * battery_level_monitor fires when adc_counter == 2000 (~every few seconds). */

void battery_monitor_before_hook(void) {
    rtt_init();  /* idempotent; ensures RTT works in all modes */

    volatile kbd_state_t *kbd = (volatile kbd_state_t *)&g_kbd_state;

    rtt_emit(RTT_TAG_ADC_AVG, ADC_BATTERY_AVG & 0xFFFF);

    rtt_emit(RTT_TAG_BATT_RAW, kbd->battery_raw_level);
    rtt_emit(RTT_TAG_BATT_LEVEL, kbd->battery_level);
    rtt_emit(RTT_TAG_CHARGER, kbd->charger_connected);
    rtt_emit(RTT_TAG_DEBOUNCE_CTR, kbd->battery_update_ctr);

    rtt_emit(RTT_TAG_ADC_COUNTER, ADC_SCAN_COUNTER);
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
    WDESCLEN_STANDALONE[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_STANDALONE[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_FS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_FS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_HS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_HS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_OS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_OS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);

    /* Only intercept GET_REPORT for IF1 battery Feature report.
     * All other requests (GET_DESCRIPTOR, SET_IDLE, etc.) pass through to
     * the original handler, which now reads from our extended_rdesc buffer. */
    if (wIndex == 1 && bmReqType == USB_BMREQ_CLASS_IN && bRequest == HID_GET_REPORT) {
        /* GET_REPORT — wValue = (report_type << 8) | report_id
         * Feature report type = 3, Report ID = 7 → wValue = 0x0307 */
        if (wValue == WVALUE_FEATURE_REPORT(7)) {
            volatile kbd_state_t *kbd = (volatile kbd_state_t *)&g_kbd_state;
            uint8_t bat_level = kbd->battery_level;
            uint8_t charging  = kbd->charger_connected;

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

/* ── Animation engine math helpers ────────────────────────────────────── */

/* Integer easing: t is 0-255 fixed-point, returns 0-255.
 * Cortex-M4 single-cycle multiply keeps all of these fast. */
static inline uint8_t ease_apply(uint8_t easing, uint8_t t) {
    switch (easing) {
    case EASE_HOLD:
        return 0;
    case EASE_IN_QUAD:
        /* t^2 / 255 */
        return (uint8_t)(((uint16_t)t * t) >> 8);
    case EASE_OUT_QUAD: {
        /* 1 - (1-t)^2 */
        uint8_t inv = 255 - t;
        return (uint8_t)(255 - (((uint16_t)inv * inv) >> 8));
    }
    case EASE_INOUT_QUAD:
        if (t < 128) {
            return (uint8_t)(((uint16_t)t * t * 2) >> 8);
        } else {
            uint8_t inv = 255 - t;
            return (uint8_t)(255 - (((uint16_t)inv * inv * 2) >> 8));
        }
    case EASE_IN_EXPO:
        /* Approximate 2^(10*(t/255-1)): use t^3/255^2 (steep curve) */
        return (uint8_t)(((uint32_t)t * t * t) >> 16);
    case EASE_OUT_EXPO: {
        /* 1 - (1-t)^3/255^2 */
        uint8_t inv = 255 - t;
        return (uint8_t)(255 - (((uint32_t)inv * inv * inv) >> 16));
    }
    default: /* EASE_LINEAR and unknown */
        return t;
    }
}

/* Linear interpolation: a + ((b-a) * t) >> 8, t in 0-255 */
static inline uint8_t lerp8(uint8_t a, uint8_t b, uint8_t t) {
    return (uint8_t)((int16_t)a + ((((int16_t)b - (int16_t)a) * (int16_t)t) >> 8));
}

/* HSV→RGB (h,s,v all 0-255) */
static void hsv_to_rgb(uint8_t h, uint8_t s, uint8_t v,
                        uint8_t *r, uint8_t *g, uint8_t *b) {
    uint8_t region = h / 43, remainder = (h % 43) * 6;
    uint8_t p = (uint8_t)((v * (255 - s)) >> 8);
    uint8_t q = (uint8_t)((v * (255 - ((s * remainder) >> 8))) >> 8);
    uint8_t t = (uint8_t)((v * (255 - ((s * (255 - remainder)) >> 8))) >> 8);
    switch (region) {
        case 0: *r=v; *g=t; *b=p; break;
        case 1: *r=q; *g=v; *b=p; break;
        case 2: *r=p; *g=v; *b=t; break;
        case 3: *r=p; *g=q; *b=v; break;
        case 4: *r=t; *g=p; *b=v; break;
        default:*r=v; *g=p; *b=q; break;
    }
}

/* Evaluate a definition at local time t_local (in ticks).
 * Writes RGB result to *out_r, *out_g, *out_b. */
static void anim_evaluate(const anim_def_t *def, uint16_t t_local,
                           uint8_t *out_r, uint8_t *out_g, uint8_t *out_b) {
    /* Rainbow mode: hue from time, brightness from keyframes */
    if (def->flags & ANIM_FLAG_RAINBOW) {
        /* Compute brightness from keyframes */
        uint8_t bri = 255;
        if (def->num_kf >= 2) {
            /* Find segment */
            uint8_t seg = 0;
            for (uint8_t i = 0; i < def->num_kf - 1; i++) {
                if (t_local < def->kf[i + 1].t_ticks) { seg = i; goto found_bri; }
            }
            seg = def->num_kf - 1;
found_bri:
            if (seg < def->num_kf - 1) {
                uint16_t dt = def->kf[seg + 1].t_ticks - def->kf[seg].t_ticks;
                if (dt > 0) {
                    uint8_t frac = (uint8_t)(((uint32_t)(t_local - def->kf[seg].t_ticks) * 255) / dt);
                    uint8_t eased = ease_apply(def->kf[seg].easing, frac);
                    bri = lerp8(def->kf[seg].r, def->kf[seg + 1].r, eased); /* r = brightness in rainbow */
                } else {
                    bri = def->kf[seg].r;
                }
            } else {
                bri = def->kf[seg].r;
            }
        } else if (def->num_kf == 1) {
            bri = def->kf[0].r;
        }
        /* Hue sweeps 0-255 over duration */
        uint8_t hue = (def->duration_ticks > 0)
            ? (uint8_t)(((uint32_t)t_local * 255) / def->duration_ticks)
            : 0;
        hsv_to_rgb(hue, 255, bri, out_r, out_g, out_b);
        return;
    }

    /* Normal keyframe mode: find segment, interpolate RGB */
    if (def->num_kf == 0) { *out_r = *out_g = *out_b = 0; return; }
    if (def->num_kf == 1) {
        *out_r = def->kf[0].r; *out_g = def->kf[0].g; *out_b = def->kf[0].b;
        return;
    }

    /* Find surrounding keyframes */
    uint8_t seg = 0;
    for (uint8_t i = 0; i < def->num_kf - 1; i++) {
        if (t_local < def->kf[i + 1].t_ticks) { seg = i; goto found_seg; }
    }
    seg = def->num_kf - 1;
found_seg:
    if (seg >= def->num_kf - 1) {
        /* At or past last keyframe */
        *out_r = def->kf[seg].r; *out_g = def->kf[seg].g; *out_b = def->kf[seg].b;
        return;
    }

    uint16_t dt = def->kf[seg + 1].t_ticks - def->kf[seg].t_ticks;
    if (dt == 0) {
        *out_r = def->kf[seg].r; *out_g = def->kf[seg].g; *out_b = def->kf[seg].b;
        return;
    }

    /* Compute fractional position 0-255 */
    uint8_t frac = (uint8_t)(((uint32_t)(t_local - def->kf[seg].t_ticks) * 255) / dt);
    uint8_t eased = ease_apply(def->kf[seg].easing, frac);

    *out_r = lerp8(def->kf[seg].r, def->kf[seg + 1].r, eased);
    *out_g = lerp8(def->kf[seg].g, def->kf[seg + 1].g, eased);
    *out_b = lerp8(def->kf[seg].b, def->kf[seg + 1].b, eased);
}

/* Tick the animation engine. Called from led_overlay_memcpy_and_blend
 * which runs at ~100Hz (LED DMA refresh rate, measured). Each call = 1 tick.
 * The daemon converts ms→ticks at 10ms/tick to match this rate. */
static void anim_tick(void) {
    if (anim_engine.active_count == 0)
        return;

    /* Advance elapsed for active defs */
    for (int d = 0; d < ANIM_MAX_DEFS; d++) {
        if (anim_defs[d].num_kf == 0)
            continue;
        anim_defs[d].elapsed_ticks++;
    }

    /* Evaluate each assigned key */
    uint8_t any_active = 0;
    for (int i = 0; i < LED_COUNT; i++) {
        if (key_table[i].anim_id >= ANIM_MAX_DEFS)
            continue;

        anim_def_t *def = &anim_defs[key_table[i].anim_id];
        if (def->num_kf == 0) {
            key_table[i].anim_id = 0xFF; /* def was cancelled */
            continue;
        }
        any_active = 1;

        uint16_t phase = (uint16_t)key_table[i].phase_offset * 8;
        uint8_t r, g, b;

        if (def->flags & ANIM_FLAG_ONE_SHOT) {
            int32_t local_t = (int32_t)def->elapsed_ticks - (int32_t)phase;
            if (local_t < 0) {
                r = def->kf[0].r; g = def->kf[0].g; b = def->kf[0].b;
            } else if (local_t >= (int32_t)def->duration_ticks) {
                uint8_t last = def->num_kf - 1;
                r = def->kf[last].r; g = def->kf[last].g; b = def->kf[last].b;
            } else {
                anim_evaluate(def, (uint16_t)local_t, &r, &g, &b);
            }
        } else {
            /* Looping */
            if (def->duration_ticks == 0) {
                r = def->kf[0].r; g = def->kf[0].g; b = def->kf[0].b;
            } else {
                uint16_t t = (uint16_t)((def->elapsed_ticks + phase) % def->duration_ticks);
                anim_evaluate(def, t, &r, &g, &b);
            }
        }

        overlay_buf[i * 3 + 0] = r;
        overlay_buf[i * 3 + 1] = g;
        overlay_buf[i * 3 + 2] = b;
    }

    if (any_active)
        overlay_active = 1;
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

/* ── Patch discovery (0xE7) ──────────────────────────────────────────────
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
    buf[6]  = 0x4F;           /* capabilities: battery(0) + led_stream(1) + debug_log(2) + consumer_fix(3) + anim_engine(6) */
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

    /* Raw kbd_state fields for battery debugging */
    volatile kbd_state_t *kbd = (volatile kbd_state_t *)&g_kbd_state;
    buf[30] = kbd->battery_level;
    buf[31] = kbd->charger_connected;
    buf[32] = kbd->charger_debounce_ctr;
    buf[33] = kbd->battery_update_ctr;
    buf[34] = kbd->battery_raw_level;
    buf[35] = kbd->animation_dirty;
    uint32_t adc_ctr = ADC_SCAN_COUNTER;
    buf[36] = (uint8_t)(adc_ctr & 0xFF);
    buf[37] = (uint8_t)((adc_ctr >> 8) & 0xFF);

    buf[38] = kbd->scan_tick_counter;
    buf[39] = kbd->report_state;
    buf[40] = kbd->charge_status;
    buf[41] = *(volatile uint8_t *)&g_connection_mode;

    uint32_t avg = ADC_BATTERY_AVG;
    buf[42] = (uint8_t)(avg & 0xFF);
    buf[43] = (uint8_t)((avg >> 8) & 0xFF);

    volatile uint16_t *adc_s0 = (volatile uint16_t *)&ADC_RAW_SAMPLE;  /* raw ADC sample 0, 16-bit */
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

/* ── LED overlay (0xE8) ───────────────────────────────────────────────
 *
 * Persistent additive overlay: host-set RGB values are stored in overlay_buf
 * and added (saturating) to the animation output every frame by
 * led_overlay_memcpy_and_blend(), which replaces the firmware's frame→DMA
 * memcpy via a BL patch at 0x080161a8.
 *
 * Page 0-6:  Write 18 keys × raw RGB into overlay_buf
 * Page 0xFF: Commit — no-op (overlay applied every frame automatically)
 * Page 0xFE: Clear overlay
 *
 * Data layout: buf[3] = page, buf[4..57] = 18×RGB (54 bytes).
 * Host sends row-major indices (page*18 + i), where pos = row*16 + col.
 *
 * Uses static_led_pos_tbl from firmware ROM (0x08025031, via fw_symbols.ld).
 * Row-major: static_led_pos_tbl[row*16+col] → WS2812 strip index (0–81).
 * 0xFF = no LED (gap for wide keys / empty slots). */

/* Per-LED RGB overlay: additive, saturating. 0 = no overlay for that channel.
 * overlay_buf and overlay_active defined above (near anim structs) for
 * visibility to both anim_tick() and led_overlay_memcpy_and_blend(). */

/* Decode one color byte from WS2812 SPI encoding (8 SPI bytes → 1 data byte).
 * Each SPI byte's bit 4 carries one data bit (0xF0 → 1, 0xC0 → 0). */
static inline uint8_t decode_ws2812_byte(volatile uint8_t *p) {
    return ((p[0] & 0x10) << 3) | ((p[1] & 0x10) << 2) |
           ((p[2] & 0x10) << 1) | ((p[3] & 0x10)     ) |
           ((p[4] & 0x10) >> 1) | ((p[5] & 0x10) >> 2) |
           ((p[6] & 0x10) >> 3) | ((p[7] & 0x10) >> 4);
}

/* Replacement for firmware's memcpy(dma_buf, frame_buf, 0x7b0) at 0x080161a8.
 * Called every scan cycle after led_render_frame writes to g_led_frame_buf.
 * Copies frame→DMA then applies the additive overlay. */
void led_overlay_memcpy_and_blend(void *dst, const void *src, uint32_t len) {
    memcpy(dst, src, len);

    /* Count frames (monotonic, for sync) */
    anim_engine.frame_count++;

    /* Tick the animation engine */
    anim_tick();

    if (!overlay_active)
        return;

    volatile uint8_t *dma = (volatile uint8_t *)dst;
    for (int i = 0; i < LED_COUNT; i++) {
        uint8_t *ov = &overlay_buf[i * 3];
        if (ov[0] == 0 && ov[1] == 0 && ov[2] == 0)
            continue;

        volatile uint8_t *p = &dma[i * 24];
        /* Decode GRB from WS2812, add overlay RGB, re-encode */
        uint16_t g = decode_ws2812_byte(p);
        uint16_t r = decode_ws2812_byte(p + 8);
        uint16_t b = decode_ws2812_byte(p + 16);

        r += ov[0]; if (r > 255) r = 255;
        g += ov[1]; if (g > 255) g = 255;
        b += ov[2]; if (b > 255) b = 255;

        encode_ws2812_byte(p,      (uint8_t)g);  /* GRB order */
        encode_ws2812_byte(p + 8,  (uint8_t)r);
        encode_ws2812_byte(p + 16, (uint8_t)b);
    }
}

static int handle_led_stream(volatile uint8_t *buf) {
    uint8_t page = buf[3];

    if (page == 0xFD) {
        /* Sparse overlay: buf[4]=count, buf[5..]=([matrix_idx,R,G,B] × count)
         * 4 bytes per LED, max 13 entries (13×4+1 = 53, fits in 54 payload bytes).
         * Firmware maps matrix_idx → strip_idx via static_led_pos_tbl. */
        uint8_t count = buf[4];
        if (count > 13) count = 13;
        for (uint8_t i = 0; i < count; i++) {
            uint8_t *entry = (uint8_t *)&buf[5 + i * 4];
            uint8_t matrix_idx = entry[0];
            if (matrix_idx >= MATRIX_LEN) continue;
            uint8_t strip_idx = static_led_pos_tbl[matrix_idx];
            if (strip_idx >= LED_COUNT) continue;
            overlay_buf[strip_idx * 3 + 0] = entry[1];  /* R */
            overlay_buf[strip_idx * 3 + 1] = entry[2];  /* G */
            overlay_buf[strip_idx * 3 + 2] = entry[3];  /* B */
        }
        overlay_active = 1;
        buf[0] = 0;
        return 1;
    }

    if (page == 0xFE) {
        /* Clear overlay + all animations */
        for (int i = 0; i < ANIM_MAX_DEFS; i++)
            anim_defs[i].num_kf = 0;
        for (int i = 0; i < LED_COUNT; i++)
            key_table[i].anim_id = 0xFF;
        anim_engine.active_count = 0;
        for (int i = 0; i < LED_COUNT * 3; i++)
            overlay_buf[i] = 0;
        overlay_active = 0;
        buf[0] = 0;
        return 1;
    }

    if (page == 0xFF) {
        /* Commit — no-op (overlay applied every frame automatically).
         * Kept for protocol compatibility. */
        buf[0] = 0;
        return 1;
    }

    if (page < 7) {
        /* Write overlay RGB values (raw, not WS2812 encoded) */
        uint8_t *rgb = (uint8_t *)&buf[4];
        uint8_t start = page * 18;

        for (int i = 0; i < 18 && (start + i) < MATRIX_LEN; i++) {
            uint32_t pos = start + i;
            uint8_t strip_idx = static_led_pos_tbl[pos];
            if (strip_idx >= LED_COUNT)
                continue;
            overlay_buf[strip_idx * 3 + 0] = rgb[i * 3];     /* R */
            overlay_buf[strip_idx * 3 + 1] = rgb[i * 3 + 1]; /* G */
            overlay_buf[strip_idx * 3 + 2] = rgb[i * 3 + 2]; /* B */
        }

        overlay_active = 1;
        buf[0] = 0;
        return 1;
    }

    return 0;  /* unknown page, passthrough */
}

/* ── Animation command handler (0xEA) ──────────────────────────────────── */

/* Unpack RGB565 to RGB888 */
static inline void unpack_rgb565(uint16_t c565, uint8_t *r, uint8_t *g, uint8_t *b) {
    *r = (uint8_t)(((c565 >> 8) & 0xF8) | ((c565 >> 13) & 0x07));
    *g = (uint8_t)(((c565 >> 3) & 0xFC) | ((c565 >> 9)  & 0x03));
    *b = (uint8_t)(((c565 << 3) & 0xF8) | ((c565 >> 2)  & 0x07));
}

static void anim_recount_active(void) {
    uint8_t count = 0;
    for (int i = 0; i < ANIM_MAX_DEFS; i++) {
        if (anim_defs[i].num_kf > 0) count++;
    }
    anim_engine.active_count = count;
}

/* Auto-clean zombie defs: num_kf > 0 but no keys assigned.
 * Call ONLY after ASSIGN (when key ownership may have changed),
 * NOT after DEF (which creates defs before keys are assigned). */
static void anim_cleanup_zombies(void) {
    for (int d = 0; d < ANIM_MAX_DEFS; d++) {
        if (anim_defs[d].num_kf == 0)
            continue;
        uint8_t has_key = 0;
        for (int k = 0; k < LED_COUNT; k++) {
            if (key_table[k].anim_id == d) { has_key = 1; break; }
        }
        if (!has_key) {
            anim_defs[d].num_kf = 0;
            anim_defs[d].elapsed_ticks = 0;
        }
    }
    anim_recount_active();
}

static void anim_cancel_def(uint8_t def_id) {
    if (def_id >= ANIM_MAX_DEFS) return;

    /* Zero the definition */
    anim_defs[def_id].num_kf = 0;
    anim_defs[def_id].elapsed_ticks = 0;

    /* Clear key_table entries pointing to this def + zero their overlay */
    for (int i = 0; i < LED_COUNT; i++) {
        if (key_table[i].anim_id == def_id) {
            key_table[i].anim_id = 0xFF;
            overlay_buf[i * 3 + 0] = 0;
            overlay_buf[i * 3 + 1] = 0;
            overlay_buf[i * 3 + 2] = 0;
        }
    }

    anim_recount_active();

    /* If no animations remain, check if overlay can be deactivated */
    if (anim_engine.active_count == 0) {
        uint8_t any = 0;
        for (int i = 0; i < LED_COUNT * 3; i++) {
            if (overlay_buf[i]) { any = 1; break; }
        }
        if (!any) overlay_active = 0;
    }
}

static int handle_anim_cmd(volatile uint8_t *buf) {
    uint8_t sub = buf[3];

    if (sub <= 0x07) {
        /* ── ANIM_ASSIGN ─────────────────────────────────────────── */
        uint8_t def_id = sub;
        if (anim_defs[def_id].num_kf == 0) goto done; /* def not loaded */

        uint8_t count = buf[4];
        if (count > 30) count = 30;

        for (uint8_t i = 0; i < count; i++) {
            uint8_t matrix_idx   = buf[5 + i * 2];
            uint8_t phase_offset = buf[5 + i * 2 + 1];
            if (matrix_idx >= MATRIX_LEN) continue;
            uint8_t strip_idx = static_led_pos_tbl[matrix_idx];
            if (strip_idx >= LED_COUNT) continue;

            /* Priority check: only replace if new def has >= priority */
            uint8_t cur_id = key_table[strip_idx].anim_id;
            if (cur_id < ANIM_MAX_DEFS && anim_defs[cur_id].num_kf > 0) {
                if (anim_defs[def_id].priority < anim_defs[cur_id].priority)
                    continue; /* current has higher priority */
            }

            key_table[strip_idx].anim_id = def_id;
            key_table[strip_idx].phase_offset = phase_offset;
        }
        anim_cleanup_zombies();
        goto done;
    }

    if (sub >= 0x08 && sub <= 0x0F) {
        /* ── ANIM_DEF ────────────────────────────────────────────── */
        uint8_t def_id = sub & 0x07;
        anim_def_t *def = &anim_defs[def_id];

        uint8_t num_kf = buf[4];
        if (num_kf > ANIM_MAX_KF) num_kf = ANIM_MAX_KF;

        def->flags = buf[5];
        def->priority = (int8_t)buf[6];
        def->duration_ticks = (uint16_t)(buf[7] | ((uint16_t)buf[8] << 8));
        def->elapsed_ticks = 0;

        /* Unpack up to 4 keyframes from this packet */
        uint8_t kf_in_pkt = (num_kf > 4) ? 4 : num_kf;
        for (uint8_t i = 0; i < kf_in_pkt; i++) {
            uint8_t off = 9 + i * 5;
            def->kf[i].t_ticks = (uint16_t)(buf[off] | ((uint16_t)buf[off + 1] << 8));
            uint16_t c565 = (uint16_t)(buf[off + 2] | ((uint16_t)buf[off + 3] << 8));
            unpack_rgb565(c565, &def->kf[i].r, &def->kf[i].g, &def->kf[i].b);
            def->kf[i].easing = buf[off + 4];
        }

        /* Only set num_kf now: if num_kf > 4, more KFs come via DEF_EXT */
        def->num_kf = (num_kf <= 4) ? num_kf : 0; /* 0 = pending ext */
        if (num_kf <= 4) {
            def->num_kf = num_kf;
            anim_recount_active();
        } else {
            /* Store expected count in _pad so DEF_EXT knows */
            def->_pad = num_kf;
        }
        goto done;
    }

    if (sub >= 0x10 && sub <= 0x17) {
        /* ── ANIM_DEF_EXT ────────────────────────────────────────── */
        uint8_t def_id = sub & 0x07;
        anim_def_t *def = &anim_defs[def_id];

        uint8_t num_kf = def->_pad; /* stored by ANIM_DEF */
        if (num_kf > ANIM_MAX_KF) num_kf = ANIM_MAX_KF;

        /* Unpack KFs 4-7 */
        for (uint8_t i = 4; i < num_kf; i++) {
            uint8_t off = 4 + (i - 4) * 5;
            def->kf[i].t_ticks = (uint16_t)(buf[off] | ((uint16_t)buf[off + 1] << 8));
            uint16_t c565 = (uint16_t)(buf[off + 2] | ((uint16_t)buf[off + 3] << 8));
            unpack_rgb565(c565, &def->kf[i].r, &def->kf[i].g, &def->kf[i].b);
            def->kf[i].easing = buf[off + 4];
        }

        def->num_kf = num_kf;
        def->_pad = 0;
        anim_recount_active();
        goto done;
    }

    if (sub == 0xF0) {
        /* ── ANIM_QUERY ──────────────────────────────────────────── */
        /* Response layout (host reads from cmd_buf+2, so resp[N] = buf[N+2]):
         *   buf[3]    = 0xF0 (sub-command echo for disambiguation)
         *   buf[4]    = active_count
         *   buf[5..8] = frame_count (u32 LE, for sync)
         *   buf[9]    = overlay_active
         *   buf[10..57]= per-def status (8 × 6 bytes)
         *     [0] num_kf, [1] flags, [2] priority, [3] key_count,
         *     [4..5] duration_ticks (u16 LE) */
        buf[3] = 0xF0;  /* sub echo — driver verifies to reject stale responses */
        buf[4] = anim_engine.active_count;
        uint32_t fc = anim_engine.frame_count;
        buf[5] = (uint8_t)(fc & 0xFF);
        buf[6] = (uint8_t)((fc >> 8) & 0xFF);
        buf[7] = (uint8_t)((fc >> 16) & 0xFF);
        buf[8] = (uint8_t)((fc >> 24) & 0xFF);
        buf[9] = overlay_active;

        for (int d = 0; d < ANIM_MAX_DEFS; d++) {
            uint8_t base = 10 + d * 6;
            buf[base + 0] = anim_defs[d].num_kf;
            buf[base + 1] = anim_defs[d].flags;
            buf[base + 2] = (uint8_t)anim_defs[d].priority;
            /* Count keys assigned to this def */
            uint8_t kc = 0;
            for (int k = 0; k < LED_COUNT; k++) {
                if (key_table[k].anim_id == d) kc++;
            }
            buf[base + 3] = kc;
            buf[base + 4] = (uint8_t)(anim_defs[d].duration_ticks & 0xFF);
            buf[base + 5] = (uint8_t)(anim_defs[d].duration_ticks >> 8);
        }
        goto done;
    }

    if (sub >= 0xF1 && sub <= 0xF8) {
        /* ── ANIM_QUERY_KEYS ─────────────────────────────────────── */
        /* Returns key assignments for one def.
         *   buf[3] = sub (echo for disambiguation from QUERY status)
         *   buf[4] = count
         *   buf[5..] = (strip_idx, phase_offset) × count, max 28 */
        uint8_t def_id = sub - 0xF1;
        buf[3] = sub;  /* echo sub-command so driver can verify */
        uint8_t count = 0;
        for (int k = 0; k < LED_COUNT && count < 28; k++) {
            if (key_table[k].anim_id == def_id) {
                buf[5 + count * 2]     = (uint8_t)k;
                buf[5 + count * 2 + 1] = key_table[k].phase_offset;
                count++;
            }
        }
        buf[4] = count;
        goto done;
    }

    if (sub == 0xFE) {
        /* ── ANIM_CANCEL ─────────────────────────────────────────── */
        anim_cancel_def(buf[4]);
        goto done;
    }

    if (sub == 0xFF) {
        /* ── ANIM_CLEAR ──────────────────────────────────────────── */
        for (int i = 0; i < ANIM_MAX_DEFS; i++)
            anim_defs[i].num_kf = 0;
        for (int i = 0; i < LED_COUNT; i++) {
            key_table[i].anim_id = 0xFF;
            overlay_buf[i * 3 + 0] = 0;
            overlay_buf[i * 3 + 1] = 0;
            overlay_buf[i * 3 + 2] = 0;
        }
        overlay_active = 0;
        anim_engine.active_count = 0;
        goto done;
    }

    return 0; /* unknown sub-command, passthrough */

done:
    buf[0] = 0;
    return 1;
}

/* ── USB connect init (patches config descriptors before enumeration) ──── */

int handle_usb_connect(void) {
    /* Zero PATCH_SRAM .bss — stock crt0 only initializes the firmware's own
     * .bss region, not ours.  SRAM survives soft reboot (flash + reset)
     * so statics from the previous run persist as garbage.
     * Uses linker-provided __patch_bss_start/__patch_bss_end symbols. */
    zero_patch_bss();

    /* key_table uses 0xFF as "unassigned" sentinel, but zero_patch_bss sets
     * everything to 0 which means "assigned to def 0".  Fix it. */
    for (int i = 0; i < LED_COUNT; i++)
        key_table[i].anim_id = 0xFF;

    log_entry(LOG_USB_CONNECT, (const uint8_t *)0, 0);

    /* Initialize RTT control block (re-initializes on each USB plug). */
    rtt_init();

    /* Patch wDescriptorLength to EXTENDED_RDESC_LEN in all SRAM descriptor
     * copies.  Must happen BEFORE enumeration so the config descriptor
     * advertises the extended report descriptor size (171 + 46 battery). */
    WDESCLEN_STANDALONE[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_STANDALONE[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_FS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_FS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_HS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_HS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_OS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_OS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);

    /* Pre-populate extended_rdesc buffer so it's ready if GET_DESCRIPTOR
     * arrives before any hid_setup call. */
    memcpy(extended_rdesc, (void *)&g_if1_report_desc, IF1_RDESC_LEN);
    for (int i = 0; i < (int)BATTERY_RDESC_LEN; i++)
        extended_rdesc[IF1_RDESC_LEN + i] = battery_rdesc[i];

    return 0;   /* passthrough */
}

/* ── Debug log read (0xE9) ─────────────────────────────────────────────
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
        static uint8_t prev_charging;

        volatile kbd_state_t *kbd = (volatile kbd_state_t *)&g_kbd_state;
        uint8_t cur_charging = kbd->charger_connected;
        if (cur_charging != prev_charging) {
            prev_charging = cur_charging;

            static uint8_t bat_input[4] __attribute__((aligned(4)));
            bat_input[0] = 0x07;
            bat_input[1] = kbd->battery_level;
            bat_input[2] = cur_charging;
            ep2_send_if_ready(bat_input, 3);
        }
    }

    /* No pending command — cmd_buf[0] is set non-zero by firmware SET_REPORT handler */
    if (cmd_buf[0] == 0)
        return 0;

    /* ── OOB guards (bugs 1-3 from oob_hazards.txt) ───────────────── */
    {
        uint8_t cmd = cmd_buf[2];

        /* Bug 1: chunked staging overflow — cap chunk_index per command */
        if (cmd == 0x0A && cmd_buf[5] > 9)  goto reject;  /* SET_KEYMATRIX */
        if (cmd == 0x10 && cmd_buf[6] > 9)  goto reject;  /* SET_FN_LAYER  */
        if (cmd == 0x0B && cmd_buf[4] > 9)  goto reject;  /* SET_MACRO     */
        if (cmd == 0x0C && cmd_buf[5] > 6)  goto reject;  /* SET_USERPIC   */

        /* Bug 2: flash_save_userpic stack overflow — slot_id must be < 5 */
        if (cmd == 0x0C && cmd_buf[3] >= 5) goto reject;

        /* Bug 3: flash_save_macro flash overflow — macro_id must be < 50 */
        if (cmd == 0x0B && cmd_buf[3] >= 50) goto reject;
    }

    /* Log vendor command entry (skip 0xE9 to avoid contaminating the log
     * when reading it — each log read would otherwise add 3 bytes) */
    if (cmd_buf[2] != 0xE9) {
        uint8_t log_payload[2] = { cmd_buf[0], cmd_buf[2] };
        log_entry(LOG_VENDOR_CMD_ENTRY, log_payload, 2);
    }

    /* Command byte is at cmd_buf[2] = lp_class_report_buf[0]
     * (SET_REPORT data lands at cmd_buf+2, first byte = command) */
    switch (cmd_buf[2]) {
    case 0xE7:
        return handle_patch_info(cmd_buf);
    case 0xE8:
        return handle_led_stream(cmd_buf);
    case 0xE9:
        return handle_log_read(cmd_buf);
    case 0xEA:
        return handle_anim_cmd(cmd_buf);
    default:
        return 0;   /* passthrough to original firmware */
    }

reject:
    cmd_buf[0] = 0;   /* discard command */
    return 1;          /* intercepted — skip original */
}

/* ── Boot-time config validation (bugs 4-5 from oob_hazards.txt) ──────────
 * Called via BL.W from config_load_all (0x08012376), replacing:
 *   ldr r4, [pc, #0xEC]   ; r4 = g_fw_config ptr
 *   ldrb r0, [r4, #0]     ; r0 = profile_id
 * Must return with r4 = g_fw_config, r0 = clamped profile_id.
 * r1 is caller-saved (AAPCS) and safe to clobber. */

__attribute__((naked))
void validate_config_after_load(void) {
    __asm__ volatile (
        "ldr  r4, =g_fw_config          \n"

        /* Bug 4: clamp profile_id (offset 0x00) to < 4 */
        "ldrb r0, [r4, #0]              \n"
        "cmp  r0, #4                    \n"
        "blo  1f                        \n"
        "movs r0, #0                    \n"
        "strb r0, [r4, #0]             \n"
        "1:                             \n"

        /* Bug 5: clamp led_effect_mode (offset 0x08) to < 32 */
        "ldrb r1, [r4, #8]             \n"
        "cmp  r1, #32                   \n"
        "blo  2f                        \n"
        "movs r1, #0                    \n"
        "strb r1, [r4, #8]             \n"
        "2:                             \n"

        /* Return: r4 = g_fw_config, r0 = clamped profile_id */
        "ldrb r0, [r4, #0]             \n"
        "bx   lr                        \n"
        ".ltorg                         \n"
    );
}
