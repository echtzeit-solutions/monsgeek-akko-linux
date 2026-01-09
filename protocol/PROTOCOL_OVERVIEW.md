# MonsGeek/Akko Keyboard Protocol Overview

This document describes the communication protocol stack for MonsGeek/Akko keyboards, based on reverse engineering of the Windows `iot_driver.exe` and USB packet captures.

## Protocol Stack

```
┌─────────────────────────────────────────────────────────────┐
│  MonsGeek/Akko Vendor Protocol (FEA_CMD_*)                  │
│  - SET_LEDPARAM, GET_BATTERY, SET_KEYMATRIX, etc.           │
├─────────────────────────────────────────────────────────────┤
│  HID Reports                                                │
│  - Feature Reports (bidirectional config, 65 bytes)         │
│  - Input Reports (keyboard→host events, 64 bytes)           │
│  - Output Reports (host→keyboard, e.g. LED indicators)      │
├─────────────────────────────────────────────────────────────┤
│  HID Interfaces (identified by Usage Page + Usage)          │
│  - Interface 0: Keyboard (Usage Page 0x01, Usage 0x06)      │
│  - Interface 1: Consumer/Media (Usage Page 0x0C)            │
│  - Interface 2: Vendor (Usage Page 0xFFFF, Usage 0x02)  ←── │
├─────────────────────────────────────────────────────────────┤
│  USB Device (VID:PID, multiple interfaces)                  │
│  - VID 0x3151, PID 0x5038 (2.4GHz dongle)                   │
│  - VID 0x3151, PID 0x4007 (wired keyboard)                  │
├─────────────────────────────────────────────────────────────┤
│  USB (transport)                                            │
└─────────────────────────────────────────────────────────────┘
```

## Layer 1: USB

Raw USB with control/interrupt transfers. Each device exposes multiple **interfaces** (like virtual sub-devices within one physical device).

## Layer 2: HID Interfaces

Each USB device can have multiple HID interfaces, identified by **Usage Page** and **Usage**:

| Interface | Usage Page | Usage | Purpose |
|-----------|------------|-------|---------|
| 0 | 0x0001 (Generic Desktop) | 0x06 (Keyboard) | Standard keyboard input |
| 1 | 0x000C (Consumer) | 0x01 (Consumer Control) | Media keys, volume |
| 2 | 0xFFFF (Vendor-defined) | 0x02 | **Vendor protocol** |

- **Usage Page**: Category of device function (defined by USB-IF HID specification)
- **Usage**: Specific function within that category

The vendor interface (Usage Page 0xFFFF) is where all proprietary communication happens.

## Layer 3: HID Reports

Within each interface, communication happens via **reports**:

| Report Type | Direction | Purpose |
|-------------|-----------|---------|
| **Input Report** | Device → Host | Key presses, magnetism depth data, events |
| **Output Report** | Host → Device | Keyboard LEDs (Caps/Num/Scroll Lock) |
| **Feature Report** | Bidirectional | **All vendor commands** (config, battery, settings) |

Each report has a **Report ID** (byte 0) to multiplex multiple report formats on one interface.

### Report IDs

| Report ID | Usage |
|-----------|-------|
| 0x00 | Default (some systems) |
| 0x05 | Vendor feature reports |

Note: On Linux, the Report ID is prepended to the buffer. On Windows, it may be handled differently.

## Layer 4: MonsGeek Vendor Protocol (FEA_CMD)

The proprietary protocol uses **Feature Reports** on the vendor interface.

### Request Format (65 bytes)

```
┌────────┬────────┬────────┬────────┬────────┬────────┬────────┬──────────┬──────────┐
│ Byte 0 │ Byte 1 │ Byte 2 │ Byte 3 │ Byte 4 │ Byte 5 │ Byte 6 │ Byte 7   │ Byte 8+  │
├────────┼────────┼────────┼────────┼────────┼────────┼────────┼──────────┼──────────┤
│ Report │ CMD    │ Param1 │ Param2 │ Param3 │ Param4 │ Param5 │ Checksum │ Payload  │
│ ID     │        │        │        │        │        │        │          │          │
└────────┴────────┴────────┴────────┴────────┴────────┴────────┴──────────┴──────────┘
```

- **Report ID** (byte 0): Usually 0x05 for vendor commands
- **CMD** (byte 1): FEA_CMD_* command code
- **Params** (bytes 2-6): Command-specific parameters
- **Checksum** (byte 7): `255 - (sum of bytes 0-6) & 0xFF`
- **Payload** (bytes 8-63): Additional data for bulk transfers

### Command Codes (FEA_CMD_*)

Commands follow a pattern: SET commands are 0x00-0x7F, GET commands are 0x80-0xFF (usually SET | 0x80).

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
| - | - | GET_BATTERY | 0x83 | Battery status (wired only) |
| - | - | GET_INFOR | 0x8F | Device info |

### Response Format

Responses use the same format. For GET commands, the response contains:
- Byte 1: Echo of command (with success indicated by 0xAA in some cases)
- Bytes 2+: Requested data

## Layer 5: Connection Types

The protocol differs based on how the keyboard connects:

### USB Wired Connection

```
┌──────────┐                    ┌──────────┐
│ Keyboard │ ←───── USB ──────→ │   Host   │
└──────────┘                    └──────────┘
```

- Direct FEA_CMD communication with keyboard
- Battery via `GET_BATTERY (0x83)`
- Full command set supported

### 2.4GHz Wireless (Dongle)

```
┌──────────┐     2.4GHz      ┌──────────┐      USB       ┌──────────┐
│ Keyboard │ ←────────────→  │  Dongle  │ ←───────────→  │   Host   │
│          │   proprietary   │          │    HID         │          │
└──────────┘                 └──────────┘                └──────────┘
```

- Host talks to **dongle**, not keyboard directly
- Dongle has `dongle_common = true` in device table
- Uses **DangleCommon** protocol for status
- FEA_CMD commands are forwarded through dongle

### Bluetooth LE

```
┌──────────┐                    ┌──────────┐
│ Keyboard │ ←──── BLE ───────→ │   Host   │
└──────────┘                    └──────────┘
```

- Device has `ble = true` in device table
- Uses GATT characteristics
- Battery via standard Battery Service (0x180F)
- Vendor commands via custom GATT service

## Dongle Protocol (DangleCommon)

For 2.4GHz wireless dongles, querying status returns a different format than FEA_CMD responses.

### Status Query

Send a feature report read request (Report ID 0x05) to get dongle status.

### Status Response (Status24)

```
┌────────┬─────────┬────────┬────────┬──────────┬──────────┬────────┬─────────┐
│ Byte 0 │ Byte 1  │ Byte 2 │ Byte 3 │ Byte 4   │ Byte 5   │ Byte 6 │ Byte 7+ │
├────────┼─────────┼────────┼────────┼──────────┼──────────┼────────┼─────────┤
│ 0x00   │ Battery │ 0x00   │ 0x00   │ is_online│ flow_ctrl│ ???    │ padding │
│        │ (0-100) │        │        │ (0/1)    │ (0/1)    │        │         │
└────────┴─────────┴────────┴────────┴──────────┴──────────┴────────┴─────────┘
```

**Confirmed fields** (from Windows driver decompilation):
- **Byte 1**: Battery percentage (0-100)
- **Byte 4**: is_online flag (1 = keyboard connected, 0 = disconnected)
- **Byte 5**: Flow control flag (toggles between 0 and 1)

**Evidence from USB capture** (`windows_app_start.pcapng`):
```
Response 1: 00 5f 00 00 01 01 01 00  (byte[5] = 1)
Response 2: 00 5f 00 00 01 00 01 00  (byte[5] = 0)
                         ^^
                         toggles during polling
```

The internal `Status24` struct in the driver has 4 fields:
- `is_ready_read` - flow control for reading
- `battery` - battery percentage
- `is_online` - keyboard connected
- `is_write_finish` - flow control for writing

Debug strings "fb wait write" and "fb wait read" suggest the driver polls this flag during data transfers. However, the gRPC protobuf only exposes `battery` and `is_online` to the application.

**Unknown fields**:
- Byte 6: Unknown (always 0x01 in captures)

**Protobuf definition** (from driver):
```protobuf
message Status24 {
    uint32 battery = 1;
    bool is_online = 2;
}

message DangleCommonStatus {
    Status24 keyboard_status = 1;
    Status24 mouse_status = 2;
    uint32 keyboard_id = 5;
    uint32 mouse_id = 6;
}
```

Note: The protobuf only defines `battery` and `is_online` - charging status is NOT available via the dongle protocol.

## Device Table

The Windows driver contains a device table mapping VID/PID to protocol type:

```
{ vid = 0x3151, pid = 0x5038, usage = 0x2, usage_page = 0xffff,
  interface_number = 2, dongle_common = true }
```

### Device Flags

| Flag | Meaning | Protocol |
|------|---------|----------|
| (none) | USB wired keyboard | Direct FEA_CMD |
| `dongle_common = true` | 2.4GHz wireless dongle | DangleCommon + forwarded FEA_CMD |
| `ble = true` | Bluetooth LE device | GATT-based |
| `libusb_index = N` | Specific libusb interface | Same as above |

### Example Device Entries

```
// USB wired keyboards
{ vid = 0x3151, pid = 0x4007, usage = 0x2, usage_page = 0xffff, interface_number = 2 }
{ vid = 0x3151, pid = 0x5002, usage = 0x2, usage_page = 0xffff, interface_number = 2 }

// 2.4GHz dongles
{ vid = 0x3151, pid = 0x5038, usage = 0x2, usage_page = 0xffff, interface_number = 2, dongle_common = true }
{ vid = 0x3151, pid = 0x5037, usage = 0x2, usage_page = 0xffff, interface_number = 2, dongle_common = true }

// Bluetooth LE
{ vid = 0x3151, pid = 0x4012, usage = 0x202, usage_page = 0xff66, interface_number = -1, ble = true }
{ vid = 0x3151, pid = 0x5004, usage = 0x202, usage_page = 0xff55, interface_number = -1, ble = true }
```

## gRPC Layer (Windows Driver)

The Windows `iot_driver.exe` exposes a gRPC interface for the Electron app:

| Endpoint | Purpose |
|----------|---------|
| `/driver.DriverGrpc/sendRawFeature` | Send HID feature report |
| `/driver.DriverGrpc/readRawFeature` | Read HID feature report |
| `/driver.DriverGrpc/watchDevList` | Stream device list changes |
| `/driver.DriverGrpc/watchSystemInfo` | Stream system info |
| `/driver.DriverGrpc/upgradeOTAGATT` | BLE OTA firmware update |
| `/driver.DriverGrpc/muteMicrophone` | Mute system microphone |
| `/driver.DriverGrpc/getVersion` | Get driver version |
| `/driver.DriverGrpc/getWeather` | Get weather data (for OLED?) |

## References

- USB HID Specification: https://www.usb.org/hid
- USB HID Usage Tables: https://usb.org/document-library/hid-usage-tables-15
- Windows driver source: `D:\work\dj_code\dj_hid_sdk_rs\` (from debug strings)
- Rust source files:
  - `src\dj_dev_api\cmd_list.rs` - FEA_CMD definitions
  - `src\dj_dev_api\dangle_common.rs` - Dongle protocol
  - `src\dj_dev_api\ble_hid.rs` - Bluetooth HID
  - `src\dj_hid_device\dj_hid\hid_ble.rs` - BLE implementation
