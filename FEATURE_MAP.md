# MonsGeek/Akko Linux Driver - Feature Implementation Status

This document tracks the implementation progress compared to the official Akko Cloud driver.

**Status Legend:**
- ✅ Complete
- 🟡 Partial / In Progress
- ⬜ Not Started
- ❌ Won't Implement

---

## 1. Core HID Protocol

### 1.1 Device Connection & Management

| Feature | Status | Notes |
|---------|--------|-------|
| USB device enumeration | ✅ | Via hidapi |
| Device connect/disconnect | ✅ | Automatic detection |
| Multiple device support | ✅ | Device selection in TUI/CLI |
| Device identification (whoAmI) | ✅ | `0x8F` command |
| 2.4GHz dongle support | ✅ | Full F7/FC protocol |
| Bluetooth LE support | 🟡 | Battery via BlueZ, commands limited |

### 1.2 HID Communication

| Feature | Status | Notes |
|---------|--------|-------|
| Send feature report | ✅ | |
| Read feature report | ✅ | |
| Checksum Bit7 | ✅ | |
| Checksum Bit8 | ✅ | |
| Bluetooth timing | ✅ | Extra delays for BT |
| Vendor sleep/block | ✅ | Thread-safe access |
| Linux hidraw buffering workaround | ✅ | Retry logic |

---

## 2. Keyboard Features

### 2.1 Basic Configuration

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Get device info | ✅ | `info` | Firmware, device ID |
| Get all settings | ✅ | `all` | Bulk settings read |
| Reset device | ✅ | `reset` | Factory reset |
| Get/Set profile | ✅ | `profile` / `set-profile` | 0-3 profiles |
| Get/Set debounce | ✅ | `debounce` / `set-debounce` | |
| Get/Set polling rate | ✅ | `rate` / `set-rate` | 125-8000Hz |
| Get sleep time | 🟡 | | Read implemented |
| Set sleep time | ⬜ | | |

### 2.2 Key Matrix / Remapping

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Get key matrix | ✅ | `keymap` | Full keyboard layout |
| Set single key remap | ✅ | `remap` | |
| Swap two keys | ✅ | `swap` | |
| Reset key to default | ✅ | `reset-key` | |
| Fn layer matrix | 🟡 | | Read implemented |
| Bulk key config | ⬜ | | Full matrix write |

### 2.3 Macros

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Get macro | ✅ | `macro` | |
| Set text macro | ✅ | `set-macro` | Simple text strings |
| Clear macro | ✅ | `clear-macro` | |
| Complex macro editor | ⬜ | | Delays, mouse, combos |

### 2.4 Lighting - Main

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Get LED settings | ✅ | `led` | |
| Set LED mode | ✅ | `mode` | By name or number |
| Set LED params | ✅ | `set-led` | Mode, brightness, speed |
| Set LED color | ✅ | `set-led-color` | RGB values |
| List LED modes | ✅ | `modes` | All 26 modes |
| Per-key RGB (static) | 🟡 | | Read implemented |
| Per-key RGB (set) | ⬜ | | Full color matrix |

### 2.5 Lighting - Side LEDs

| Feature | Status | Notes |
|---------|--------|-------|
| Get side LED settings | ✅ | TUI shows side LED |
| Set side LED settings | ✅ | TUI can adjust |

### 2.6 Audio Reactive

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Audio capture | ✅ | `audio` | ALSA/JACK support |
| Frequency analysis | ✅ | `audio-levels` | 16-band FFT |
| Music mode streaming | ✅ | `audio` | Real-time to keyboard |
| Audio device selection | ✅ | `audio-test` | List devices |

### 2.7 Screen Sync

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Screen capture | ✅ | `screen` | PipeWire support |
| Ambient color extraction | ✅ | | Average screen color |
| Real-time streaming | ✅ | | Continuous update |

### 2.8 Animations

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Upload GIF to keyboard | ✅ | `gif` | Store in keyboard memory |
| Stream GIF real-time | ✅ | `gif-stream` | Per-frame streaming |
| Rainbow animation | ✅ | `rainbow` | Built-in demo |
| Wave animation | ✅ | `wave` | Built-in demo |

---

## 3. Magnetic/Hall Effect Features

### 3.1 Analog Key Settings

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Get trigger settings | ✅ | `triggers` | All per-key data |
| Set actuation point | ✅ | `set-actuation` | All keys |
| Set per-key actuation | 🟡 | | TUI can adjust individual |
| Set Rapid Trigger | ✅ | `set-rt` | Enable/disable + sensitivity |
| Get supported features | ✅ | `features` | Feature bitmap |

### 3.2 Key Modes

| Feature | Status | Notes |
|---------|--------|-------|
| Normal mode | ✅ | |
| Rapid Trigger mode | ✅ | |
| DKS mode | 🟡 | Read implemented, basic set |
| Mod-Tap mode | ⬜ | |
| Toggle mode | ⬜ | |
| Snap-Tap mode | 🟡 | Read implemented |

### 3.3 Key Depth Monitoring

| Feature | Status | CLI Command | Notes |
|---------|--------|-------------|-------|
| Enable/disable monitoring | ✅ | `depth` | |
| Real-time depth display | ✅ | `depth` | Bar chart view |
| Raw depth values | ✅ | `depth --raw` | Numeric output |
| TUI visualization | ✅ | TUI Key Depth tab | Time series + bar chart |

### 3.4 Calibration

| Feature | Status | Notes |
|---------|--------|-------|
| Min calibration | 🟡 | Command implemented |
| Max calibration | 🟡 | Command implemented |
| Full calibration wizard | ⬜ | Guided procedure |

---

## 4. Display Features (OLED/TFT)

| Feature | Status | Notes |
|---------|--------|-------|
| Get OLED version | ✅ | |
| Set clock | ⬜ | |
| Set language | ⬜ | |
| Custom image | ⬜ | |
| Custom GIF | ⬜ | |

---

## 5. gRPC Server

| Feature | Status | Notes |
|---------|--------|-------|
| sendRawFeature | ✅ | |
| readRawFeature | ✅ | |
| watchDevList | ✅ | Device hotplug events |
| getVersion | ✅ | |
| insertDb | ✅ | Local storage |
| getItemFromDb | ✅ | |
| Web app compatibility | ✅ | app.monsgeek.com works |

---

## 6. User Interfaces

### 6.1 CLI

| Feature | Status | Notes |
|---------|--------|-------|
| Device info commands | ✅ | info, all, features |
| LED control commands | ✅ | led, mode, set-led |
| Trigger commands | ✅ | triggers, set-actuation, set-rt |
| Remap commands | ✅ | remap, swap, reset-key |
| Animation commands | ✅ | gif, rainbow, wave |
| Audio commands | ✅ | audio, audio-test |
| Screen sync | ✅ | screen |
| Depth monitoring | ✅ | depth |

### 6.2 TUI (Terminal UI)

| Feature | Status | Notes |
|---------|--------|-------|
| Device Info tab | ✅ | |
| LED Settings tab | ✅ | Main + side LED |
| Key Depth tab | ✅ | Bar chart + time series |
| Triggers tab | ✅ | List + keyboard layout view |
| Options tab | ✅ | KB options |
| Macros tab | 🟡 | View implemented |
| Interactive value editing | ✅ | Arrow keys to adjust |
| Keyboard layout view | ✅ | Visual key selection |

---

## 7. System Integration

### 7.1 Battery Support

| Feature | Status | Notes |
|---------|--------|-------|
| USB dongle battery query | ✅ | F7 protocol |
| BLE battery via BlueZ | ✅ | D-Bus integration |
| HID-BPF power_supply | ✅ | Kernel 6.12+ |
| Desktop battery indicator | ✅ | Via BPF |

### 7.2 Installation

| Feature | Status | Notes |
|---------|--------|-------|
| udev rules | ✅ | Non-root access |
| Makefile install | ✅ | make install |
| systemd service | ✅ | Auto BPF load |

---

## Progress Summary

| Category | Complete | Partial | Not Started | Total | Progress |
|----------|----------|---------|-------------|-------|----------|
| Core HID | 11 | 1 | 0 | 12 | ~95% |
| Keyboard Basic | 9 | 2 | 1 | 12 | ~80% |
| Key Remapping | 5 | 1 | 1 | 7 | ~75% |
| Macros | 3 | 0 | 1 | 4 | ~75% |
| Lighting | 8 | 1 | 1 | 10 | ~85% |
| Audio/Screen | 6 | 0 | 0 | 6 | 100% |
| Animations | 4 | 0 | 0 | 4 | 100% |
| Magnetic Keys | 6 | 4 | 2 | 12 | ~70% |
| Key Depth | 5 | 0 | 0 | 5 | 100% |
| Calibration | 0 | 2 | 1 | 3 | ~30% |
| Display | 1 | 0 | 4 | 5 | ~20% |
| gRPC Server | 6 | 0 | 0 | 6 | 100% |
| CLI | 7 | 0 | 0 | 7 | 100% |
| TUI | 6 | 1 | 0 | 7 | ~90% |
| System | 4 | 0 | 0 | 4 | 100% |
| **Total** | **81** | **12** | **11** | **104** | **~85%** |

---

## Remaining Work

### High Priority
- [ ] Full per-key RGB color editor
- [ ] Complex macro editor (delays, mouse, combos)
- [ ] DKS/Mod-Tap/Toggle mode configuration
- [ ] Calibration wizard

### Medium Priority
- [ ] Sleep time configuration
- [ ] Fn layer editing
- [ ] Bulk key matrix write

### Low Priority (Device-specific)
- [ ] OLED clock/language/images
- [ ] TFT display support

---

*Last updated: 2026-01-25*
*Based on iot_driver_linux implementation*
