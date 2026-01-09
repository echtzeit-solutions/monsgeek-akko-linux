# MonsGeek/Akko HID Command Reference

Complete reference of all HID commands extracted from decompilation and driver analysis.

## Command Categories

Commands are organized by connection type and purpose:
- **Wired USB**: Direct keyboard communication
- **2.4GHz Dongle**: Via wireless dongle (dongle_common=true)
- **Bluetooth LE**: Via GATT characteristics (ble=true)

## Standard Keyboard Commands (FEA_CMD_*)

### SET Commands (Host → Device)

| Hex | Dec | Name | Description |
|-----|-----|------|-------------|
| 0x01 | 1 | SET_RESET | Reset device to defaults |
| 0x03 | 3 | SET_REPORT | Set polling rate (0-6 → 8000-125Hz) |
| 0x04 | 4 | SET_PROFILE | Change active profile (0-3) |
| 0x06 | 6 | SET_DEBOUNCE | Set debounce timing |
| 0x07 | 7 | SET_LEDPARAM | Set LED effect parameters |
| 0x08 | 8 | SET_SLEDPARAM | Set side/secondary LED params |
| 0x09 | 9 | SET_KBOPTION | Set keyboard options |
| 0x0A | 10 | SET_KEYMATRIX | Set key mappings |
| 0x0B | 11 | SET_MACRO | Set macro definitions |
| 0x0C | 12 | SET_USERPIC | Set per-key RGB colors (static) |
| 0x0D | 13 | SET_AUDIO_VIZ | Send 16 frequency bands for music mode |
| 0x0E | 14 | SET_SCREEN_COLOR | Send RGB for screen sync mode (21) |
| 0x10 | 16 | SET_FN | Set Fn layer configuration |
| 0x11 | 17 | SET_SLEEPTIME | Set sleep/deep sleep timeouts |
| 0x12 | 18 | SET_USERGIF | Set per-key RGB animation (dynamic) |
| 0x17 | 23 | SET_AUTOOS_EN | Set auto-OS detection |
| 0x1B | 27 | SET_MAGNETISM_REPORT | Enable magnetism depth reporting |
| 0x1C | 28 | SET_MAGNETISM_CAL | Calibrate magnetism sensors |
| 0x1D | 29 | SET_KEY_MAGNETISM_MODE | Set per-key magnetism mode |
| 0x1E | 30 | SET_MAGNETISM_MAX_CAL | Calibrate max magnetism values |
| 0x22 | 34 | SET_OLEDOPTION | Set OLED screen options |
| 0x25 | 37 | SET_TFTLCDDATA | Send TFT LCD image data |
| 0x27 | 39 | SET_OLEDLANGUAGE | Set OLED language |
| 0x28 | 40 | SET_OLEDCLOCK | Set OLED clock display |
| 0x29 | 41 | SET_SCREEN_24BITDATA | Set 24-bit color screen data |
| 0x30 | 48 | SET_OLEDBOOTLOADER | Enter OLED bootloader mode |
| 0x31 | 49 | SET_OLEDBOOTSTART | Start OLED firmware transfer |
| 0x32 | 50 | SET_TFTFLASHDATA | Write TFT flash data |
| 0x50 | 80 | SET_SKU | Set factory SKU (manufacturing) |
| 0x65 | 101 | SET_MULTI_MAGNETISM | Set multi-key magnetism settings |
| 0x7F | 127 | FACTORY_RESET | Factory reset (magic bytes required) |
| 0xAC | 172 | SET_FLASHCHIPERASSE | Erase flash chip (DANGEROUS) |

### GET Commands (Device → Host)

| Hex | Dec | Name | Description |
|-----|-----|------|-------------|
| 0x80 | 128 | GET_REV / GET_RF_VERSION | Get firmware revision |
| 0x83 | 131 | GET_REPORT | Get polling rate |
| 0x84 | 132 | GET_PROFILE | Get active profile |
| 0x85 | 133 | GET_LEDONOFF | Get LED on/off state |
| 0x86 | 134 | GET_DEBOUNCE | Get debounce settings |
| 0x87 | 135 | GET_LEDPARAM | Get LED parameters |
| 0x88 | 136 | GET_SLEDPARAM | Get secondary LED params |
| 0x89 | 137 | GET_KBOPTION | Get keyboard options |
| 0x8A | 138 | GET_KEYMATRIX | Get key mappings |
| 0x8B | 139 | GET_MACRO | Get macros |
| 0x8C | 140 | GET_USERPIC | Get per-key RGB colors |
| 0x8F | 143 | GET_USB_VERSION | Get USB firmware version |
| 0x90 | 144 | GET_FN | Get Fn layer |
| 0x91 | 145 | GET_SLEEPTIME | Get sleep timeout |
| 0x97 | 151 | GET_AUTOOS_EN | Get auto-OS setting |
| 0x9D | 157 | GET_KEY_MAGNETISM_MODE | Get key magnetism mode |
| 0xA5 | 165 | GET_TFTLCDDATA | Get TFT LCD data readback |
| 0xA9 | 169 | GET_SCREEN_24BITDATA | Get 24-bit screen data |
| 0xAD | 173 | GET_OLED_VERSION | Get OLED firmware version |
| 0xAE | 174 | GET_MLED_VERSION | Get matrix LED controller version |
| 0xB0 | 176 | GET_OLEDBOOTLOADER | Get OLED bootloader state |
| 0xB1 | 177 | GET_OLEDBOOTCHECKSUM | Get OLED firmware checksum |
| 0xB2 | 178 | GET_TFTFLASHDATA | Read TFT flash data |
| 0xD0 | 208 | GET_SKU | Get factory SKU |
| 0xE5 | 229 | GET_MULTI_MAGNETISM | Get RT/DKS per-key settings |
| 0xE6 | 230 | GET_FEATURE_LIST | Get supported features |

## Dongle-Specific Commands (2.4GHz Wireless)

These commands are used when communicating via 2.4GHz wireless dongle.

| Hex | Dec | Name | Description |
|-----|-----|------|-------------|
| 0xF7 | 247 | DONGLE_STATUS | Query dongle status (battery, online) |
| 0xF0 | 240 | GET_DONGLE_USB_VERSION | Get dongle USB firmware (Cherry only) |
| 0x80 | 128 | GET_DONGLE_RF_VERSION | Get dongle RF firmware (Cherry only) |

### F7 Command: Battery Refresh Protocol

**Critical:** The F7 command must be sent via SET_FEATURE to trigger a fresh battery read!

The dongle does NOT automatically poll the keyboard for battery status. Without sending F7:
- GET_FEATURE returns cached/stale data
- After device replug, GET_FEATURE returns zeros until F7 is sent

#### Protocol Flow

```
1. Host sends SET_FEATURE Report ID 0:
   [0x00, 0xF7, 0x00, 0x00, ...] (64 bytes)

2. Dongle queries keyboard over 2.4GHz RF link

3. Keyboard responds with battery level from ADC

4. Dongle caches the battery value

5. Host reads GET_FEATURE Report ID 5:
   Response: [0x00, 0x53, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00]
                   ^^^^
                   Battery = 83% (0x53)
```

#### Example (Python)

```python
import os, fcntl

HIDIOCSFEATURE = lambda l: 0xC0000000 | (l << 16) | (ord('H') << 8) | 0x06
HIDIOCGFEATURE = lambda l: 0xC0000000 | (l << 16) | (ord('H') << 8) | 0x07

fd = os.open('/dev/hidrawN', os.O_RDWR)  # vendor interface

# Send F7 to refresh battery
cmd = bytearray([0x00, 0xF7] + [0]*62)
fcntl.ioctl(fd, HIDIOCSFEATURE(64), cmd)

# Read battery
buf = bytearray([0x05] + [0]*63)
fcntl.ioctl(fd, HIDIOCGFEATURE(64), buf)
battery = buf[1]  # 0-100%
```

### Dongle Status Response (0xF7)

After sending F7, GET_FEATURE Report ID 5 returns:

```
┌────────┬─────────┬────────┬────────┬──────────┬──────────┬────────┬─────────┐
│ Byte 0 │ Byte 1  │ Byte 2 │ Byte 3 │ Byte 4   │ Byte 5   │ Byte 6 │ Byte 7+ │
├────────┼─────────┼────────┼────────┼──────────┼──────────┼────────┼─────────┤
│ 0x00   │ Battery │ 0x00   │ 0x00   │ is_online│ is_online│ ???    │ padding │
│        │ (0-100) │        │        │ (0/1)    │ (0/1)    │        │         │
└────────┴─────────┴────────┴────────┴──────────┴──────────┴────────┴─────────┘
```

**Note:** Byte 0 is always 0x00 (firmware quirk - should be Report ID 0x05)

## Nordic Bootloader Commands (Pan1086 variant)

| Hex | Dec | Name | Description |
|-----|-----|------|-------------|
| 0xC3 | 195 | GET_NORDIC_BOOT | Get Nordic bootloader state |
| 0xC4 | 196 | GET_NORDIC_BOOTSTART | Start Nordic firmware transfer |

## Firmware Update Commands

**WARNING: These commands can brick your device. Do not use without proper recovery procedures.**

### Boot Mode Entry

| Hex | Dec | Name | Magic Bytes | Description |
|-----|-----|------|-------------|-------------|
| 0x7F | 127 | BOOT_ENTRY_USB | 0x55, 0xAA, 0x55, 0xAA | Enter USB bootloader |
| 0xF8 | 248 | BOOT_ENTRY_RF | 0x55, 0xAA, 0x55, 0xAA, 0x00, 0x00, 0x82 | Enter RF bootloader |

### Firmware Transfer

| Bytes | Name | Description |
|-------|------|-------------|
| 0xBA, 0xC0 | TRANSFER_START | Begin firmware transfer |
| 0xBA, 0xC2 | TRANSFER_COMPLETE | End firmware transfer |

### Boot Mode VID/PIDs

When in bootloader mode, device uses different VID/PID:

| VID | PID | Mode |
|-----|-----|------|
| 0x3141 | 0x504A | USB boot mode 1 |
| 0x3141 | 0x404A | USB boot mode 2 |
| 0x046A | 0x012E | RF boot mode 1 |
| 0x046A | 0x0130 | RF boot mode 2 |

## Response Status Codes

| Hex | Dec | Meaning |
|-----|-----|---------|
| 0xAA | 170 | Success |

## Report IDs

| ID | Hex | Purpose |
|----|-----|---------|
| 0 | 0x00 | Default command report |
| 5 | 0x05 | Vendor feature report (battery, depth, commands) |

## Magnetism Sub-Commands (0x65 / 0xE5)

Used with SET/GET_MULTI_MAGNETISM for per-key hall effect settings:

| Hex | Dec | Name | Description |
|-----|-----|------|-------------|
| 0x00 | 0 | PRESS_TRAVEL | Actuation point (in precision units) |
| 0x01 | 1 | LIFT_TRAVEL | Release point |
| 0x02 | 2 | RT_PRESS | Rapid Trigger press sensitivity |
| 0x03 | 3 | RT_LIFT | Rapid Trigger lift sensitivity |
| 0x04 | 4 | DKS_TRAVEL | DKS (Dynamic Keystroke) travel |
| 0x05 | 5 | MODTAP_TIME | Mod-Tap activation time |
| 0x06 | 6 | BOTTOM_DEADZONE | Bottom dead zone |
| 0x07 | 7 | KEY_MODE | Mode flags (Normal/RT/DKS/ModTap/Toggle/SnapTap) |
| 0x09 | 9 | SNAPTAP_ENABLE | Snap Tap anti-SOCD enable |
| 0x0A | 10 | DKS_MODES | DKS trigger modes/actions |
| 0xFB | 251 | TOP_DEADZONE | Top dead zone (firmware >= 1024) |
| 0xFC | 252 | SWITCH_TYPE | Switch type (if replaceable) |
| 0xFE | 254 | CALIBRATION | Raw sensor calibration values |

### Key Mode Values

| Value | Mode |
|-------|------|
| 0 | Normal |
| 1 | Rapid Trigger |
| 2 | DKS (Dynamic Keystroke) |
| 3 | Mod-Tap |
| 4 | Toggle |
| 5 | Snap Tap |

## LED Effect Modes (0x07 / 0x87)

| Value | Name | Notes |
|-------|------|-------|
| 0 | Off | LEDs disabled |
| 1 | Constant | Static color |
| 2 | Breathing | Pulsing effect |
| 3 | Neon | Neon glow |
| 4 | Wave | Color wave |
| 5 | Ripple | Ripple from keypress |
| 6 | Raindrop | Raindrops falling |
| 7 | Snake | Snake pattern |
| 8 | Reactive | React to keypress (keep lit) |
| 9 | Converge | Converging pattern |
| 10 | Sine Wave | Sine wave pattern |
| 11 | Kaleidoscope | Kaleidoscope effect |
| 12 | Line Wave | Line wave pattern |
| 13 | User Picture | Custom per-key colors (4 layers) |
| 14 | Laser | Laser effect |
| 15 | Circle Wave | Circular wave |
| 16 | Rainbow | Rainbow/dazzle effect |
| 17 | Rain Down | Rain downward |
| 18 | Meteor | Meteor shower |
| 19 | Reactive Off | React to keypress (brief flash) |
| 20 | Music Patterns | Audio reactive with patterns (uses 0x0D) |
| 21 | Screen Sync | Ambient RGB from screen (uses 0x0E) |
| 22 | Music Bars | Audio reactive bars (uses 0x0D) |
| 23 | Train | Train pattern |
| 24 | Fireworks | Fireworks effect |
| 25 | Per-Key Color | Dynamic per-key animation (GIF) |

## Polling Rates

| Code | Rate |
|------|------|
| 0 | 8000 Hz (0.125ms) |
| 1 | 4000 Hz (0.25ms) |
| 2 | 2000 Hz (0.5ms) |
| 3 | 1000 Hz (1ms) |
| 4 | 500 Hz (2ms) |
| 5 | 250 Hz (4ms) |
| 6 | 125 Hz (8ms) |

## BLE Protocol

BLE devices use GATT characteristics instead of HID feature reports.

### Usage Pages (BLE)

| Usage Page | Hex | Description |
|------------|-----|-------------|
| 0xFF55 | 65365 | Vendor HID (primary) |
| 0xFF66 | 65382 | Vendor HID (alternate) |

### Standard Services

| Service UUID | Name |
|--------------|------|
| 0x180F | Battery Service |
| 0x1812 | HID Service |

### Battery Characteristic

- **UUID**: 0x2A19 (Battery Level)
- **Format**: Single byte 0-100 (percentage)

## Checksum Calculation

Most commands use Bit7 checksum:

```
checksum = 255 - (sum of bytes 0-6) & 0xFF
```

Position in buffer: byte 7 (Bit7) or byte 8 (Bit8 for LED commands)

## Device Identification

### VID Table

| VID | Hex | Manufacturer |
|-----|-----|--------------|
| 12625 | 0x3151 | MonsGeek/Akko |
| 5215 | 0x145F | Akko (alternate) |
| 13357 | 0x342D | Epomaker |
| 13434 | 0x347A | Feker |
| 14154 | 0x374A | Womier |
| 14234 | 0x379A | DrunkDeer |
| 14505 | 0x38A9 | Unknown |
| 14574 | 0x38EE | Unknown |
| 9642 | 0x25AA | Cherry |

### Device Flags

| Flag | Value | Meaning |
|------|-------|---------|
| dongle_common | true | 2.4GHz wireless dongle |
| ble | true | Bluetooth LE device |
| reportRate | 1000/8000 | Max polling rate |

## Command Format

### Standard Command (65 bytes)

```
┌────────┬────────┬────────┬────────┬────────┬────────┬────────┬──────────┬──────────┐
│ Byte 0 │ Byte 1 │ Byte 2 │ Byte 3 │ Byte 4 │ Byte 5 │ Byte 6 │ Byte 7   │ Byte 8+  │
├────────┼────────┼────────┼────────┼────────┼────────┼────────┼──────────┼──────────┤
│ Report │ CMD    │ Param1 │ Param2 │ Param3 │ Param4 │ Param5 │ Checksum │ Payload  │
│ ID (5) │        │        │        │        │        │        │          │          │
└────────┴────────┴────────┴────────┴────────┴────────┴────────┴──────────┴──────────┘
```

Note: On Linux, Report ID is prepended as byte 0. On Windows, it may be handled differently by the HID API.
