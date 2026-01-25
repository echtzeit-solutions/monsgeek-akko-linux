# MonsGeek M1 V5 HE - Historical Reverse Engineering Notes

> **Archive Notice:** This file preserves the original reverse engineering discovery process. For current protocol documentation, see [PROTOCOL.md](PROTOCOL.md).

## Initial Problem (December 2024)

- Akko Cloud Driver (web-based) doesn't work on Linux
- Wants an "iot driver" native helper that doesn't exist for Linux
- VIA doesn't recognize this keyboard
- WebHID test pages can see the device but can't configure it

## Keyboard Info

- **Model:** MonsGeek M1 V5 HE Magnetic Keyboard (Akko collaboration)
- **Vendor ID:** 0x3151
- **Product ID:** 0x5030
- **Firmware:** 4.05
- **Connection:** USB 2.0 High Speed (480Mbps)

## HID Interface Discovery

The keyboard presents 3 HID interfaces:

| hidraw | Descriptor Size | Purpose |
|--------|----------------|---------|
| hidraw2 | 59 bytes | Standard 6KRO keyboard (Usage Page 01, Usage 06) |
| hidraw5 | 171 bytes | Multi-function: Consumer Control (Report ID 03), System Control (02), NKRO (01), Mouse (06), Vendor (05, 31 bytes) |
| hidraw12 | 20 bytes | **Config interface** - Vendor page 0xFFFF, 64-byte feature reports |

## Report Descriptor Hex Dumps

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

## Initial Probing Results

### Feature Reports (hidraw12)
- Most report IDs return all zeros or echo sent data
- One interesting response: sending all zeros returned `000b00...` (byte 2 = 0x0b = 11, possibly protocol version)

### Interrupt Transfers (hidraw2)
Got some responses but they appear to be keyboard state reports, not protocol responses:
- Pattern 0x07 → `0000100000000000`
- Pattern 0x08 → `0000101200000000`
- Pattern 0x09 → `0000120000000000`

## Wine Testing Results

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
- Wine's HID passthrough can be unreliable
- Native Linux driver is more reliable

## Akko Cloud Driver Architecture Discovery

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

### gRPC Endpoints Discovered (driver.DriverGrpc service)

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

### Protobuf Messages Extracted

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

## Verified Device Data (First Successful Query)

| Field | Value |
|-------|-------|
| Device ID | 2949 (0x0B85) |
| Firmware Version | 1029 (v10.29) |
| Default Profile | 0 |
| LED Mode | 1 (Constant/Static) |
| LED Brightness | 2 |
| LED Speed | 4 |
| LED Color | RGB(7, 255, 255) |
| Precision | 0.1mm (factor=10) |

## Useful Debug Commands

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

## Resources Found

- No existing Linux reverse-engineering project found at time of research
- Official MonsGeek GitHub has firmware but no protocol docs: https://github.com/MonsGeek/m1_v5
- SRGBmods QMK fork exists but doesn't help with proprietary protocol
- Windows driver debug strings reference `D:\work\dj_code\dj_hid_sdk_rs\`

---

*Original notes from December 2024 - January 2025*
