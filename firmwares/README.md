# ry5088 Firmware Collection

Downloaded and analyzed firmwares from the Akko Cloud API (api2.rongyuan.tech).

## Confirmed ry5088 Firmwares

All these share the same ARM Cortex-M vector table: SP=0x20004EB0, Reset=0x080002C1

| Device ID | Company | Model | Version | Size (decompressed) | Notes |
|-----------|---------|-------|---------|---------------------|-------|
| 2116 | TITANHUB | Storm68 | v304 | 127KB | Reference firmware |
| 2450 | Skyloong | GK75 | v307 | 132KB | |
| 2454 | Keydous | NJ81-CP | v408 | 155KB | **Has HE/TMR commands!** |
| 2626 | Cherry | K5 TMR | v311 | 118KB | TMR keyboard |
| 2816 | TITANHUB | Storm68 | v507 | 116KB | Newer version |
| 3098 | Cherry | (unknown) | v316 | 130KB | |

### Notable Findings

**2454 Keydous NJ81-CP** - Contains Hall Effect/TMR specific HID commands:
- SET_MULTI_MAGNETISM (0x65)
- GET_MULTI_MAGNETISM (0xE5)

**2626 Cherry K5 TMR** - Contains embedded driver launch URL:
- `www.cherry.cn/CHERRY_MAGCRATE`

## Other Firmwares (non-ry5088)

| Device ID | Company | Version | Format | Notes |
|-----------|---------|---------|--------|-------|
| 1802 | DELUX | v104_rfv200 | ZIP | Contains RF firmware (289KB) |
| 1812 | 键赏家 | v108 | deflate | Different MCU |
| 1823 | Darmoshark | v104_rfv200 | ZIP | Contains RF firmware |
| 1848 | PIIFOXDRIVER | v111_oledv102 | ZIP | Contains OLED firmware (106KB) |
| 3175 | Hator | v106 | deflate | Different MCU (SP=0x00030000) |

## File Structure

```
firmwares/
├── *.bin                    # Raw downloads from API
└── extracted/
    ├── *_decompressed.bin   # Deflate-extracted firmwares
    ├── *_firmwareFile.bin   # Main firmware from ZIPs
    ├── *_firmwareRFFile.bin # RF receiver firmware
    └── *_firmwareOledFile.bin # OLED display firmware
```

## Version String Pattern

The version_str from the API follows patterns:
- `vXXX` - Main USB firmware version (e.g., v304, v507)
- `vXXX_rfvYYY` - Main + RF firmware
- `vXXX_oledvYYY` - Main + OLED firmware

## Scripts

- `fetch_ry5088_firmwares.py` - Fetch firmwares for known ry5088 devices
- `scan_firmware_api.py` - Scan device ID range for any available firmware
- `extract_firmware.py` - Download and extract firmware by device ID
