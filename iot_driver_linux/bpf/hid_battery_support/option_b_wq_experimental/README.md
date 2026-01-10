# Option B WQ: Experimental bpf_wq Auto-Refresh

**Status: EXPERIMENTAL - WORKS ON KERNEL 6.17**

This experimental version uses BPF work queues (`bpf_wq`) to automatically send F7 battery refresh commands from within BPF, eliminating the need for userspace polling after initial load.

## Key Discovery

The initial attempt crashed the kernel verifier because `bpf_wq` was used incorrectly. The fix:

**WRONG** (causes kernel verifier crash):
```c
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __type(value, struct bpf_wq);  // bpf_wq directly as value - CRASH
} wq_map SEC(".maps");
```

**CORRECT** (works):
```c
struct wq_state {
    struct bpf_wq work;  // bpf_wq embedded in struct
    __u64 last_f7_time_ns;
    // ... other fields
};

struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __type(value, struct wq_state);  // struct containing bpf_wq
} wq_state_map SEC(".maps");
```

## How It Works

1. BPF attaches to vendor interface (06 FF FF)
2. `rdesc_fixup` replaces descriptor and initializes bpf_wq
3. Loader sends initial F7, rebinds device
4. Kernel polls battery -> `hw_request` hook runs
5. Hook checks if 30s elapsed since last F7
6. If expired, schedules F7 via `bpf_wq_start()`
7. Work queue callback sends F7 using `hid_bpf_hw_request()`

## Architecture

```
Kernel polls GET_FEATURE
         │
         ▼
┌─────────────────────────────────────────┐
│  HID_BPF_HW_REQUEST hook                │
│  1. Fix Report ID (0x00 → 0x05)         │
│  2. Check rate limit (30s elapsed?)     │
│  3. If expired: bpf_wq_start()          │
└─────────────────────────────────────────┘
         │                     │
         │                     ▼
         │            ┌──────────────────────┐
         │            │ bpf_wq callback      │
         │            │ (async, sleepable)   │
         │            │                      │
         │            │ hid_bpf_allocate_ctx │
         │            │ hid_bpf_hw_request   │
         │            │   (sends F7)         │
         │            │ hid_bpf_release_ctx  │
         │            └──────────────────────┘
         ▼
   Return to kernel
```

## Advantages over Loader-based Refresh

| Aspect | Loader-based | bpf_wq |
|--------|--------------|--------|
| F7 source | Userspace daemon | BPF work queue |
| After load | Needs daemon running | Self-contained |
| Failure mode | Daemon crash = stale data | Kernel handles |

## Requirements

- Kernel 6.10+ (bpf_wq support)
- Tested on kernel 6.17

## Usage

```bash
# Build
cd /path/to/bpf && make option_b_wq

# Load (auto-detects vendor interface)
sudo ./hid_battery_support/option_b_wq_experimental/loader_wq

# Check battery
cat /sys/class/power_supply/hid-*-battery/capacity

# Monitor bpf_wq activity
sudo cat /sys/kernel/debug/tracing/trace_pipe | grep akko_wq
```

## Files

- `akko_wq.bpf.c` - BPF program with bpf_wq for auto F7
- `loader_wq.c` - Minimal loader (no polling loop needed)

## References

- [bpf_wq tutorial](https://eunomia.dev/tutorials/features/bpf_wq/)
- [bpf_wq_init kfunc](https://docs.ebpf.io/linux/kfuncs/bpf_wq_init/)
- [Kernel patch series](https://mail-archive.com/linux-kselftest@vger.kernel.org/msg10547.html)
