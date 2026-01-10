# Option B: Vendor Interface with Periodic F7 Refresh

**Status: WORKS**

This approach attaches to the vendor interface (00DA) and exposes battery via power_supply. The loader sends periodic F7 commands to refresh battery data from the keyboard.

## Key Discovery: F7 Command Required

The dongle caches battery data and returns stale/zeros until an **F7 SET_FEATURE** command is sent. The F7 command triggers the dongle to query the keyboard over the 2.4GHz RF link.

```
# After device replug (no F7 sent yet):
GET_FEATURE Report 5: 0000000000000000  -> zeros

# After F7 command:
GET_FEATURE Report 5: 0053000001010100  -> Battery=83%
```

## How It Works

1. BPF attaches to vendor interface (00DA) via probe function
2. `rdesc_fixup` replaces vendor descriptor with Keyboard/Battery descriptor
3. Loader sends initial F7 to prime dongle cache before rebind
4. Device is rebound to apply new descriptor
5. Kernel creates power_supply due to Battery Strength Input report
6. `hw_request` hook fixes Report ID quirk (0x00 -> 0x05)
7. Loader periodically sends F7 every 30 seconds to keep battery fresh

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                          Userspace                               │
│  ┌─────────────────┐                                            │
│  │  loader_bidir   │  Sends F7 command every 30s                │
│  │  (daemon)       │  to refresh battery in dongle cache        │
│  └────────┬────────┘                                            │
│           │ SET_FEATURE [0x00, 0xF7, ...]                       │
├───────────┼─────────────────────────────────────────────────────┤
│           ▼                     Kernel                           │
│  ┌─────────────────┐                                            │
│  │  HID-BPF on     │  rdesc_fixup: replace with battery desc    │
│  │  vendor iface   │  hw_request: fix Report ID (0x00 -> 0x05)  │
│  │  (00DA/00E6)    │                                            │
│  └────────┬────────┘                                            │
│           │                                                      │
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

## Comparison with Option A

| Aspect | Option A (keyboard inject) | Option B (vendor interface) |
|--------|---------------------------|----------------------------|
| Interface | Keyboard (00D8) | Vendor (00DA) |
| F7 source | Userspace daemon | Userspace daemon |
| Descriptor | Append to keyboard | Replace vendor |
| Power supply on | Keyboard device | Vendor device |

Both options require the loader to run continuously for F7 refresh.

## Usage

```bash
# Build from parent bpf/ directory
cd /path/to/bpf && make option_b

# Load BPF (auto-detects vendor interface)
sudo ./hid_battery_support/option_b_bidirectional/loader_bidir

# Check battery
cat /sys/class/power_supply/hid-0003:3151:5038.*-battery/capacity

# Check with upower
upower -i "$(upower -e | grep 3151)"
```

## Files

- `akko_bidirectional.bpf.c` - BPF program for vendor interface
- `loader_bidir.c` - C loader with periodic F7 refresh

## Notes on bpf_wq

Initial implementation attempted to use BPF work queues (`bpf_wq`) to send F7 commands from within BPF, eliminating the need for userspace polling. However, this caused a kernel verifier crash on kernel 6.17:

```
RIP: 0010:check_kfunc_args+0x777/0x1610
CR2: 0000000000000014
```

This appears to be a kernel bug in the BPF verifier when handling `bpf_wq` kfuncs. The current implementation uses loader-based F7 refresh instead, which is proven to work reliably.
