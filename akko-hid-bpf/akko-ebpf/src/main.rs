//! HID-BPF driver for Akko/MonsGeek 2.4GHz dongle battery integration
//! On-demand F7 refresh triggered by UPower/userspace reads
//!
//! This version sends F7 refresh commands when the battery is queried.
//! Key insights:
//!
//! 1. The hw_request hook fires when kernel reads battery Feature report
//! 2. F7 must go to the VENDOR interface (hid_id + 2), not keyboard interface
//! 3. We allocate a fresh context to avoid nested call protection
//! 4. F7 queries do NOT wake the keyboard (idle flag stays set)
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
// GPL License - Required for BPF programs using GPL-only kernel helpers/kfuncs
// =============================================================================

#[link_section = "license"]
#[used]
static LICENSE: [u8; 4] = *b"GPL\0";

// =============================================================================
// Safe wrappers for BPF helpers
// =============================================================================

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

/// BPF revision number - INCREMENT THIS when making changes to help identify loaded version
const REVISION: u32 = 2;

/// Battery Feature Report ID
const BATTERY_REPORT_ID: u8 = 0x05;

/// Keyboard HID descriptor signature: Usage Page (Generic Desktop), Usage (Keyboard)
const KEYBOARD_SIGNATURE: [u8; 4] = [0x05, 0x01, 0x09, 0x06];

// =============================================================================
// BPF Maps
// =============================================================================

/// Vendor HID ID map - set by loader, holds the vendor interface hid_id
/// This avoids having to read hid_device.id from kernel struct (complex offset)
#[btf_map]
static VENDOR_HID_MAP: Array<u32, 1> = Array::new();

// =============================================================================
// struct_ops definitions
// =============================================================================

/// Kernel's list_head struct for linked lists.
/// Must have exact name for BTF matching.
#[repr(C)]
struct list_head {
    next: *mut list_head,
    prev: *mut list_head,
}

/// Opaque hid_device pointer type for BTF matching.
#[repr(C)]
struct hid_device {
    _opaque: u8,
}

/// Matches kernel's struct hid_bpf_ops layout.
/// Field names and types MUST match exactly for BTF struct_ops matching.
/// Function pointers use raw *const () to generate PTR BTF entries.
#[repr(C)]
struct hid_bpf_ops {
    hid_id: i32,
    flags: u32,
    list: list_head,
    hid_device_event: *const (),
    hid_rdesc_fixup: *const (),
    hid_hw_request: *const (),
    hid_hw_output_report: *const (),
    hdev: *mut hid_device,
}

unsafe impl Sync for hid_bpf_ops {}

#[unsafe(link_section = ".struct_ops.link")]
#[unsafe(no_mangle)]
static akko_on_demand: hid_bpf_ops = hid_bpf_ops {
    hid_id: 0,
    flags: 0,
    list: list_head {
        next: core::ptr::null_mut(),
        prev: core::ptr::null_mut(),
    },
    // Reference actual functions to create relocations for loader to find
    hid_device_event: akko_on_demand_event as *const (),
    hid_rdesc_fixup: akko_on_demand_rdesc_fixup as *const (),
    hid_hw_request: akko_on_demand_hw_request as *const (),
    hid_hw_output_report: core::ptr::null(), // Not implemented
    hdev: core::ptr::null_mut(),
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

// -----------------------------------------------------------------------------
// struct_ops Context Wrapper Pattern
// -----------------------------------------------------------------------------
//
// IMPORTANT: struct_ops callbacks do NOT receive the typed struct pointer
// directly. Instead, the kernel passes a pointer to an array of u64 values
// where the actual typed pointer is at index 0.
//
// In C, the BPF_PROG macro (from bpf_tracing.h) handles this automatically:
//
//   SEC("struct_ops/hid_rdesc_fixup")
//   int BPF_PROG(my_fixup, struct hid_bpf_ctx *hctx) { ... }
//
// The macro expands to receive `unsigned long long *ctx` and extracts
// the typed pointer via `ctx[0]`.
//
// In Rust, we must do this manually:
//
//   pub extern "C" fn my_fixup(ctx_wrapper: *mut u64) -> i32 {
//       let hctx = unsafe { *ctx_wrapper as *mut hid_bpf_ctx };
//       ...
//   }
//
// This generates the correct bytecode:
//   r6 = *(u64 *)(r1 + 0x0)    // Load from ctx[0]
//
// The verifier then recognizes r6 as `trusted_ptr_hid_bpf_ctx()`, which
// passes kfunc argument type checks. Without this extraction, the verifier
// sees the raw wrapper pointer and fails with:
//   "arg#0 pointer type STRUCT hid_bpf_ctx must point to scalar"
//
// Reference: linux/tools/testing/selftests/bpf/progs/ and bpf_tracing.h
// -----------------------------------------------------------------------------

/// Extract the actual hid_bpf_ctx pointer from struct_ops context wrapper.
#[inline(always)]
unsafe fn extract_ctx(ctx_wrapper: *mut u64) -> *mut hid_bpf_ctx {
    *ctx_wrapper as *mut hid_bpf_ctx
}

// Device event handler - not used
#[no_mangle]
#[link_section = "struct_ops/hid_device_event"]
pub extern "C" fn akko_on_demand_event(_ctx: *mut u64) -> i32 {
    0
}

// Report descriptor fixup - appends battery Feature report
#[no_mangle]
#[link_section = "struct_ops/hid_rdesc_fixup"]
pub extern "C" fn akko_on_demand_rdesc_fixup(ctx_wrapper: *mut u64) -> i32 {
    // SAFETY: kernel passes valid context wrapper, extract the actual hid_bpf_ctx pointer
    let ctx = unsafe { HidBpfContext::new(extract_ctx(ctx_wrapper)) };

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

    trace!(b"akko_rev%d: RDESC appending battery, orig=%d", REVISION, orig_size as u32);

    // Append battery descriptor using safe copy
    if !data.copy_from_slice(orig_size, &BATTERY_FEATURE_DESC) {
        return 0;
    }

    let new_size = orig_size + BATTERY_FEATURE_DESC.len();
    trace!(b"akko_rev%d: RDESC new size=%d bytes", REVISION, new_size as u32);

    new_size as i32
}

// HW request handler (sleepable) - sends F7 on-demand and returns battery value
#[no_mangle]
#[link_section = "struct_ops.s/hid_hw_request"]
pub extern "C" fn akko_on_demand_hw_request(ctx_wrapper: *mut u64) -> i32 {
    // SAFETY: kernel passes valid context wrapper, extract the actual hid_bpf_ctx pointer
    let ctx = unsafe { HidBpfContext::new(extract_ctx(ctx_wrapper)) };

    // Need at least 2 bytes for the battery response [report_id, battery]
    if ctx.allocated_size() < 2 {
        return 0;
    }

    let Some(mut data) = ctx.data(0, 2) else {
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

    trace!(b"akko_rev%d: REQUEST report_id=%d", REVISION, report_id as u32);

    // Vendor hid_id is set by loader in VENDOR_HID_MAP
    let Some(&vendor_hid_id) = VENDOR_HID_MAP.get(0) else {
        trace!(b"akko_rev%d: ERROR vendor_hid_id not set", REVISION);
        return 0;
    };

    if vendor_hid_id == 0 {
        trace!(b"akko_rev%d: ERROR vendor_hid_id=0", REVISION);
        return 0;
    }

    // RAII guard - context automatically released on drop (even on early return)
    let Some(vendor) = AllocatedContext::new(vendor_hid_id) else {
        trace!(b"akko_rev%d: ERROR failed to alloc vendor ctx", REVISION);
        return 0;
    };

    // Send F7 command to tell dongle to query keyboard battery
    // F7 queries do NOT wake the keyboard (verified via idle flag)
    let mut f7_buf: [u8; 65] = [0; 65];
    f7_buf[0] = 0x00; // Report ID
    f7_buf[1] = 0xF7; // F7 command
    let ret = vendor.hw_request(&mut f7_buf, HidReportType::Feature, HidClassRequest::SetReport);
    trace!(b"akko_rev%d: F7_SEND %db", REVISION, ret);

    // Read battery response via GET_FEATURE Report 5
    let mut response: [u8; 65] = [0; 65];
    response[0] = 0x05; // Request Report ID 5
    let ret = vendor.hw_request(&mut response, HidReportType::Feature, HidClassRequest::GetReport);
    trace!(b"akko_rev%d: VENDOR_GET %db [%02x %02x %02x %02x]", REVISION, ret,
        response[0] as u32, response[1] as u32, response[2] as u32, response[3] as u32);
    trace!(b"akko_rev%d: VENDOR_GET [%02x %02x %02x %02x]", REVISION,
        response[4] as u32, response[5] as u32, response[6] as u32, response[7] as u32);

    if ret < 2 {
        trace!(b"akko_rev%d: ERROR VENDOR_GET failed", REVISION);
        return 0;
    }

    let raw_battery = response[1];
    let idle = response[3];
    let online = response[4];

    // Log abnormal values for debugging
    if raw_battery > 100 {
        trace!(b"akko_rev%d: WARN abnormal value=%d", REVISION, raw_battery as u32);
    }

    // Clamp to 100 to avoid reporting invalid percentages
    let battery = if raw_battery > 100 { 100 } else { raw_battery };

    trace!(b"akko_rev%d: RESULT bat=%d%% idle=%d online=%d", REVISION, battery as u32, idle as u32, online as u32);

    // Write battery response to kernel's buffer
    // Format: [report_id, battery_level]
    data.set(0, BATTERY_REPORT_ID);
    data.set(1, battery);

    // Return 2 = we handled the request and wrote 2 bytes
    2
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
