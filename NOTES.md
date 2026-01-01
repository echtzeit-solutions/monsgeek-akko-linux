# MonsGeek M1 V5 HE Magnetic Keyboard - Linux Reverse Engineering

## Keyboard Info

- **Model:** MonsGeek M1 V5 HE Magnetic Keyboard (Akko collaboration)
- **Vendor ID:** 0x3151
- **Product ID:** 0x5030
- **Firmware:** 4.05
- **Connection:** USB 2.0 High Speed (480Mbps)

## Problem

- Akko Cloud Driver (web-based) doesn't work on Linux
- Wants an "iot driver" native helper that doesn't exist for Linux
- VIA doesn't recognize this keyboard
- WebHID test pages can see the device

## HID Interfaces

The keyboard presents 3 HID interfaces:

| hidraw | Descriptor Size | Purpose |
|--------|----------------|---------|
| hidraw2 | 59 bytes | Standard 6KRO keyboard (Usage Page 01, Usage 06) |
| hidraw5 | 171 bytes | Multi-function: Consumer Control (Report ID 03), System Control (02), NKRO (01), Mouse (06), Vendor (05, 31 bytes) |
| hidraw12 | 20 bytes | **Config interface** - Vendor page 0xFFFF, 64-byte feature reports |

## Report Descriptors (hex dumps)

### hidraw2 (Standard Keyboard)
```
05 01 09 06 a1 01 05 08 15 00 25 01 19 01 29 05
75 01 95 05 91 02 95 03 91 01 05 07 19 e0 29 e7
75 01 95 08 81 02 95 08 81 01 15 00 26 ff 00 19
00 2a ff 00 75 08 95 06 81 00 c0
```

### hidraw5 (Multi-function)
```
05 0c 09 01 a1 01 85 03 19 00 2a 3c 03 15 00 26
3c 03 95 01 75 10 81 00 c0 05 01 09 80 a1 01 85
02 15 00 26 ff 7f 19 00 2a ff 7f 75 10 95 01 81
00 c0 05 01 09 06 a1 01 85 01 05 07 15 00 25 01
19 00 29 77 95 78 75 01 81 02 c0 05 01 09 02 a1
01 85 06 09 01 a1 00 05 09 15 00 25 01 19 01 29
05 75 01 95 05 81 02 95 03 81 01 05 01 16 01 80
26 ff 7f 09 30 09 31 75 10 95 02 81 06 15 81 25
7f 09 38 75 08 95 01 81 06 05 0c 0a 38 02 95 01
81 06 c0 c0 06 ff ff 09 01 a1 01 85 05 09 01 15
00 26 ff 00 75 08 95 1f 81 02 c0
```

### hidraw12 (Config Interface) - KEY INTERFACE
```
06 ff ff 09 02 a1 01 09 02 15 80 25 7f 95 40 75 08 b1 02 c0
```
Decoded:
- Usage Page 0xFFFF (Vendor)
- Usage 0x02
- Feature Report: 64 bytes (0x40), signed bytes (-128 to 127)

## Probing Results

### Feature Reports (hidraw12)
- Most report IDs return all zeros or echo sent data
- One interesting response: sending all zeros returned `000b00...` (byte 2 = 0x0b = 11, possibly protocol version)

### Interrupt Transfers (hidraw2)
Got some responses but they appear to be keyboard state reports, not protocol responses:
- Pattern 0x07 → `0000100000000000`
- Pattern 0x08 → `0000101200000000`
- Pattern 0x09 → `0000120000000000`
- etc.

## Files in This Directory

- `99-monsgeek.rules` - udev rules for hidraw permissions (already installed to /etc/udev/rules.d/)
- `monsgeek_probe.py` - Basic probe using feature reports
- `monsgeek_probe2.py` - Advanced probe using interrupt read/write

## Akko Cloud Driver Architecture (Reverse Engineered)

### Overview

```
┌─────────────────────────────────────────────────────────┐
│  Web App / Electron App (React UI)                      │
│  - web.monsgeek.com / web.akkogear.com                  │
│  - Uses @protobuf-ts/grpcweb-transport                  │
│  - 95MB JS bundle (mostly UI, icons, animations)        │
└─────────────────────┬───────────────────────────────────┘
                      │ gRPC-Web (HTTP/2)
                      │ localhost:3814
                      ▼
┌─────────────────────────────────────────────────────────┐
│  iot_driver.exe (Rust binary, ~6.4MB)                   │
│  - gRPC server using tonic                              │
│  - HID access via hidapi                                │
│  - BLE support via btleplug                             │
│  - Local database (sled)                                │
└─────────────────────┬───────────────────────────────────┘
                      │ HID Feature Reports (64 bytes)
                      ▼
┌─────────────────────────────────────────────────────────┐
│  Keyboard (Proprietary Firmware)                        │
│  - Interface 2: Vendor HID (0xFFFF:0x02)                │
│  - 64-byte feature reports with checksum                │
└─────────────────────────────────────────────────────────┘
```

### Key Discovery

**Both web apps (monsgeek.com, akkogear.com) require the local iot_driver!**
- No WebHID fallback exists
- Browser makes requests to `http://127.0.0.1:3814/driver.DriverGrpc/*`
- Without iot_driver running, page shows blank/loading

### gRPC Endpoints (driver.DriverGrpc service)

| Endpoint | Purpose |
|----------|---------|
| `/sendRawFeature` | Send HID feature report to device |
| `/readRawFeature` | Read HID feature report from device |
| `/watchDevList` | Stream device connect/disconnect events |
| `/watchSystemInfo` | System information stream |
| `/getVersion` | Get iot_driver version |
| `/getItemFromDb` | Read from local database |
| `/insertDb` | Write to local database |
| `/upgradeOTAGATT` | Firmware update via BLE |

### Protobuf Messages

```protobuf
// Send a HID feature report
message SendMsg {
  string devicePath = 1;      // HID device path
  bytes msg = 2;              // 64-byte message
  CheckSumType checkSumType = 3;
  DangleDevType dangleDevType = 4;
}

// Read response
message ResRead {
  string err = 1;             // Error string (empty = success)
  bytes msg = 2;              // Response data
}

enum CheckSumType {
  Bit7 = 0;   // Checksum at byte 7
  Bit8 = 1;   // Checksum at byte 8
  None = 2;   // No checksum
}
```

## HID Protocol

### Message Format

- **Size:** 64 bytes (padded with zeros)
- **Byte 0:** Command ID
- **Bytes 1-6:** Parameters
- **Byte 7:** Checksum (for Bit7 mode)
- **Checksum:** `255 - (sum(bytes[0:7]) & 0xFF)`

### Response Validation

- Byte 1 = `0xAA` (170) indicates success

### Command IDs (FEA_CMD_*)

| ID | Hex | Command | Description |
|----|-----|---------|-------------|
| 1 | 0x01 | SET_RESET | Reset device |
| 3-4 | 0x03-04 | SET_REPORT | Set report rate |
| 4-5 | 0x04-05 | SET_PROFILE | Set active profile |
| 6 | 0x06 | SET_DEBOUNCE | Set debounce time |
| 7 | 0x07 | SET_LEDPARAM | Set LED parameters |
| 8 | 0x08 | SET_SLEDPARAM | Set secondary LED |
| 9-10 | 0x09-0A | SET_KEYMATRIX | Set key mappings |
| 11 | 0x0B | SET_MACRO | Set macro |
| 16 | 0x10 | SET_FN | Set Fn layer |
| 27 | 0x1B | SET_MAGNETISM_REPORT | Magnetic switch report |
| 28 | 0x1C | SET_MAGNETISM_CAL | Calibrate magnetic switches |
| 101 | 0x65 | SET_MULTI_MAGNETISM | Set RT/DKS per-key |
| 128 | 0x80 | GET_REV / GET_RF_VERSION | Get firmware version |
| 131 | 0x83 | GET_REPORT | Get report rate |
| 132-133 | 0x84-85 | GET_PROFILE | Get active profile |
| 134 | 0x86 | GET_DEBOUNCE | Get debounce |
| 135 | 0x87 | GET_LEDPARAM | Get LED params |
| 137-138 | 0x89-8A | GET_KEYMATRIX | Get key mappings |
| 139 | 0x8B | GET_MACRO | Get macro |
| 143 | 0x8F | GET_USB_VERSION | Get USB version |
| 144 | 0x90 | GET_FN | Get Fn layer |
| 229 | 0xE5 | GET_MULTI_MAGNETISM | Get RT/DKS settings |
| 230 | 0xE6 | GET_FEATURE_LIST | Get supported features |

### Device Database Entry

Our keyboard in iot_driver:
```
{ vid = 0x3151, pid = 0x5030, usage = 0x2, usage_page = 0xffff, interface_number = 2 }
```

## Wine Testing

### iot_driver.exe under Wine

**Status: PARTIALLY WORKS**

```bash
wine ./iot_driver.exe
# Output:
# addr :: 127.0.0.1:3814
# SSSSSSSSSSSTTTTTTTTTTTTTTTAAAAAAAAAAAARRRRRRRRRRRTTTTTTTTTTT!!!!!!!
# HH 测试 调用 ----
```

- gRPC server starts and listens on port 3814
- Web app can connect to it
- **Unknown:** Whether Wine's HID passthrough works with the keyboard

## Next Steps

1. **Test Wine HID access** - See if iot_driver under Wine can actually see the keyboard
2. **Write Python iot_driver replacement** - Reimplement the gRPC server for native Linux
3. **Test protocol commands** - Use the extracted command IDs to probe the keyboard

## Useful Commands

```bash
# Check permissions
ls -la /dev/hidraw{2,5,12}

# Reload udev rules if needed
sudo udevadm control --reload-rules && sudo udevadm trigger

# Run probes
python3 monsgeek_probe.py
python3 monsgeek_probe2.py

# Monitor HID traffic
sudo cat /dev/hidraw12 | xxd
```

## Resources

- No existing Linux reverse-engineering project found
- Official MonsGeek GitHub has firmware but no protocol docs: https://github.com/MonsGeek/m1_v5
- SRGBmods QMK fork exists but doesn't help with proprietary protocol
