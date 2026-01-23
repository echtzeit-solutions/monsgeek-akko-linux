# MonsGeek/Akko Keyboard Protocol Overview

This document describes the communication protocol stack for MonsGeek/Akko keyboards, based on reverse engineering of the Windows `iot_driver.exe` and live USB packet captures on Linux.

> **Note**: This document primarily covers the **2.4GHz wireless dongle** (VID:3151 PID:5038). USB wired and Bluetooth connections may differ in endpoint layout, report IDs, and command patterns. The dongle-specific commands (F7, FC, 0x11) and EP2 notification channel are unique to the wireless dongle protocol.

## Protocol Stack

| Layer | Details |
|-------|---------|
| **Vendor Protocol** (FEA_CMD_*) | SET_LEDPARAM, GET_BATTERY, SET_KEYMATRIX, etc. |
| **HID Reports** | Feature Reports (bidirectional config, 65 bytes), Input Reports (keyboard→host events), Output Reports (host→keyboard, e.g. LED indicators) |
| **HID Interfaces** | Interface 0: Keyboard (Boot, EP1), Interface 1: "Keyboard" (Boot, EP2) - actually vendor!, Interface 2: Vendor (EP3, feature reports) ← |
| **USB Device** | VID 0x3151, PID 0x5038 (2.4GHz dongle), VID 0x3151, PID 0x4007 (wired keyboard) |
| **USB** | Transport layer |

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

<table>
<tr>
  <th>Byte 0</th>
  <th>Byte 1</th>
  <th>Byte 2</th>
  <th>Byte 3</th>
  <th>Byte 4</th>
  <th>Byte 5</th>
  <th>Name</th>
  <th>Description</th>
</tr>
<tr>
  <td rowspan="14"><code>0x05</code><br>(Report ID)</td>
  <td><code>0x00</code></td>
  <td><code>0x00</code></td>
  <td><code>0x00</code></td>
  <td><code>0x00</code></td>
  <td>-</td>
  <td>Wake</td>
  <td>Keyboard wake from sleep (all zeros after report ID)</td>
</tr>
<tr>
  <td><code>0x01</code></td>
  <td>profile</td>
  <td>-</td>
  <td>-</td>
  <td>-</td>
  <td>ProfileChange</td>
  <td>Profile changed via Fn+F9..F12 (profile: 0-3)</td>
</tr>
<tr>
  <td rowspan="5"><code>0x03</code><br>(KB Func)</td>
  <td>state</td>
  <td><code>0x01</code></td>
  <td>-</td>
  <td>-</td>
  <td>WinLockToggle</td>
  <td>Win key lock via Fn+Win (state: 0=off, 1=on)</td>
</tr>
<tr>
  <td>state</td>
  <td><code>0x03</code></td>
  <td>-</td>
  <td>-</td>
  <td>WasdSwapToggle</td>
  <td>WASD/Arrow swap via Fn+W (state: 0=normal, 8=swapped)</td>
</tr>
<tr>
  <td>layer</td>
  <td><code>0x08</code></td>
  <td>-</td>
  <td>-</td>
  <td>FnLayerToggle</td>
  <td>Fn layer toggle via Fn+Alt (layer: 0=default, 1=alternate)</td>
</tr>
<tr>
  <td><code>0x04</code></td>
  <td><code>0x09</code></td>
  <td>-</td>
  <td>-</td>
  <td>BacklightToggle</td>
  <td>Backlight toggle via Fn+L</td>
</tr>
<tr>
  <td><code>0x00</code></td>
  <td><code>0x11</code></td>
  <td>-</td>
  <td>-</td>
  <td>DialModeToggle</td>
  <td>Dial mode toggle (volume ↔ brightness)</td>
</tr>
<tr>
  <td><code>0x04</code></td>
  <td>effect_id</td>
  <td>-</td>
  <td>-</td>
  <td>-</td>
  <td>LedEffectMode</td>
  <td>LED effect mode (1-20) via Fn+Home/PgUp/End/PgDn</td>
</tr>
<tr>
  <td><code>0x05</code></td>
  <td>speed</td>
  <td>-</td>
  <td>-</td>
  <td>-</td>
  <td>LedEffectSpeed</td>
  <td>LED effect speed (0-4) via Fn+←/→</td>
</tr>
<tr>
  <td><code>0x06</code></td>
  <td>level</td>
  <td>-</td>
  <td>-</td>
  <td>-</td>
  <td>BrightnessLevel</td>
  <td>Brightness level (0-4) via Fn+↑/↓ or dial</td>
</tr>
<tr>
  <td><code>0x07</code></td>
  <td>color</td>
  <td>-</td>
  <td>-</td>
  <td>-</td>
  <td>LedColor</td>
  <td>LED color (0-7) via Fn+\</td>
</tr>
<tr>
  <td><code>0x0F</code></td>
  <td>started</td>
  <td>-</td>
  <td>-</td>
  <td>-</td>
  <td>SettingsAck</td>
  <td>Settings ACK (started: 1=begin, 0=complete)</td>
</tr>
<tr>
  <td><code>0x1B</code></td>
  <td colspan="2">depth (u16 LE)</td>
  <td>key_idx</td>
  <td>-</td>
  <td>KeyDepth</td>
  <td>Magnetism/key depth report (hall effect sensor)</td>
</tr>
<tr>
  <td><code>0x88</code></td>
  <td>-</td>
  <td>-</td>
  <td>level</td>
  <td>flags</td>
  <td>BatteryStatus</td>
  <td>Battery status from dongle (level 0-100)</td>
</tr>
</table>

**Battery flags (byte 4):** bit 0 = online, bit 1 = charging

**LED effect ranges:**
- Fn+Home: effects 1-5 (0x01-0x05)
- Fn+PgUp: effects 6-10 (0x06-0x0A)
- Fn+End: effects 11-15 (0x0B-0x0F)
- Fn+PgDn: effects 16-20 (0x10-0x14)

**LED colors (0x07 values):** 0=red, 1=green, 2=blue, 3=orange, 4=magenta, 5=yellow, 6=white, 7=rainbow

**Preliminary: LED notification type correlation**

Types 0x04-0x07 may correspond to LEDPARAM field indices + 4:
```
LEDPARAM payload: [cmd][mode][speed][bright][opt][r][g][b]
                        ↓     ↓      ↓            ↓
Notification:          0x04  0x05   0x06         0x07
Field index:            0     1      2            3
```
Formula (unconfirmed): `notification_type = ledparam_field_index + 4`

```
05 0f 01       Settings saved ACK (keyboard confirmed via RF)
05 0f 00       Settings saved ACK complete
```

**Not notified via interrupt** (must poll F7):
- Keyboard going idle/sleep
- Keyboard disconnect

## Vendor Protocol Commands

### Command Format (SET_REPORT, 65 bytes)

| Byte 0 | Byte 1 | Byte 2 | Byte 3 | Byte 4 | Byte 5 | Byte 6 | Byte 7 | Byte 8+ |
|--------|--------|--------|--------|--------|--------|--------|--------|---------|
| Report ID=0 | CMD | Param1 | Param2 | Param3 | Param4 | Param5 | Checksum (Bit7) | Padding |

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

| Byte | Field | Values | Description |
|------|-------|--------|-------------|
| 0 | Report ID | `0x00` | Always 0x00 in response |
| 1 | Battery | 1-100 | Battery percentage |
| 2 | Unknown | `0x00` | Always 0x00 |
| 3 | Idle | 0/1 | 1 = keyboard idle/sleeping, 0 = active |
| 4 | Online | 0/1 | 1 = keyboard connected, 0 = disconnected |
| 5-6 | Marker | `0x01, 0x01` | Validity markers |
| 7+ | Padding | `0x00` | Unused |

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

**Commands that do NOT trigger SettingsAck:**
- SET_MAGNETISM (0x1B) - monitor mode toggle
- BATTERY_REFRESH (0xF7) - battery query

## Magnetism Monitoring (Hall Effect)

The keyboard's hall effect switches can report real-time key depth via command 0x1B.

### Enable/Disable Monitoring

| Command | Param1 | Description |
|---------|--------|-------------|
| `1b 01 00 00 00 00 00 e3` | 0x01 | Enable key depth monitoring |
| `1b 00 00 00 00 00 00 e4` | 0x00 | Disable key depth monitoring |

Send command followed by FC flush. No SettingsAck is sent.

### KeyDepth Notifications (0x1B)

When monitoring is enabled, the keyboard streams depth reports via EP2:

| Byte | Field | Description |
|------|-------|-------------|
| 0 | Report ID | `0x05` |
| 1 | Type | `0x1B` |
| 2-3 | Depth | u16 LE, raw hall effect value |
| 4 | Key Index | Matrix index of key being pressed |

**Example key press cycle:**
```
05 1b 0f 00 29    depth=15,  key=41 (press start)
05 1b 69 01 29    depth=361, key=41 (bottom out)
05 1b 00 00 29    depth=0,   key=41 (release)
```

Reports arrive at ~3-20ms intervals during key movement. Depth values typically range 0-400+ depending on switch travel.

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
| SET_MAGNETISM | 0x1B | - | - | Key depth monitor (no ACK) |
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
