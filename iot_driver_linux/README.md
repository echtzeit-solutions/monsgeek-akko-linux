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

Streams RGB data directly to the WS2812 frame buffer over USB via the 0xFC patch protocol — no flash writes, real-time host-driven control. Requires the [firmware patch](#firmware-patch).

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
| `0xFB` | Patch Discovery | Returns patch magic (0xCAFE), version, capabilities, name |
| `0xFC` | LED Streaming | Per-key RGB data to WS2812 frame buffer (6 pages, commit/release) |
| `0xFD` | Debug Log | Ring buffer in SRAM, 10-page read, 5 entry types |
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

## License

GPL-3.0
