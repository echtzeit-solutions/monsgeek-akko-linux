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

#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

use aya_ebpf::helpers::{bpf_printk, bpf_ktime_get_ns};
use aya_ebpf::btf_maps::Array;
use aya_ebpf::macros::btf_map;

// =============================================================================
// Constants
// =============================================================================

/// Battery Feature Report ID
const BATTERY_REPORT_ID: u8 = 0x05;

/// Default throttle: 10 minutes in nanoseconds
const DEFAULT_THROTTLE_NS: u64 = 600 * 1_000_000_000;

/// HID report types (from linux/hid.h)
const HID_FEATURE_REPORT: u32 = 2;

/// HID class request types (from linux/hid.h)
const HID_REQ_SET_REPORT: u32 = 0x09;

// =============================================================================
// Kernel BTF-compatible type definitions
// =============================================================================

/// Forward declaration of hid_device (opaque kernel struct).
#[repr(C)]
pub struct hid_device {
    _opaque: [u8; 0],
}

/// The kernel's HID-BPF context structure.
#[repr(C)]
pub struct hid_bpf_ctx {
    pub hid: usize,
    pub allocated_size: u32,
    pub retval: i32,
}

// =============================================================================
// Idiomatic Rust wrapper for HID-BPF context
// =============================================================================

/// Safe wrapper around the kernel's hid_bpf_ctx.
#[derive(Clone, Copy)]
pub struct HidBpfCtx(*mut hid_bpf_ctx);

impl From<*const u64> for HidBpfCtx {
    #[inline(always)]
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn from(ctx_array: *const u64) -> Self {
        unsafe { HidBpfCtx(*ctx_array as *mut hid_bpf_ctx) }
    }
}

impl HidBpfCtx {
    /// Get a pointer to the HID report data buffer.
    #[inline(always)]
    pub fn get_data(&self, offset: u32, size: u32) -> Option<*mut u8> {
        let ptr = unsafe { call_hid_bpf_get_data(self.0, offset, size) };
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }

    /// Get the return value / descriptor size from context.
    #[inline(always)]
    pub fn retval(&self) -> i32 {
        unsafe { (*self.0).retval }
    }

    /// Get the allocated buffer size.
    #[inline(always)]
    pub fn allocated_size(&self) -> u32 {
        unsafe { (*self.0).allocated_size }
    }

    /// Get raw pointer for advanced use.
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut hid_bpf_ctx {
        self.0
    }
}

// =============================================================================
// Macro for struct_ops entry points
// =============================================================================

/// Define a HID-BPF struct_ops callback.
macro_rules! hid_bpf_prog {
    ($member:ident, $name:ident, |$ctx:ident| $body:expr) => {
        #[no_mangle]
        #[link_section = concat!("struct_ops/", stringify!($member))]
        pub unsafe extern "C" fn $name(ctx_array: *const u64) -> i32 {
            let $ctx = HidBpfCtx::from(ctx_array);
            $body
        }
    };
}

/// Define a sleepable HID-BPF struct_ops callback.
macro_rules! hid_bpf_prog_sleepable {
    ($member:ident, $name:ident, |$ctx:ident| $body:expr) => {
        #[no_mangle]
        #[link_section = concat!("struct_ops.s/", stringify!($member))]
        pub unsafe extern "C" fn $name(ctx_array: *const u64) -> i32 {
            let $ctx = HidBpfCtx::from(ctx_array);
            $body
        }
    };
}

// =============================================================================
// Kernel function (kfunc) declarations
// =============================================================================

extern "C" {
    fn hid_bpf_get_data(ctx: *mut hid_bpf_ctx, offset: u32, size: u32) -> *mut u8;
    fn hid_bpf_allocate_context(hid_id: u32) -> *mut hid_bpf_ctx;
    fn hid_bpf_release_context(ctx: *mut hid_bpf_ctx);
    fn hid_bpf_hw_request(
        ctx: *mut hid_bpf_ctx,
        buf: *mut u8,
        buf_sz: usize,
        rtype: u32,
        reqtype: u32,
    ) -> i32;
}

// Force externs to be emitted in .ksyms section for BTF generation
#[used]
#[link_section = ".ksyms"]
static HID_BPF_GET_DATA_REF: unsafe extern "C" fn(*mut hid_bpf_ctx, u32, u32) -> *mut u8 =
    hid_bpf_get_data;

#[used]
#[link_section = ".ksyms"]
static HID_BPF_ALLOCATE_CONTEXT_REF: unsafe extern "C" fn(u32) -> *mut hid_bpf_ctx =
    hid_bpf_allocate_context;

#[used]
#[link_section = ".ksyms"]
static HID_BPF_RELEASE_CONTEXT_REF: unsafe extern "C" fn(*mut hid_bpf_ctx) =
    hid_bpf_release_context;

#[used]
#[link_section = ".ksyms"]
static HID_BPF_HW_REQUEST_REF: unsafe extern "C" fn(*mut hid_bpf_ctx, *mut u8, usize, u32, u32) -> i32 =
    hid_bpf_hw_request;

/// Call hid_bpf_get_data using inline assembly.
#[inline(always)]
unsafe fn call_hid_bpf_get_data(ctx: *mut hid_bpf_ctx, offset: u32, size: u32) -> *mut u8 {
    let result: *mut u8;
    core::arch::asm!(
        "call hid_bpf_get_data",
        in("r1") ctx,
        in("r2") offset,
        in("r3") size,
        lateout("r0") result,
        clobber_abi("C"),
    );
    result
}

/// Call hid_bpf_allocate_context using inline assembly.
#[inline(always)]
unsafe fn call_hid_bpf_allocate_context(hid_id: u32) -> *mut hid_bpf_ctx {
    let result: *mut hid_bpf_ctx;
    core::arch::asm!(
        "call hid_bpf_allocate_context",
        in("r1") hid_id,
        lateout("r0") result,
        clobber_abi("C"),
    );
    result
}

/// Call hid_bpf_release_context using inline assembly.
#[inline(always)]
unsafe fn call_hid_bpf_release_context(ctx: *mut hid_bpf_ctx) {
    core::arch::asm!(
        "call hid_bpf_release_context",
        in("r1") ctx,
        clobber_abi("C"),
    );
}

/// Call hid_bpf_hw_request using inline assembly.
#[inline(always)]
unsafe fn call_hid_bpf_hw_request(
    ctx: *mut hid_bpf_ctx,
    buf: *mut u8,
    buf_sz: usize,
    rtype: u32,
    reqtype: u32,
) -> i32 {
    let result: i32;
    core::arch::asm!(
        "call hid_bpf_hw_request",
        in("r1") ctx,
        in("r2") buf,
        in("r3") buf_sz,
        in("r4") rtype,
        in("r5") reqtype,
        lateout("r0") result,
        clobber_abi("C"),
    );
    result
}

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

// bpf_ktime_get_ns is provided by aya_ebpf::helpers

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
    0x05, 0x01,        // Usage Page (Generic Desktop)
    0x09, 0x06,        // Usage (Keyboard)
    0xA1, 0x01,        // Collection (Application)
    0x85, BATTERY_REPORT_ID, // Report ID (5)
    0x05, 0x06,        // Usage Page (Generic Device Controls)
    0x09, 0x20,        // Usage (Battery Strength)
    0x15, 0x00,        // Logical Minimum (0)
    0x26, 0x64, 0x00,  // Logical Maximum (100)
    0x75, 0x08,        // Report Size (8)
    0x95, 0x01,        // Report Count (1)
    0xB1, 0x02,        // Feature (Data,Var,Abs)
    0xC0               // End Collection
];

// =============================================================================
// HID-BPF callbacks
// =============================================================================

// Device event handler - not used
hid_bpf_prog!(hid_device_event, akko_on_demand_event, |_ctx| {
    0
});

// Report descriptor fixup - appends battery Feature report
hid_bpf_prog!(hid_rdesc_fixup, akko_on_demand_rdesc_fixup, |ctx| {
    let Some(data) = ctx.get_data(0, 128) else {
        return 0;
    };

    // Verify keyboard interface (05 01 09 06)
    if *data != 0x05 || *data.add(1) != 0x01 || *data.add(2) != 0x09 || *data.add(3) != 0x06 {
        return 0;
    }

    let orig_size = ctx.retval() as usize;
    if orig_size > 100 {
        return 0;
    }

    bpf_printk!(b"akko_ondemand: appending battery, orig=%d", orig_size as u32);

    // Append battery descriptor
    for (i, &byte) in BATTERY_FEATURE_DESC.iter().enumerate() {
        *data.add(orig_size + i) = byte;
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
    bpf_printk!(b"akko_ondemand: new size = %d bytes", new_size as u32);

    new_size as i32
});

// HW request handler (sleepable) - sends F7 on-demand
hid_bpf_prog_sleepable!(hid_hw_request, akko_on_demand_hw_request, |ctx| {
    // Need at least 4 bytes for the request buffer
    if ctx.allocated_size() < 4 {
        return 0;
    }

    let Some(data) = ctx.get_data(0, 4) else {
        return 0;
    };

    let report_id = *data;

    // Only handle battery report requests (Report ID 0 or 5)
    if report_id != 0x00 && report_id != BATTERY_REPORT_ID {
        return 0;
    }

    bpf_printk!(b"akko_ondemand: battery request, report_id=%d", report_id as u32);

    // Check throttle
    let Some(&last_f7) = STATE_MAP.get(0) else {
        return 0;
    };
    let Some(&throttle) = CONFIG_MAP.get(0) else {
        return 0;
    };

    let now = bpf_ktime_get_ns();
    let elapsed = now - last_f7;

    if elapsed <= throttle {
        bpf_printk!(b"akko_ondemand: throttle active (%d sec ago)", (elapsed / 1_000_000_000) as u32);
        return 0;
    }

    // Throttle expired - send F7 to vendor interface
    // Vendor hid_id is set by loader in VENDOR_HID_MAP
    let Some(&vendor_hid_id) = VENDOR_HID_MAP.get(0) else {
        bpf_printk!(b"akko_ondemand: vendor_hid_id not set in map");
        return 0;
    };

    if vendor_hid_id == 0 {
        bpf_printk!(b"akko_ondemand: vendor_hid_id is 0, not configured");
        return 0;
    }

    bpf_printk!(b"akko_ondemand: sending F7 to vendor=%d", vendor_hid_id);

    // Allocate context for vendor interface
    let vendor_ctx = call_hid_bpf_allocate_context(vendor_hid_id);
    if vendor_ctx.is_null() {
        bpf_printk!(b"akko_ondemand: failed to allocate vendor context");
        let _ = STATE_MAP.set(0, now, 0);
        return 0;
    }

    // Send F7 command (64-byte buffer, F7 at byte 0)
    let mut f7_buf: [u8; 64] = [0; 64];
    f7_buf[0] = 0xF7;

    let ret = call_hid_bpf_hw_request(
        vendor_ctx,
        f7_buf.as_mut_ptr(),
        f7_buf.len(),
        HID_FEATURE_REPORT,
        HID_REQ_SET_REPORT,
    );

    bpf_printk!(b"akko_ondemand: F7 ret=%d", ret);

    // Release vendor context
    call_hid_bpf_release_context(vendor_ctx);

    // Update timestamp
    let _ = STATE_MAP.set(0, now, 0);

    0
});

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
