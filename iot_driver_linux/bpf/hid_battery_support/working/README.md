# Working Solution: Vendor Interface with Dummy Input

**Status: WORKS**

This is the currently working solution that successfully creates a power_supply entry.

## How it Works

1. BPF attaches to vendor interface (00DA) via the probe function
2. `rdesc_fixup` replaces the vendor descriptor with:
   - Usage Page 0x01 (Generic Desktop) + Usage 0x06 (Keyboard)
   - Battery Strength as Input report (triggers battery detection)
   - Dummy modifier key (forces input device creation)
3. `hw_request` fixes firmware quirk (Report ID 0x00 → 0x05)
4. Kernel creates input device → creates power_supply → UPower detects it

## Result

```
/sys/class/power_supply/hid-0003:3151:5038.00DA-battery/
├── capacity (e.g., 87)
├── model_name ("MonsGeek 2.4G Wireless Keyboard")
├── status ("Discharging")
└── type ("Battery")
```

## Usage

```bash
# Build
make clean && make

# Load (finds vendor interface automatically)
sudo ./loader

# Or specify hid_id directly (00DA = 218 decimal)
sudo ./loader 218
```

## Files

- `akko_dongle.bpf.c` - BPF program with probe, device_event, rdesc_fixup, hw_request
- `loader.c` - C loader using libbpf skeleton

## Limitations

- UPower shows device type as "gaming-input" (not "keyboard")
- Creates a phantom input device on the vendor interface
- Loader must stay running (or pin to bpffs)
