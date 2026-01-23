# MonsGeek/Akko Keyboard Protocol Overview

This document describes the communication protocol stack for MonsGeek/Akko keyboards, based on reverse engineering of the Windows `iot_driver.exe` and live USB packet captures on Linux.

> **Note**: This document primarily covers the **2.4GHz wireless dongle** (VID:3151 PID:5038). USB wired and Bluetooth connections may differ in endpoint layout, report IDs, and command patterns. The dongle-specific commands (F7, FC, 0x11) and EP2 notification channel are unique to the wireless dongle protocol.

## Protocol Stack

```
┌─────────────────────────────────────────────────────────────┐
│  MonsGeek/Akko Vendor Protocol (FEA_CMD_*)                  │
│  - SET_LEDPARAM, GET_BATTERY, SET_KEYMATRIX, etc.           │
├─────────────────────────────────────────────────────────────┤
│  HID Reports                                                │
│  - Feature Reports (bidirectional config, 65 bytes)         │
│  - Input Reports (keyboard→host events via interrupt)       │
│  - Output Reports (host→keyboard, e.g. LED indicators)      │
├─────────────────────────────────────────────────────────────┤
│  HID Interfaces (from USB descriptor)                       │
│  - Interface 0: Keyboard (Boot, EP1)                        │
│  - Interface 1: "Keyboard" (Boot, EP2) - actually vendor!   │
│  - Interface 2: Vendor (EP3, feature reports)           ←── │
├─────────────────────────────────────────────────────────────┤
│  USB Device (VID:PID, multiple interfaces)                  │
│  - VID 0x3151, PID 0x5038 (2.4GHz dongle)                   │
│  - VID 0x3151, PID 0x4007 (wired keyboard)                  │
├─────────────────────────────────────────────────────────────┤
│  USB (transport)                                            │
└─────────────────────────────────────────────────────────────┘
```

## USB Interfaces and Endpoints

The 2.4GHz dongle (VID:3151 PID:5038) exposes 3 HID interfaces, each with one interrupt endpoint:

| Interface | USB Descriptor | Endpoint | Actual Usage |
|-----------|----------------|----------|--------------|
| 0 | Boot Interface, Keyboard | EP1 (0x81) | Keyboard HID input |
| 1 | Boot Interface, Keyboard | EP2 (0x82) | Vendor notifications (misadvertised!) |
| 2 | Unknown subclass | EP3 (0x83) | Vendor feature reports |

> **Note:** Interface 1 claims to be "Boot Keyboard" in the USB descriptor but actually sends vendor-specific reports (Report ID 0x05). This is likely a compatibility workaround.

### Endpoint Behavior

**EP0 (Control)**: All vendor commands use SET_REPORT/GET_REPORT control transfers. Commands are sent to interface 2.

**EP1 (0x81) - Keyboard Input**: Standard keyboard HID reports from interface 0.

**EP2 (0x82) - Vendor Notifications**: Despite being on a "keyboard" interface, this endpoint receives vendor-specific reports:
- Dial rotation (volume/brightness)
- Dial mode toggle
- Settings saved ACK from keyboard

**EP3 (0x83) - Vendor Interface Interrupt**: This is the interrupt endpoint for interface 2 (the vendor interface used for feature reports). When our code opens the vendor hidraw device, the kernel automatically polls EP3. However, the dongle never sends unsolicited data here - all notifications go through EP2 instead. The `> INT-IN EP3` (submit) occurs on device open, `< INT-IN EP3` (complete) occurs on device close.

## HID Report IDs

Each report is identified by its first byte (Report ID):

| Report ID | Name | Interface | Usage |
|-----------|------|-----------|-------|
| 0x00 | CMD | Vendor (Feature) | SET_REPORT commands to device |
| 0x01 | KBD | Keyboard (Input) | Standard keyboard scancodes |
| 0x03 | CONSUMER | Consumer (Input) | Media keys, volume (standard HID) |
| 0x05 | VENDOR | Vendor (Feature/Input) | Vendor responses and notifications |

### Report ID 0x03 (Consumer)

Standard HID Consumer Page codes via interrupt (16-bit little-endian usage):
```
03 e9 00    Volume Up (Usage 0x00E9) - Dial in volume mode
03 ea 00    Volume Down (Usage 0x00EA) - Dial in volume mode
03 94 01    My Computer (Usage 0x0194) - Fn+F1
03 8a 01    Email (Usage 0x018A) - Fn+F2
03 21 02    Search (Usage 0x0221) - Fn+F3
03 23 02    Browser Home (Usage 0x0223) - Fn+F4
03 83 01    Media Player (Usage 0x0183) - Fn+F5
03 cd 00    Play/Pause (Usage 0x00CD) - Fn+F6
03 b6 00    Previous Track (Usage 0x00B6) - Fn+F7
03 b5 00    Next Track (Usage 0x00B5) - Fn+F8
03 00 00    Release
```

### Report ID 0x05 (Vendor)

Used for both feature report responses and interrupt notifications:

**Feature Report Response (via GET_REPORT):**
```
05 XX ...   Response to command (battery, settings, etc.)
```

**Interrupt Notifications (via EP2):**

The notification type byte (byte 1) uses firmware-internal codes:
```
05 00 00 ...   Keyboard wake notification (all zeros after report ID)
05 01 XX       Profile changed (XX = 0-3 for profiles 1-4, via Fn+F9..F12)
05 03 XX YY    Keyboard function changed (XX=category/state, YY=action)
               - 05 03 XX 01 = Win key lock (XX: 0=off, 1=on, Fn+Win)
               - 05 03 XX 03 = WASD/Arrow swap (XX: 0=normal, 8=swapped, Fn+W)
               - 05 03 XX 08 = Unknown toggle (XX: 0/1, Fn+RAlt)
               - 05 03 04 09 = Backlight toggled (Fn+L)
               - 05 03 00 11 = Dial mode toggled (volume ↔ brightness)
05 04 XX       LED effect mode (XX = effect ID 1-20)
               - Fn+Home: effects 1-5 (0x01-0x05)
               - Fn+PgUp: effects 6-10 (0x06-0x0A)
               - Fn+End: effects 11-15 (0x0B-0x0F)
               - Fn+PgDn: effects 16-20 (0x10-0x14)
05 05 XX       LED effect speed (XX = 0-4, Fn+=/Fn+- to adjust)
               Not emitted in static light mode
05 06 XX       Keyboard brightness level (XX = 0-4, via dial or Fn+Up/Down)
05 07 XX       LED color (XX = 0-7: red/green/blue/orange/magenta/yellow/white/rainbow, via Fn+\)
```

**Preliminary: LED notification type correlation**

Types 0x04-0x07 may correspond to LEDPARAM field indices + 4:
```
LEDPARAM payload: [cmd][mode][speed][bright][opt][r][g][b]
                        ↓     ↓      ↓            ↓
Notification:          0x04  0x05   0x06         0x07
Field index:            0     1      2            3
```
Formula (unconfirmed): `notification_type = ledparam_field_index + 4`
05 0f 01       Settings saved ACK (keyboard confirmed via RF)
05 0f 00       Settings saved ACK complete
```

**Not notified via interrupt** (must poll F7):
- Keyboard going idle/sleep
- Keyboard disconnect

## Vendor Protocol Commands

### Command Format (SET_REPORT, 65 bytes)

```
┌────────┬────────┬────────┬────────┬────────┬────────┬────────┬──────────┬──────────┐
│ Byte 0 │ Byte 1 │ Byte 2 │ Byte 3 │ Byte 4 │ Byte 5 │ Byte 6 │ Byte 7   │ Byte 8+  │
├────────┼────────┼────────┼────────┼────────┼────────┼────────┼──────────┼──────────┤
│ Report │ CMD    │ Param1 │ Param2 │ Param3 │ Param4 │ Param5 │ Checksum │ Padding  │
│ ID=0   │        │        │        │        │        │        │ (Bit7)   │          │
└────────┴────────┴────────┴────────┴────────┴────────┴────────┴──────────┴──────────┘
```

**Checksum (Bit7)**: `255 - (sum of bytes 1-7) & 0xFF`

Example: F7 command
```
Bytes 1-7: f7 00 00 00 00 00 00
Sum: 0xF7 = 247
Checksum: 255 - 247 = 8 = 0x08
Full: 00 f7 00 00 00 00 00 08 00 00 ...
```

### Dongle-Specific Commands

These commands are specific to the 2.4GHz dongle protocol:

| Command | Value | Type | Pattern | Description |
|---------|-------|------|---------|-------------|
| BATTERY_REFRESH | 0xF7 | Read | SET → GET(id=5) | Query battery from keyboard |
| GET_INFOR | 0x8F | Read | SET → FC → GET(id=0) | Query device settings |
| FLUSH_NOP | 0xFC | Sync | SET only | Flush response buffer |
| SET_SLEEP | 0x11 | Write | SET → FC | Set sleep timeout |

### Command Patterns

**Pattern 1: Immediate Response (F7 Battery)**
```
SET_REPORT(id=0): f7 00 00 00 00 00 00 08 ...
GET_REPORT(id=5): → response ready immediately (~20µs)
```
No flush or delay needed. Response is buffered during SET_REPORT.

**Pattern 2: Flush-Based Response (8F Settings)**
```
SET_REPORT(id=0): 8f 00 00 00 00 00 00 70 ...
SET_REPORT(id=0): fc 00 00 00 00 00 00 03 ...  (FC flush)
GET_REPORT(id=0): → response ready
```
Flush command (0xFC) synchronizes the response buffer.

**Pattern 3: Write-Only with ACK (0x11 Sleep)**
```
SET_REPORT(id=0): 11 01 00 00 00 00 00 ed ...  (sleep=1 min)
SET_REPORT(id=0): fc 00 00 00 00 00 00 03 ...  (FC flush)
... ~220ms later ...
EP2 Interrupt: 05 0f 01  (keyboard ACK)
EP2 Interrupt: 05 0f 00  (ACK complete)
```
Write commands receive acknowledgment via EP2 interrupt after RF round-trip.

## Battery Response Format

Response to F7 command (GET_REPORT id=5):

```
┌────────┬─────────┬────────┬────────┬──────────┬────────┬────────┬─────────┐
│ Byte 0 │ Byte 1  │ Byte 2 │ Byte 3 │ Byte 4   │ Byte 5 │ Byte 6 │ Byte 7+ │
├────────┼─────────┼────────┼────────┼──────────┼────────┼────────┼─────────┤
│ 0x00   │ Battery │ 0x00   │ Idle   │ Online   │ 0x01   │ 0x01   │ 0x00    │
│        │ (1-100) │        │ (0/1)  │ (0/1)    │        │        │         │
└────────┴─────────┴────────┴────────┴──────────┴────────┴────────┴─────────┘
```

| Field | Byte | Values | Description |
|-------|------|--------|-------------|
| Report ID | 0 | 0x00 | Always 0x00 in response |
| Battery | 1 | 1-100 | Battery percentage |
| Unknown | 2 | 0x00 | Always 0x00 |
| Idle | 3 | 0/1 | 1 = keyboard idle/sleeping, 0 = active |
| Online | 4 | 0/1 | 1 = keyboard connected, 0 = disconnected |
| Marker | 5-6 | 0x01, 0x01 | Validity markers |

**Example:**
```
00 5f 00 00 01 01 01 00  →  Battery=95%, idle=0, online=1
00 60 00 01 01 01 01 00  →  Battery=96%, idle=1 (sleeping), online=1
```

## Settings Response Format

Response to 8F command (GET_REPORT id=0):

```
8f 85 0b 00 00 00 00 05 04 00 ...
│  │  │              │  │
│  │  │              │  └─ Unknown
│  │  │              └──── Profile/mode
│  │  └───────────────── Unknown
│  └──────────────────── Status byte
└─────────────────────── Command echo (0x8F)
```

## Dial/Rotary Encoder Events

The volume/brightness dial sends events via EP2 interrupt:

### Volume Mode (Report ID 0x03)
```
03 e9 00    Rotate clockwise (Volume Up, HID Usage 0xE9)
03 ea 00    Rotate counter-clockwise (Volume Down, HID Usage 0xEA)
03 00 00    Release
```

### Brightness Mode (Report ID 0x05)
```
05 06 00    Brightness level 0 (off)
05 06 01    Brightness level 1
05 06 02    Brightness level 2
05 06 03    Brightness level 3
05 06 04    Brightness level 4 (max)
```
Brightness mode sends absolute level, not relative changes. Fn+Up/Fn+Down keys also trigger these notifications (same as dial in brightness mode), followed by Settings Saved ACK.

### Mode Toggle
```
05 03 00 11    Dial mode toggled (sent in both directions)
```
The 0x11 value appears constant; host must track current mode state.

### Settings Saved ACK
```
05 0f 01       Keyboard acknowledged setting (received via RF)
05 0f 00       ACK complete
```
When a setting is written (e.g., sleep timeout via 0x11), the keyboard sends this ACK ~220ms later after receiving the command over RF. This confirms the setting was saved to the keyboard, not just received by the dongle.

## FEA_CMD Command Codes

Standard keyboard commands (work on both wired and wireless):

| SET Command | Value | GET Command | Value | Description |
|-------------|-------|-------------|-------|-------------|
| SET_REV | 0x00 | GET_REV | 0x80 | Firmware revision |
| SET_RESET | 0x02 | - | - | Reset device |
| SET_REPORT | 0x04 | GET_REPORT | 0x84 | Polling rate |
| SET_PROFILE | 0x05 | GET_PROFILE | 0x85 | Active profile |
| SET_KBOPTION | 0x06 | GET_KBOPTION | 0x86 | Keyboard options |
| SET_LEDPARAM | 0x07 | GET_LEDPARAM | 0x87 | LED effect parameters |
| SET_SLEDPARAM | 0x08 | GET_SLEDPARAM | 0x88 | Secondary LED params |
| SET_KEYMATRIX | 0x09 | GET_KEYMATRIX | 0x89 | Key mappings |
| SET_KEYENABLE | 0x0A | GET_KEYENABLE | 0x8A | Key enable flags |
| SET_MACRO | 0x0B | GET_MACRO | 0x8B | Macro definitions |
| SET_USERPIC | 0x0C | GET_USERPIC | 0x8C | Per-key RGB colors |
| SET_AUDIO | 0x0D | - | - | Audio visualizer data |
| SET_WINDOS | 0x0E | - | - | Screen color sync |
| SET_SLEEP | 0x11 | - | - | Sleep timeout (dongle) |
| - | - | GET_BATTERY | 0x83 | Battery (wired only) |
| - | - | GET_INFOR | 0x8F | Device info/settings |
| BATTERY_REFRESH | 0xF7 | - | - | Battery query (dongle) |
| FLUSH_NOP | 0xFC | - | - | Flush buffer (dongle) |

## Connection Types

### USB Wired
- Direct FEA_CMD communication
- Battery via GET_BATTERY (0x83)
- Full command set supported

### 2.4GHz Wireless (Dongle)
- Host ↔ Dongle ↔ Keyboard (RF)
- Battery via F7 command (triggers RF query)
- Commands forwarded through dongle
- ACKs received via EP2 interrupt (~220ms RF round-trip)

### Bluetooth LE
- Direct BLE connection
- Battery via standard Battery Service (0x180F)
- Vendor commands via custom GATT service

## Device Table

| VID | PID | Type | Notes |
|-----|-----|------|-------|
| 0x3151 | 0x5038 | 2.4GHz Dongle | M1 V5 HE TMR |
| 0x3151 | 0x5037 | 2.4GHz Dongle | Other models |
| 0x3151 | 0x4007 | Wired | Direct USB |
| 0x3151 | 0x4012 | Bluetooth | BLE mode |

## Timing Characteristics

| Operation | Timing | Notes |
|-----------|--------|-------|
| F7 SET→GET | ~20µs | Immediate, no delay needed |
| 8F SET→FC→GET | ~1ms | Flush provides sync |
| RF round-trip (ACK) | ~220ms | Keyboard ACK via EP2 |

## References

- USB HID Specification: https://www.usb.org/hid
- USB HID Usage Tables: https://usb.org/document-library/hid-usage-tables-15
- Linux usbmon: `/sys/kernel/debug/usb/usbmon/`
- Windows driver debug strings reference `D:\work\dj_code\dj_hid_sdk_rs\`
