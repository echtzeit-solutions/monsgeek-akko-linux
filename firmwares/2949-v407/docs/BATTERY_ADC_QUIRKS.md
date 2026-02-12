# Battery ADC Quirks — MonsGeek M1 V5 TMR (v407)

## Summary

The battery voltage ADC reading drops ~18% when the USB data path is active,
causing the firmware to report ~48% battery on USB even when fully charged.
The same battery correctly reports 99-100% in Bluetooth mode.

## ADC Measurements (2026-02-11, BMP/GDB)

All 8 ADC channels sampled from `g_mag_engine_state` (base 0x20003410):

| Offset | Address      | BT mode | USB+LEDs on | USB+LEDs dim |
|--------|-------------|---------|-------------|--------------|
| 0x878  | 0x20003C88  | 0x06CE  | 0x0596      | 0x05AB       |
| 0x8a2  | 0x20003CB2  | 0x06CC  | 0x0596      | 0x05AA       |
| 0x8cc  | 0x20003CDC  | 0x06CE  | 0x0591      | 0x05A8       |
| 0x8f6  | 0x20003D06  | 0x06CE  | 0x0590      | 0x05AB       |
| 0x920  | 0x20003D30  | 0x06CF  | 0x0590      | 0x05AB       |
| 0x94a  | 0x20003D5A  | 0x06CD  | 0x058E      | 0x05AB       |
| 0x974  | 0x20003D84  | 0x06CF  | 0x058F      | 0x05A8       |
| 0x99e  | 0x20003DAE  | 0x06CE  | 0x058E      | 0x05AA       |
| **Avg** | 0x20000010 | **0x06CC** | **0x0595** | **0x05AA** |

### ADC-to-percent mapping (from `battery_level_monitor` @ 0x0801695c)

```
ADC < 0x47A         → 1%
ADC 0x47A–0x500     → 1–20%   (linear)
ADC 0x501–0x6A8     → 20–100% (formula: (adc-0x500)*0x50/0x1A9 + 0x14)
ADC >= 0x6A9        → 100%
```

### Resulting battery levels

| Mode           | ADC avg | Mapped % | Reported % | Reason for difference    |
|----------------|---------|----------|------------|--------------------------|
| BT             | 0x6CC   | 100%     | **99%**    | GPIOB charge cap (see below) |
| USB + LEDs on  | 0x595   | 48%      | **48%**    | ADC too low for any override |
| USB + LEDs dim | 0x5AA   | 52%      | **52%**    | Slight improvement from lower current |

## Root Cause

Activating the USB data path drops the ADC input voltage by ~311 counts (18%).
LED current draw accounts for ~21 counts (7% of the drop); the remaining 93%
is caused by the USB peripheral/PHY activity itself.

Likely mechanism: the AT32F405 USB OTG PHY shares power/ground routing with
the ADC reference or the battery voltage divider, introducing a DC offset when
the USB data lines are active. The 8 ADC channels (which double as Hall-effect
magnetism sensors) all shift uniformly, ruling out per-channel noise.

## Firmware Behavior

### `battery_level_monitor` (0x0801695c)

When charger is connected (GPIOC pin 13 LOW):

1. **ADC == 1% override**: If `battery_raw_level == 1`, force to 100%.
   This catches the case where ADC reads extremely low on USB (< 0x47A).
   Does NOT trigger for intermediate values like 48%.

2. **Smoothing**: `battery_level` only increases toward `battery_raw_level`
   (via `battery_update_ctr`, 5 cycles). Never decreases while charging.

3. **99% cap**: `gpio_input_data_bit_read(GPIOB, 0x400)` — when HIGH
   AND `battery_level > 99`, caps to 99%. This pin fluctuates; once the
   cap triggers, the smoothing logic (increase-only) prevents recovery to 100%.

4. **No force-to-100 for charge complete**: There is no code path that
   overrides battery_level to 100% when charge is complete. The `charge_status`
   field at `g_kbd_state + 0x4D` is set elsewhere, not consulted here.

### GPIO signals

| GPIO | Pin | Address    | Meaning when LOW | Meaning when HIGH |
|------|-----|------------|------------------|-------------------|
| GPIOC | 13 | 0x40020810 | Charger connected | Charger disconnected |
| GPIOB | 10 | 0x40020410 | (fluctuates)     | Cap battery at 99%  |

GPIOB pin 10 fluctuates between reads — it appears to toggle periodically
rather than being a stable charge-complete indicator.

## Potential Fixes

### Option A: Patch override in battery HID report (simplest)

In `handle_hid_setup`, when building the battery Feature Report response,
check `charger_connected` and override:
- If `charger_connected == 1` AND `battery_raw_level == 100` → report 100%
- If `charger_connected == 1` AND `battery_raw_level < 100` → report 99%
- Otherwise → report `battery_level` as-is

### Option B: USB-mode ADC compensation

Apply a +311 count offset to the ADC average when connection mode is USB.
More accurate but requires calibration per unit and may drift with temperature.

### Option C: Ignore ADC on USB, infer from charger state

When `charger_connected == 1`, don't trust ADC at all:
- GPIOB pin 10 stable LOW → report 100% (fully charged)
- Otherwise → report 99% (charging)

This matches the OEM app behavior (which never shows battery % on USB).
