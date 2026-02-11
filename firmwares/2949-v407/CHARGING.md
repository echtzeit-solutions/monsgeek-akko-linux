# Battery & Charging State Reporting — Firmware Analysis

Firmware: AT32F405 v407 (`firmware_reconstructed.bin` in Ghidra project `ghidra_v407`)

## Summary

**Charging state is NOT available in USB wired mode.**
It is only reported in wireless/dongle mode, where the dongle firmware
combines battery level (RF report 0x90) with charging state (RF event
type 3) and presents them as a single 0x88 vendor event to the host.

This is a firmware design limitation — the charging GPIOs are read but
the result is only propagated to the wireless event path, not to the USB
feature report.

## Hardware: Charger & Battery GPIOs

| GPIO        | Pin | Signal              | Active   |
|-------------|-----|---------------------|----------|
| GPIOC       | 13  | Charger connected   | LOW      |
| GPIOB       | 10  | Charge complete     | HIGH     |
| GPIOA       | 11  | VBUS (USB cable)    | HIGH     |

## Firmware Functions

### battery_level_monitor — `0x0801195c`

Core battery state machine, called from main loop.

- Averages 8 ADC samples from battery voltage channel
- ADC→percent mapping:
  - `< 0x47A` → 1%
  - `0x47A–0x500` → scaled 1–20%
  - `0x500–0x6A9` → linear 20–100%
  - `>= 0x6A9` → 100%
- Charger detection: reads GPIOC pin 13 (LOW = charger connected)
- Charge complete: reads GPIOB pin 10 (HIGH = charge done)
- Caps level at 99% while actively charging (to distinguish from full)
- On level change: sets `g_dongle_report_flags[6] = 1` to trigger RF
  report 0x90

### usb_cable_detect — `0x08011384`

USB VBUS detection, called from main loop.

- Reads GPIOA pin 11 (0x800)
- Debounces 15 consecutive same-state readings
- Updates `g_connection_config+4` bits 1–2 with cable status
- Sends `bt_event_queue(3, flags_byte, 2)` to notify dongle of cable
  state change
- The dongle uses this to derive the "charging" flag in 0x88 events

### build_dongle_reports — `0x080124c0`

Builds RF packets for dongle transmission.

When `g_dongle_report_flags[6]` is set (battery changed), sends:

```
RF report 0x90: [0x90, 0x01, battery_level, checksum]
```

This carries ONLY the battery level byte. **No charging flag.**

### bt_event_queue — `0x0800d7ac`

Queues events for BT/dongle transport as `[report_id=5, cmd, d1, d2]`.
Only active when NOT in USB wired mode (`connection_mode != 6`) or when
dongle is paired.

## Data Flow

### USB Wired Mode

```
battery_level_monitor
  ├─ writes g_battery_level (g_kbd_state+0x40)
  └─ (dongle_report_flags[6] ignored — build_dongle_reports skipped)

Host polls feature report 0x05 (via SET cmd 0xF7 BATTERY_REFRESH):
  response[1] = battery_level   ← available
  charging                      ← NOT populated
```

### Dongle Wireless Mode

```
battery_level_monitor
  └─ sets g_dongle_report_flags[6] = 1
       └─ build_dongle_reports sends RF 0x90 [level]
            └─ dongle receives battery level

usb_cable_detect
  └─ bt_event_queue(3, flags, 2) → RF event
       └─ dongle receives cable/charging flags

Dongle firmware (separate chip, not in this binary) COMBINES:
  battery level from 0x90  +  charging from event type 3
  → presents as async HID vendor event 0x88:
    [0x88, 0x00, 0x00, battery_level, flags]
    flags: bit 0 = online, bit 1 = charging
```

### BT Wireless Mode

Same as dongle — `bt_event_queue` sends events over BT profile instead
of RF. The BT host stack on the receiving end handles the 0x88 event
equivalently.

## What the Driver Can Read Today

| Mode              | Source                         | Level | Charging | Driver method                    |
|-------------------|--------------------------------|-------|----------|----------------------------------|
| USB wired         | Feature report 0x05 (cmd 0xF7) | Yes   | **No**   | `BatteryInfo::from_feature_report` |
| Dongle wireless   | Async event 0x88 on input EP   | Yes   | Yes      | `BatteryInfo::from_vendor_event`   |
| BT wireless       | Async event via BT profile     | Yes   | Likely   | Same event mechanism               |

## Driver TODO: Charging in USB Wired Mode

The firmware reads GPIOC:13 (charger) and GPIOB:10 (charge complete)
every cycle but only propagates the result through the wireless event
path. In USB mode the information is stranded in RAM:

| RAM address    | Field                   | Description                      |
|----------------|-------------------------|----------------------------------|
| `0x2000049D`   | `g_charger_connected`   | Debounced flag, 1 = on charger   |
| `0x200004A9`   | `g_charge_status`       | 0=unknown, 1=charging, 2=complete|

Possible approaches to expose charging over USB:

1. **Add charging to the feature report response.** The 0xF7 response
   has spare bytes. `data[2]` is currently unused. The firmware would
   need a patch (or a BPF fixup on the host side if the report
   descriptor allows it).

2. **Use the vendor GET command 0x88 with different semantics.**
   Currently GET 0x88 returns LED/display config (offsets 0x0D–0x13 of
   `g_connection_config` at `0x20007470`). Reusing it for battery would
   break that config channel.

3. **Poll the HID raw data.** The driver could send a custom vendor
   command that reads `g_charge_status` directly. This requires knowing
   a vendor command that exposes arbitrary RAM reads (none found yet).

4. **Infer from behavior.** If the battery level is exactly 99% and
   rising, assume charging. Fragile but requires no firmware changes.

## Vendor Command 0x88 GET vs Async Event 0x88

These share the same byte value `0x88` but are completely different:

| Context          | Meaning                        | Data source                      |
|------------------|--------------------------------|----------------------------------|
| GET cmd response | LED/display config (7 bytes)   | `g_connection_config` +0x0D–0x13 |
| Async input event| Battery status                 | Dongle-constructed from RF data  |

The GET 0x88 / SET 0x08 pair is for LED configuration, not battery.

## bt_event_queue Event Type Map

| Event | Source               | Description                                   |
|-------|----------------------|-----------------------------------------------|
| 0x03  | usb_cable_detect     | USB cable connect/disconnect (charging flags)  |
| 0x04  | key_setting_adjust   | Profile/mode switch                            |
| 0x05  | key_setting_adjust   | Sensitivity/DPI change                         |
| 0x06  | key_setting_adjust   | Magnetic switch setting                        |
| 0x07  | key_setting_adjust   | LED mode/effect change                         |
| 0x0F  | flash_save_* (7 fns) | Flash save status (1=pending, 0=complete)      |

## RAM Labels (Ghidra)

| Address      | Label                    | Notes                             |
|--------------|--------------------------|-----------------------------------|
| `0x20007132` | `g_dongle_report_flags`  | Byte array, index 6 = battery     |
| `0x20007470` | `g_connection_config`    | +4 bits 1–2 = USB cable state     |
| `0x2000708C` | `g_rf_tx_buffer`         | 0x50 byte RF packet buffer        |
| `0x200082DC` | `g_vendor_cmd_buffer`    | Vendor HID report buffer          |
| `0x2000045C` | `g_kbd_state`            | Main state struct (battery at +0x40) |
