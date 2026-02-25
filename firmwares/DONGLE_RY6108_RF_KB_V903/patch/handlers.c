/*
 * Dongle patch handlers — battery HID over USB.
 *
 * The dongle already caches the keyboard's battery level and charging status
 * from RF packets (dongle_state.kb_battery_info/kb_charging).  This patch
 * exposes them as a standard HID battery via IF1's report descriptor and
 * GET_REPORT interception, with proactive Input report push on changes.
 *
 * Three hooks:
 *   1. "before" hook on usb_init — populates extended_rdesc + patches
 *      wDescriptorLength before USB enumeration starts.
 *   2. "filter" hook on hid_class_setup_handler — intercepts GET_REPORT
 *      Feature ID 7 for battery data.
 *   3. "before" hook on rf_packet_dispatch — detects battery/charging
 *      changes and pushes HID Input reports on EP2 interrupt endpoint.
 *      Linux kernel's hidinput_update_battery() picks these up.
 *
 * Convention (filter mode):
 *   return 0     = passthrough to original firmware handler
 *   return non-0 = intercepted (original handler skipped)
 */

#include <stdint.h>
#include "fw_dongle.h"
#include "hid_desc.h"

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
    patch_descriptors();
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

    /* Only intercept GET_REPORT for IF1 battery Feature report.
     * All other requests pass through to the original handler. */
    if (wIndex == 1 && bmReqType == 0xA1 && bRequest == 0x01) {
        /* GET_REPORT — wValue = (report_type << 8) | report_id
         * Feature report type = 3, Report ID = 7 → wValue = 0x0307 */
        if (wValue == 0x0307) {
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

            /* Also push Input report on EP2 so kernel's event chain fires
             * (hidinput_update_battery → hidinput_update_battery_charge_status).
             * Use g_ep2_report_buf as the transmit buffer (same as firmware). */
            volatile uint8_t *ep2_buf = (volatile uint8_t *)g_ep2_report_buf;
            ep2_buf[0] = 0x07;
            ep2_buf[1] = bat_level;
            ep2_buf[2] = charging;
            usb_otg_in_ep_xfer_start(g_usb_device, 0x82, (void *)ep2_buf, 3);

            return 1;  /* intercepted */
        }
    }

    return 0;  /* passthrough to original handler */
}

/* ── RF packet dispatch hook (proactive battery notifications) ─────── */
/* "before" hook on rf_packet_dispatch: runs every SPI cycle.
 * Compares current battery/charging values against cached copies.
 * If either changed, pushes a HID Input report on EP2 (interrupt IN).
 * The Linux kernel's hidinput_update_battery() processes these
 * automatically, updating power_supply without host polling.
 *
 * One SPI-cycle delay (µs) between the RF packet updating dongle_state
 * and our detection — negligible for battery-level changes. */

void handle_rf_dispatch(void) {
    /* All statics are in .bss (zero-initialized).
     * prev_inited starts as 0; first call always sends a report. */
    static uint8_t prev_inited;
    static uint8_t prev_battery;
    static uint8_t prev_charging;

    volatile dongle_state_t *ds = (volatile dongle_state_t *)&g_dongle_state;
    uint8_t bat = ds->kb_battery_info;
    uint8_t chg = ds->kb_charging;

    if (!prev_inited || bat != prev_battery || chg != prev_charging) {
        prev_inited = 1;
        prev_battery = bat;
        prev_charging = chg;

        /* Push Input report on EP2 (interrupt IN endpoint 0x82).
         * Use a separate static buffer to avoid races with keyboard
         * HID reports that also use EP2. */
        static uint8_t bat_input[4] __attribute__((aligned(4)));
        bat_input[0] = 0x07;       /* Report ID 7 */
        bat_input[1] = bat;
        bat_input[2] = chg;
        usb_otg_in_ep_xfer_start(g_usb_device, 0x82, bat_input, 3);
    }
}
