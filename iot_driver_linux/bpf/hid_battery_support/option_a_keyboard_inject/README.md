# Option A: Inject Battery into Keyboard Interface (RECOMMENDED)

**Status: WORKS!**

This approach injects a battery Feature report into the main keyboard interface (00D8) instead of modifying the vendor interface.

## Key Discovery: F7 Command Required

**The dongle requires an F7 command to refresh battery data from the keyboard!**

Without F7, Feature Report 5 returns stale/cached data or zeros:
```
# After replug (no F7):
GET_FEATURE Report 5: 0000000000000000  → zeros

# After sending F7:
GET_FEATURE Report 5: 0053000001010100  → Battery=83%
```

The F7 command triggers the dongle to query the keyboard over the 2.4GHz RF link. The battery value is then cached in the dongle until the next F7 command.

**This means userspace must periodically send F7 to keep battery fresh!**

## How It Works

1. BPF attaches to keyboard interface (00D8) via probe function
2. `rdesc_fixup` appends a Battery Strength Feature report to the existing keyboard descriptor
3. **Loader sends F7 command** to trigger initial battery read from keyboard
4. Loader rebinds device to apply new descriptor
5. Kernel polls Feature Report 5 → dongle returns cached battery data
6. `hw_request` hook fixes the Report ID quirk (0x00 → 0x05)
7. **Loader periodically sends F7** to refresh battery (every 30-60s)

## Advantages

- **No phantom input device** - uses existing keyboard input device
- **No BPF maps needed** - dongle caches battery data
- **Cleaner architecture** - simple descriptor append + Report ID fix
- **Power supply on keyboard** - associated with actual keyboard device

## Limitations

- **Requires userspace daemon** - F7 must be sent periodically to refresh battery
- **Battery can become stale** - if daemon stops, cached value remains until next F7

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                          Userspace                               │
│  ┌─────────────────┐                                            │
│  │  BPF Loader     │  Sends F7 command every 30-60s             │
│  │  (daemon)       │  to refresh battery in dongle cache        │
│  └────────┬────────┘                                            │
│           │ SET_FEATURE [0x00, 0xF7, ...]                       │
├───────────┼─────────────────────────────────────────────────────┤
│           ▼                     Kernel                           │
│  ┌─────────────────┐                                            │
│  │  HID-BPF on     │  rdesc_fixup: append battery descriptor    │
│  │  keyboard iface │  hw_request: fix Report ID (0x00 → 0x05)   │
│  │  (00D8)         │                                            │
│  └────────┬────────┘                                            │
│           │ GET_FEATURE Report 5 (kernel polls)                 │
│           ▼                                                      │
│  ┌─────────────────┐        ┌─────────────────────┐             │
│  │  USB/HID layer  │───────▶│  Dongle             │◀──── 2.4GHz │
│  │                 │◀───────│  (returns cache)    │      RF     │
│  └────────┬────────┘        └─────────────────────┘             │
│           │                          ▲                          │
│           ▼                          │ F7 triggers              │
│  ┌─────────────────┐                 │ RF query                 │
│  │  power_supply   │                 ▼                          │
│  │                 │        ┌─────────────────────┐             │
│  └─────────────────┘        │  Keyboard           │             │
│           │                 │  (battery ADC)      │             │
│           ▼                 └─────────────────────┘             │
│  /sys/class/power_supply/hid-*-battery                          │
└─────────────────────────────────────────────────────────────────┘
```

## Usage

```bash
# Build from parent bpf/ directory
cd /path/to/bpf && make option_a

# Load BPF (auto-detects keyboard interface 00D8)
sudo ./hid_battery_support/option_a_keyboard_inject/loader_kb

# Or specify hid_id directly (00D8 = 216 decimal)
sudo ./hid_battery_support/option_a_keyboard_inject/loader_kb 216

# Rebind to apply descriptor changes
echo "0003:3151:5038.00D8" | sudo tee /sys/bus/hid/drivers/hid-generic/unbind
echo "0003:3151:5038.00D8" | sudo tee /sys/bus/hid/drivers/hid-generic/bind

# Check battery
cat /sys/class/power_supply/hid-0003:3151:5038.00D8-battery/capacity
```

## Files

- `akko_keyboard_battery.bpf.c` - BPF program (can be simplified to remove unused map)
- `loader_kb.c` - C loader using libbpf skeleton

## Comparison with Working Solution

| Aspect | Working (vendor + dummy key) | Option A (keyboard inject) |
|--------|------------------------------|---------------------------|
| Power supply location | 00DA (vendor) | 00D8 (keyboard) |
| Model name | MonsGeek 2.4G Wireless Keyboard | MonsGeek 2.4G Wireless Keyboard System Control |
| UPower type | gaming-input | gaming-input |
| Creates phantom input | Yes (on vendor iface) | No (uses existing keyboard) |
| BPF complexity | Simple | Simpler (no map needed) |

## TODO

- [ ] Simplify BPF code to remove unused map
- [ ] Create systemd service for auto-loading
- [ ] Handle device hot-plug via udev rules
