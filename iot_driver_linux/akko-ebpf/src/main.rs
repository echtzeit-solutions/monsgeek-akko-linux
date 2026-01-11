//! HID-BPF driver for Akko/MonsGeek 2.4GHz dongle battery integration
//! Option A: Keyboard inject - appends battery Feature report to keyboard descriptor
//!
//! Key discovery: The dongle firmware responds to Feature Report ID 5 on ANY
//! interface with identical battery data. We just need to:
//! 1. Append a Battery Strength Feature report to the keyboard descriptor
//! 2. Fix the Report ID quirk (firmware returns 0x00 instead of 0x05)
//!
//! Dongle: VID 0x3151 / PID 0x5038

#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

use aya_ebpf::helpers::bpf_printk;

// =============================================================================
// Kernel BTF-compatible type definitions
// =============================================================================

/// Forward declaration of hid_device (opaque kernel struct).
/// Name must match kernel's BTF exactly for kfunc verification.
#[repr(C)]
pub struct hid_device {
    _opaque: [u8; 0],
}

/// The kernel's HID-BPF context structure (raw).
///
/// This struct MUST match the kernel's `struct hid_bpf_ctx` layout.
/// We use usize for the pointer field to ensure it's treated as a scalar
/// by the verifier (avoids "must point to scalar" error).
#[repr(C)]
pub struct hid_bpf_ctx {
    /// Pointer to the HID device (as usize to avoid nested pointer BTF).
    pub hid: usize,
    /// Allocated size for data buffer access.
    pub allocated_size: u32,
    /// Return value / size (union in kernel, we use i32).
    pub retval: i32,
}

// =============================================================================
// Idiomatic Rust wrapper for HID-BPF context
// =============================================================================

/// Safe wrapper around the kernel's hid_bpf_ctx.
///
/// struct_ops callbacks receive a `*const u64` array where element [0] contains
/// the actual context pointer. This wrapper handles the dereference automatically
/// via `From<*const u64>`.
#[derive(Clone, Copy)]
pub struct HidBpfCtx(*mut hid_bpf_ctx);

impl From<*const u64> for HidBpfCtx {
    /// Convert from struct_ops callback argument to typed context.
    ///
    /// # Safety
    /// The caller must ensure ctx_array points to a valid u64 array
    /// where element [0] is a valid hid_bpf_ctx pointer.
    #[inline(always)]
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn from(ctx_array: *const u64) -> Self {
        unsafe { HidBpfCtx(*ctx_array as *mut hid_bpf_ctx) }
    }
}

impl HidBpfCtx {
    /// Get a pointer to the HID report data buffer.
    ///
    /// Returns None if offset+size exceeds allocated_size.
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

/// Define a HID-BPF struct_ops callback with automatic context handling.
///
/// This macro handles:
/// - `#[no_mangle]` and `#[link_section]` attributes
/// - `extern "C"` calling convention
/// - Automatic dereference of ctx_array[0] to HidBpfCtx
///
/// # Example
/// ```ignore
/// hid_bpf_prog!(hid_rdesc_fixup, my_rdesc_fixup, |ctx| {
///     let data = ctx.get_data(0, 64)?;
///     // ... modify descriptor ...
///     0
/// });
/// ```
macro_rules! hid_bpf_prog {
    ($member:ident, $name:ident, |$ctx:ident| $body:expr) => {
        /// HID-BPF callback - called by kernel BPF subsystem.
        ///
        /// # Safety
        /// This function is called by the kernel with a valid context array.
        #[no_mangle]
        #[link_section = concat!("struct_ops/", stringify!($member))]
        pub unsafe extern "C" fn $name(ctx_array: *const u64) -> i32 {
            let $ctx = HidBpfCtx::from(ctx_array);
            $body
        }
    };
}

// =============================================================================
// Kernel function (kfunc) declarations
// =============================================================================

// Declare kfuncs - the static reference below forces BTF emission.
extern "C" {
    /// Get pointer to HID report data buffer.
    /// Returns NULL if offset+size exceeds allocated_size.
    fn hid_bpf_get_data(ctx: *mut hid_bpf_ctx, offset: u32, size: u32) -> *mut u8;
}

// Force the extern to be emitted in .ksyms section for BTF generation
#[used]
#[link_section = ".ksyms"]
static HID_BPF_GET_DATA_REF: unsafe extern "C" fn(*mut hid_bpf_ctx, u32, u32) -> *mut u8 = hid_bpf_get_data;

/// Call hid_bpf_get_data using inline assembly to ensure correct BPF calling convention.
/// The extern declaration above provides BTF; this wrapper makes the actual call.
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

// =============================================================================
// struct_ops definitions
// =============================================================================

/// The hid_bpf_ops struct that holds function pointers for HID-BPF callbacks.
/// This must match the kernel's struct hid_bpf_ops layout.
#[repr(C)]
struct hid_bpf_ops {
    hid_id: i32,
    flags: u32,
    // list_head is opaque, we just reserve space (2 pointers)
    _list: [usize; 2],
    hid_device_event: usize,
    hid_rdesc_fixup: usize,
    hid_hw_request: usize,
    hid_hw_output_report: usize,
    hdev: usize,
}

// SAFETY: The struct contains only integers and is only accessed by the BPF subsystem
unsafe impl Sync for hid_bpf_ops {}

/// The struct_ops map definition that registers our HID-BPF callbacks.
#[unsafe(link_section = ".struct_ops.link")]
#[unsafe(no_mangle)]
static akko_keyboard_battery: hid_bpf_ops = hid_bpf_ops {
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

/// Battery Feature report descriptor to append to keyboard descriptor.
/// Uses Feature report (B1) so kernel polls it, not Input report.
static BATTERY_FEATURE_DESC: [u8; 24] = [
    // Battery application collection
    0x05, 0x01,        // Usage Page (Generic Desktop)
    0x09, 0x06,        // Usage (Keyboard) - same as main for association
    0xA1, 0x01,        // Collection (Application)
    0x85, 0x05,        //   Report ID (5)
    // Battery strength as Feature report
    0x05, 0x06,        //   Usage Page (Generic Device Controls)
    0x09, 0x20,        //   Usage (Battery Strength)
    0x15, 0x00,        //   Logical Minimum (0)
    0x26, 0x64, 0x00,  //   Logical Maximum (100)
    0x75, 0x08,        //   Report Size (8)
    0x95, 0x01,        //   Report Count (1)
    0xB1, 0x02,        //   Feature (Data,Var,Abs)
    0xC0               // End Collection
];

// =============================================================================
// HID-BPF callbacks
// =============================================================================

// Device event handler - not used for keyboard interface.
// We only need rdesc_fixup and hw_request for battery support.
hid_bpf_prog!(hid_device_event, akko_dongle_event, |_ctx| {
    0
});

// Report descriptor fixup - appends battery Feature report to keyboard descriptor.
hid_bpf_prog!(hid_rdesc_fixup, akko_rdesc_fixup, |ctx| {
    let Some(data) = ctx.get_data(0, 128) else {
        return 0;
    };

    // Verify this is the keyboard interface (05 01 09 06)
    if *data != 0x05 || *data.add(1) != 0x01 || *data.add(2) != 0x09 || *data.add(3) != 0x06 {
        return 0;
    }

    // Get original descriptor size from ctx.retval() (size union member)
    let orig_size = ctx.retval() as usize;
    if orig_size > 100 {
        // Too large, might overflow buffer
        return 0;
    }

    bpf_printk!(b"akko_kb: appending battery to keyboard, orig=%d", orig_size as u32);

    // Append battery Feature report descriptor after the original
    for (i, &byte) in BATTERY_FEATURE_DESC.iter().enumerate() {
        *data.add(orig_size + i) = byte;
    }

    let new_size = orig_size + BATTERY_FEATURE_DESC.len();
    bpf_printk!(b"akko_kb: new descriptor size = %d bytes", new_size as u32);

    // Return new total size
    new_size as i32
});

// HW request handler - fixes firmware Report ID quirk.
// The dongle firmware returns Report ID 0x00 instead of 0x05 for
// Feature Report requests. We fix this so the kernel correctly
// processes the battery data.
hid_bpf_prog!(hid_hw_request, akko_hw_request, |ctx| {
    let Some(data) = ctx.get_data(0, 8) else {
        return 0;
    };

    // Fix Report ID quirk: firmware returns 0x00 instead of 0x05
    // Check that byte[1] looks like battery percentage (0-100)
    let report_id = *data;
    let battery = *data.add(1);

    if report_id == 0x00 && battery <= 100 {
        bpf_printk!(b"akko_kb: fixing report_id 0->5, battery=%d%%", battery as u32);
        *data = 0x05;
    }

    0
});

// Panic handler for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
