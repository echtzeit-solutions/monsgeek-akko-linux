// SPDX-License-Identifier: GPL-2.0-only
/*
 * HID-BPF driver for Akko/MonsGeek 2.4GHz keyboard battery integration
 * Option B: Vendor interface battery with F7 refresh from loader
 *
 * This approach attaches to the vendor interface (00DA) and exposes battery.
 * The loader sends periodic F7 commands to refresh battery data.
 *
 * Note: Work queue approach (bpf_wq) caused kernel verifier crash on 6.17,
 * so we use the simpler loader-based F7 refresh instead.
 *
 * Dongle: VID 0x3151 / PID 0x5038
 */

#include "vmlinux.h"
#include "hid_bpf.h"
#include "hid_bpf_helpers.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

#define VID_AKKO   0x3151
#define PID_DONGLE 0x5038

/* Battery Feature Report ID */
#define BATTERY_REPORT_ID 0x05

/* Device configuration */
HID_BPF_CONFIG(
    HID_DEVICE(BUS_USB, HID_GROUP_GENERIC, VID_AKKO, PID_DONGLE)
);

/*
 * Probe function - attach to vendor interface (00DA)
 * Vendor descriptor starts with: 06 FF FF (Usage Page 0xFFFF)
 */
SEC("syscall")
int probe(struct hid_bpf_probe_args *ctx)
{
    unsigned int size = ctx->rdesc_size;

    /*
     * Match vendor interface: starts with Usage Page Vendor (06 FF FF)
     * and is around 20-24 bytes (the vendor interface descriptor).
     */
    if (size >= 18 && size <= 30 &&
        ctx->rdesc[0] == 0x06 &&
        ctx->rdesc[1] == 0xFF &&
        ctx->rdesc[2] == 0xFF) {
        /* Found vendor interface */
        ctx->retval = 0;
        return 0;
    }

    ctx->retval = -EINVAL;
    return 0;
}

/*
 * Battery descriptor with Input report + dummy key
 *
 * The kernel only creates power_supply when there's an input device.
 * We use Generic Desktop/Keyboard Collection (not Vendor) so the kernel
 * creates an input device, and Battery Strength as Input report.
 *
 * Based on working version that successfully creates power_supply.
 */
static const __u8 battery_rdesc[] = {
    0x05, 0x01,        /* Usage Page (Generic Desktop) */
    0x09, 0x06,        /* Usage (Keyboard) */
    0xA1, 0x01,        /* Collection (Application) */
    0x85, BATTERY_REPORT_ID, /*   Report ID (5) */

    /* Battery strength - kernel picks this up for power_supply */
    0x05, 0x06,        /*   Usage Page (Generic Device Controls) */
    0x09, 0x20,        /*   Usage (Battery Strength) */
    0x15, 0x00,        /*   Logical Minimum (0) */
    0x26, 0x64, 0x00,  /*   Logical Maximum (100) */
    0x75, 0x08,        /*   Report Size (8) */
    0x95, 0x01,        /*   Report Count (1) */
    0x81, 0x02,        /*   Input (Data,Var,Abs) */

    /* Dummy modifier key to ensure input device creation */
    0x05, 0x07,        /*   Usage Page (Keyboard) */
    0x19, 0xE0,        /*   Usage Minimum (Left Control) */
    0x29, 0xE0,        /*   Usage Maximum (Left Control) */
    0x15, 0x00,        /*   Logical Minimum (0) */
    0x25, 0x01,        /*   Logical Maximum (1) */
    0x75, 0x01,        /*   Report Size (1) */
    0x95, 0x01,        /*   Report Count (1) */
    0x81, 0x02,        /*   Input (Data,Var,Abs) */

    /* Padding to byte boundary */
    0x75, 0x07,        /*   Report Size (7) */
    0x95, 0x01,        /*   Report Count (1) */
    0x81, 0x01,        /*   Input (Const) */

    0xC0               /* End Collection */
};

/*
 * Report descriptor fixup - replaces vendor descriptor with battery-enabled version
 */
SEC(HID_BPF_RDESC_FIXUP)
int BPF_PROG(akko_bidir_rdesc_fixup, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 64);
    if (!data)
        return 0;

    /* Verify this is the vendor interface */
    if (data[0] != 0x06 || data[1] != 0xFF || data[2] != 0xFF) {
        return 0;
    }

    bpf_printk("akko_bidir: replacing vendor descriptor with battery-enabled version");

    /* Replace entire descriptor with our battery-enabled version */
    __builtin_memcpy(data, battery_rdesc, sizeof(battery_rdesc));

    return sizeof(battery_rdesc);
}

/*
 * HW request handler - fixes Report ID quirk
 *
 * The dongle firmware returns Report ID 0x00 instead of 0x05 for Feature
 * Report requests. We fix this so the kernel correctly processes the data.
 */
SEC(HID_BPF_HW_REQUEST)
int BPF_PROG(akko_bidir_hw_request, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 8);
    if (!data)
        return 0;

    /*
     * Fix Report ID quirk: firmware returns 0x00 instead of 0x05
     * Check that byte[1] looks like battery percentage (0-100)
     */
    if (data[0] == 0x00 && data[1] <= 100) {
        bpf_printk("akko_bidir: fixing report_id 0->5, battery=%d%%", data[1]);
        data[0] = BATTERY_REPORT_ID;
    }

    return 0;
}

/*
 * Device event handler - not used but required for struct_ops
 */
SEC(HID_BPF_DEVICE_EVENT)
int BPF_PROG(akko_bidir_event, struct hid_bpf_ctx *hctx)
{
    return 0;
}

/* Register HID-BPF operations */
HID_BPF_OPS(akko_bidirectional) = {
    .hid_device_event = (void *)akko_bidir_event,
    .hid_rdesc_fixup = (void *)akko_bidir_rdesc_fixup,
    .hid_hw_request = (void *)akko_bidir_hw_request,
};

char _license[] SEC("license") = "GPL";
