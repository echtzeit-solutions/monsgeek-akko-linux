# MonsGeek M1 V5 HE - Linux Setup Guide

## Overview

The Akko Cloud web driver (web.monsgeek.com / web.akkogear.com) requires a local helper application called `iot_driver` that communicates with the keyboard over HID and exposes a gRPC-Web API on `localhost:3814`. On Windows this is `iot_driver.exe` (a Rust binary using hidapi + tonic).

We developed two approaches to get it working on Linux:

1. **Wine Approach** - Run the Windows `iot_driver.exe` under Wine
2. **Native Linux Driver** - Our own Rust implementation (`iot_driver_linux`)

## Quick Start (Recommended: Native Linux Driver)

### 1. Set up udev rules for HID access

```bash
# Copy the udev rules file
sudo cp 99-monsgeek.rules /etc/udev/rules.d/

# Reload rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

The rules file (`99-monsgeek.rules`) contains:
```
# MonsGeek/Akko keyboards - allow user access to HID interfaces
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3151", MODE="0666"
SUBSYSTEM=="usb", ATTRS{idVendor}=="3151", MODE="0666"
```

### 2. Build and run the native Linux driver

```bash
cd iot_driver_linux
cargo build --release

# Run the gRPC server (matches Windows iot_driver.exe behavior)
./target/release/iot_driver
```

Output:
```
Starting IOT Driver Linux on 127.0.0.1:3814
Found device: VID=3151 PID=5030 path=/dev/hidraw6
Device ID: 2949
Server ready with gRPC-Web support
```

### 3. Open the web app

Navigate to https://web.monsgeek.com or https://web.akkogear.com in your browser.

The web app will detect the keyboard through our local gRPC server.

## Alternative: Wine Approach

If you need features not yet implemented in the native driver:

```bash
# Set up a Wine prefix
export WINEPREFIX=/path/to/monsgeek/wine_iot
mkdir -p "$WINEPREFIX"
wine wineboot

# Run the Windows driver
wine ./iot_driver.exe
```

**Limitations:**
- Wine's HID passthrough can be unreliable
- May require proton/experimental Wine for full HID support
- The native Linux driver is more reliable

## Native TUI Application

We also built a standalone TUI (Terminal User Interface) that doesn't require a browser:

```bash
./target/release/monsgeek-tui
```

Features:
- **Device Info** - View firmware version, device ID, current settings
- **LED Settings** - Adjust main LED and side LED (mode, brightness, speed, color, dazzle)
- **Key Depth Monitor** - Real-time visualization of key press depth
- **Trigger Settings** - View/edit per-key actuation points and modes
- **Options** - Configure Fn layer, WASD swap, RT stability, anti-mistouch

### TUI Keybindings

| Key | Action |
|-----|--------|
| Tab/Shift+Tab | Switch tabs |
| Up/Down, j/k | Navigate/scroll |
| Left/Right, h/l | Adjust values |
| Shift+Left/Right | Adjust by ±10 |
| 1-4 | Switch profile |
| r | Refresh data |
| m | Toggle key depth monitoring |
| c | Reconnect to device |
| q | Quit |

**Triggers Tab (mode switching):**
| Key | Mode |
|-----|------|
| n | Normal |
| t | Rapid Trigger |
| d | DKS (Dynamic Keystroke) |
| s | SnapTap |

## CLI Commands

The driver also supports CLI commands for scripting:

```bash
# Query commands
./target/release/iot_driver info          # Device info
./target/release/iot_driver led           # LED settings
./target/release/iot_driver triggers      # Trigger settings
./target/release/iot_driver options       # Keyboard options
./target/release/iot_driver all           # All settings

# SET commands
./target/release/iot_driver set-profile 0         # Switch to profile 0
./target/release/iot_driver set-led 2 3 2         # Mode 2, brightness 3, speed 2
./target/release/iot_driver set-led-color 255 0 0 # Red LED
./target/release/iot_driver set-debounce 5        # 5ms debounce
./target/release/iot_driver set-actuation 1.5     # 1.5mm actuation for all keys
./target/release/iot_driver set-rt 0.3            # 0.3mm RT sensitivity
./target/release/iot_driver reset                 # Factory reset
```

---

# How We Reverse Engineered the Protocol

## Architecture Discovery

```
┌─────────────────────────────────────────────────────────┐
│  Web App (React)                                        │
│  - web.monsgeek.com / web.akkogear.com                  │
│  - Uses @protobuf-ts/grpcweb-transport                  │
│  - 95MB JS bundle (mostly UI assets)                    │
└─────────────────────┬───────────────────────────────────┘
                      │ gRPC-Web (HTTP/2)
                      │ http://127.0.0.1:3814
                      ▼
┌─────────────────────────────────────────────────────────┐
│  iot_driver.exe (Windows) / iot_driver (Linux)          │
│  - gRPC server using tonic                              │
│  - HID access via hidapi                                │
│  - Exposes: sendRawFeature, readRawFeature, watchDevList│
└─────────────────────┬───────────────────────────────────┘
                      │ HID Feature Reports (64 bytes)
                      ▼
┌─────────────────────────────────────────────────────────┐
│  Keyboard Firmware                                      │
│  - VID=0x3151 PID=0x5030                               │
│  - Interface 2, Usage Page 0xFFFF, Usage 0x02          │
└─────────────────────────────────────────────────────────┘
```

## Extraction Process

### Step 1: Extract the Electron app

The Akko Cloud installer is an NSIS package containing an Electron app:

```bash
cd driver_extract

# Extract NSIS installer
7z x "Akko Cloud_v4_setup_370.2.17_WIN20251225.exe" -oextracted

# The app resources are in:
# extracted/PLUGINSDIR/app/resources/app/dist/
```

### Step 2: Unbundle and deobfuscate the JavaScript

```bash
# Install webcrack (JS unbundler/deobfuscator)
npm install -g webcrack

# Run extraction pipeline
./extract-all.sh
```

This runs:
1. **webcrack** - Unbundles the Vite/Rollup bundle, deobfuscates variable names
2. **refactor-bundle.js** - AST-based extraction of:
   - Device definitions (VID/PID, key counts, features)
   - Protocol commands (FEA_CMD_* constants)
   - HID communication classes

### Step 3: Analyze the protocol

Key files extracted:
- `extracted_protocol.md` - Full protocol documentation
- `devices_electron.json` - All supported devices with parameters
- `akko_business_logic.js` - Core HID communication logic

## HID Protocol

For complete protocol documentation including message format, all commands, data structures, and transport-specific details, see **[docs/PROTOCOL.md](docs/PROTOCOL.md)**.

## Files in This Project

```
monsgeek-m1-v5-tmr/
├── NOTES.md                    # Initial research notes
├── SETUP.md                    # This file
├── extracted_protocol.md       # Full protocol documentation
├── FEATURE_MAP.md             # Feature comparison with official app
│
├── 99-monsgeek.rules          # udev rules for HID access
├── iot_driver.exe             # Original Windows driver (for reference)
├── run_wine_iot.sh            # Helper script for Wine approach
│
├── iot_driver_linux/          # Native Linux driver (Rust)
│   ├── src/
│   │   ├── main.rs            # gRPC server + CLI
│   │   ├── tui.rs             # Terminal UI
│   │   ├── hid.rs             # HID communication
│   │   ├── protocol.rs        # Command definitions
│   │   └── devices.rs         # Device registry
│   └── proto/
│       └── driver.proto       # gRPC service definition
│
├── driver_extract/            # Electron app extraction tools
│   ├── extract-all.sh         # Main extraction script
│   ├── refactor-bundle.js     # AST-based JS analyzer
│   └── extracted/             # Extracted app contents
│
├── webapp_source/             # Extracted web app JS
│   ├── monsgeek_main.js       # Main bundle (beautified)
│   └── all_devices.json       # Device definitions
│
└── *.py                       # Various probe/test scripts
```

## Troubleshooting

### "Permission denied" on /dev/hidraw*

Make sure udev rules are installed and reloaded:
```bash
sudo cp 99-monsgeek.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
# Unplug and replug the keyboard
```

### Web app shows "Waiting for device"

1. Check that the driver is running: `./target/release/iot_driver`
2. Check that port 3814 is listening: `ss -tlnp | grep 3814`
3. Check browser console for CORS errors

### Keyboard not detected

1. Check that the keyboard is connected: `lsusb | grep 3151`
2. Check hidraw devices: `ls -la /dev/hidraw*`
3. Try running with sudo to rule out permissions

### TUI shows wrong values

Press `r` to refresh data from the device. Some settings are cached.

## Contributing

The native Linux driver implements ~80% of the official app's features. Remaining work:

- [ ] Per-key RGB color settings
- [ ] Key remapping
- [ ] Macro editor
- [ ] Firmware update

See `FEATURE_MAP.md` for detailed feature comparison.
