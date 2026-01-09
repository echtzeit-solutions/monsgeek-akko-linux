// SPDX-License-Identifier: GPL-2.0-only
/*
 * HID-BPF driver for Akko/MonsGeek 2.4GHz dongle battery integration
 *
 * Translates vendor HID reports (Usage Page 0xFFFF) to standard
 * HID Battery System reports (Usage Page 0x85) so the kernel
 * automatically creates /sys/class/power_supply/ entries.
 *
 * Dongle: VID 0x3151 / PID 0x5038
 * Report ID 0x05: [battery%, charging, online, ...]
 */

#include "vmlinux.h"
#include "hid_bpf.h"
#include "hid_bpf_helpers.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

#define VID_AKKO   0x3151
#define PID_DONGLE 0x5038

/* Device configuration - match the 2.4GHz dongle */
HID_BPF_CONFIG(
    HID_DEVICE(BUS_USB, HID_GROUP_GENERIC, VID_AKKO, PID_DONGLE)
);

/*
 * Probe function - called to check if we should attach to this device.
 * We only want the vendor Feature report interface for battery data.
 *
 * The dongle has 3 HID interfaces:
 *   00B4: Keyboard (Usage Page 0x01 Generic Desktop)
 *   00B5: Multi-function with vendor Input report (0x81)
 *   00B6: Pure vendor Feature report (0xB1) - this is the battery interface
 *
 * We target 00B6: 20 bytes, starts with "06 FF FF", has Feature report.
 */
SEC("syscall")
int probe(struct hid_bpf_probe_args *ctx)
{
    unsigned int size = ctx->rdesc_size;

    /*
     * Match interface 00B6: small descriptor (20 bytes) starting with
     * vendor page (06 FF FF) and containing Feature report (B1).
     * This is the interface the kernel will poll for battery status.
     */
    if (size >= 20 && size <= 24 &&
        ctx->rdesc[0] == 0x06 &&
        ctx->rdesc[1] == 0xFF &&
        ctx->rdesc[2] == 0xFF) {
        /* Found the vendor Feature report interface */
        ctx->retval = 0;
        return 0;
    }

    /* Not the battery interface */
    ctx->retval = -EINVAL;
    return 0;
}

/*
 * Device event handler - called for input reports.
 */
SEC(HID_BPF_DEVICE_EVENT)
int BPF_PROG(akko_dongle_event, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 8);
    if (!data)
        return 0;

    /* Log battery data from input reports */
    if (data[0] == 0x05 && data[1] <= 100) {
        bpf_printk("akko_event: bat=%d%% online=%d", data[1], data[3]);
    }

    return 0;
}

/*
 * Raw HID request handler - called for Feature report polling.
 *
 * The dongle has a firmware bug: when we request Feature Report ID 5,
 * it returns Report ID 0 instead of 5. The kernel battery code uses
 * hid_hw_raw_request() which triggers this hook.
 *
 * Original response: [0x00, battery%, charging, online, ...]
 * Fixed response:    [0x05, battery%, charging, online, ...]
 */
SEC(HID_BPF_HW_REQUEST)
int BPF_PROG(akko_hw_request, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 8);
    if (!data)
        return 0;

    /*
     * Fix Report ID: dongle returns 0x00 for Feature Report 5.
     * Check for battery-like data pattern to avoid breaking other reports.
     */
    if (data[0] == 0x00 && data[1] <= 100 && (data[3] == 0 || data[3] == 1)) {
        bpf_printk("akko_hw_req: fixing report_id 0->5, bat=%d%%", data[1]);
        data[0] = 0x05;
    }

    return 0;
}

/*
 * Report descriptor fixup - modifies the HID descriptor at device probe time.
 * Replaces vendor Usage Page (0xFFFF) with Battery System Usage Page (0x85).
 *
 * Original vendor descriptor (at ~offset 0x90):
 *   06 FF FF   Usage Page (Vendor 0xFFFF)
 *   09 01      Usage (0x01)
 *   A1 01      Collection (Application)
 *   85 05      Report ID (5)
 *   09 01      Usage (0x01)
 *   15 00      Logical Minimum (0)
 *   26 FF 00   Logical Maximum (255)
 *   75 08      Report Size (8)
 *   95 1F      Report Count (31)
 *   81 02      Input (Data,Var,Abs)
 *   C0         End Collection
 *
 * New battery descriptor:
 *   05 85      Usage Page (Battery System)
 *   09 2C      Usage (Battery)
 *   A1 01      Collection (Application)
 *   85 05      Report ID (5)
 *   09 29      Usage (Remaining Capacity)
 *   15 00      Logical Minimum (0)
 *   26 64 00   Logical Maximum (100)
 *   75 08      Report Size (8)
 *   95 01      Report Count (1)
 *   B1 02      Feature (Data,Var,Abs)
 *   09 44      Usage (Charging)
 *   25 01      Logical Maximum (1)
 *   B1 02      Feature (Data,Var,Abs)
 *   C0         End Collection
 */

/*
 * Option C: Pure Feature report descriptor for battery.
 *
 * Attempt to create power_supply without needing an input device.
 * Uses Generic Device Controls page (0x06) with Battery Strength usage (0x20)
 * as a Feature report (0xB1) that the kernel polls via hid_hw_raw_request().
 *
 * Data format from dongle (Report ID 0x05):
 *   Byte 0: Report ID (0x05, but dongle returns 0x00 - fixed by hw_request hook)
 *   Byte 1: Battery % (0-100)
 *   Byte 2: Charging (0/1)
 *   Byte 3: Online (0/1)
 */
static const __u8 battery_rdesc[] = {
    /* Application collection - using Generic Desktop for device class */
    0x05, 0x01,        /* Usage Page (Generic Desktop) */
    0x09, 0x06,        /* Usage (Keyboard) - device class hint */
    0xA1, 0x01,        /* Collection (Application) */
    0x85, 0x05,        /*   Report ID (5) */

    /* Battery strength as Feature report */
    0x05, 0x06,        /*   Usage Page (Generic Device Controls) */
    0x09, 0x20,        /*   Usage (Battery Strength) */
    0x15, 0x00,        /*   Logical Minimum (0) */
    0x26, 0x64, 0x00,  /*   Logical Maximum (100) */
    0x75, 0x08,        /*   Report Size (8) */
    0x95, 0x01,        /*   Report Count (1) */
    0xB1, 0x02,        /*   Feature (Data,Var,Abs) - polled via get_feature */

    0xC0               /* End Collection */
};

SEC(HID_BPF_RDESC_FIXUP)
int BPF_PROG(akko_rdesc_fixup, struct hid_bpf_ctx *hctx)
{
    /*
     * Interface 00B6 has vendor page at offset 0, so we replace from the start.
     * Only need enough bytes for our battery descriptor (32 bytes).
     */
    __u8 *data = hid_bpf_get_data(hctx, 0, 64);
    if (!data)
        return 0;

    /*
     * Verify this is the vendor interface (06 FF FF at offset 0).
     * The probe should have already filtered, but double-check.
     */
    if (data[0] != 0x06 || data[1] != 0xFF || data[2] != 0xFF) {
        return 0;
    }

    bpf_printk("akko_rdesc: replacing vendor descriptor with battery page");

    /* Copy battery descriptor at offset 0, replacing the vendor descriptor */
    __builtin_memcpy(data, battery_rdesc, sizeof(battery_rdesc));

    bpf_printk("akko_rdesc: new descriptor size = %d bytes",
               (int)sizeof(battery_rdesc));

    /* Return new descriptor size */
    return sizeof(battery_rdesc);
}

/* Register our HID-BPF operations */
HID_BPF_OPS(akko_dongle) = {
    .hid_device_event = (void *)akko_dongle_event,
    .hid_rdesc_fixup = (void *)akko_rdesc_fixup,
    .hid_hw_request = (void *)akko_hw_request,
};

char _license[] SEC("license") = "GPL";
