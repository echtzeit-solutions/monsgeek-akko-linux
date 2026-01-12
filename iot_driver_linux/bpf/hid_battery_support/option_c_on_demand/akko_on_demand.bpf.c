// SPDX-License-Identifier: GPL-2.0-only
/*
 * HID-BPF driver for Akko/MonsGeek 2.4GHz keyboard battery integration
 * Option C: On-demand F7 refresh triggered by UPower/userspace reads
 *
 * This version sends F7 refresh commands synchronously when the battery
 * is read, with a configurable throttle interval. This eliminates the
 * need for a periodic refresh daemon while ensuring fresh battery data.
 *
 * Key features:
 * - F7 sent on-demand when battery is read (if throttle expired)
 * - Configurable throttle interval via BPF map
 * - No userspace daemon needed after initial load
 * - Standards-compliant: battery refreshes only when queried
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

/* Default throttle: 10 minutes in nanoseconds */
#define DEFAULT_THROTTLE_NS (600ULL * 1000000000ULL)

/* Device configuration - target keyboard interface */
HID_BPF_CONFIG(
    HID_DEVICE(BUS_USB, HID_GROUP_GENERIC, VID_AKKO, PID_DONGLE)
);

/*
 * Configuration map - throttle interval in nanoseconds
 * Set by loader, can be updated at runtime via bpftool
 */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, int);
    __type(value, __u64);
} config_map SEC(".maps");

/*
 * State map - tracks last F7 time
 */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, int);
    __type(value, __u64);
} state_map SEC(".maps");

/*
 * Probe function - attach to keyboard interface (05 01 09 06)
 */
SEC("syscall")
int probe(struct hid_bpf_probe_args *ctx)
{
    /* Match keyboard interface: Usage Page 0x01 (Generic Desktop), Usage 0x06 (Keyboard) */
    if (ctx->rdesc_size >= 4 &&
        ctx->rdesc[0] == 0x05 &&
        ctx->rdesc[1] == 0x01 &&
        ctx->rdesc[2] == 0x09 &&
        ctx->rdesc[3] == 0x06) {
        ctx->retval = 0;
        return 0;
    }

    ctx->retval = -EINVAL;
    return 0;
}

/*
 * Battery Feature Report descriptor to append to keyboard descriptor.
 * Uses Feature report (B1) so kernel polls it via GET_FEATURE.
 */
static const __u8 battery_feature_desc[] = {
    /* Battery application collection */
    0x05, 0x01,        /* Usage Page (Generic Desktop) */
    0x09, 0x06,        /* Usage (Keyboard) - same as main for association */
    0xA1, 0x01,        /* Collection (Application) */
    0x85, BATTERY_REPORT_ID,  /*   Report ID (5) */

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
int BPF_PROG(akko_on_demand_rdesc_fixup, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 128);

    if (!data)
        return 0;

    /* Verify this is the keyboard interface (05 01 09 06) */
    if (data[0] != 0x05 || data[1] != 0x01 || data[2] != 0x09 || data[3] != 0x06)
        return 0;

    /* Get original descriptor size from hctx->size (retval union member) */
    int orig_size = hctx->size;
    if (orig_size <= 0 || orig_size > 100)
        return 0;  /* Invalid or too large */

    bpf_printk("akko_on_demand: appending battery to keyboard, orig=%d", orig_size);

    /* Append battery Feature report descriptor after the original */
    __builtin_memcpy(data + orig_size, battery_feature_desc, sizeof(battery_feature_desc));

    int new_size = orig_size + sizeof(battery_feature_desc);
    bpf_printk("akko_on_demand: new descriptor size = %d bytes", new_size);

    /* Initialize state map */
    int key = 0;
    __u64 initial_time = 0;
    bpf_map_update_elem(&state_map, &key, &initial_time, BPF_ANY);

    /* Initialize config with default throttle if not set */
    __u64 *throttle = bpf_map_lookup_elem(&config_map, &key);
    if (!throttle || *throttle == 0) {
        __u64 default_throttle = DEFAULT_THROTTLE_NS;
        bpf_map_update_elem(&config_map, &key, &default_throttle, BPF_ANY);
    }

    return new_size;
}

/*
 * HW request handler - fixes Report ID quirk and triggers on-demand F7 refresh
 *
 * This hook is called AFTER the firmware responds to GET_FEATURE.
 * If the throttle has expired, we:
 * 1. Send F7 SET_FEATURE to refresh dongle's battery cache
 * 2. Send GET_FEATURE Report 5 to get fresh data
 * 3. Copy fresh data over the stale response
 */
SEC(HID_BPF_HW_REQUEST_SLEEPABLE)
int BPF_PROG(akko_on_demand_hw_request, struct hid_bpf_ctx *hctx)
{
    int key = 0;

    /* Log hctx details for debugging */
    bpf_printk("akko_on_demand: hw_request size=%d alloc=%d", hctx->size, hctx->allocated_size);

    /* Need at least 1 byte to check report ID */
    if (hctx->size < 1) {
        return 0;
    }

    __u8 *data = hid_bpf_get_data(hctx, 0, 4);

    if (!data) {
        bpf_printk("akko_on_demand: hw_request get_data(4) returned NULL");
        return 0;
    }

    /* Log the raw data to understand the request format */
    bpf_printk("akko_on_demand: hw_request buf: %02x %02x %02x %02x", data[0], data[1], data[2], data[3]);

    /*
     * This hook fires BEFORE the request goes to hardware.
     * For GET_FEATURE requests, data[0] contains the report ID being requested.
     * Check if this is a battery (Report 5) request.
     */
    __u8 report_id = data[0];

    /* Only intercept battery report requests */
    if (report_id != BATTERY_REPORT_ID && report_id != 0x00) {
        return 0;
    }

    bpf_printk("akko_on_demand: detected battery report request (report_id=%02x)", report_id);

    /* Check if we need F7 refresh */
    __u64 *last_f7 = bpf_map_lookup_elem(&state_map, &key);
    __u64 *throttle = bpf_map_lookup_elem(&config_map, &key);

    if (!last_f7 || !throttle)
        return 0;

    __u64 now = bpf_ktime_get_ns();
    __u64 elapsed = now - *last_f7;

    if (elapsed <= *throttle) {
        /* Throttle not expired, let request proceed with cached data */
        bpf_printk("akko_on_demand: throttle active (%llu sec ago)", elapsed / 1000000000ULL);
        return 0;
    }

    /*
     * Throttle expired - send F7 refresh BEFORE the battery request.
     *
     * NOTE: We cannot call hid_bpf_hw_request() from within the hw_request hook
     * because the kernel has nested call protection (returns -EDEADLOCK/-EINVAL).
     *
     * F7 commands must go to the VENDOR interface (0xEF), not the keyboard
     * interface (0xED) where we're attached. The vendor interface is typically
     * hid_id = keyboard_hid_id + 2.
     */
    unsigned int keyboard_hid_id = hctx->hid->id;
    unsigned int vendor_hid_id = keyboard_hid_id + 2;  /* 0xED -> 0xEF */
    bpf_printk("akko_on_demand: throttle expired, kb_hid=%u vendor_hid=%u",
               keyboard_hid_id, vendor_hid_id);

    /* Allocate a context for the VENDOR interface to send F7 */
    struct hid_bpf_ctx *new_ctx = hid_bpf_allocate_context(vendor_hid_id);
    if (!new_ctx) {
        bpf_printk("akko_on_demand: failed to allocate context for vendor hid_id=%u", vendor_hid_id);
        bpf_map_update_elem(&state_map, &key, &now, BPF_ANY);
        return 0;
    }
    bpf_printk("akko_on_demand: allocated vendor context=%p", new_ctx);

    /* Send F7 with the fresh context - vendor interface uses 64-byte reports */
    __u8 f7_buf[64] = {0};
    f7_buf[0] = 0xF7;  /* F7 = refresh command (no report ID prefix for vendor interface) */

    int ret = hid_bpf_hw_request(new_ctx, f7_buf, sizeof(f7_buf),
                                  HID_FEATURE_REPORT, HID_REQ_SET_REPORT);
    bpf_printk("akko_on_demand: F7 hid_bpf_hw_request ret=%d", ret);

    /* Release the allocated context */
    hid_bpf_release_context(new_ctx);

    /* Update last F7 timestamp regardless of result to prevent retry flood */
    bpf_map_update_elem(&state_map, &key, &now, BPF_ANY);

    /* Return 0 to let the original GET_FEATURE request proceed */
    return 0;
}

/*
 * Device event handler - not used for keyboard interface
 */
SEC(HID_BPF_DEVICE_EVENT)
int BPF_PROG(akko_on_demand_event, struct hid_bpf_ctx *hctx)
{
    return 0;
}

/* Register HID-BPF operations */
HID_BPF_OPS(akko_on_demand) = {
    .hid_device_event = (void *)akko_on_demand_event,
    .hid_rdesc_fixup = (void *)akko_on_demand_rdesc_fixup,
    .hid_hw_request = (void *)akko_on_demand_hw_request,
};

char _license[] SEC("license") = "GPL";
