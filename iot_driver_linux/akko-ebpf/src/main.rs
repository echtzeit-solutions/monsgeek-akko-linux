//! HID-BPF driver for Akko/MonsGeek 2.4GHz dongle battery integration
//! Option C: On-demand F7 refresh triggered by UPower/userspace reads
//!
//! This version sends F7 refresh commands when the battery is queried,
//! with a configurable throttle interval. Key insights:
//!
//! 1. The hw_request hook fires when kernel reads battery Feature report
//! 2. F7 must go to the VENDOR interface (hid_id + 2), not keyboard interface
//! 3. We allocate a fresh context to avoid nested call protection
//! 4. Throttle prevents excessive F7 commands
//!
//! Dongle: VID 0x3151 / PID 0x5038
//!
//! Limitations:
//! - Charging status is NOT available. The keyboard's HID protocol does not
//!   expose charging state - USB packet analysis confirmed the F7 response
//!   bytes are identical whether charger is connected or not. Only battery
//!   percentage is available. Power supply will always show "Discharging".

#![no_std]
#![no_main]
// BPF struct_ops callbacks receive raw pointers from kernel - always valid in this context
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use aya_ebpf::btf_maps::Array;
use aya_ebpf::macros::btf_map;
use aya_ebpf::programs::hid_bpf::{
    hid_bpf_ctx, AllocatedContext, HidBpfContext, HidClassRequest, HidReportType,
};

// =============================================================================
// Safe wrappers for BPF helpers
// =============================================================================

/// Get current kernel time in nanoseconds (safe wrapper).
#[inline(always)]
fn ktime_get_ns() -> u64 {
    // SAFETY: bpf_ktime_get_ns is always safe to call, returns monotonic time
    unsafe { aya_ebpf::helpers::bpf_ktime_get_ns() }
}

/// Safe wrapper for bpf_printk that hides the unsafe.
macro_rules! trace {
    ($($arg:tt)*) => {
        // SAFETY: bpf_printk is safe when given valid format string and matching args
        unsafe { aya_ebpf::helpers::bpf_printk!($($arg)*) }
    };
}

// =============================================================================
// Constants
// =============================================================================

/// Battery Feature Report ID
const BATTERY_REPORT_ID: u8 = 0x05;

/// Default throttle: 10 minutes in nanoseconds
const DEFAULT_THROTTLE_NS: u64 = 600 * 1_000_000_000;

/// Keyboard HID descriptor signature: Usage Page (Generic Desktop), Usage (Keyboard)
const KEYBOARD_SIGNATURE: [u8; 4] = [0x05, 0x01, 0x09, 0x06];

// =============================================================================
// BPF Maps
// =============================================================================

/// Configuration map - holds throttle interval in nanoseconds
#[btf_map]
static CONFIG_MAP: Array<u64, 1> = Array::new();

/// State map - holds last F7 timestamp in nanoseconds
#[btf_map]
static STATE_MAP: Array<u64, 1> = Array::new();

/// Vendor HID ID map - set by loader, holds the vendor interface hid_id
/// This avoids having to read hid_device.id from kernel struct (complex offset)
#[btf_map]
static VENDOR_HID_MAP: Array<u32, 1> = Array::new();

// =============================================================================
// struct_ops definitions
// =============================================================================

#[repr(C)]
struct hid_bpf_ops {
    hid_id: i32,
    flags: u32,
    _list: [usize; 2],
    hid_device_event: usize,
    hid_rdesc_fixup: usize,
    hid_hw_request: usize,
    hid_hw_output_report: usize,
    hdev: usize,
}

unsafe impl Sync for hid_bpf_ops {}

#[unsafe(link_section = ".struct_ops.link")]
#[unsafe(no_mangle)]
static akko_on_demand: hid_bpf_ops = hid_bpf_ops {
    hid_id: 0,
    flags: 0,
    _list: [0; 2],
    hid_device_event: 0,
    hid_rdesc_fixup: 0,
    hid_hw_request: 0,
    hid_hw_output_report: 0,
    hdev: 0,
};

// =============================================================================
// Battery descriptor
// =============================================================================

static BATTERY_FEATURE_DESC: [u8; 24] = [
    0x05, 0x01,             // Usage Page (Generic Desktop)
    0x09, 0x06,             // Usage (Keyboard)
    0xA1, 0x01,             // Collection (Application)
    0x85, BATTERY_REPORT_ID, // Report ID (5)
    0x05, 0x06,             // Usage Page (Generic Device Controls)
    0x09, 0x20,             // Usage (Battery Strength)
    0x15, 0x00,             // Logical Minimum (0)
    0x26, 0x64, 0x00,       // Logical Maximum (100)
    0x75, 0x08,             // Report Size (8)
    0x95, 0x01,             // Report Count (1)
    0xB1, 0x02,             // Feature (Data,Var,Abs)
    0xC0,                   // End Collection
];

// =============================================================================
// HID-BPF callbacks (safe code using Aya abstractions)
// =============================================================================

// Device event handler - not used
#[no_mangle]
#[link_section = "struct_ops/hid_device_event"]
pub extern "C" fn akko_on_demand_event(_ctx: *mut hid_bpf_ctx) -> i32 {
    0
}

// Report descriptor fixup - appends battery Feature report
#[no_mangle]
#[link_section = "struct_ops/hid_rdesc_fixup"]
pub extern "C" fn akko_on_demand_rdesc_fixup(ctx_ptr: *mut hid_bpf_ctx) -> i32 {
    // SAFETY: kernel passes valid context pointer
    let ctx = unsafe { HidBpfContext::new(ctx_ptr) };

    let Some(mut data) = ctx.data(0, 128) else {
        return 0;
    };

    // Verify keyboard interface (05 01 09 06)
    if !data.starts_with(&KEYBOARD_SIGNATURE) {
        return 0;
    }

    let orig_size = ctx.retval() as usize;
    if orig_size > 100 {
        return 0;
    }

    trace!(b"akko_ondemand: appending battery, orig=%d", orig_size as u32);

    // Append battery descriptor using safe copy
    if !data.copy_from_slice(orig_size, &BATTERY_FEATURE_DESC) {
        return 0;
    }

    // Initialize state map
    let _ = STATE_MAP.set(0, 0u64, 0);

    // Initialize config with default throttle if not set
    if let Some(&throttle) = CONFIG_MAP.get(0) {
        if throttle == 0 {
            let _ = CONFIG_MAP.set(0, DEFAULT_THROTTLE_NS, 0);
        }
    }

    let new_size = orig_size + BATTERY_FEATURE_DESC.len();
    trace!(b"akko_ondemand: new size = %d bytes", new_size as u32);

    new_size as i32
}

// HW request handler (sleepable) - sends F7 on-demand
#[no_mangle]
#[link_section = "struct_ops.s/hid_hw_request"]
pub extern "C" fn akko_on_demand_hw_request(ctx_ptr: *mut hid_bpf_ctx) -> i32 {
    // SAFETY: kernel passes valid context pointer
    let ctx = unsafe { HidBpfContext::new(ctx_ptr) };

    // Need at least 4 bytes for the request buffer
    if ctx.allocated_size() < 4 {
        return 0;
    }

    let Some(data) = ctx.data(0, 4) else {
        return 0;
    };

    // Safe bounds-checked access
    let Some(report_id) = data.get(0) else {
        return 0;
    };

    // Only handle battery report requests (Report ID 0 or 5)
    if report_id != 0x00 && report_id != BATTERY_REPORT_ID {
        return 0;
    }

    trace!(b"akko_ondemand: battery request, report_id=%d", report_id as u32);

    // Check throttle
    let Some(&last_f7) = STATE_MAP.get(0) else {
        return 0;
    };
    let Some(&throttle) = CONFIG_MAP.get(0) else {
        return 0;
    };

    let now = ktime_get_ns();
    let elapsed = now - last_f7;

    if elapsed <= throttle {
        trace!(b"akko_ondemand: throttle active (%d sec ago)", (elapsed / 1_000_000_000) as u32);
        return 0;
    }

    // Throttle expired - send F7 to vendor interface
    // Vendor hid_id is set by loader in VENDOR_HID_MAP
    let Some(&vendor_hid_id) = VENDOR_HID_MAP.get(0) else {
        trace!(b"akko_ondemand: vendor_hid_id not set in map");
        return 0;
    };

    if vendor_hid_id == 0 {
        trace!(b"akko_ondemand: vendor_hid_id is 0, not configured");
        return 0;
    }

    trace!(b"akko_ondemand: sending F7 to vendor=%d", vendor_hid_id);

    // RAII guard - context automatically released on drop (even on early return)
    let Some(vendor) = AllocatedContext::new(vendor_hid_id) else {
        trace!(b"akko_ondemand: failed to allocate vendor context");
        let _ = STATE_MAP.set(0, now, 0);
        return 0;
    };

    // Send F7 command (64-byte buffer, F7 at byte 0)
    let mut f7_buf: [u8; 64] = [0; 64];
    f7_buf[0] = 0xF7;

    let ret = vendor.hw_request(&mut f7_buf, HidReportType::Feature, HidClassRequest::SetReport);

    trace!(b"akko_ondemand: F7 ret=%d", ret);

    // Update timestamp
    let _ = STATE_MAP.set(0, now, 0);

    // vendor automatically released here via Drop
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
