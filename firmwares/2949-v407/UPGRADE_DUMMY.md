# RY Upgrade Tool + UHID Dummy Device

This document describes how to run the Windows `ry_upgrade.exe` firmware updater under Wine against UHID dummy devices on Linux to capture the firmware update protocol. This workflow was used to extract the decrypted AT32F405 firmware from the MonsGeek M1 V5 TMR keyboard updater.

## Overview

The `ry_upgrade.exe` (Rongyuan RY Upgrade Tool 1.1.22, Rust+Slint) is a Windows HID application. We run it under a patched Wine build against Linux UHID virtual devices that simulate the keyboard's HID interfaces, capturing the entire firmware update protocol.

## What the updater expects

From `support_config.json` and the firmware bundle:

- **VID/PID**: `0x3151` / `0x5030` (MonsGeek M1 V5 TMR, RY5088).
- **Config interface**: usage_page `0xFFFF`, usage `0x02`, **interface 2** (MI_02) — feature reports (SET_REPORT / GET_REPORT) for commands and firmware transfer.
- **Vendor interface**: usage_page `0xFFFF`, usage `0x01`, **interface 1** (MI_01) — input reports (vendor notifications); the app opens this and blocks on ReadFile.
- **Device ID**: `2949` (`0x0B85`) for the bundle `ID_2949_RY5088_AKKO_M1V5 TMR_RY1033_ARGB_KB_V407_20260122`.
- **Bootloader**: VID `0x3151`, PID `0x502A`, usage_page `0xFF01`. After "enter bootloader", the device re-enumerates with this PID and the updater sends the firmware.

## Wine patches required

Stock Wine lacks several features needed for HID device hierarchy traversal. Our patches are on the `hid-device-hierarchy` branch in `~/src-misc/wine/`:

### Potentially upstreamable patches

1. **`hidclass.sys: Include MI (multiple interface) index in hardware IDs`**
   - Adds `HID\VID_XXXX&PID_YYYY&MI_ZZ` to the hardware ID list. Windows does this; Wine only emitted `HID\VID_XXXX&PID_YYYY`.

2. **`winebus.sys: Allow shared serial numbers for composite device interfaces`**
   - On Windows, all interfaces of a USB composite device share the same serial from the parent USB descriptor. Wine's `make_unique_serial()` generates synthetic unique serials, breaking interface pairing in hidapi.

3. **`winebus.sys: Read HID_MANUFACTURER from udev properties`**
   - Populates manufacturer metadata from `HID_MANUFACTURER` udev property instead of defaulting to "hidraw".

4. **`setupapi: Implement CM_Get_Parent for HID device hierarchy traversal`**
   - Replaces the `CM_Get_Parent` stub with an implementation that synthesizes parent device nodes (HID→USB→HTREE\ROOT). This enables applications to walk the device tree.

5. **`setupapi: Return USB\COMPOSITE CompatibleIds for synthetic USB parent nodes`**
   - hidapi 0.12+ queries parent CompatibleIds for "USB" to determine bus_type. Without this, serial numbers aren't populated and multi-interface device pairing fails.

6. **`setupapi: Don't lowercase device interface detail paths`**
   - `SetupDiGetDeviceInterfaceDetailW` was calling `CharLowerW` on the returned `DevicePath`, breaking case-sensitive string comparisons between interface paths and device instance IDs.

### Debug-only patch

7. **`hid/setupapi/cfgmgr32: Enhance trace logging for device property queries`**
   - Detailed TRACE output for CM_ property APIs, HidD_GetAttributes, HidP_GetCaps, and pdo_read. Useful for debugging but verbose for upstream.

### Building Wine with patches

```bash
cd ~/src-misc/wine
git checkout hid-device-hierarchy

# Build wow64 (if not already set up)
mkdir -p build-wow64 && cd build-wow64
../configure --enable-archs=i386,x86_64
make -j$(nproc)
make install-lib DESTDIR=../install-wow64
```

### Stale registry warning

Wine caches HardwareIds in its registry. After changing Wine code that affects device IDs, you must delete stale entries and restart wineserver:

```bash
WINEPREFIX=~/.wine-master wine reg delete \
  'HKLM\SYSTEM\CurrentControlSet\Enum\HID\VID_3151&PID_5030' /f 2>/dev/null
WINEPREFIX=~/.wine-master wineserver -k
```

## Firmware update protocol (captured)

The complete protocol sequence:

### Phase 1: Device enumeration and query (normal mode, PID=0x5030)

| Step | Direction | Command | Description |
|------|-----------|---------|-------------|
| 1 | SET_REPORT | `0x8F` GET_USB_VERSION | Query device identity |
| 2 | GET_REPORT | `0x8F` reply | Returns chip_id (0x850B) + fw_version |
| 3 | SET_REPORT | `0x8F` (repeated) | Second query for confirmation |
| 4 | GET_REPORT | `0x8F` reply | Same response |
| 5 | SET_REPORT | `0xC5` ISP_PREPARE | Prepare for firmware update (param 0x3A) |
| 6 | GET_REPORT | `0xC5` reply | Acknowledgement (zeros) |

### Phase 2: Enter bootloader

| Step | Direction | Command | Description |
|------|-----------|---------|-------------|
| 7 | SET_REPORT | `0x7F` + `55AA55AA` magic | ENTER_BOOTLOADER command |
| 8 | — | Device disconnects | Old PID=0x5030 disappears |
| 9 | — | Device re-enumerates | New PID=0x502A, usage_page=0xFF01 |

### Phase 3: Firmware transfer (bootloader mode, PID=0x502A)

| Step | Direction | Command | Description |
|------|-----------|---------|-------------|
| 10 | SET_REPORT | `0xBA 0xC0` FW_TRANSFER_START | chunk_count=2074, size=132708 bytes |
| 11 | GET_REPORT | `0xBA` reply | Acknowledgement |
| 12 | SET_REPORT | (2074 chunks) | Raw firmware data, 64 bytes per chunk |
| 13 | SET_REPORT | `0xBA 0xC2` FW_TRANSFER_COMPLETE | Includes checksum/size |
| 14 | GET_REPORT | `0xBA` reply | Acknowledgement |

### Phase 4: Post-flash verification (not yet captured)

After transfer, the app expects the device to reboot back to normal mode (PID=0x5030) and verifies the new firmware version. Our dummy doesn't implement this, so the app reports "update failed" after reaching 100%.

## Captured firmware analysis

The reconstructed firmware (`firmware_reconstructed.bin`, 132,736 bytes):

- **Header** (0x000-0x1FF): `AT32F405 8KMKB` — identifies Artery AT32F405 MCU
- **ARM Cortex-M vector table** (0x200): Initial SP=0x2000D510, Reset=0x080054F9 (Thumb)
- **Key strings**: "MonsGeek Keyboard", "M1 V5 HE BT1", "Keyboard Config"
- The encrypted `resources/x1` file (62,817 bytes) is decompressed/decrypted by the updater before transfer — the captured firmware is the plaintext image

## x1 format (SOLVED)

### Decoding

The `resources/x1` file is **NOT encrypted**. It is raw deflate with a 203-byte (0xCB)
junk header inserted after the first byte of the deflate stream.

**Decode algorithm:**
```python
import zlib

HEADER_SKIP = 0xCB  # 203

def decode_x1(data: bytes) -> bytes:
    """Decode x1 firmware blob → bootloader + firmware + trailer."""
    # Byte 0 is the deflate stream's first byte (always 0xEC = dynamic Huffman).
    # Bytes 1..202 are junk metadata — discarded by the updater.
    # Bytes 203+ are the rest of the deflate stream.
    wrapped = data[:1] + data[HEADER_SKIP:]
    return zlib.decompressobj(-15).decompress(wrapped)
```

**Decompressed layout:**
```
Offset 0x0000: Bootloader (20KB = 0x5000 bytes)
               ARM vector table at offset 0, SP=0x20004EB0
Offset 0x5000: Firmware (starts with "AT32F405 8KMKB  " chip ID header)
               Size varies per version (v407=132736B, v408=132864B)
End - N bytes: Trailer (v407=768B, v408=2019B, purpose unknown)
```

**One-liner to extract firmware from x1:**
```bash
python3 -c "import zlib; d=open('resources/x1','rb').read(); \
  open('firmware.bin','wb').write(zlib.decompressobj(-15).decompress(d[:1]+d[0xCB:])[0x5000:])"
```

Note: the trailing bytes after the firmware in the decompressed blob are included in
the output. To get exact firmware size, compare with the known size from the 0x8F
version query, or trim trailing 0xFF bytes.

### Why earlier analysis concluded "AES-encrypted"

The x1 body (bytes 148+) has entropy 7.99 bits/byte — near-maximum, consistent with
either encryption OR high-ratio compression. The `aes-0.8.4` crate (fixslice32) IS
compiled into `ry_upgrade.exe`, but is used only for the zip export feature
(`保存文件` ZipCrypto password), not for x1 decoding.

The critical mistake: we tried `zlib.decompress(x1[offset:], -15)` at offsets 0-259,
which **skips** bytes. The correct operation **preserves byte 0** and concatenates:
`data[:1] + data[0xCB:]`. Without byte 0 (the deflate block header 0xEC), the stream
is invalid. The junk header (bytes 1-202) acts as a simple obfuscation layer that
makes naive "skip N bytes and decompress" attempts fail.

### AESANDUH

The string `AESANDUH` at file offset `0xB55C71` in `ry_upgrade.exe` is a **false
positive** — coincidental ASCII alignment in Slint UI font glyph data, not a crypto
key. Verified by examining surrounding byte patterns (font metrics and outline
coordinates).

### Firmware distribution formats

Rongyuan distributes firmware in three formats:

| Format | First bytes | Description |
|--------|-------------|-------------|
| **Raw deflate** | `EC BD xx xx` | Plain compressed bootloader+firmware. Served via API. Decompresses directly with `zlib.decompress(data, -15)`. |
| **x1 bundle** | `EC xx xx xx` | Same deflate stream but with 202 junk bytes inserted after byte 0. Used inside `ry_upgrade.exe` zip bundles. Decode: `data[:1] + data[0xCB:]` then deflate. |
| **ZIP package** | `PK\x03\x04` | Older devices. ZIP with `firmwareFile.bin` + optional `firmwareOledFile.bin`. |

All formats decompress to: `bootloader(0x5000) + firmware + trailer`.

## How to run

### 1. Start the UHID dummy (as root; needs `/dev/uhid`)

```bash
sudo python3 scripts/uhid_dummy_device.py \
  --log-dir /tmp/monsgeek_dummy \
  --fw-version 0x0405
```

The dummy creates two UHID devices (MI_01 and MI_02), responds to HID queries, simulates bootloader reboot on ENTER_BOOTLOADER, and reconstructs firmware from the transfer.

### 2. Run the updater under Wine

```bash
WINEPREFIX=~/.wine-master \
SLINT_BACKEND=software \
WINEDEBUG=-all \
  ~/src-misc/wine/build-wow64/wine \
  "firmwares/2949-v407/ID_2949_RY5088_AKKO_M1V5 TMR_RY1033_ARGB_KB_V407_20260122/ry_upgrade.exe"
```

For debugging, use `WINEDEBUG=+setupapi,+hid` instead of `-all`.

### 3. Click Upgrade in the GUI

The device should appear in the dropdown. Click Upgrade to trigger the full protocol sequence. The reconstructed firmware is written to `$log_dir/firmware_reconstructed.bin`.

## Troubleshooting

### Device not listed in updater

- Check dummy log for `UHID_OPEN` — if missing, Wine didn't enumerate the device
- Delete stale Wine registry entries (see above) and restart wineserver
- Verify Wine is the patched build (check for CM_Get_Parent in `dlls/setupapi/devinst.c`)

### "Failed to enter boot"

- The bootloader device needs usage_page `0xFF01` (not `0xFFFF`)
- The dummy simulates this: destroys normal-mode devices, waits 2s, creates a single bootloader device with PID=0x502A

### "Update failed" at 100%

Expected with the dummy — the app successfully transferred firmware but can't verify the post-flash reboot. The firmware was captured successfully; check `firmware_reconstructed.bin`.

### Blocked / no progress

- The app may block on ReadFile on IF1. The dummy sends periodic heartbeat input reports and an initial report on OPEN.
- Check with `WINEDEBUG=+hid` for stuck I/O.

## Known command bytes

| Byte | Name | Direction |
|------|------|-----------|
| 0x01 | SET_RESET | → device |
| 0x7F | ENTER_BOOTLOADER | → device |
| 0x80 | GET_REV | → device |
| 0x8F | GET_USB_VERSION | → device |
| 0xBA | FW_TRANSFER | → device |
| 0xBA 0xC0 | FW_TRANSFER_START | → device |
| 0xBA 0xC2 | FW_TRANSFER_COMPLETE | → device |
| 0xC5 | ISP_PREPARE | → device |

See `COMMAND_NAMES` in `scripts/uhid_dummy_device.py` for the full list.
