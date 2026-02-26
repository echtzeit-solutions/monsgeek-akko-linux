# MonsGeek M1 V5 TMR Hardware

## Overview

The MonsGeek M1 V5 HE TMR is a wireless mechanical keyboard with TMR (Tunnel
Magneto-Resistance) analog Hall-effect switches and a dual-MCU architecture.
The main controller handles keyboard logic, USB, and LED control. A secondary
Bluetooth controller handles the wireless radio stack.

A separate USB 2.4 GHz dongle is included for low-latency wireless mode.

**Manufacturer**: RongYuan Technology (`rongyuan.tech`), sold under the MonsGeek
and Akko brands.

## Keyboard

### Main Controller: Artery AT32F405

| Property | Value |
|----------|-------|
| Marketing name | RY5088 (RongYuan branding for the same silicon) |
| Core | ARM Cortex-M4F @ 216 MHz |
| Flash | 256 KB |
| SRAM | 56 KB (0x20000000 - 0x2000DFFF) |
| Package | LQFP64 (4 x 16 pins) |
| Chip ID string | `AT32F405 8KMKB` (at flash 0x08005000) |
| USB | Full-Speed OTG (OTGFS1) |
| USB IDs (normal) | VID 0x3151, PID 0x5030 |
| USB IDs (bootloader) | VID 0x3151, PID 0x502A ("RY5088 Keyboard Boot") |
| USB IDs (ROM DFU) | VID 0x2E3C, PID 0xDF11 ("Artery-Tech DFU in FS Mode") |
| SWD Part ID | Designer 0x43B, Part ID 0x4C4 |
| Datasheet | `docs/AT32F405_datasheet.pdf` |
| Reference manual | `docs/AT32F405_refmanual.pdf` |

### Bluetooth Controller: Panchip PAN1080

| Property | Value |
|----------|-------|
| Package marking | APAN1080UA3C |
| Core | ARM Cortex-M0 |
| Flash | 512 KB (base address 0x00000000, not 0x08000000) |
| SRAM | ~34 KB (SP init 0x200085F8) |
| Radio | BLE 5.1 + 2.4 GHz proprietary |
| SDK | Panchip ZDK (Zephyr-based), NOT YiChip |
| BLE device names | `ROYUAN KEYBOARD`, `ROYUAN_Mouse` |
| Product string | `PAN108 MEKBRF-9K` |
| Firmware string | `RTL:X.XXX-XX0XX_FIRMWARE:1.00A-LCA01` |
| SWD Part ID | Designer 0x43B, Part ID 0x471 |
| SWD pins | P46 = SWCLK, P47 = SWDIO |
| JLink device name | `PAN1080XA` |

**PAN1080 flash layout:**

| Region | Size | Content |
|--------|------|---------|
| 0x00000 - 0x0FFFF | 64 KB | MCUBoot bootloader |
| 0x10000 - 0x2FFFF | 128 KB | LL Controller (Link Layer) |
| 0x30000 - 0x7FFFF | ~150 KB | Application (BLE stack + HID) |

**PAN1080 features**: L2CAP, ATT/GATT, SMP (pairing/bonding), NVS key storage,
BAS (Battery Service). UART DFU on P30(TX)/P31(RX) if MCUBoot serial is enabled.

### Key Matrix

- **Switch type**: TMR (Tunnel Magneto-Resistance) Hall-effect, analog
- **Matrix**: 8 rows x 16 columns (with gaps), 82 active keys
- **Sensors**: Passive magnetoresistors excited by VDDA-VSSA (ratiometric)
- **ADC**: Single 12-bit ADC peripheral, 15 channels scanned via DMA in a
  continuous loop. Channels 0-13 are Hall-effect sensor columns; channel 14 is
  the battery voltage divider.
- **GPIO pins**: PA0-PA5 (6), PB0-PB7 (8), PC0 (1) = 15 analog inputs total
- **Row multiplexer**: Advances every 6 DMA cycles across 8 rows
- **Scan rate**: Timer-triggered ADC -> DMA transfers 15 readings per ISR
- **LED strip**: WS2812 serpentine layout, 82 LEDs. Even rows L->R, odd rows R->L.
  Position table at flash 0x08025031 (96 bytes): `[row*16+col]` -> strip index.

### Battery

| Property | Value |
|----------|-------|
| ADC channel | 14 (battery voltage divider) |
| Charger detect | GPIOC.13 = charger connected (active LOW) |
| Charge status | GPIOB.10 = charging indicator (flaky, fluctuates) |
| ADC -> percentage | > 0x6A8 = 100%, 0x500-0x6A8 = 20-100% (linear), 0x479-0x500 = 1-20%, <= 0x479 = 1% |
| Debounce | 5 consecutive readings to update UP (charging), 10 for DOWN (discharging) |
| Anti-glitch | raw == 1% on charger -> force 100%. GPIOB.10 HIGH -> cap at 99% |
| Charger disconnect | 20-cycle debounce before clearing charger_connected flag |

**USB ADC quirk**: On USB power the battery ADC drops ~18% (311 counts) due to
USB OTG PHY power/ground routing affecting the ADC reference voltage. BT mode
reads 0x6CC (100%), USB reads 0x595 (48%) for the same charge level. LED current
draw accounts for ~21 counts (7% of the drop); the remaining 93% is caused by
the USB peripheral/PHY activity itself.

The TMR key sensors are unaffected because they are ratiometric: the sensor
output is proportional to (VDDA-VSSA), and the ADC measures V_out / (VDDA-VSSA)
= k(B), so supply voltage terms cancel. The battery is an external
electrochemical source whose voltage is fixed by chemistry and does not track
the supply shift, so its ADC ratio changes when VSSA lifts.

Measured ADC values (BMP/GDB, 2026-02-11). The 8 readings are temporal samples
of channel 14 (one per row scan cycle), not different ADC channels:

| Mode | ADC avg | Mapped % | Reported % |
|------|---------|----------|------------|
| BT | 0x6CC | 100% | 99% (GPIOB.10 cap) |
| USB + LEDs on | 0x595 | 48% | 48% |
| USB + LEDs dim | 0x5AA | 52% | 52% |

**Debounce issue** (confirmed via RTT, 2026-02-26): ADC noise (+/-3-5 counts)
constantly resets the debounce counter, preventing the battery level from
updating. The raw level oscillates (e.g. 46%-47%-48%) and never achieves the
required 5 (charging) or 10 (discharging) consecutive same-direction readings.
Only large step changes (like the 311-count USB shift) survive.

**GPIO signals:**

| GPIO | Pin | Meaning when LOW | Meaning when HIGH |
|------|-----|------------------|-------------------|
| GPIOC | 13 | Charger connected | Charger disconnected |
| GPIOB | 10 | (fluctuates) | Cap battery at 99% |

**Firmware behavior** (`battery_level_monitor` at 0x0801695C):
- When charger connected and raw == 1%: forces to 100% (catches extreme USB ADC drop)
- Level only increases toward raw while charging (never decreases)
- GPIOB.10 HIGH caps level at 99%; once triggered, increase-only logic prevents recovery to 100%
- No force-to-100% code path for charge complete
- Charger disconnect: 20-cycle debounce before clearing charger_connected flag

**Potential fixes:**
1. **Patch HID report** (simplest): Override battery % in GET_REPORT response based on charger state
2. **USB-mode ADC offset** (+311 counts when connection_mode == USB): More accurate but per-unit calibration needed
3. **Ignore ADC on USB**: When charger connected, infer from GPIO state (matches OEM app behavior)

## 2.4 GHz Dongle

| Property | Value |
|----------|-------|
| PCB marking | ry6208 v0.1 20250215 (board design name, NOT a chip) |
| MCU | RY5088 (= AT32F405), QFN32 (4 x 8 pins) |
| Wireless transceiver | PAN1082 (2.4 GHz "Logic Chip", not full BT SoC) |
| Chip ID string | `AT32F405 8K-DGKB` |
| USB IDs (normal) | VID 0x3151, PID 0x5038 |
| USB IDs (bootloader) | VID 0x3151, PID 0x5039 ("USB DONGLE BOOT") |
| USB IDs (ROM DFU) | VID 0x2E3C, PID 0xDF11 |
| USB interfaces | IF0 boot kbd, IF1 boot kbd (rdesc 171B), IF2 vendor HID (rdesc 20B) |
| Firmware size | ~34 KB (0x08005000 - 0x0800D5D8) |
| USB speed | High-Speed (OTGHS, not OTGFS1 like the keyboard) |

**QFN32 pinout** (chip text orientation rotated 90 deg CCW from datasheet; pin 1
dot at bottom-left when label is readable):

| Pin | Function | PCB pad |
|-----|----------|---------|
| 4 | NRST | - |
| 23 (PA13) | SWDIO | Yes |
| 24 (PA14) | SWCLK | - |
| 31 | BOOT0 | - (3.3k pulldown to GND) |
| 32 (PB8) | USART1_TX | Yes (factory debug serial) |

**DFU entry** (RY bootloader): Send vendor command 0x7F + 55AA55AA as feature
report (report ID 0) on IF1 (usage_page=0xFFFF, usage=0x01). The bootloader
is upload-only (no flash read command). WARNING: entering the bootloader triggers
an immediate mass erase of the application region.

**Dongle role**: Pure USB-to-RF bridge. Vendor HID commands are relayed to the
keyboard over the 2.4 GHz link and are not processed locally. The dongle caches
battery level and charging state from incoming RF packets (Feature Report ID 5).

## AT32F405 Flash Memory Map

Applies to both the keyboard (256 KB) and dongle (256 KB). The keyboard uses
the full map; the dongle only uses through ~0x0800D600.

```
0x08000000 - 0x08004FFF  Bootloader (20 KB, protected)
  0x08004800              Bootloader mailbox (0x55AA55AA = enter DFU on next boot)
0x08005000 - 0x080051FF  Chip ID header ("AT32F405 8KMKB" or "AT32F405 8K-DGKB")
0x08005200 - 0x0800523F  Vector table (VTOR = 0x08005200)
0x08005240 - 0x08025680  Firmware code + data (~129 KB for v407)
0x08025800 - 0x08027FFF  Patch zone (10 KB, used by firmware patches)
0x08028000 - 0x080287FF  Config header (profile, LED, settings)
0x08028800 - 0x0802A7FF  Keymaps (stride 0x800/profile)
0x0802A800 - 0x0802B7FF  Fn layers (stride 0x200)
0x0802B800 - 0x0802EFFF  Macros (stride 0x100, 256 B/entry)
0x0802F800 - 0x0802FFFF  User pictures / LED patterns (stride 0x180)
0x08032000 - 0x080337FF  Magnetism calibration data (preserved on reset)
0x08033800 - 0x080377FF  Magnetism per-key tables (stride 0x1000)
```

**Flash erase granularity**: 2048 bytes (AT32F405 256 KB variant).

**Factory reset**: Erase 2 KB at 0x08028000 via DFU. This resets all config,
keymaps, and macros without touching the firmware or calibration.

## Bootloader (RY Bootloader)

Both the keyboard and dongle share the same bootloader design at 0x08000000-0x08004FFF.

**Entry conditions** (either triggers bootloader):
1. Mailbox at 0x08004800 == 0x55AA55AA
2. Chip ID at 0x08005000 does not match expected string

**Behavior on entry**:
1. Immediately erases application region: 70 pages x 2 KB = 140 KB
   (0x08005000-0x08027FFF) BEFORE USB init. This is the point of no return.
2. Initializes USB with bootloader PID (0x502A keyboard, 0x5039 dongle)
3. Accepts firmware via 0xBA command frames (CRC-24 checksum)

**Successful flash**: Clears mailbox page -> reboot -> chip ID valid -> jumps to firmware.
**Failed flash**: Mailbox still set -> re-enters bootloader (firmware already gone).

**Checksum bug**: The bootloader checksums ALL bytes of ALL 64-byte chunks
including 0xFF padding in the last chunk. The firmware `0x7F` handler erases
config BEFORE writing the mailbox, so config is lost on DFU entry too.

**Custom memcmp** (0x08000A62): Returns 1 for match, 0 for mismatch (inverted
from libc convention).

## ROM DFU Recovery (AT32F405 Built-in Bootloader)

The AT32F405 has a mask ROM bootloader accessible via the BOOT0 pin, independent
of the RY bootloader. This is the last-resort recovery method.

1. Bridge BOOT0 pad to 3.3V, then plug USB (or reset)
2. Enumerates as VID:PID `2E3C:DF11` ("Artery-Tech DFU in FS Mode")
3. Read flash: `dfu-util -a 0 -d 2e3c:df11 --dfuse-address ADDR -U out.bin --upload-size SIZE`
4. Write flash: `dfu-util -a 0 -d 2e3c:df11 --dfuse-address ADDR -D in.bin`

**Flash Access Protection (FAP)**:
- FAP byte at 0x1FFF_F800[7:0] in user system data area
- 0xA5 = disabled (debug and DFU read OK)
- 0xCC = high-level protection (permanent, cannot unlock)
- Any other value = low-level protection (locked)
- **DANGER**: Writing 0xA5 to unlock low-level protection triggers a mass erase
  of ALL flash. Always check FAP before attempting to read.
- Keyboard FAP = 0xA5 (confirmed). Dongle FAP: verify before first read.

**Udev rule** (for non-root access):
```
ATTRS{idVendor}=="2e3c", ATTRS{idProduct}=="df11", TAG+="uaccess"
```

## Debug Interfaces

### AT32F405 SWD (Keyboard, 5-pin header)

| Pin | Function |
|-----|----------|
| 1 (square pad) | VCC (3.3V) |
| 2 | SWDIO (PA13) |
| 3 | SWCLK (PA14) |
| 4 | GND |
| 5 | NRST (leave disconnected - BMP drives it low and resets MCU) |

### PAN1080 SWD (Keyboard, 6-pin header)

| Pin | Function |
|-----|----------|
| 1 (square pad) | VDD_TARGET (3.3V sense) |
| 2 | SWCLK |
| 3 | GND |
| 4 | SWDIO |
| 5 | NRST |
| 6 | SWO |

### Dongle SWD (test pads on PCB)

SWDIO (PA13, pin 23) and PB8/USART1_TX (pin 32) are exposed as PCB pads.

### Debug Notes

- Use `gdb-multiarch` on Ubuntu (not `arm-none-eabi-gdb` which may not be installed)
- BMP clone quirk: reverts to ST-Link firmware on power loss, needs reflashing
- SEGGER RTT works via BMP: `monitor rtt ram 0x20009800 0x20009C00`, then
  `monitor rtt enable`. Target must remain attached (not detached) for polling.
  RTT data appears on the secondary BMP serial port.
- RTT ident quirk: `monitor rtt ident SEGGER` (single word only; the full string
  `"SEGGER RTT"` with a space returns `what?`)
- PAN1080 flash is readable via SWD (no readout protection observed). Full 512 KB
  dump obtained successfully.

## ADC Limitations

From the AT32F405 reference manual analysis:

- VDDA/VSSA are the sole ADC reference (no separate VREF pins)
- Self-calibration (ADCAL): corrects internal capacitance errors only (7-bit),
  does NOT compensate external reference shifts
- VINTRV (1.2V internal reference, ADC_IN17): shares VDDA/VSSA, shifts with
  ground lift, useless for offset calibration
- Hardware oversampling (up to 256x): averages noise, NOT DC offset
- ADC_PCDTOx offset registers: only for preempted channels (firmware uses
  ordinary + DMA mode)
- **No hardware mechanism can compensate the USB VSSA ground shift.** Software
  offset compensation based on connection mode is the only viable fix.

## Firmware Versions

| Version | Size | Notes |
|---------|------|-------|
| v407 | 132,736 B | Current, fully RE'd in Ghidra (170 functions, 0 unnamed) |
| v405 | ~207 KB | Dumped from original PCB via DFU |
| Dongle | 34,264 B | Dumped via SWD, fully RE'd (396 functions, 100% coverage) |
| PAN1080 | 512 KB | Dumped via SWD, Zephyr BLE stack |
| Bootloader | 20 KB | Shared design, 103/163 functions named (63%) |

Firmware binaries tracked in `firmwares/`:
- `firmwares/2949-v407/firmware_reconstructed.bin` - keyboard v407
- `firmwares/DONGLE_RY6108_RF_KB_V903/dfu_dumps/dongle_working_256k.bin` - dongle
