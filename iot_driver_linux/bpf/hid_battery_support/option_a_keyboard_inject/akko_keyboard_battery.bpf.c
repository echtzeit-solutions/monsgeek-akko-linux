// SPDX-License-Identifier: GPL-2.0-only
/*
 * HID-BPF driver for Akko/MonsGeek 2.4GHz keyboard battery integration
 * Option A: Inject battery Feature report into keyboard interface (RECOMMENDED)
 *
 * Key discovery: The dongle firmware responds to Feature Report ID 5 on ANY
 * interface with identical battery data. This means we just need to:
 * 1. Append a Battery Strength Feature report to the keyboard descriptor
 * 2. Fix the Report ID quirk (firmware returns 0x00 instead of 0x05)
 *
 * No BPF maps or userspace polling needed - the firmware handles everything!
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
 * Probe function - attach to keyboard interface (00D8)
 * Keyboard descriptor starts with: 05 01 09 06 (Generic Desktop, Keyboard)
 */
SEC("syscall")
int probe(struct hid_bpf_probe_args *ctx)
{
    unsigned int size = ctx->rdesc_size;

    /*
     * Match keyboard interface: starts with Usage Page Generic Desktop (05 01),
     * Usage Keyboard (09 06), and is around 60 bytes.
     */
    if (size >= 50 && size <= 70 &&
        ctx->rdesc[0] == 0x05 &&
        ctx->rdesc[1] == 0x01 &&
        ctx->rdesc[2] == 0x09 &&
        ctx->rdesc[3] == 0x06) {
        /* Found keyboard interface */
        ctx->retval = 0;
        return 0;
    }

    ctx->retval = -EINVAL;
    return 0;
}

/*
 * Battery Feature report descriptor to append to keyboard descriptor.
 *
 * This adds a separate Application Collection for battery reporting.
 * The kernel will poll Feature Report ID 5, and the firmware will
 * respond with battery data (works on any interface!).
 */
static const __u8 battery_feature_desc[] = {
    /* Battery application collection */
    0x05, 0x01,        /* Usage Page (Generic Desktop) */
    0x09, 0x06,        /* Usage (Keyboard) - same as main for association */
    0xA1, 0x01,        /* Collection (Application) */
    0x85, BATTERY_REPORT_ID, /*   Report ID (5) */

    /* Battery strength as Feature report */
    0x05, 0x06,        /*   Usage Page (Generic Device Controls) */
    0x09, 0x20,        /*   Usage (Battery Strength) */
    0x15, 0x00,        /*   Logical Minimum (0) */
    0x26, 0x64, 0x00,  /*   Logical Maximum (100) */
    0x75, 0x08,        /*   Report Size (8) */
    0x95, 0x01,        /*   Report Count (1) */
    0xB1, 0x02,        /*   Feature (Data,Var,Abs) */

    0xC0               /* End Collection */
};

/*
 * Report descriptor fixup - appends battery Feature report to keyboard descriptor
 */
SEC(HID_BPF_RDESC_FIXUP)
int BPF_PROG(akko_kb_rdesc_fixup, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 128);
    if (!data)
        return 0;

    /* Verify this is the keyboard interface */
    if (data[0] != 0x05 || data[1] != 0x01 ||
        data[2] != 0x09 || data[3] != 0x06) {
        return 0;
    }

    /* Get the current descriptor size */
    unsigned int orig_size = hctx->size;
    if (orig_size > 100) {
        /* Too large, might overflow our buffer */
        return 0;
    }

    bpf_printk("akko_kb: appending battery to keyboard desc, orig_size=%d", orig_size);

    /* Append battery Feature report descriptor */
    __builtin_memcpy(data + orig_size, battery_feature_desc, sizeof(battery_feature_desc));

    unsigned int new_size = orig_size + sizeof(battery_feature_desc);
    bpf_printk("akko_kb: new descriptor size = %d bytes", new_size);

    return new_size;
}

/*
 * HW request handler - fixes firmware Report ID quirk
 *
 * The dongle firmware returns Report ID 0x00 instead of 0x05 for
 * Feature Report requests. We fix this so the kernel correctly
 * processes the battery data.
 */
SEC(HID_BPF_HW_REQUEST)
int BPF_PROG(akko_kb_hw_request, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 8);
    if (!data)
        return 0;

    /*
     * Fix Report ID quirk: firmware returns 0x00 instead of 0x05
     * Check that byte[1] looks like battery percentage (0-100)
     */
    if (data[0] == 0x00 && data[1] <= 100) {
        bpf_printk("akko_kb: fixing report_id 0->5, battery=%d%%", data[1]);
        data[0] = BATTERY_REPORT_ID;
    }

    return 0;
}

/*
 * Device event handler - not used for keyboard interface
 */
SEC(HID_BPF_DEVICE_EVENT)
int BPF_PROG(akko_kb_event, struct hid_bpf_ctx *hctx)
{
    return 0;
}

/* Register HID-BPF operations */
HID_BPF_OPS(akko_keyboard_battery) = {
    .hid_device_event = (void *)akko_kb_event,
    .hid_rdesc_fixup = (void *)akko_kb_rdesc_fixup,
    .hid_hw_request = (void *)akko_kb_hw_request,
};

char _license[] SEC("license") = "GPL";
