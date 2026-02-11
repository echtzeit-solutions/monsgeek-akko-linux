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

## x1 encryption analysis

### File format

The `resources/x1` file (62,817 bytes) consists of two parts:

| Offset | Size | Entropy | Description |
|--------|------|---------|-------------|
| 0 | 148 bytes | 6.68 bits/byte | **Header** — structured metadata, lower entropy |
| 148 | 62,669 bytes | 7.99 bits/byte | **Body** — encrypted compressed firmware |

The body size (62,669 bytes) **exactly matches** the output of `flate2 1.1.2` / `miniz_oxide 0.8.8` deflate compression at level 1. This confirms:

```
x1 = header(148) + encrypt(deflate_level1(firmware))
```

### Firmware distribution formats

Rongyuan distributes firmware in two formats:

| Format | Header bytes | Description |
|--------|-------------|-------------|
| **Raw deflate** | `EC BD xx xx` | Plain compressed firmware, no encryption. Served via API for older/some devices. Decompresses directly with `zlib.decompress(data, -15)`. |
| **Encrypted x1** | `EC 8C C2 F1...` | Used inside `ry_upgrade.exe` bundles. 148-byte header + AES-encrypted deflate body. |

Examples of raw deflate firmware files (all decompress successfully):
- `2116_v300.bin` (51,027 B) → 126,240 B firmware
- `2450_ry5088_gk75_dm_8k_002.bin` (55,286 B) → 131,580 B firmware
- `3198_v804.bin` (149,230 B) → 256,084 B firmware

ZIP-packaged firmware (older devices, unencrypted):
- `2454_ry5088_nj81cp_8k_8k.bin` — ZIP with `firmwareFile.bin` + `firmwareOledFile.bin`

### Encryption details (partially reverse-engineered)

**Crate versions in the binary** (from embedded Rust source paths):
- `aes-0.8.4` — AES cipher (`src/soft/fixslice32.rs`, 32-bit bitsliced implementation)
- `zip-2.2.0` — ZIP library with `src/aes_ctr.rs` (WinZip AES-CTR variant)
- `flate2-1.1.2` / `miniz_oxide-0.8.8` — deflate compression
- `rand-0.8.5` / `rand_chacha-0.3.1` — random number generation
- `zeroize-1.8.1` — secure memory wiping

**Critically absent** (zero occurrences in binary):
- `pbkdf2` — no password-based key derivation
- `hmac` — no HMAC authentication
- `sha1`, `sha2`, `sha256` — no hash functions
- `constant_time_eq` — no constant-time comparison
- `md5`, `digest`, `crypto_common` — no cryptographic hash infrastructure

This means the zip crate's **standard WinZip AES pipeline** (PBKDF2-HMAC-SHA1, 1000 iterations) **is NOT used**. The AES key must be directly embedded in the binary or derived through simple byte manipulation — not from a password via any standard KDF.

**String of interest**: `AESANDUH` at file offset `0xB55C71` (.rdata section, RVA `0xB56C71`). Appears exactly once. Its role is unknown — it does NOT work as a direct AES key (padded/repeated to 16 or 32 bytes) or as a PBKDF2 password.

### What was tried (and failed)

| Approach | Details | Result |
|----------|---------|--------|
| WinZip AES (PBKDF2-HMAC-SHA1) | 42 password candidates × 2 offsets × 2 key sizes | No pwd_verify match |
| Direct AES-CTR (WinZip variant) | AESANDUH padded/repeated to 16/32 bytes | Decrypted output is not deflate |
| Direct AES-CTR (NIST variant) | Big-endian counter, start 0 and 1 | Same |
| Header bytes as AES key | x1[0:16], x1[0:32], x1[1:17], x1[-16:], x1[-32:] | No valid deflate |
| Hash-derived keys | MD5/SHA256 of AESANDUH and variants | No valid deflate |
| Simple transformations | All 256 single-byte XOR, bit reverse, nibble swap, NOT | No valid deflate at any offset |
| Multi-byte XOR | "AESANDUH" repeated, position-dependent XOR | No valid deflate |
| Raw deflate at offsets 0-300 | With various wbits settings | Only trivial matches |
| Alternative compression | LZMA, bzip2, gzip wrappers | All fail |

### Next steps

The AES key is embedded somewhere in the 20 MB binary but not derivable from strings alone. Recommended approaches:

1. **Frida runtime hooking** — Hook `aes::Aes128::new()` or `aes::Aes256::new()` while the app decrypts x1 to capture the raw key bytes. Infrastructure exists: `frida_attach.py` + `hook_hid.js`.
2. **Ghidra cross-reference analysis** — Find code that references the AESANDUH string address (RVA `0xB56C71`) and trace how the AES key is constructed. Ghidra project exists at `ghidra_project/`.
3. **Memory dump** — Run the app, let it decrypt x1, then dump process memory to find the plaintext deflate stream or the AES round keys.

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
