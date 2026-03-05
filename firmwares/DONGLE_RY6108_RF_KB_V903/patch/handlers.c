/*
 * Dongle patch handlers — battery HID over USB.
 *
 * The dongle already caches the keyboard's battery level and charging status
 * from RF packets (dongle_state.kb_battery_info/kb_charging).  This patch
 * exposes them as a standard HID battery via IF1's report descriptor and
 * GET_REPORT interception, with proactive Input report push on changes.
 *
 * Four hooks:
 *   1. "before" hook on usb_init — populates extended_rdesc + patches
 *      wDescriptorLength before USB enumeration starts.
 *   2. "filter" hook on hid_class_setup_handler — intercepts GET_REPORT
 *      Feature ID 7 for battery data.
 *   3. "before" hook on rf_packet_dispatch — detects battery/charging
 *      changes and sets a pending flag for deferred EP2 send.
 *   4. Trampoline in rf_tx_handler (speed gate site) — sends pending
 *      battery Input reports using the stock EP2 busy-flag contract.
 *
 * Convention (filter mode):
 *   return 0     = passthrough to original firmware handler
 *   return non-0 = intercepted (original handler skipped)
 */

#include <stdint.h>
#include "fw_dongle.h"
#include "hid_desc.h"

/* ── USB HID request constants ───────────────────────────────────────── */

#define USB_BMREQ_CLASS_IN         0xA1   /* bmRequestType: class, device-to-host, interface */
#define HID_GET_REPORT             0x01   /* bRequest: GET_REPORT */
#define WVALUE_FEATURE_REPORT(id)  ((3 << 8) | (id))  /* wValue for Feature report by ID */

/* ── Derived addresses ───────────────────────────────────────────────── */

#define IF1_RDESC_LEN  171   /* original IF1 report descriptor length */

/* wDescriptorLength field offsets in SRAM descriptor copies.
 * Each is a 2-byte LE field within a 9-byte HID descriptor. */
#define WDESCLEN_FS   ((volatile uint8_t *)0x200000DA)   /* FS config descriptor */
#define WDESCLEN_HS   ((volatile uint8_t *)0x2000012E)   /* HS config descriptor */
#define WDESCLEN_OS   ((volatile uint8_t *)0x20000182)   /* OS config descriptor */
#define WDESCLEN_STANDALONE ((volatile uint8_t *)0x200002BF) /* standalone IF1 HID desc */

/* ── Battery HID report descriptor (appended to IF1) ─────────────────── */

/* 46 bytes: Battery Strength + Charging status, Feature + Input reports.
 * Identical to keyboard patch — shared hid_desc.h macros. */
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

#define BATTERY_RDESC_LEN  (sizeof(battery_rdesc))        /* 46 */
#define EXTENDED_RDESC_LEN (IF1_RDESC_LEN + BATTERY_RDESC_LEN)  /* 217 = 0xD9 */

/* Buffer for extended IF1 descriptor (original 171B + battery 46B).
 * Non-static: address must be visible in ELF for build-time literal pool patch.
 * Placed in .bss → PATCH_SRAM (0x20002000+). */
uint8_t extended_rdesc[EXTENDED_RDESC_LEN];

/* ── Deferred battery EP2 send state ─────────────────────────────────── */
/* These live in PATCH_SRAM (.bss), zeroed by handle_usb_init. */

static uint8_t battery_ep2_pending;
static uint8_t pending_bat_level;
static uint8_t pending_bat_charging;

/* ── Descriptor patching (idempotent) ────────────────────────────────── */

static void patch_descriptors(void) {
    /* Copy original IF1 rdesc + append battery descriptor */
    memcpy(extended_rdesc, (void *)g_if1_report_desc, IF1_RDESC_LEN);
    for (int i = 0; i < (int)BATTERY_RDESC_LEN; i++)
        extended_rdesc[IF1_RDESC_LEN + i] = battery_rdesc[i];

    /* Patch wDescriptorLength in all SRAM descriptor copies */
    WDESCLEN_FS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_FS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_HS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_HS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_OS[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_OS[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
    WDESCLEN_STANDALONE[0] = (uint8_t)(EXTENDED_RDESC_LEN & 0xFF);
    WDESCLEN_STANDALONE[1] = (uint8_t)(EXTENDED_RDESC_LEN >> 8);
}

/* ── USB init hook (descriptor patching before enumeration) ──────────── */
/* "before" hook on usb_init: called before the original usb_init runs.
 * At this point crt0 has already copied .data → SRAM, so g_if1_report_desc
 * contains the original 171-byte IF1 descriptor. We populate extended_rdesc
 * and patch wDescriptorLength so they're ready when the host enumerates. */

void handle_usb_init(void) {
    /* Zero PATCH_SRAM — stock crt0 only initializes the firmware's own
     * .bss region, not ours.  SRAM survives soft reboot (flash + reset)
     * so statics from the previous run persist as garbage. */
    uint8_t *p = (uint8_t *)0x20002000;
    for (int i = 0; i < 1024; i++)
        p[i] = 0;

    patch_descriptors();

    /* Clear EP busy flags — USB peripheral state can survive a soft
     * reboot.  If the bootloader's USB session left a transfer
     * in-flight, EP2 starts wedged and consumer/mouse reports die. */
    volatile dongle_state_t *ds = (volatile dongle_state_t *)&g_dongle_state;
    ds->ep1_in_xfer_busy = 0;
    ds->ep2_in_xfer_busy = 0;
}

/* ── HID class setup handler (battery reporting) ─────────────────────── */
/* The stub saves {r0-r3,r12,lr} then does `bl handle_hid_setup`.
 * At the bl, r0 = udev (param_1), r1 = setup_pkt (param_2).
 *
 * Unlike the keyboard where setup_pkt is embedded in udev at +0x2CC,
 * the dongle's hid_class_setup_handler receives setup_pkt as a separate
 * pointer in r1 (second parameter). */

int handle_hid_setup(void *udev, uint8_t *setup_pkt) {
    uint8_t  bmReqType = setup_pkt[0];
    uint8_t  bRequest  = setup_pkt[1];
    uint16_t wValue    = setup_pkt[2] | ((uint16_t)setup_pkt[3] << 8);
    uint16_t wIndex    = setup_pkt[4] | ((uint16_t)setup_pkt[5] << 8);
    uint16_t wLength   = setup_pkt[6] | ((uint16_t)setup_pkt[7] << 8);

    /* Populate extended_rdesc + patch wDescriptorLength (idempotent).
     * Runs on every call so descriptors are ready before any GET_DESCRIPTOR
     * is served by the original handler.  The literal pool at 0x080073C8
     * has been patched at build time to point to extended_rdesc, and the
     * length cap at 0x080072C6/CA patched from 0xAB to 0xD9. */
    patch_descriptors();

    /* Only intercept GET_REPORT for IF1 Feature reports.
     * All other requests pass through to the original handler. */
    if (wIndex == 1 && bmReqType == USB_BMREQ_CLASS_IN && bRequest == HID_GET_REPORT) {
        /* GET_REPORT Feature ID 8 — dongle patch discovery.
         * Same format as keyboard 0xE7 but via HID Feature report. */
        if (wValue == WVALUE_FEATURE_REPORT(8)) {
            static uint8_t patch_rsp[16] __attribute__((aligned(4)));
            patch_rsp[0]  = 0x08;       /* Report ID 8 */
            patch_rsp[1]  = 0xCA;       /* magic hi */
            patch_rsp[2]  = 0xFE;       /* magic lo */
            patch_rsp[3]  = 0x01;       /* version */
            patch_rsp[4]  = 0x31;       /* caps lo: battery(0x01) + consumer_redirect(0x10) + speed_gate_nop(0x20) */
            patch_rsp[5]  = 0x00;       /* caps hi */
            /* name: "MONSDON\0" */
            patch_rsp[6]  = 'M'; patch_rsp[7]  = 'O'; patch_rsp[8]  = 'N'; patch_rsp[9]  = 'S';
            patch_rsp[10] = 'D'; patch_rsp[11] = 'O'; patch_rsp[12] = 'N'; patch_rsp[13] = '\0';
            uint16_t xfer_len = (wLength < 14) ? wLength : 14;
            usb_ep0_in_xfer_start(udev, patch_rsp, xfer_len);
            return 1;  /* intercepted */
        }

        /* GET_REPORT Feature ID 7 — battery report */
        if (wValue == WVALUE_FEATURE_REPORT(7)) {
            volatile dongle_state_t *ds = (volatile dongle_state_t *)&g_dongle_state;
            uint8_t bat_level = ds->kb_battery_info;
            uint8_t charging  = ds->kb_charging;

            /* Respond directly via EP0, capped at min(wLength, 3).
             * Report format: [ID=7] [battery 0-100] [charging 0/1] */
            static uint8_t bat_report[4] __attribute__((aligned(4)));
            bat_report[0] = 0x07;       /* Report ID 7 */
            bat_report[1] = bat_level;
            bat_report[2] = charging;
            uint16_t xfer_len = (wLength < 3) ? wLength : 3;
            usb_ep0_in_xfer_start(udev, bat_report, xfer_len);

            /* Queue EP2 Input report for rf_tx_handler trampoline.
             * Will be sent at the correct point in the main loop. */
            pending_bat_level = bat_level;
            pending_bat_charging = charging;
            battery_ep2_pending = 1;

            return 1;  /* intercepted */
        }
    }

    return 0;  /* passthrough to original handler */
}

/* ── RF packet dispatch hook (battery change detection) ──────────────────
 * "before" hook on rf_packet_dispatch: runs every SPI cycle.
 *
 * Consumer reports now flow through sub=3 natively: the keyboard hook
 * copies encoder data to g_dongle_consumer_buf (sub=3 source buffer),
 * the stock rf_packet_dispatch sets consumer_ready, and rf_tx_handler
 * sends it on EP2 (binary-patched from EP1 in hooks.py).
 *
 * Battery notifications: compares battery/charging values against cached
 * copies.  If changed, sets a pending flag — the actual EP2 send happens
 * in handle_battery_ep2_send (called from rf_tx_handler trampoline). */

void handle_rf_dispatch(void) {
    volatile dongle_state_t *ds = (volatile dongle_state_t *)&g_dongle_state;

    /* ── Battery change detection ─────────────────────────────────────── */
    static uint8_t prev_inited;
    static uint8_t prev_battery;
    static uint8_t prev_charging;

    uint8_t bat = ds->kb_battery_info;
    uint8_t chg = ds->kb_charging;

    if (!prev_inited || bat != prev_battery || chg != prev_charging) {
        prev_inited = 1;
        prev_battery = bat;
        prev_charging = chg;
        pending_bat_level = bat;
        pending_bat_charging = chg;
        battery_ep2_pending = 1;
    }
}

/* ── Battery EP2 send (called from rf_tx_handler trampoline) ─────────────
 * Runs inside rf_tx_handler at the correct point in the main loop,
 * following the stock EP2 contract: check busy → set busy → send.
 * If EP2 is busy, the pending flag stays set and retries next cycle. */

void handle_battery_ep2_send(void) {
    if (!battery_ep2_pending)
        return;

    volatile dongle_state_t *ds = (volatile dongle_state_t *)&g_dongle_state;
    if (ds->ep2_in_xfer_busy)
        return;  /* retry next cycle */

    ds->ep2_in_xfer_busy = 1;
    battery_ep2_pending = 0;

    volatile uint8_t *ep2_buf = (volatile uint8_t *)g_ep2_report_buf;
    ep2_buf[0] = 0x07;
    ep2_buf[1] = pending_bat_level;
    ep2_buf[2] = pending_bat_charging;
    usb_otg_in_ep_xfer_start(g_usb_device, 0x82, (void *)ep2_buf, 3);
}
