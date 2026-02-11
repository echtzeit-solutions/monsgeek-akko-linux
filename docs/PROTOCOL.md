# MonsGeek/Akko HID Protocol Specification

This document is the single source of truth for the MonsGeek/Akko keyboard HID protocol, covering USB wired, 2.4GHz wireless (dongle), and Bluetooth LE connections.

## Table of Contents

1. [Overview](#1-overview)
2. [Transport Layer](#2-transport-layer)
3. [Message Format](#3-message-format)
4. [Command Reference](#4-command-reference)
5. [Data Structures](#5-data-structures)
6. [Events & Notifications](#6-events--notifications)
7. [Device Database](#7-device-database)
8. [Firmware Update (RY Bootloader)](#8-firmware-update-ry-bootloader)
9. [Firmware Limits: Chunked SET Commands](#firmware-limits-chunked-set-commands)

---

## 1. Overview

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Web App / Electron App (React UI)                      │
│  - app.monsgeek.com / web.akkogear.com                  │
│  - Uses @protobuf-ts/grpcweb-transport                  │
└─────────────────────┬───────────────────────────────────┘
                      │ gRPC-Web (HTTP/2)
                      │ localhost:3814
                      ▼
┌─────────────────────────────────────────────────────────┐
│  iot_driver (Rust binary)                               │
│  - gRPC server using tonic                              │
│  - HID access via hidapi                                │
│  - BLE support via btleplug                             │
└─────────────────────┬───────────────────────────────────┘
                      │ HID Feature Reports (65 bytes)
                      ▼
┌─────────────────────────────────────────────────────────┐
│  Keyboard (RY5088/YC3121 Firmware)                      │
│  - Interface 2: Vendor HID (0xFFFF:0x02)                │
│  - 64-byte feature reports with checksum                │
└─────────────────────────────────────────────────────────┘
```

### Protocol Stack

| Layer | Details |
|-------|---------|
| **Vendor Protocol** (FEA_CMD_*) | SET_LEDPARAM, GET_BATTERY, SET_KEYMATRIX, etc. |
| **HID Reports** | Feature Reports (bidirectional, 65 bytes), Input Reports (events), Output Reports (LEDs) |
| **HID Interfaces** | Interface 0: Keyboard, Interface 1: Vendor Input/Consumer, Interface 2: Vendor Config |
| **Transport** | USB / 2.4GHz RF (via dongle) / Bluetooth LE |

### Connection Types Summary

| Type | Battery Method | Event Channel | Command Latency |
|------|---------------|---------------|-----------------|
| USB Wired | GET_BATTERY (0x83) | Feature reports | ~1ms |
| 2.4GHz Dongle | F7 command → EP2 | EP2 interrupts | ~220ms RF round-trip |
| Bluetooth LE | BLE Battery Service | GATT notifications | Variable |

---

## 2. Transport Layer

### 2.1 USB Wired

**Device Identification:**
```
Vendor ID:      0x3151 (12625)
Product ID:     0x5030 (M1 V5 HE wired)
Usage Page:     0xFFFF (Vendor defined)
Usage:          0x02
Interface:      2
Report Size:    64 bytes
Report ID:      0
```

**HID Interfaces:**

| Interface | Descriptor | Purpose | Report IDs |
|-----------|------------|---------|------------|
| 0 | Boot Keyboard | Standard 6KRO keyboard | 0x01 |
| 1 | Multi-function | Mouse (0x02), Consumer (0x03), NKRO (0x01), Vendor Input (0x05) | 0x01, 0x02, 0x03, 0x05 |
| 2 | Vendor Config | Feature reports for configuration | 0x00 |

**Report Descriptors:**

Interface 2 (Config) - 20 bytes:
```
06 ff ff 09 02 a1 01 09 02 15 80 25 7f 95 40 75 08 b1 02 c0
```
Decoded: Usage Page 0xFFFF, Usage 0x02, Feature Report 64 bytes signed.

**Communication Pattern:**
```
1. Send: SET_FEATURE Report ID 0 (64 bytes command)
2. Recv: GET_FEATURE Report ID 0 (64 bytes response)
```

### 2.2 2.4GHz Wireless (Dongle)

**Device Identification:**
```
Vendor ID:      0x3151
Product ID:     0x5038 (M1 V5 HE dongle)
                0x5037 (Other models)
```

**USB Interfaces:**

| Interface | Descriptor | Endpoint | Actual Usage |
|-----------|------------|----------|--------------|
| 0 | Boot Keyboard | EP1 (0x81) | Keyboard HID input |
| 1 | Boot Keyboard | EP2 (0x82) | Vendor notifications (misadvertised!) |
| 2 | Vendor | EP3 (0x83) | Feature reports |

> **Note:** Interface 1 claims "Boot Keyboard" but sends vendor reports (Report ID 0x05). This is a firmware compatibility workaround.

**Endpoint Behavior:**

- **EP0 (Control):** All vendor commands use SET_REPORT/GET_REPORT to interface 2.
- **EP1 (0x81):** Standard keyboard HID input from interface 0.
- **EP2 (0x82):** Vendor notifications despite being on "keyboard" interface:
  - Dial rotation, mode toggle
  - Settings saved ACK
  - Battery status
  - Key depth reports
- **EP3 (0x83):** Vendor interface interrupt (polled on open, no unsolicited data).

**Command Patterns:**

*Pattern 1: Immediate Response (F7 Battery)*
```
SET_REPORT(id=0): f7 00 00 00 00 00 00 08 ...
GET_REPORT(id=5): → response ready immediately (~20µs)
```

*Pattern 2: Flush-Based Response (8F Settings)*
```
SET_REPORT(id=0): 8f 00 00 00 00 00 00 70 ...
SET_REPORT(id=0): fc 00 00 00 00 00 00 03 ...  (FC flush)
GET_REPORT(id=0): → response ready
```

*Pattern 3: Write-Only with ACK (0x11 Sleep)*
```
SET_REPORT(id=0): 11 01 00 00 00 00 00 ed ...
SET_REPORT(id=0): fc 00 00 00 00 00 00 03 ...  (FC flush)
... ~220ms later via EP2 ...
05 0f 01  (keyboard ACK)
05 0f 00  (ACK complete)
```

**Battery Query Protocol (F7):**

The dongle does NOT automatically poll battery. Without sending F7:
- GET_FEATURE returns stale/cached data
- After device replug, GET_FEATURE returns zeros

```python
# Send F7 to refresh battery
cmd = bytearray([0x00, 0xF7] + [0]*62)
fcntl.ioctl(fd, HIDIOCSFEATURE(64), cmd)

# Read battery (Report ID 5)
buf = bytearray([0x05] + [0]*63)
fcntl.ioctl(fd, HIDIOCGFEATURE(64), buf)
battery = buf[1]  # 0-100%
```

**Battery Response Format:**

| Byte | Field | Values |
|------|-------|--------|
| 0 | Report ID | 0x00 (firmware quirk) |
| 1 | Battery % | 1-100 |
| 2 | Unknown | 0x00 |
| 3 | Idle | 0=active, 1=sleeping |
| 4 | Online | 0=disconnected, 1=connected |
| 5-6 | Marker | 0x01, 0x01 |

### 2.3 Bluetooth LE

**Device Identification:**
```
VID:        0x3151
PID:        0x5027 (M1 V5 HE Bluetooth)
Bus Type:   Bluetooth (0x0005)
Usage Page: 0xFF55 (vendor)
Usage:      0x0202 (vendor)
Report ID:  6
```

**GATT Structure:**

| Characteristic | Report ID | Type | Flags | Notes |
|---------------|-----------|------|-------|-------|
| char0032 | 6 | Input | read, notify | Vendor responses (65 bytes) |
| char0039 | 6 | Output | write | Vendor commands (65 bytes) |
| char0036 | 1 | Output | write | Keyboard LED output |

**Protocol Analysis:**

1. Vendor protocol transported over GATT (HOGP) using Report characteristics.
2. Commands written to vendor Output Report, responses via notifications on Input Report.
3. Payload framed with leading marker byte:
   - **0x55**: command/response channel
   - **0x66**: event channel
4. Checksum applies starting at the `cmd` byte (skipping the 0x55 marker).

**Windows Capture Example:**
```
OUT: ATT.WriteCommand(handle=0x003a, value[65])
     value: 55 8f ...  (0x55 marker + 0x8f command)

IN:  ATT.HandleValueNotification(handle=0x0033, value[65])
     value: 55 8f 85 0b ...  (response with device id 0x0b85)
```

**Limitations:**

| Feature | Status |
|---------|--------|
| Vendor Events (Fn key) | Works |
| Battery | Via BLE Battery Service (UUID 0x180F), NOT vendor commands |
| Keyboard Input | Works (standard HID) |
| GET Commands | ATT writes succeed, no notification response |
| SET Commands | ATT writes succeed, keyboard ignores them |

**Battery Access (BLE):**
```bash
# Via bluetoothctl
bluetoothctl info F4:EE:25:AF:3A:38 | grep "Battery Percentage"

# Via D-Bus
org.bluez /org/bluez/hci0/dev_XX_XX_XX_XX_XX_XX
org.bluez.Battery1 interface → Percentage property
```

**Event Format Difference:**
```
USB:       [Report ID 0x05] [type] [value] ...
Bluetooth: [Report ID 0x06] [0x66] [type] [value] ...
```

---

## 3. Message Format

### 3.1 Command Format (65 bytes)

```
┌────────┬────────┬────────┬────────┬────────┬────────┬────────┬──────────┬──────────┐
│ Byte 0 │ Byte 1 │ Byte 2 │ Byte 3 │ Byte 4 │ Byte 5 │ Byte 6 │ Byte 7   │ Byte 8+  │
├────────┼────────┼────────┼────────┼────────┼────────┼────────┼──────────┼──────────┤
│ Report │ CMD    │ Param1 │ Param2 │ Param3 │ Param4 │ Param5 │ Checksum │ Payload  │
│ ID (0) │        │        │        │        │        │        │ (Bit7)   │          │
└────────┴────────┴────────┴────────┴────────┴────────┴────────┴──────────┴──────────┘
```

> **Note:** On Linux, Report ID is prepended as byte 0 (total 65 bytes). On Windows, HID API may handle it differently.

### 3.2 Checksum Algorithms

**Bit7 Mode (most common):**
```javascript
// Pad message to 8 bytes minimum
const dt = d.length < 8 ? [...d, ...new Array(8 - d.length).fill(0)] : [...d];
// Calculate checksum over bytes 0-6 (cmd + params)
const sum = dt.slice(0, 7).reduce((a, b) => a + b, 0);
// Store inverted checksum at byte 7
dt[7] = 255 - (sum & 255);
```

**Bit8 Mode (LED commands):**
```javascript
const sum = dt.slice(0, 8).reduce((a, b) => a + b, 0);
dt[8] = 255 - (sum & 255);
```

**Example: F7 Command**
```
Bytes 1-7: f7 00 00 00 00 00 00
Sum: 0xF7 = 247
Checksum: 255 - 247 = 8 = 0x08
Full: 00 f7 00 00 00 00 00 08 00 00 ...
```

### 3.3 Response Validation

- **Byte 0:** Echoes command ID
- **Byte 1:** `0xAA` (170) indicates success
- **Byte 1:** `0x00` may indicate failure or no data

### 3.4 Linux hidraw Buffering

**Critical:** On Linux with hidraw, responses aren't immediately available:

1. After `HIDIOCSFEATURE` (send), response isn't ready immediately
2. `HIDIOCGFEATURE` (read) may return **previous** buffered response
3. Retry read 2-3 times with ~50-100ms delays

```python
def query(fd, cmd):
    for attempt in range(3):
        send_feature(fd, [cmd])
        time.sleep(0.1)
        resp = read_feature(fd)
        if resp[1] == cmd:  # Command echo matches
            return resp
    return None
```

### 3.5 Response Echo Behavior

Most commands echo their command byte at position 0 of the response, allowing the host to correlate responses with requests. However, some commands return raw data without this echo.

#### Commands WITH Command Echo (Standard)

All GET commands in the 0x80-0xE6 range echo their command byte:

| Command | Echo | Response Format |
|---------|------|-----------------|
| GET_REV (0x80) | ✓ | `[0x80, data...]` |
| GET_PROFILE (0x84) | ✓ | `[0x84, profile, ...]` |
| GET_DEBOUNCE (0x86) | ✓ | `[0x86, debounce_ms, ...]` |
| GET_LEDPARAM (0x87) | ✓ | `[0x87, mode, speed, ...]` |
| GET_USB_VERSION (0x8F) | ✓ | `[0x8F, device_id[4], ?, ?, version[2]]` |
| etc. | ✓ | First byte matches command |

#### Commands WITHOUT Command Echo (Raw Responses)

| Command | Response Format | Notes |
|---------|-----------------|-------|
| GET_MULTI_MAGNETISM (0xE5) | `[data_byte_0, data_byte_1, ...]` | 64 bytes raw, no echo |
| Battery (Report 0x05) | `[0x01, level, ...]` | Marker byte 0x01, not command echo |

**GET_MULTI_MAGNETISM Details:**

The response contains raw per-key data starting at byte 0:
- **2-byte values** (subcmds 0x00-0x03, 0x06, 0xFB, 0xFC, 0xFE): 32 × u16 LE per page
- **1-byte values** (subcmds 0x05, 0x07, 0x09): 64 × u8 per page
- **4-byte values** (subcmd 0x04 DKS): 16 × [u16, u16] per page

Since there's no command echo, the host must track:
1. Which subcmd was sent
2. Which page was requested
3. Expected response size based on subcmd type

**Battery Response (Dongle Only):**

Battery uses feature report 0x05, not a standard query. The response marker is 0x01:
```
[0x01, level(0-100), 0x00, idle(0/1), online(0/1), 0x01, 0x01, ...]
```

This is NOT a command echo—0x01 is a fixed marker byte for battery data.

#### Implementation Note

When implementing a protocol parser/monitor, track the last command sent to properly decode raw responses. For GET_MULTI_MAGNETISM, store:
- `last_subcmd`: The magnetism sub-command (0x00-0xFE)
- `last_page`: The page number (0-N)

Clear this context after receiving a response to avoid stale matches.

---

## 4. Command Reference

### 4.1 SET Commands (Host → Device)

#### Core Configuration (0x01-0x19)

| Hex | Name | Description | Checksum |
|-----|------|-------------|----------|
| 0x01 | SET_RESET | Factory reset device | Bit7 |
| 0x03 | SET_REPORT | Set polling rate (0-6 → 8000-125Hz) | Bit7 |
| 0x04 | SET_PROFILE | Change active profile (0-3) | Bit7 |
| 0x06 | SET_DEBOUNCE | Set debounce timing (ms) | Bit7 |
| 0x07 | SET_LEDPARAM | Set LED effect parameters | Bit8 |
| 0x08 | SET_SLEDPARAM | Set side/secondary LED params | Bit8 |
| 0x09 | SET_KBOPTION | Set keyboard options | Bit7 |
| 0x0A | SET_KEYMATRIX | Set key mappings (chunked, see [Limits](#firmware-limits-chunked-set-commands)) | Bit7 |
| 0x0B | SET_MACRO | Set macro definitions (chunked, see [Limits](#firmware-limits-chunked-set-commands)) | Bit7 |
| 0x0C | SET_USERPIC | Set per-key RGB colors (static) | Bit7 |
| 0x0D | SET_AUDIO_VIZ | Send 16 frequency bands for music mode | Bit7 |
| 0x0E | SET_SCREEN_COLOR | Send RGB for screen sync mode | Bit7 |
| 0x10 | SET_FN | Set Fn layer configuration (chunked, see [Limits](#firmware-limits-chunked-set-commands)) | Bit7 |
| 0x11 | SET_SLEEPTIME | Set sleep/deep sleep timeouts | Bit7 |
| 0x12 | SET_USERGIF | Set per-key RGB animation | Bit7 |
| 0x17 | SET_AUTOOS_EN | Set auto-OS detection | Bit7 |

#### Magnetism/HE Commands (0x1B-0x1E, 0x65)

| Hex | Name | Description |
|-----|------|-------------|
| 0x1B | SET_MAGNETISM_REPORT | Enable/disable key depth reporting |
| 0x1C | SET_MAGNETISM_CAL | Start min position calibration |
| 0x1D | SET_KEY_MAGNETISM_MODE | Set per-key trigger mode |
| 0x1E | SET_MAGNETISM_MAX_CAL | Start max position calibration |
| 0x65 | SET_MULTI_MAGNETISM | Bulk per-key actuation/RT/DKS |

#### OLED/Display Commands (0x20-0x32)

| Hex | Name | Description |
|-----|------|-------------|
| 0x22 | SET_OLEDOPTION | Set OLED display options |
| 0x25 | SET_TFTLCDDATA | Send TFT LCD image data |
| 0x27 | SET_OLEDLANGUAGE | Set OLED language |
| 0x28 | SET_OLEDCLOCK | Set OLED clock display |
| 0x29 | SET_SCREEN_24BITDATA | Set 24-bit color screen data |
| 0x30 | SET_OLEDBOOTLOADER | Enter OLED bootloader mode |
| 0x31 | SET_OLEDBOOTSTART | Start OLED firmware transfer |

### 4.2 GET Commands (Device → Host)

#### Core Configuration

| Hex | Name | Description |
|-----|------|-------------|
| 0x80 | GET_REV / GET_RF_VERSION | Firmware revision |
| 0x83 | GET_REPORT | Polling rate |
| 0x84 | GET_PROFILE | Active profile |
| 0x85 | GET_LEDONOFF | LED on/off state |
| 0x86 | GET_DEBOUNCE | Debounce settings |
| 0x87 | GET_LEDPARAM | LED parameters |
| 0x88 | GET_SLEDPARAM | Secondary LED params |
| 0x89 | GET_KBOPTION | Keyboard options |
| 0x8A | GET_KEYMATRIX | Key mappings |
| 0x8B | GET_MACRO | Macro data |
| 0x8C | GET_USERPIC | Per-key RGB colors |
| 0x8F | GET_USB_VERSION | USB firmware version / device ID |
| 0x90 | GET_FN | Fn layer |
| 0x91 | GET_SLEEPTIME | Sleep timeout |
| 0x97 | GET_AUTOOS_EN | Auto-OS setting |

#### Magnetism GET Commands

| Hex | Name | Description |
|-----|------|-------------|
| 0x9D | GET_KEY_MAGNETISM_MODE | Per-key trigger mode |
| 0xE5 | GET_MULTI_MAGNETISM | RT/DKS per-key settings |
| 0xE6 | GET_FEATURE_LIST | Supported features bitmap |

#### Version/Info Commands

| Hex | Name | Description |
|-----|------|-------------|
| 0xAD | GET_OLED_VERSION | OLED firmware version (u16 LE) |
| 0xAE | GET_MLED_VERSION | Matrix LED controller version (u16 LE) |
| 0xD0 | GET_SKU | Factory SKU |

#### Protocol Commands

| Hex | Name | Description |
|-----|------|-------------|
| 0xF7 | BATTERY_REFRESH | Wake dongle, query battery |
| 0xFC | FLUSH | Flush response buffer (no-op) |
| 0xFD | CAL_INIT? | Calibration app init/sync - only seen in calibration capture, sent first, echoes back zeros |
| 0xFE | GET_CALIBRATION | Raw sensor calibration values |

### 4.3 Magnetism Sub-Commands (0x65 / 0xE5)

Used with SET/GET_MULTI_MAGNETISM for per-key hall effect settings:

| Sub-cmd | Name | Description |
|---------|------|-------------|
| 0x00 | PRESS_TRAVEL | Actuation point (in precision units) |
| 0x01 | LIFT_TRAVEL | Release point |
| 0x02 | RT_PRESS | Rapid Trigger press sensitivity |
| 0x03 | RT_LIFT | Rapid Trigger lift sensitivity |
| 0x04 | DKS_TRAVEL | DKS (Dynamic Keystroke) travel |
| 0x05 | MODTAP_TIME | Mod-Tap activation time |
| 0x06 | BOTTOM_DEADZONE | Bottom dead zone |
| 0x07 | KEY_MODE | Mode flags |
| 0x09 | SNAPTAP_ENABLE | Snap Tap anti-SOCD enable |
| 0x0A | DKS_MODES | DKS trigger modes/actions |
| 0xFB | TOP_DEADZONE | Top dead zone (firmware >= 1024) |
| 0xFC | SWITCH_TYPE | Switch type (if replaceable) |
| 0xFE | CALIBRATION | Raw sensor calibration values |

**GET Format:** `[0xE5, sub_cmd, 0x01, profile, 0, 0, 0, checksum]`

**Key Mode Values:**

| Value | Mode |
|-------|------|
| 0 | Normal |
| 1 | Rapid Trigger |
| 2 | DKS (Dynamic Keystroke) |
| 3 | Mod-Tap |
| 4 | Toggle |
| 5 | Snap Tap |

---

## 5. Data Structures

### 5.1 LED Parameters (0x87 / 0x07)

**Response format:**
```
Byte 1: Mode (0-25)
Byte 2: Speed (inverted: MAXSPEED - actual, where MAXSPEED=4)
Byte 3: Brightness (0-4)
Byte 4: Options (high nibble = direction, low nibble = dazzle flag)
Byte 5: Red
Byte 6: Green
Byte 7: Blue
```

**Options byte decoding:**
- `option = byte[4] >> 4` (direction/variant)
- `dazzle = (byte[4] & 0x0F) == 8` (rainbow color cycle)
- `normal = (byte[4] & 0x0F) == 7` (single color)

**LED Effect Modes:**

| Value | Name | Description |
|-------|------|-------------|
| 0 | Off | LEDs disabled |
| 1 | Constant | Static color |
| 2 | Breathing | Pulsing effect |
| 3 | Neon | Neon glow |
| 4 | Wave | Color wave |
| 5 | Ripple | Ripple from keypress |
| 6 | Raindrop | Raindrops falling |
| 7 | Snake | Snake pattern |
| 8 | Reactive | React to keypress (stay lit) |
| 9 | Converge | Converging pattern |
| 10 | Sine Wave | Sine wave pattern |
| 11 | Kaleidoscope | Kaleidoscope effect |
| 12 | Line Wave | Line wave pattern |
| 13 | User Picture | Custom per-key (4 layers) |
| 14 | Laser | Laser effect |
| 15 | Circle Wave | Circular wave |
| 16 | Rainbow | Rainbow/dazzle effect |
| 17 | Rain Down | Rain downward |
| 18 | Meteor | Meteor shower |
| 19 | Reactive Off | React briefly then fade |
| 20 | Music Patterns | Audio reactive (uses 0x0D) |
| 21 | Screen Sync | Ambient RGB (uses 0x0E) |
| 22 | Music Bars | Audio reactive bars (uses 0x0D) |
| 23 | Train | Train pattern |
| 24 | Fireworks | Fireworks effect |
| 25 | Per-Key Color | Dynamic animation (GIF) |

### 5.2 Polling Rates

| Code | Rate | Interval |
|------|------|----------|
| 0 | 8000 Hz | 0.125ms |
| 1 | 4000 Hz | 0.25ms |
| 2 | 2000 Hz | 0.5ms |
| 3 | 1000 Hz | 1ms |
| 4 | 500 Hz | 2ms |
| 5 | 250 Hz | 4ms |
| 6 | 125 Hz | 8ms |

### 5.3 Magnetism/Trigger Settings

**Travel Value Conversion:**
```javascript
// Reading: divide by precision factor
travel_mm = raw_value / precision_factor;

// Writing: multiply by precision factor
raw_value = travel_mm * precision_factor;

// Example with precision=10 (0.1mm):
// raw=20 → 2.0mm actuation point
// raw=3  → 0.3mm RT sensitivity
```

**Normal mode per-key data:**
```javascript
{
    travel: 20,           // Actuation point (2.0mm)
    liftTravel: 20,       // Release point (2.0mm)
    deadZoneTravel: 0,    // Bottom dead zone
    topDeadZoneTravel: 0, // Top dead zone
    fire: false,          // Rapid Trigger enabled
    firePressTravel: 3,   // RT press sensitivity (0.3mm)
    fireLiftTravel: 3     // RT release sensitivity (0.3mm)
}
```

**DKS mode per-key data:**
```javascript
{
    dynamicTravel: 10,    // DKS activation travel
    triggerModes: [1, 2, 0, 3]  // Actions at 4 depth levels
}
```

### 5.4 Key Matrix Format

```rust
// Each key is 4 bytes: [byte0, byte1, keycode, modifier]
// Matrix size = num_keys * 4

enum ConfigType {
    Keyboard,      // [0, modifier, keycode, 0]
    Combo,         // [0, modifier, key1, key2]
    Mouse,         // [1, 0, mouse_code, 0]
    Macro,         // [9, macro_type, macro_index, 0]
    Function,      // [3, 0, func_code_lo, func_code_hi]
    Forbidden,     // [0, 0, 0, 0]
    Gamepad,       // [21, type, code, 0]
    ControlRecoil, // [22, method, gun_index, 0]
    Snap,          // [22, number, keycode, 0]
}
```

### 5.5 Macro Format

```rust
struct Macro {
    repeat_count: u16,  // Bytes 0-1, little endian
    events: Vec<MacroEvent>,
}

enum MacroEvent {
    Keyboard { action: KeyAction, keycode: u8 },
    MouseButton { action: KeyAction, button: MouseKey },
    MouseMove { dx: i8, dy: i8 },
    Delay { ms: u16 },
}

// Macro byte encoding:
// Keyboard: [keycode, delay_or_action]
//   - delay_or_action & 0x80 = down, else up
//   - delay_or_action & 0x7F = delay if < 128
// Mouse move: [0xF9, delay_short, dx, dy]
```

---

## 6. Events & Notifications

The keyboard sends unsolicited notifications via EP2 (dongle) or the vendor input interface (wired). These report settings changes made via Fn key combinations and other hardware events.

### 6.1 Event Report Format

All vendor events use Report ID `0x05`:
```
[0x05] [event_type] [value1] [value2] [value3] ...
```

### 6.2 Settings Sync Events (0x0F)

The SettingsAck event indicates when the keyboard is saving settings to flash:

| Bytes | Meaning |
|-------|---------|
| `05 0F 01` | Settings save **started** |
| `05 0F 00` | Settings save **complete** |

**When sent:**
- After any SET command that modifies persistent settings
- After Fn key combinations that change settings (LED mode, brightness, etc.)
- ~220ms after command on dongle (RF round-trip delay)

**Commands that trigger SettingsAck:**
- SET_PROFILE (0x04)
- SET_LEDPARAM (0x07)
- SET_SLEDPARAM (0x08)
- SET_KBOPTION (0x09)
- SET_SLEEPTIME (0x11)
- Most other SET commands

**Commands that do NOT trigger SettingsAck:**
- SET_MAGNETISM_REPORT (0x1B) - monitor mode toggle
- BATTERY_REFRESH (0xF7) - battery query
- SET_AUDIO_VIZ (0x0D) - streaming data
- SET_SCREEN_COLOR (0x0E) - streaming data

### 6.3 Setting Changed Events

These events are sent when the user changes settings via Fn key combinations:

#### Profile Change (0x01)
```
05 01 [profile]    Profile changed to 0-3 (via Fn+F9/F10/F11/F12)
```

#### Keyboard Function Events (0x03)
```
05 03 [state] 01   Win key lock toggled (via Fn+Win)
                   state: 0=unlocked, 1=locked

05 03 [state] 03   WASD/Arrow swap toggled (via Fn+W)
                   state: 0=normal, 8=swapped

05 03 [layer] 08   Fn layer toggled (via Fn+Alt)
                   layer: 0=default, 1=alternate

05 03 04 09        Backlight toggled (via Fn+L)

05 03 00 11        Dial mode toggled (volume ↔ brightness)
```

#### Main LED Events (0x04-0x07)
```
05 04 [mode]       LED effect mode changed (via Fn+Home/PgUp/End/PgDn)
                   mode: 1-20 (cycles through effects in groups of 5)

05 05 [speed]      LED speed changed (via Fn+←/→)
                   speed: 0-4

05 06 [level]      LED brightness changed (via Fn+↑/↓ or dial)
                   level: 0-4

05 07 [color]      LED color changed (via Fn+\)
                   color: 0-7 (red, green, blue, orange, magenta, yellow, white, rainbow)
```

#### Side LED Events (0x08-0x0B)
```
05 08 [mode]       Side LED effect mode changed
05 09 [speed]      Side LED speed changed
05 0A [level]      Side LED brightness changed
05 0B [color]      Side LED color changed
```

### 6.4 System Events

#### Reset Event (0x0D)
```
05 0D 00           Factory reset triggered (via Fn+~)
```
Followed by SettingsAck (0x0F 0x01, then 0x0F 0x00) when complete.

#### Sleep Mode Change (0x13)
```
05 13 [state]      Sleep mode changed
```

#### Magnetic Mode Change (0x1D)
```
05 1D [mode]       Per-key magnetic mode changed
```

#### Screen Clear Complete (0x2C)
```
05 2C 00           OLED/TFT screen clear operation complete
```

### 6.5 Battery Status (0x88)

Via dongle EP2 after F7 command:
```
05 88 00 00 [level] [flags]
```

| Field | Description |
|-------|-------------|
| level | Battery percentage (0-100) |
| flags | bit 0 = online, bit 1 = charging |

### 6.6 Vendor Input Report Table (Complete)

| Type | Bytes 2-4 | Event | Trigger |
|------|-----------|-------|---------|
| 0x00 | 00 00 00 | Wake | Keyboard wake from sleep |
| 0x01 | profile - - | ProfileChange | Fn+F9..F12 |
| 0x03 | state 01 - | WinLockToggle | Fn+Win |
| 0x03 | state 03 - | WasdSwapToggle | Fn+W |
| 0x03 | layer 08 - | FnLayerToggle | Fn+Alt |
| 0x03 | 04 09 - | BacklightToggle | Fn+L |
| 0x03 | 00 11 - | DialModeToggle | Dial press |
| 0x04 | mode - - | LedEffectMode | Fn+Home/PgUp/End/PgDn |
| 0x05 | speed - - | LedEffectSpeed | Fn+←/→ |
| 0x06 | level - - | BrightnessLevel | Fn+↑/↓, Dial |
| 0x07 | color - - | LedColor | Fn+\ |
| 0x08 | mode - - | SideLedMode | Side LED change |
| 0x09 | speed - - | SideLedSpeed | Side LED change |
| 0x0A | level - - | SideLedBrightness | Side LED change |
| 0x0B | color - - | SideLedColor | Side LED change |
| 0x0D | 00 - - | ResetTriggered | Fn+~ (factory reset) |
| 0x0F | status - - | SettingsAck | Settings saved (1=start, 0=done) |
| 0x13 | state - - | SleepModeChange | Sleep state changed |
| 0x1B | lo hi idx | KeyDepth | Key depth (when monitoring enabled) |
| 0x1D | mode - - | MagneticModeChange | Per-key mode changed |
| 0x2C | 00 - - | ScreenClearDone | Screen clear complete |
| 0x88 | 00 00 lvl flags | BatteryStatus | After F7 query |

### 6.2 Mouse Reports (Report ID 0x02)

The keyboard includes a built-in mouse function for gaming macros, dial mouse mode, or other pointing features. Mouse reports are sent on Interface 1 (EP2 IN, 0x82).

**Report Format (9 bytes):**
```
[0x02] [buttons] [00] [X_lo] [X_hi] [Y_lo] [Y_hi] [wheel_lo] [wheel_hi]
```

| Byte | Field | Description |
|------|-------|-------------|
| 0 | Report ID | 0x02 |
| 1 | Buttons | Bitmap: bit 0 = left, bit 1 = right, bit 2 = middle |
| 2 | Reserved | Always 0x00 |
| 3-4 | X movement | Signed 16-bit LE (negative = left) |
| 5-6 | Y movement | Signed 16-bit LE (negative = up) |
| 7-8 | Wheel | Signed 16-bit LE (negative = scroll up) |

**Example:**
```
02 00 00 ff ff 00 00 00 00   # X=-1, Y=0 (small leftward movement)
02 01 00 05 00 fb ff 00 00   # Left button + X=5, Y=-5
```

### 6.3 Consumer Reports (Report ID 0x03)

Standard HID Consumer Page codes (16-bit LE usage):

| Report | Usage | Description |
|--------|-------|-------------|
| 03 e9 00 | 0x00E9 | Volume Up |
| 03 ea 00 | 0x00EA | Volume Down |
| 03 cd 00 | 0x00CD | Play/Pause |
| 03 b6 00 | 0x00B6 | Previous Track |
| 03 b5 00 | 0x00B5 | Next Track |
| 03 94 01 | 0x0194 | My Computer |
| 03 8a 01 | 0x018A | Email |
| 03 00 00 | - | Release |

### 6.4 Key Depth Monitoring (0x1B)

**Enable/Disable:**
```
Enable:  [0x1B, 0x01, 0, 0, 0, 0, 0, checksum]
Disable: [0x1B, 0x00, 0, 0, 0, 0, 0, checksum]
```

**Key Depth Reports (via EP2/input):**
```
05 1b 0f 00 29    depth=15,  key=41 (press start)
05 1b 69 01 29    depth=361, key=41 (bottom out)
05 1b 00 00 29    depth=0,   key=41 (release)
```

- Reports arrive at ~3-20ms intervals during key movement
- Depth values typically range 0-400+ depending on switch travel
- Decode: `depth = (byte3 << 8) | byte2`, `key_index = byte4`

### 6.5 Calibration Events

| Bytes | Event |
|-------|-------|
| 0x0F, 0x01, 0x00 | Calibration started |
| 0x0F, 0x00, 0x00 | Calibration stopped |

**Calibration Procedure:**
1. Send min calibration start (0x1C, 1) - keys should be released
2. Wait 2000ms
3. Send min calibration stop (0x1C, 0)
4. Send max calibration start (0x1E, 1) - user presses all keys
5. User presses all keys fully
6. Send max calibration stop (0x1E, 0)

---

## 7. Device Database

### 7.1 Vendor IDs

| VID | Hex | Manufacturer |
|-----|-----|--------------|
| 12625 | 0x3151 | MonsGeek/Akko |
| 5215 | 0x145F | Akko (alternate) |
| 13357 | 0x342D | Epomaker |
| 13434 | 0x347A | Feker |
| 14154 | 0x374A | Womier |
| 14234 | 0x379A | DrunkDeer |
| 9642 | 0x25AA | Cherry |

### 7.2 MonsGeek/Akko Product IDs

| PID | Type | Model |
|-----|------|-------|
| 0x5030 | Wired | M1 V5 HE USB |
| 0x503A | Dongle | M1 V5 HE 2.4GHz |
| 0x5038 | Dongle | M1 V5 HE TMR 2.4GHz |
| 0x5037 | Dongle | Other models |
| 0x5027 | Bluetooth | M1 V5 HE BT |
| 0x5029 | Wired | TITAN68HE |
| 0x502D | Wired | X65HE |
| 0x4007 | Wired | Generic wired |
| 0x4012 | Bluetooth | Generic BT |

### 7.3 Bootloader PIDs

When in bootloader mode, devices use different VID/PID:

| VID | PID | Mode |
|-----|-----|------|
| 0x3141 | 0x504A | USB boot mode 1 |
| 0x3141 | 0x404A | USB boot mode 2 |
| 0x046A | 0x012E | RF boot mode 1 |
| 0x046A | 0x0130 | RF boot mode 2 |

### 7.4 HID Interface Parameters

Standard configuration interface:
```
Usage Page:     0xFFFF (Vendor)
Usage:          0x02
Interface:      2
Report Size:    64 bytes
Report ID:      0
```

---

## 8. Firmware Update (RY Bootloader)

The RY bootloader occupies the first 20KB of flash (0x08000000–0x08004FFF) and handles firmware updates via a vendor HID protocol over USB Feature Reports.

### 8.1 Boot Decision

On every power-on/reset, the bootloader decides whether to boot the firmware or stay in update mode:

1. **Read mailbox** at flash 0x08004800 (4 bytes)
2. **Read chip ID** from 0x08005000 (14 bytes: `"AT32F405 8KMKB"`)
3. **Compare:**
   - If mailbox == `0x55AA55AA` → **stay in bootloader** (firmware update requested)
   - If chip ID doesn't match expected string → **stay in bootloader** (no valid firmware)
   - Otherwise → **boot firmware**: set VTOR to 0x08005200, load SP from vector table, jump to reset handler

### 8.2 Bootloader Device Identification

```
VID:        0x3151
PID:        0x502A (MonsGeek M1 V5 HE TMR)
Usage Page: 0xFF01
Report Size: 64 bytes
Report ID:  0
```

Additional bootloader VID/PIDs for other models are listed in section 7.3.

### 8.3 Update Protocol Sequence

The firmware update is a multi-phase process using Feature Reports:

```
Phase 1: Prepare (normal mode, PID=0x5030)
──────────────────────────────────────────
1. SET_REPORT: ISP_PREPARE      [0xC5, 0x3A, 0,0,0,0,0, checksum]
2. SET_REPORT: ENTER_BOOTLOADER [0x7F, 0x55, 0xAA, 0x55, 0xAA, 0,0, checksum]
   → Device erases config, writes 0x55AA55AA to mailbox, reboots
   → Device re-enumerates as PID=0x502A

Phase 2: Transfer (bootloader mode, PID=0x502A)
────────────────────────────────────────────────
3. SET_REPORT: FW_TRANSFER_START
   [0xBA, 0xC0, chunk_count_lo, chunk_count_hi, size_lo, size_mid, size_hi, 0, ...]

4. GET_REPORT: Read ack (1 report)

5. SET_REPORT × N: Firmware data chunks (64 bytes each)
   Last chunk padded with 0xFF if firmware size is not a multiple of 64.

6. SET_REPORT: FW_TRANSFER_COMPLETE
   [0xBA, 0xC2, chunk_count(2B), checksum(4B LE), size(4B LE), 0, ...]

Phase 3: Verification (bootloader validates, then reboots)
──────────────────────────────────────────────────────────
7. Bootloader compares received checksum (masked to 24 bits) with its
   running sum. If match AND no transfer errors:
   - Erases flash page at 0x08004800 (clears mailbox)
   - Reboots → firmware boots normally
8. If mismatch or errors:
   - Does NOT clear mailbox
   - Reboots → stays in bootloader mode (mailbox still 0x55AA55AA)
```

### 8.4 Checksum Calculation

The bootloader accumulates a running checksum by summing **every byte of every 64-byte chunk**, including 0xFF padding bytes in the last chunk. The host must match this exactly.

```python
def calculate_checksum(firmware_data: bytes) -> int:
    """Calculate checksum matching the bootloader's algorithm."""
    total = sum(firmware_data)
    remainder = len(firmware_data) % 64
    if remainder != 0:
        # Bootloader checksums the full 64-byte chunk including padding
        total += (64 - remainder) * 0xFF
    return total
```

The FW_TRANSFER_COMPLETE command sends the checksum as 4 bytes (little-endian u32), but the bootloader only compares the lower 24 bits (`checksum & 0xFFFFFF`).

> **Bug:** If the firmware size is an exact multiple of 64, no padding exists and host/bootloader checksums naturally agree. Firmware sizes that are NOT multiples of 64 will cause a checksum mismatch if the host omits the padding bytes from its calculation, leaving the device stuck in bootloader mode.

### 8.5 ENTER_BOOTLOADER Side Effects

The `ENTER_BOOTLOADER` command (0x7F + 0x55AA55AA magic) triggers:

1. **Config erase** — the firmware erases the config header at 0x08028000 before writing the mailbox. This means all LED settings, profiles, keymaps, macros, and Fn layers are lost on every firmware update.
2. **Mailbox write** — writes 0x55AA55AA to flash 0x08004800.
3. **Immediate reboot** — the device resets, re-enumerates with bootloader PID. The USB SET_REPORT may return EIO (expected).

### 8.6 Recovery via AT32 ROM DFU

If the RY bootloader itself is non-functional, the AT32F405's built-in ROM bootloader provides a fallback:

1. Bridge the **BOOT0** pad to 3.3V (VDD)
2. Plug USB (or reset while bridged)
3. Device enumerates as `VID:PID 2e3c:df11` ("Artery-Tech DFU in FS Mode")

```bash
# Read flash (e.g., dump entire 256KB)
dfu-util -a 0 -d 2e3c:df11 --dfuse-address 0x08000000 -U dump.bin --upload-size 262144

# Write firmware (starting at 0x08005000, after bootloader)
dfu-util -a 0 -d 2e3c:df11 --dfuse-address 0x08005000 -D firmware.bin

# Factory reset (erase config only, 2KB at 0x08028000)
# Write 2KB of 0xFF to config region
dfu-util -a 0 -d 2e3c:df11 --dfuse-address 0x08028000 -D ff_2k.bin
```

> **Warning:** Do NOT write to 0x08000000–0x08004FFF unless restoring a bootloader backup. Corrupting the bootloader requires physical BOOT0 access to recover.

### 8.7 Flash Memory Map

```
0x08000000  ┌──────────────────────────┐
            │  RY Bootloader (20KB)    │  Protected, do not overwrite
0x08004800  │  ├─ Mailbox (4B)         │  0x55AA55AA = enter bootloader
0x08005000  ├──────────────────────────┤
            │  Chip ID Header (512B)   │  "AT32F405 8KMKB\0\0..."
0x08005200  │  Vector Table (64B)      │  VTOR set here by bootloader
0x08005240  │  Firmware Code + Data    │  ~129KB for v407
0x08025800  │  [Patch Zone] (10KB)     │  Gap between code and config
0x08028000  ├──────────────────────────┤
            │  Config Header (2KB)     │  Profile, LED, settings
0x08028800  │  Keymaps                 │
0x0802A800  │  FN Layers               │
0x0802B800  │  Macros                  │
0x0802F800  │  User Pictures / LEDs    │
0x08032000  │  Magnetism Calibration   │  Preserved across factory reset
0x08033800  │  Magnetism Per-Key Data  │
0x08040000  └──────────────────────────┘  End of 256KB flash
```

---

## Firmware Limits: Chunked SET Commands

> **Warning:** The v407 firmware (AT32F405) performs **no bounds checking** on
> chunked SET commands. A malicious or buggy host can corrupt RAM or trigger
> stack buffer overflows. The limits below are derived from the firmware
> decompilation and must be enforced by the host driver.

### Chunked Write Protocol

`SET_KEYMATRIX` (0x0A), `SET_MACRO` (0x0B), and `SET_FN` (0x10) use a
multi-report chunked write protocol. The first report carries the
`slot_id` / `macro_id` / `layer_id` and the first chunk of payload; subsequent
reports carry continuation chunks. The firmware accumulates data into a shared
staging buffer at `g_vendor_cmd_buf + 0x42` (RAM 0x2000831E).

```
Report layout (64 bytes):
  [0]    command (0x0A / 0x0B / 0x10)
  [1]    slot/macro/layer id      ← NOT bounds-checked
  [2]    chunk_index (0-based)    ← NOT bounds-checked
  [3-6]  padding / params
  [7]    checksum
  [8-63] payload (up to 56 bytes per chunk)
```

### Per-Command Limits

| Command | ID Field | Safe Range | Staging Size | Overflow Target |
|---------|----------|------------|--------------|-----------------|
| SET_KEYMATRIX (0x0A) | layer_id (byte 1) | 0-5 | 514 bytes | g_rgb_anim_state (0x20008528) |
| SET_MACRO (0x0B) | macro_id (byte 1) | 0-31 | 514 bytes | g_rgb_anim_state (0x20008528) |
| SET_FN (0x10) | layer_id (byte 1) | 0-5 | 514 bytes | g_rgb_anim_state (0x20008528) |

**chunk_index** is also unchecked for all three commands. Each chunk writes 56
bytes at `staging + chunk_index * 56`. With staging at offset 0x42 inside the
588-byte `g_vendor_cmd_buf`, only ~9 chunks fit before overflowing into
adjacent RAM.

### SET_MACRO (0x0B) Flash Save Hazard

After chunked transfer, `flash_save_macro` (0x0800eaf0) writes the
accumulated macro to flash at `0x0802B800 + macro_id * 0x100`. Two paths:

1. **Single-event path** — writes one 256-byte flash page. Uses
   `macro_id` as an array index into a stack-allocated 16×256-byte buffer
   without bounds checking. macro_id > 15 overflows the stack frame.

2. **Multi-page path** — copies `accumulated_size` bytes through a
   stack-local 4096-byte page buffer. Same unbounded `macro_id` index.

**Impact:** A SET_MACRO with `macro_id >= 16` can overwrite the return
address on the stack, giving arbitrary code execution on the keyboard MCU.

### Host Driver Recommendations

```
assert 0 <= layer_id <= 5    for SET_KEYMATRIX / SET_FN
assert 0 <= macro_id <= 15   for SET_MACRO (flash path limit)
assert chunk_index <= 9      for all chunked commands
assert total_size <= 514     accumulated payload bytes
```

`SET_AUDIO_VIZ` (0x0D) is safe — it reads a fixed 16-band array from
byte offsets 1-48 with no index indirection.

---

## Timing Characteristics

| Operation | Timing | Notes |
|-----------|--------|-------|
| F7 SET→GET | ~20µs | Immediate, no delay needed |
| 8F SET→FC→GET | ~1ms | Flush provides sync |
| RF round-trip (ACK) | ~220ms | Keyboard ACK via EP2 |
| BT extra delay | +60ms send, +100ms read | Bluetooth timing |
| Reset operation | 2000ms | After factory reset |
| Flash operations | 1000ms | Flash erase/write |

---

## References

- USB HID Specification: https://www.usb.org/hid
- USB HID Usage Tables: https://usb.org/document-library/hid-usage-tables-15
- Linux usbmon: `/sys/kernel/debug/usb/usbmon/`
- Bluetooth HID over GATT Profile (HOGP)
