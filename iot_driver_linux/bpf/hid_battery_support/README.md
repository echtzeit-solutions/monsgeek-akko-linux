# HID-BPF Battery Support Variants

This directory contains different approaches to exposing the Akko/MonsGeek keyboard battery to the Linux power_supply subsystem via HID-BPF.

## Background

The 2.4GHz dongle has three HID interfaces:
- **00D8**: Main keyboard interface (Usage Page 0x01, Keyboard)
- **00D9**: Consumer control interface
- **00DA**: Vendor interface (Usage Page 0xFFFF)

## Key Discovery: F7 Command Required for Battery Refresh

**Critical Finding:** The dongle does NOT automatically poll the keyboard for battery status. The host must send an **F7 command** via SET_FEATURE to trigger a fresh battery read.

### Protocol Flow

1. **Without F7**: GET_FEATURE Report 5 returns cached/stale data (or zeros after replug)
2. **With F7**: SET_FEATURE with `[0x00, 0xF7, 0x00...]` triggers dongle → keyboard RF query
3. **After F7**: GET_FEATURE Report 5 returns fresh battery data

```
# After device replug (no F7 sent yet):
GET_FEATURE Report 5: 0000000000000000  → zeros (no data)

# Send F7 command:
SET_FEATURE Report 0: [0x00, 0xF7, 0x00, ...]

# Now battery is available:
GET_FEATURE Report 5: 0053000001010100  → Battery=83%
```

### All Interfaces Share the Same Handler

All three interfaces respond identically to Feature Report ID 5 queries (after F7):

```
00D8 (Keyboard):       0053000001010100  → Battery=83%
00D9 (Consumer Ctrl):  0053000001010100  → Battery=83%
00DA (Vendor):         0053000001010100  → Battery=83%
```

### Implications for BPF Solution

The kernel's power_supply subsystem only calls GET_FEATURE to poll battery - it does NOT send the F7 command. This means:
- **BPF alone is insufficient** - need userspace to send F7 periodically
- The BPF loader should send F7 before rebinding the device
- A background thread or timer should refresh battery every 30-60 seconds
- Without F7, battery values become stale

## Approaches

### `working/` - Vendor Interface with Dummy Input

**Status: WORKS**

Modifies the vendor interface (00DA) descriptor to include:
- Battery Strength as Input report (0x81)
- A dummy modifier key to force input device creation

The kernel only creates `power_supply` entries when an input device exists. By adding a dummy key, we trick the kernel into creating the input device, which enables the power_supply.

**Result**: `/sys/class/power_supply/hid-0003:3151:5038.00DA-battery`

**Downsides**:
- UPower shows device type as "gaming-input" instead of "keyboard"
- Creates a phantom input device

### `option_a_keyboard_inject/` - Inject into Keyboard Interface (RECOMMENDED)

**Status: WORKS!**

Attaches to the main keyboard interface (00D8) and appends a Battery Strength Feature report to its descriptor. Thanks to the shared Feature Report handler discovery, the firmware automatically returns battery data - no BPF map or userspace polling needed!

**Result**: `/sys/class/power_supply/hid-0003:3151:5038.00D8-battery`

**Advantages**:
- No phantom input device created (uses existing keyboard input device)
- Cleaner solution architecturally
- Simpler BPF code (no maps, no cross-interface communication)
- Power supply associated with the actual keyboard device

**UPower Icon Fix**: Install the udev rule to get "keyboard" icon instead of "gaming-input":
```bash
cd iot_driver_linux && sudo make install-udev
```

### `option_c_vendor_feature/` - Pure Feature Report

**Status: DOES NOT WORK**

Attempts to use only a Feature report (0xB1) without any Input reports on the vendor interface.

**Result**: No power_supply created. The kernel requires an input device to create power_supply entries.

### `common/` - Shared Headers

Contains common header files:
- `vmlinux.h` - Kernel BTF types
- `hid_bpf.h` - HID-BPF section definitions
- `hid_bpf_helpers.h` - Helper macros

## Quick Start

```bash
# Build the working version
cd /path/to/bpf
make clean && make

# Load BPF (requires root)
sudo ./loader 218  # or auto-detect: sudo ./loader

# Rebind device (may be done automatically)
echo "0003:3151:5038.00DA" | sudo tee /sys/bus/hid/drivers/hid-generic/unbind
echo "0003:3151:5038.00DA" | sudo tee /sys/bus/hid/drivers/hid-generic/bind

# Check battery
cat /sys/class/power_supply/hid-0003:3151:5038.00DA-battery/capacity
upower -i "$(upower -e | grep 3151)"
```

## Key Insights

1. **hid_id must be set before load**: The skeleton's `hid_id` field must be set before `bpf__load()`.

2. **Power supply requires input device**: The kernel's `hidinput_connect()` only creates input devices (and thus power_supply) when there are mappable HID usages.

3. **Battery Strength usage**: Use Usage Page 0x06 (Generic Device Controls), Usage 0x20 (Battery Strength) - NOT Battery System page 0x85.

4. **Report ID quirk**: The dongle returns Report ID 0x00 instead of 0x05 for Feature reports. Fixed via `hid_hw_request` hook.
