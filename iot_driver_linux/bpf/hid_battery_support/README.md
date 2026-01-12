# HID-BPF Battery Support for Akko/MonsGeek Keyboards

This directory contains the Rust-based HID-BPF implementation for exposing keyboard battery to the Linux power_supply subsystem.

## Overview

The Akko/MonsGeek 2.4GHz dongle has three HID interfaces:
- **00D8**: Main keyboard interface (Usage Page 0x01, Keyboard)
- **00D9**: Consumer control interface
- **00DA**: Vendor interface (Usage Page 0xFFFF)

The BPF program attaches to the keyboard interface and:
1. Fixes a Report ID quirk (Report 5 returns as Report 0)
2. Triggers on-demand F7 battery refresh when UPower queries the battery
3. Proxies F7 commands to the vendor interface for battery refresh

## Components

### `akko-loader-rs/` - Rust BPF Loader

The loader uses the Aya BPF framework to:
- Load and pin the BPF program to `/sys/fs/bpf/akko/`
- Configure the throttle interval and vendor HID ID via BPF maps
- Exit after loading (BPF persists via pinned link)

```bash
# Build
cd akko-loader-rs && cargo build --release

# Load BPF (requires root)
sudo ./target/release/akko-loader

# Check status
./target/release/akko-loader status

# Unload
sudo ./target/release/akko-loader unload
```

### BPF Program (`akko-ebpf`)

Located at `../akko-ebpf/`, the Rust BPF program implements:
- `hid_rdesc_fixup` - Appends Battery Strength Feature report to keyboard descriptor
- `hid_hw_request` - Fixes Report ID 0→5 quirk and triggers on-demand F7 refresh

Build with:
```bash
cd ../akko-ebpf && cargo +nightly build --release
```

## Quick Start

```bash
# Build everything
cd /path/to/iot_driver_linux/bpf
make all

# Load BPF (requires root)
sudo ./hid_battery_support/akko-loader-rs/target/release/akko-loader

# Check battery
cat /sys/class/power_supply/hid-0003:3151:5038.*-battery/capacity

# UPower
upower -i "$(upower -e | grep 3151)"
```

## Installation

From the `iot_driver_linux/` directory:

```bash
# Install BPF loader + program
sudo make install-bpf

# Install udev rules (for device detection + UPower icon fix)
sudo make install-udev

# Install systemd service (auto-load on device plug)
sudo make install-systemd
```

## Key Technical Details

### F7 Command Required for Battery Refresh

The dongle does NOT automatically poll the keyboard for battery status. The host must send an **F7 command** via SET_FEATURE to trigger a fresh battery read:

```
# After device replug (no F7 sent yet):
GET_FEATURE Report 5: 0000000000000000  → zeros (no data)

# Send F7 command:
SET_FEATURE Report 0: [0x00, 0xF7, 0x00, ...]

# Now battery is available:
GET_FEATURE Report 5: 0053000001010100  → Battery=83%
```

### On-Demand Refresh

Instead of polling F7 periodically, the BPF program intercepts battery queries from UPower and sends F7 on-demand (with configurable throttle interval, default 10 minutes).

### Report ID Quirk

The dongle returns Report ID 0x00 instead of 0x05 for Feature reports. The BPF's `hid_hw_request` hook fixes this.

## Configuration

```bash
# Custom throttle interval (seconds)
sudo akko-loader --throttle 300   # 5 minutes

# Override HID ID (advanced)
sudo akko-loader --hid-id 42

# Verbose output
sudo akko-loader -v
```
