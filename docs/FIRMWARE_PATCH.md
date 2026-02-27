# Firmware Patches

Custom firmware patches for the MonsGeek M1 V5 keyboard and its 2.4GHz wireless dongle. These are binary patches applied on top of the stock RongYuan firmware — no source code required.

## What the patches do

### Keyboard patch

- **Battery over USB HID** — Exposes battery level (0–100%) and charging status as a standard HID power supply. Desktop environments (KDE, GNOME) show battery in the system tray automatically.
- **LED streaming** — Per-key RGB control from the host. The driver can push GIF animations frame-by-frame to the keyboard LEDs at ~30fps.
- **Debug log** — Ring buffer readable over HID for diagnostics (developer use).
- **RTT telemetry** — SEGGER RTT channel for live battery ADC/charger monitoring over SWD (developer use).

### Dongle patch

- **Battery over USB HID** — Same standard HID battery as the keyboard patch, but for the wireless path. The dongle already caches the keyboard's battery level from RF packets — this patch exposes it to the host via HID descriptors.
- **Proactive updates** — Pushes battery changes to the host as HID Input reports whenever the value changes, so the desktop battery indicator updates without polling.

### What stays the same

All stock firmware features continue to work: key scanning, LED effects, macros, magnetism calibration, wireless, Bluetooth, the MonsGeek web configurator — everything. The patches only add new capabilities in unused flash/SRAM space.

## User guide

### Battery level

After flashing the patched firmware, battery level appears automatically in your desktop environment — no driver or BPF loader needed.

**Check it works:**
```bash
# Should show a hid-*-battery entry
ls /sys/class/power_supply/

# Read battery percentage
cat /sys/class/power_supply/hid-*-battery/capacity

# Read charging status
cat /sys/class/power_supply/hid-*-battery/status
# → "Charging" or "Discharging"
```

**KDE**: Battery appears in the system tray power menu alongside laptop battery.
**GNOME**: Appears in Settings → Power, and in the top bar if `gnome-shell-extension-battery-percentage` is installed.

### LED streaming

Requires the `iot_driver` CLI with a patched keyboard (wired USB only):

```bash
# Stream a GIF to the keyboard
iot_driver stream cat.gif

# One-LED-at-a-time test pattern
iot_driver stream-test
```

LED streaming temporarily overrides the current LED effect. The built-in effect resumes when streaming stops.

### Patch detection

The driver auto-detects patched firmware via the 0xFB discovery command:

```bash
# Shows patch name, version, and capabilities
iot_driver info
# → Patch: MONSMOD v1 [battery, led_stream]

# Or on stock firmware:
# → Patch: Stock firmware (no patch support).
```

This works over both wired USB and 2.4GHz dongle (the command relays through the dongle to the keyboard).

## Comparison: firmware patch vs eBPF

There are two approaches to getting battery level on Linux. They solve the same problem differently.

| | Firmware patch | HID-BPF (akko-loader) |
|---|---|---|
| **What it does** | Adds battery HID descriptor to firmware | Injects battery descriptor via kernel eBPF |
| **Requires flashing** | Yes (one-time) | No |
| **Charging status** | Yes | No (not available via vendor protocol) |
| **Proactive updates** | Yes (Input reports on change) | No (polled on sysfs read) |
| **Kernel version** | Any | 6.12+ (HID-BPF struct_ops) |
| **Root required** | No (after flash) | Yes (BPF load) |
| **Works on** | Wired + dongle | Dongle only |
| **Latency** | Instant (kernel event) | ~100ms (F7 query + poll loop) |

**Recommendation**: Use the firmware patch if you're comfortable flashing. Use BPF if you want a non-invasive solution that doesn't modify the device.

Both can coexist — the firmware patch takes priority since the kernel sees the native HID battery descriptor and doesn't need BPF intervention.

### HID-BPF setup (no firmware modification)

```bash
# Build
make bpf

# Load (requires root, kernel 6.12+)
sudo akko-loader load

# Auto-load on device connect
sudo make install-all
```

See the main [README](../README.md#hid-bpf-battery-support-kernel-612) for full BPF instructions.

## Building the patches

### Prerequisites

```bash
# ARM cross-compiler
sudo apt install gcc-arm-none-eabi    # Debian/Ubuntu
sudo pacman -S arm-none-eabi-gcc      # Arch

# Python 3 (for hook framework)
# No pip packages needed
```

### Keyboard patch

```bash
cd firmwares/2949-v407/patch/

# Build hook binary
make

# Apply to firmware image
make patch

# Flash via driver
cd ../../../iot_driver_linux
cargo run --release -- firmware flash -y ../firmwares/2949-v407/firmware_patched.bin
```

### Dongle patch

```bash
cd firmwares/DONGLE_RY6108_RF_KB_V903/patch/

# Build hook binary
make

# Apply to firmware image
make patch

# Flash via ROM DFU (bridge BOOT0 to 3.3V, plug USB)
dfu-util -a 0 -d 2e3c:df11 --dfuse-address 0x08000000 \
  -D ../dfu_dumps/dongle_patched_256k.bin
```

**Note**: The dongle must be flashed via the AT32F405's built-in ROM DFU bootloader (BOOT0 pin), not the RongYuan application bootloader. See [HARDWARE.md](HARDWARE.md) for BOOT0 pad location and DFU recovery procedure.

## Technical reference

### Architecture

```
patch/                          Shared code (repo root)
├── hook_framework.py           Trampoline generator + binary patcher
├── hid_desc.h                  HID descriptor macros
└── at32f405_sdk/               Artery BSP headers (CMSIS + peripherals)

firmwares/2949-v407/patch/      Keyboard patch
├── hooks.py                    Hook definitions + binary patch addresses
├── handlers.c                  C handler implementations
├── handlers.S                  Asm glue (memcpy, EP2 transmit wrapper)
├── patch.ld                    Linker script (PATCH flash + PATCH_SRAM)
├── fw_v407.h                   Auto-generated firmware symbols (from Ghidra)
├── fw_symbols.ld               Auto-generated linker symbols
└── Makefile

firmwares/DONGLE_.../patch/     Dongle patch (same structure)
├── hooks.py
├── handlers.c
├── ...
```

### How hooks work

The hook framework (`hook_framework.py`) implements binary patching via ARM Thumb-2 trampolines:

1. **Displacement**: The first N bytes of the target function are copied to a stub in the patch zone.
2. **Trampoline**: A `B.W` (branch) instruction overwrites the target's prologue, redirecting to the stub.
3. **Stub**: The stub calls the C handler, then either falls through to the displaced prologue + jumps back (before/filter modes), or skips the original function (if filter returns non-zero).

Hook modes:
- **before**: Handler runs before the original function. Always falls through.
- **filter**: Handler returns 0 (passthrough) or non-zero (intercepted, original skipped).

### Keyboard patch details

**Flash layout** (patch zone):
```
0x08025800 - 0x08027FFF   Patch zone (10KB, unused in stock firmware)
```

**SRAM layout**:
```
0x20009800 - 0x20009BFF   PATCH_SRAM (4KB)
  - RTT control block (pinned at start via .rtt section)
  - extended_rdesc buffer (217 bytes)
  - Debug log ring buffer (512 bytes)
  - Diagnostic counters
```

**Hooks** (4 total, ~1906 bytes):

| Hook | Target | Mode | Purpose |
|------|--------|------|---------|
| `vendor_dispatch` | `vendor_command_dispatch` (0x08013304) | filter | Intercepts 0xFB/0xFC/0xFD vendor commands |
| `hid_class_setup` | `hid_class_setup_handler` (0x0801474C) | filter | Intercepts GET_REPORT for battery Feature report (ID 7) |
| `usb_connect` | `usb_otg_device_connect` (0x08018690) | filter | Patches descriptors before USB enumeration |
| `battery_monitor` | `battery_level_monitor` (0x0801695C) | before | Emits RTT telemetry for ADC/battery debugging |

**Binary patches** (applied at build time):
- Literal pool at 0x0801485C: pointer redirected from original IF1 rdesc to `extended_rdesc`
- CMP/MOV at 0x080147FC/0x08014800: descriptor length cap changed from 171 to 217
- wDescriptorLength in SRAM config descriptors: patched at runtime by `handle_usb_connect`

**Battery HID descriptor** (46 bytes appended to IF1):
- Report ID 7, Feature + Input reports
- Battery Strength (Usage Page 0x06, Usage 0x20): 0–100%
- Charging (Usage Page 0x85, Usage 0x44): 0/1

### Dongle patch details

**Flash layout**:
```
0x0800B000 - 0x0800D7FF   Patch zone (10KB)
```

**SRAM layout**:
```
0x20002000 - 0x200023FF   PATCH_SRAM (1KB)
  - extended_rdesc buffer (217 bytes)
  - Static report buffers
```

**Hooks** (3 total, ~290 bytes):

| Hook | Target | Mode | Purpose |
|------|--------|------|---------|
| `usb_init` | `usb_init` (0x080069D8) | before | Populates extended_rdesc before USB enumeration |
| `hid_class_setup` | `hid_class_setup_handler` (0x080071B4) | filter | Intercepts GET_REPORT for battery; patches descriptors |
| `rf_packet_dispatch` | `rf_packet_dispatch` (0x080059FC) | before | Pushes Input reports on battery/charging changes |

**Key differences from keyboard patch**:
- Setup packet is a separate parameter (r1), not embedded in udev struct
- Uses OTGHS (not OTGFS1), `g_usb_device` at 0x20000484 (no +4 offset)
- Battery data comes from `dongle_state.kb_battery_info` (+0xDB) and `.kb_charging` (+0xDC), cached from RF packets
- No vendor command dispatch — 0xFB queries relay through to the keyboard

### Vendor command protocol (keyboard only)

| Command | Name | Direction | Description |
|---------|------|-----------|-------------|
| 0xFB | PATCH_INFO | GET | Returns magic 0xCAFE, version, capabilities, name "MONSMOD", diagnostics |
| 0xFC | LED_STREAM | SET | Per-key RGB: page 0–6 = 18 keys each, 0xFF = commit, 0xFE = release |
| 0xFD | DEBUG_LOG | GET | Ring buffer read: page 0–9, 56 bytes/page, 512 byte total |

**0xFB response layout** (in GET_REPORT Feature response):

| Offset | Field | Description |
|--------|-------|-------------|
| 1–2 | Magic | 0xCA 0xFE |
| 3 | Version | Patch version (currently 1) |
| 4–5 | Capabilities | Bitmask: bit 0 = battery, bit 1 = led_stream, bit 2 = debug_log |
| 6–13 | Name | NUL-padded ASCII ("MONSMOD") |
| 14+ | Diagnostics | HID setup call counts, last setup packet, battery level, ADC values |

### Symbol export pipeline

Firmware symbols (function addresses, globals, struct types) are extracted from Ghidra and used at compile time:

```
Ghidra project
  → ExportSymbols.java (headless script)
    → symbols.json
      → generate_patch_files.py
        → fw_v407.h (C header with externs + struct typedefs)
        → fw_symbols.ld (linker symbols for firmware functions/globals)
        → patch.ld (memory regions + sections)
```

**Important**: Never edit `fw_v407.h` or `fw_symbols.ld` manually. Always update labels/types in Ghidra first, then re-export via `make symbols && make generate`.

### Recovery

If a patched firmware causes issues:

**Keyboard**: Enter ROM DFU (bridge BOOT0 to 3.3V), flash stock firmware via `dfu-util`, or use `iot_driver firmware flash` with the stock `.bin`.

**Dongle**: Same ROM DFU procedure (BOOT0 pad on dongle PCB). Flash the unmodified `dongle_working_256k.bin`.

**Factory reset** (config only, preserves firmware): Write 2KB of 0xFF to config region:
```bash
# Keyboard
dfu-util -a 0 -d 2e3c:df11 --dfuse-address 0x08028000 -D /dev/zero --upload-size 2048

# Or just re-flash stock firmware — bootloader erases config automatically
```

See [HARDWARE.md](HARDWARE.md) for BOOT0 pad locations and full DFU recovery procedure.
