# iot_driver — Linux Driver for MonsGeek/Akko Magnetic Hall Effect Keyboards

A command-line tool and gRPC server for controlling MonsGeek M1 V5 HE (and compatible Akko/RongYuan RY5088-based) keyboards on Linux.

> **Disclaimer**: This software is provided as-is, without warranty of any kind,
> express or implied. **Firmware flashing carries inherent risk** — a failed or
> interrupted flash can brick your keyboard, requiring physical DFU recovery
> (BOOT0 pin access). The authors are not responsible for any damage to your
> hardware. Use at your own risk. Always ensure you have a recovery method
> available before flashing.

## Building

```bash
cargo build --release
```

For firmware update checking/downloading from the RongYuan API:

```bash
cargo build --release --features firmware-api
```

## Supported Devices

| Device | VID:PID | Bootloader |
|---|---|---|
| Keyboard (USB wired) | `3151:5030` | `3151:502A` |
| Keyboard (2.4 GHz via dongle) | `3151:5038` | — |
| Keyboard (Bluetooth) | `3151:5027` | — |
| 2.4 GHz Dongle | `3151:5038` | `3151:5039` |

## Quick Start

```bash
# Show device info and firmware version
iot_driver info

# Show all device settings
iot_driver all

# Monitor battery level
iot_driver battery --watch

# Set LED mode
iot_driver set-led rainbow

# List all LED modes
iot_driver modes
```

## Commands

### Device Queries

| Command | Aliases | Description |
|---|---|---|
| `info` | `version`, `ver`, `v` | Device ID and firmware version |
| `profile` | `prof`, `p` | Current profile (0-3) |
| `led` | `light`, `l` | LED settings |
| `debounce` | `deb`, `d` | Debounce time |
| `rate` | `poll`, `hz` | Polling rate |
| `options` | `opts`, `o` | Fn layer, WASD swap, etc. |
| `features` | `feat`, `f` | Supported features |
| `sleep` | `s` | Sleep timers |
| `battery` | `bat`, `b` | Battery level and charging status |
| `all` | `a` | Everything above |
| `triggers` | `gt` | Trigger/actuation settings |

### Device Configuration

```bash
# Profile and basic settings
iot_driver set-profile 0          # Switch profile (0-3)
iot_driver set-debounce 5         # Debounce in ms (0-50)
iot_driver set-rate 8000          # Polling rate (125-8000 Hz)

# LED
iot_driver set-led wave 4 2       # Mode, brightness, speed
iot_driver set-led 0              # Mode by number
iot_driver set-color-all 255 0 0  # Per-key color (red)

# Sleep
iot_driver set-sleep --idle 2m --deep 10m

# Triggers and actuation
iot_driver set-actuation 0.5      # Actuation point in mm
iot_driver set-rt 0.3             # Rapid Trigger sensitivity
iot_driver set-release 0.4        # Release point in mm
iot_driver set-key-trigger A --actuation 0.3 --mode rt  # Per-key

# Key remapping
iot_driver remap CapsLock Escape  # Remap keys
iot_driver swap A B               # Swap two keys
iot_driver remap Fn+A F1          # Remap on Fn layer
iot_driver remap-list             # Show all remaps

# Macros
iot_driver set-macro F1 "Hello"          # Text macro
iot_driver set-macro F2 "Ctrl+A,Ctrl+C" --seq  # Key sequence

# Factory reset
iot_driver reset
```

### LED Animation

There are two ways to display custom LED animations on the keyboard:

#### GIF Upload (Stock Firmware)

Uploads a GIF animation to keyboard flash memory. The keyboard stores up to 255 frames and plays them back autonomously in "UserColor" mode (mode 25) — no host software needed after upload. The animation persists across reboots and power cycles.

```bash
iot_driver gif animation.gif              # Upload GIF to flash
iot_driver gif --test                     # Rainbow test animation
iot_driver gif animation.gif --mode tile  # Tile instead of scale
```

Works on stock firmware, no patch required. Best for: persistent decorative animations.

#### LED Streaming (Patched Firmware)

Streams RGB data directly to the WS2812 frame buffer over USB via the 0xE8 patch protocol — no flash writes, real-time host-driven control. Requires the [firmware patch](#firmware-patch).

```bash
# Sweep test — one LED at a time, cycling colors
iot_driver stream-test --fps 15

# Stream a GIF animation to the keyboard LEDs
iot_driver stream animation.gif --loop

# Stream with custom FPS (overrides GIF timing)
iot_driver stream animation.gif --fps 30 --loop
```

Press Ctrl+C to stop streaming; the keyboard returns to its normal LED mode. The GIF is scaled to the keyboard's 16x6 LED matrix using nearest-neighbor sampling.

Best for: real-time effects driven by the host — audio visualization, screen color sync, status indicators, progress bars, notifications, or any dynamic content that changes based on external state.

| | GIF Upload | LED Streaming |
|---|---|---|
| Firmware | Stock | Patched |
| Persistence | Survives reboot | Host-driven only |
| Frame limit | 255 frames | Unlimited |
| Flash writes | Yes | No (SRAM only) |
| Host needed | Only during upload | Continuous |
| Use case | Decorative animations | Real-time reactive effects |

### Audio Reactive LEDs (Patched Firmware)

Streams audio-reactive colors to the keyboard using system audio capture.

```bash
iot_driver audio                       # Spectrum mode
iot_driver audio --mode solid --hue 180  # Solid color pulse
iot_driver audio-test                  # List audio devices
iot_driver audio-levels                # Monitor audio levels
```

### Joystick Mapper

Maps magnetic key depth (Hall effect analog readings) to virtual joystick axes via uinput.

```bash
iot_driver joystick                    # Interactive TUI
iot_driver joystick --headless         # Daemon mode
iot_driver joystick --config path.toml # Custom config
```

### Magnetism Monitor

```bash
iot_driver depth              # Real-time key depth visualization
iot_driver depth --raw        # With hex dump
```

### Firmware Management

> **Warning**: Flashing firmware can brick your keyboard. Make sure you have
> physical DFU recovery access (BOOT0 pin) before proceeding. A failed flash
> leaves the keyboard in bootloader mode until firmware is re-flashed.

```bash
# Validate a firmware file without flashing
iot_driver firmware validate firmware.bin

# Dry-run: simulate the flash process
iot_driver firmware dry-run firmware.bin --verbose

# Check for updates from the RongYuan server
iot_driver firmware check

# Download latest firmware
iot_driver firmware download --output firmware.zip

# Flash keyboard firmware (interactive confirmation)
iot_driver firmware flash firmware.bin

# Flash without confirmation prompt
iot_driver firmware flash firmware.bin --yes

# Flash dongle firmware
iot_driver firmware flash --dongle dongle_app_only.bin --yes
```

The flash process:
1. Validates the firmware file (size, checksum, chip ID header)
2. Verifies chip ID matches target device (`AT32F405 8KMKB` for keyboard, `AT32F405 8K-DGKB` for dongle)
3. Sends the ISP prepare command (0xC5)
4. Triggers bootloader entry (0x7F + magic) — **this erases the firmware area**
5. Waits for re-enumeration as bootloader device (PID 0x502A for keyboard, 0x5039 for dongle)
6. Transfers firmware in 64-byte chunks with running checksum
7. Sends transfer complete with checksum verification
8. Device reboots to normal mode

The `--dongle` flag targets the 2.4 GHz wireless dongle instead of the keyboard. The chip ID check prevents accidentally flashing the wrong firmware to the wrong device.

### Utility Commands

```bash
iot_driver list          # List all HID devices
iot_driver raw 8f        # Send raw vendor command (hex)
iot_driver serve         # Start gRPC server on port 3814
iot_driver tui           # Interactive terminal UI
```

### Global Flags

```
--monitor    Print all USB commands and responses
--hex        Show raw hex dumps
--file FILE  Replay a pcap capture file (no device needed)
--filter X   Filter output (all, events, commands, cmd=0xNN)
```

## Firmware Patch

The firmware patch extends the stock v407 firmware with additional capabilities that the CLI can use. It lives in `firmwares/2949-v407/patch/` and is built separately.

### Patch Features

| Command | Protocol | Description |
|---|---|---|
| `0xE7` | Patch Discovery | Returns patch magic (0xCAFE), version, capabilities, name |
| `0xE8` | LED Streaming | Per-key RGB data to WS2812 frame buffer (7 pages, commit/release) |
| `0xE9` | Debug Log | Ring buffer in SRAM, 10-page read, 5 entry types |
| Battery HID | USB descriptor | Adds battery level + charging status to HID report descriptor |

### Building the Patch

```bash
cd firmwares/2949-v407/patch
make
```

Requires `arm-none-eabi-gcc` and Python 3. The build:
1. Compiles `handlers.c` (C handlers for each hook)
2. Assembles `hook.S` (ARM Thumb-2 hook stubs)
3. Links against firmware symbols (`fw_symbols.ld`)
4. Runs `patch_firmware.py` to merge `hook.bin` into the base firmware

Output: `firmware_patched.bin` in the parent directory.

### Flashing the Patch

```bash
cd iot_driver_linux
cargo run --release -- firmware flash ../firmwares/2949-v407/firmware_patched.bin
```

The patched firmware occupies ~1.2 KB of the 10 KB patch zone (0x08025800-0x08027FFF). It hooks into the firmware's vendor command dispatch and HID class handler without modifying the original code — only literal pool entries and length caps are patched at build time.

### Dongle Patch

The dongle patch exposes the keyboard's battery level over the dongle's USB HID interface, so `power_supply` works when connected wirelessly via 2.4 GHz. It lives in `firmwares/DONGLE_RY6108_RF_KB_V903/patch/`.

```bash
cd firmwares/DONGLE_RY6108_RF_KB_V903/patch
make              # Build hook.bin
make patch        # Apply to dongle firmware

# Flash to dongle
cd iot_driver_linux
cargo run --release -- firmware flash --dongle \
  ../firmwares/DONGLE_RY6108_RF_KB_V903/dfu_dumps/dongle_patched_256k.bin
```

### Recovery

If the keyboard is stuck in bootloader mode (PID 0x502A) after a failed flash, re-flash the stock or patched firmware using the same `firmware flash` command. For the dongle (PID 0x5039), use `firmware flash --dongle`.

If the bootloader itself is unresponsive, bridge the BOOT0 pad to 3.3V and use `dfu-util` with the AT32 ROM DFU bootloader (VID:PID `2e3c:df11`).

## Architecture

```
iot_driver_linux/
  src/
    cli.rs              # CLI definitions (clap)
    main.rs             # Command dispatch
    commands/           # Command handlers
    protocol.rs         # HID protocol encoding/decoding
    grpc.rs             # gRPC server
    firmware.rs         # Firmware file parsing
    flash.rs            # Flash protocol implementation
    gif.rs              # GIF loading and LED mapping
    devices.rs          # Device detection and profiles
  monsgeek-transport/   # HID transport abstraction
  monsgeek-keyboard/    # High-level keyboard API
  monsgeek-joystick/    # Joystick mapper (uinput)
```

## Changelog

### 2026-02-28

- **TUI overhaul**: Consolidated 6 tabs down to 4 — merged LED Settings and Options into Device Info with inline `< value >` spinners for all editable settings (profile, debounce, rate, LED mode/brightness/speed/RGB, sleep, fn layer, WASD swap)
- **Dongle status in TUI**: Device Info shows dongle firmware version, RF address, RF firmware version, and RF Ready status when connected via 2.4 GHz (auto-refreshes with battery tick)
- **Depth monitoring over dongle**: Firmware patch now NOPs the BT-only gate in `send_depth_monitor_report`, enabling key depth data over 2.4 GHz and USB
- **Dongle EP2 speed gate fix**: Firmware patch NOPs the `usb_device_speed_get() == FULL_SPEED` check in `rf_tx_handler` that blocked all EP2 IN transfers on the OTGHS-based dongle
- **Auto-strip bootloader**: `firmware flash` transparently strips the 20 KB bootloader prefix from full 256 KB flash dump images
- **Removed dongle pairing command**: `dongle pair` (F8) removed — it's PAN1082 SPI firmware programming mode, not user-facing pairing

### 2026-02-27

- **Patch command bytes changed**: 0xFB/0xFC/0xFD moved to 0xE7/0xE8/0xE9 — the dongle intercepts F-range commands locally and never forwards them to the keyboard
- **Dongle-local command support**: Transport layer handles dongle-local commands (F0, F7, FB, FD) with correct checksum types
- **Dongle CLI subcommand**: `dongle info` and `dongle status` for querying dongle firmware, RF info, and keyboard status via F7 polling
- **Transport priority**: Wired USB preferred over BT over dongle when multiple transports available

### 2026-02-26

- **Firmware patch build improvements**: SDK type filtering (375 types), natural struct alignment, automatic gap padding in generated headers, PATCH_SRAM expanded to 4 KB
- **Ghidra-sourced descriptor symbols**: Config descriptor addresses now come from Ghidra labels, not hardcoded SRAM addresses

### 2026-02-25

- **Dongle firmware flash**: `firmware flash --dongle` with chip ID safety validation (keyboard=`AT32F405 8KMKB`, dongle=`AT32F405 8K-DGKB`)
- **Dongle battery HID patch**: Hooks dongle's `hid_class_setup_handler` to expose keyboard battery level over dongle USB (built, pending flash)

### 2026-02-24

- **RTT debug infrastructure**: SEGGER RTT control block in firmware patch for real-time battery ADC monitoring via Black Magic Probe
- **Battery HID interrupt endpoint**: Patch pushes battery level changes to host via HID interrupt IN (no polling needed)

### 2026-02-11

- **Key action decoding**: All `config_type` cases decoded from firmware RE — ProfileSwitch, SpecialFn, ConnectionMode, Knob, LedControl. Zero "Unknown" entries in `remap-list`
- **LED streaming**: `stream-test` and `stream` commands for real-time per-key RGB via patch protocol
- **Audio reactive LEDs**: Spectrum and solid-color pulse modes driven by system audio capture
- **Battery ADC quirk documented**: USB mode drops ADC by 311 counts (18%) due to OTG PHY ground shift

### 2026-02-10

- **Battery HID**: Firmware patch adds battery level + charging status to USB HID report descriptor. Kernel creates `power_supply` device automatically
- **Firmware flash engine**: Full RY bootloader protocol implementation with checksum verification

### Earlier

- gRPC server with LED streaming and effect RPCs
- Userpic/GIF upload to keyboard flash (mode 25)
- Joystick mapper (Hall effect analog to uinput axes)
- Key remapping, macro recording, trigger/actuation configuration
- pcap replay mode for offline protocol analysis

## License

GPL-3.0
