// SPDX-License-Identifier: GPL-2.0-only
/*
 * HID-BPF driver for Akko/MonsGeek 2.4GHz keyboard battery integration
 * Option B WQ: Experimental bpf_wq-based automatic F7 refresh
 *
 * This experimental version attempts to use BPF work queues (bpf_wq)
 * to automatically send F7 commands from within BPF, eliminating the
 * need for userspace polling.
 *
 * bpf_wq was merged in kernel 6.10. See:
 * https://eunomia.dev/tutorials/features/bpf_wq/
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

/* F7 refresh interval in nanoseconds (30 seconds) */
#define F7_REFRESH_INTERVAL_NS (30ULL * 1000000000ULL)

/* Device configuration */
HID_BPF_CONFIG(
    HID_DEVICE(BUS_USB, HID_GROUP_GENERIC, VID_AKKO, PID_DONGLE)
);

/*
 * State structure - bpf_wq must be embedded in map value struct
 * This is the correct pattern per kernel docs and eunomia tutorial
 */
struct wq_state {
    struct bpf_wq work;           /* Work queue handle - must be in struct */
    __u64 last_f7_time_ns;        /* Last F7 timestamp */
    __u32 hid_id;                 /* Device ID for context allocation */
    __u8  cached_battery;         /* Cached battery % */
    __u8  f7_pending;             /* Async F7 in flight */
    __u8  initialized;            /* Work queue initialized flag */
    __u8  _pad;
};

/* State map - array with embedded bpf_wq in struct */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, int);
    __type(value, struct wq_state);
} wq_state_map SEC(".maps");

/*
 * Work queue callback - sends F7 command asynchronously
 *
 * Signature: int callback(void *map, int *key, void *value)
 * - map: pointer to the map containing the workqueue
 * - key: pointer to the map key
 * - value: pointer to the map entry (struct wq_state)
 */
static int f7_refresh_callback(void *map, int *key, void *value)
{
    struct wq_state *state = value;
    struct hid_bpf_ctx *ctx;
    __u8 f7_cmd[64] = {0};
    int ret;

    if (!state)
        return 0;

    /* Clear pending flag */
    state->f7_pending = 0;

    /* Allocate HID context for this device */
    ctx = hid_bpf_allocate_context(state->hid_id);
    if (!ctx) {
        bpf_printk("akko_wq: failed to allocate context for F7");
        return 0;
    }

    /* Prepare F7 command: [Report ID 0, F7, zeros...] */
    f7_cmd[0] = 0x00;
    f7_cmd[1] = 0xF7;

    bpf_printk("akko_wq: sending F7 refresh command");

    /* Send SET_FEATURE with F7 command */
    ret = hid_bpf_hw_request(ctx, f7_cmd, sizeof(f7_cmd),
                             HID_FEATURE_REPORT, HID_REQ_SET_REPORT);
    if (ret < 0) {
        bpf_printk("akko_wq: F7 hw_request failed: %d", ret);
    } else {
        /* Update last F7 time on success */
        state->last_f7_time_ns = bpf_ktime_get_ns();
        bpf_printk("akko_wq: F7 sent successfully");
    }

    hid_bpf_release_context(ctx);
    return 0;
}

/*
 * Probe function - attach to vendor interface (00DA)
 */
SEC("syscall")
int probe(struct hid_bpf_probe_args *ctx)
{
    unsigned int size = ctx->rdesc_size;

    if (size >= 18 && size <= 30 &&
        ctx->rdesc[0] == 0x06 &&
        ctx->rdesc[1] == 0xFF &&
        ctx->rdesc[2] == 0xFF) {
        ctx->retval = 0;
        return 0;
    }

    ctx->retval = -EINVAL;
    return 0;
}

/*
 * Battery descriptor (same as working version)
 */
static const __u8 battery_rdesc[] = {
    0x05, 0x01,        /* Usage Page (Generic Desktop) */
    0x09, 0x06,        /* Usage (Keyboard) */
    0xA1, 0x01,        /* Collection (Application) */
    0x85, BATTERY_REPORT_ID,

    /* Battery strength */
    0x05, 0x06,        /*   Usage Page (Generic Device Controls) */
    0x09, 0x20,        /*   Usage (Battery Strength) */
    0x15, 0x00,        /*   Logical Minimum (0) */
    0x26, 0x64, 0x00,  /*   Logical Maximum (100) */
    0x75, 0x08,        /*   Report Size (8) */
    0x95, 0x01,        /*   Report Count (1) */
    0x81, 0x02,        /*   Input (Data,Var,Abs) */

    /* Dummy modifier key */
    0x05, 0x07,        /*   Usage Page (Keyboard) */
    0x19, 0xE0,
    0x29, 0xE0,
    0x15, 0x00,
    0x25, 0x01,
    0x75, 0x01,
    0x95, 0x01,
    0x81, 0x02,

    /* Padding */
    0x75, 0x07,
    0x95, 0x01,
    0x81, 0x01,

    0xC0
};

/*
 * Report descriptor fixup
 */
SEC(HID_BPF_RDESC_FIXUP)
int BPF_PROG(akko_wq_rdesc_fixup, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 64);
    int key = 0;
    struct wq_state *state;
    int ret;

    if (!data)
        return 0;

    if (data[0] != 0x06 || data[1] != 0xFF || data[2] != 0xFF)
        return 0;

    bpf_printk("akko_wq: replacing vendor descriptor");

    __builtin_memcpy(data, battery_rdesc, sizeof(battery_rdesc));

    /* Initialize state and work queue */
    state = bpf_map_lookup_elem(&wq_state_map, &key);
    if (state && !state->initialized) {
        state->hid_id = hctx->hid->id;
        state->last_f7_time_ns = 0;
        state->f7_pending = 0;

        /* Initialize work queue - must pass pointer to bpf_wq in struct */
        ret = bpf_wq_init(&state->work, &wq_state_map, 0);
        if (ret == 0) {
            ret = bpf_wq_set_callback(&state->work, f7_refresh_callback, 0);
            if (ret == 0) {
                state->initialized = 1;
                bpf_printk("akko_wq: work queue initialized, hid_id=%u", state->hid_id);
            } else {
                bpf_printk("akko_wq: bpf_wq_set_callback failed: %d", ret);
            }
        } else {
            bpf_printk("akko_wq: bpf_wq_init failed: %d", ret);
        }
    }

    return sizeof(battery_rdesc);
}

/*
 * HW request handler - fixes Report ID and schedules F7 refresh
 */
SEC(HID_BPF_HW_REQUEST)
int BPF_PROG(akko_wq_hw_request, struct hid_bpf_ctx *hctx)
{
    __u8 *data = hid_bpf_get_data(hctx, 0, 8);
    int key = 0;
    struct wq_state *state;

    if (!data)
        return 0;

    /* Fix Report ID quirk */
    if (data[0] == 0x00 && data[1] <= 100) {
        bpf_printk("akko_wq: fixing report_id 0->5, battery=%d%%", data[1]);
        data[0] = BATTERY_REPORT_ID;

        /* Check if we need F7 refresh */
        state = bpf_map_lookup_elem(&wq_state_map, &key);
        if (state && state->initialized) {
            state->cached_battery = data[1];

            __u64 now = bpf_ktime_get_ns();
            __u64 elapsed = now - state->last_f7_time_ns;

            if (elapsed > F7_REFRESH_INTERVAL_NS && !state->f7_pending) {
                state->f7_pending = 1;
                int ret = bpf_wq_start(&state->work, 0);
                if (ret == 0) {
                    bpf_printk("akko_wq: scheduled F7 refresh");
                } else {
                    state->f7_pending = 0;
                    bpf_printk("akko_wq: bpf_wq_start failed: %d", ret);
                }
            }
        }
    }

    return 0;
}

/*
 * Device event handler
 */
SEC(HID_BPF_DEVICE_EVENT)
int BPF_PROG(akko_wq_event, struct hid_bpf_ctx *hctx)
{
    return 0;
}

/* Register HID-BPF operations */
HID_BPF_OPS(akko_wq) = {
    .hid_device_event = (void *)akko_wq_event,
    .hid_rdesc_fixup = (void *)akko_wq_rdesc_fixup,
    .hid_hw_request = (void *)akko_wq_hw_request,
};

char _license[] SEC("license") = "GPL";
