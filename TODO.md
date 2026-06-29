# HID-BPF Battery Integration

**Goal:** Keyboard battery appears in KDE Plasma power applet via kernel power_supply interface.

## Milestones

### Milestone 1: Development Environment Setup
- [x] Install BPF development libs (libbpf-dev)
- [x] Clone udev-hid-bpf for HID-BPF headers
- [x] Generate vmlinux.h via bpftool
- [x] Create bpf/ directory structure
- [x] Create Makefile for BPF compilation
- [x] Write akko_dongle.bpf.c (initial version with logging + descriptor fixup)
- [x] Test: Compile BPF program

### Milestone 2: HID-BPF Report Logger
- [x] Write akko_dongle.bpf.c with device config
- [x] Implement hid_device_event hook with bpf_printk
- [ ] Test: Load BPF and verify trace_pipe output - **BLOCKED: needs sudo**

### Milestone 3: Report Descriptor Fixup
- [x] Implement hid_rdesc_fixup hook
- [x] Create battery descriptor bytes
- [x] Patch vendor page (0xFFFF) to Battery System (0x85)
- [ ] Test: Verify modified descriptor in sysfs

### Milestone 4: Feature Report Handling
- [ ] Verify kernel creates power_supply entry
- [ ] Verify capacity/status values are correct
- [ ] Add hid_hw_request hook if data reordering needed
- [ ] Test: Compare with `iot_driver battery` output

### Milestone 5: Rust/Aya Loader
- [ ] Add aya dependency to Cargo.toml
- [ ] Create src/bpf_loader.rs module
- [ ] Implement AkkoBpfLoader struct
- [ ] Add CLI subcommands (bpf load/unload/status)
- [ ] Test: Load BPF via Rust CLI

### Milestone 6: Privilege & Systemd Integration
- [ ] Create sudoers entry for loader
- [ ] Create udev rule for auto-load
- [ ] Create systemd service (optional)
- [ ] Test: Unplug/replug dongle, verify auto-load

### Milestone 7: Final Verification
- [ ] Verify sysfs power_supply entry
- [ ] Verify UPower detection
- [ ] Verify KDE Plasma power applet shows battery
- [ ] Test persistence across reboot

## Current Status

**Started:** 2025-01-05
**Current Milestone:** Blocked - Dongle Firmware Limitation

## Investigation Results (2025-01-06)

### Dongle GET Query Problem

**Symptom:** All Feature report GET queries return zeros regardless of command or Report ID.

**What Works:**
- SET commands work (LED changes via SET_LEDPARAM confirmed)
- Keyboard functions normally (typing, polling rate, etc.)
- Interface enumeration correct (3 interfaces: keyboard, multi+vendor, feature)

**What Doesn't Work:**
- GET_USB_VERSION (0x8F) → zeros
- GET_LEDPARAM (0x82) → zeros
- GET_PROFILE (0x80) → zeros
- Report ID 5 (battery) → zeros
- Report ID 0 (generic) → zeros

### Root Cause Analysis

1. **Battery is on keyboard, not dongle** - Firmware at 0x12aa8 reads battery via ADC on keyboard MCU
2. **Dongle is a relay** - 2.4GHz dongle forwards data bidirectionally but doesn't cache/respond to queries
3. **Status uses 0x88 prefix** - Battery would come as INPUT report with 0x88 status prefix
4. **No spontaneous INPUT reports** - Monitoring showed no INPUT data from dongle (keyboard asleep)

### Firmware Analysis (2025-01-07)

**Battery Storage:**
- `battery_level_read()` at 0x12aa8 reads ADC channel 6, stores at wireless_state+0x54
- Takes 2 samples, averages, linear interpolation between voltage thresholds
- Result is 1-100 percentage

**Protocol Findings:**
```
Wireless Protocol (handle_bt_wireless_cmd at 0x12646):
- 0x11 = Status request → triggers 0x0F magnetism response (NOT battery!)
- 0x0F = Magnetism status (started=1, stopped=0)
- 0x77 = Version info request
- 0x88 = Status response prefix (includes battery in payload)
- 0x3F = Connect request
- 0xB0-B6 = Pairing/connection sequence
```

**Key Discovery:**
- Command 0x11 triggers INPUT reports, but sends magnetism status (0x0F), not battery (0x88)
- Received `05 0f 01` and `05 0f 00` responses - proves INPUT path works!
- Battery (0x88 prefix) may only be sent during connection handshake or version query
- Firmware is poll-based, NOT push-based for battery

### Options Forward

1. **Try 0x77 (version) command** - May trigger 0x88 status with battery
2. **Monitor during keyboard wake** - Battery might be sent on connection/wake events
3. **Capture Windows IOT traffic** - See what command triggers battery response
4. **Userspace polling daemon** - Monitor INPUT for 0x88 packets when keyboard active
5. **Accept limitation** - Battery not reliably available via 2.4GHz dongle

### Verified Interfaces

| hidraw | HID ID | Type | Vendor Features |
|--------|--------|------|-----------------|
| hidraw1 | 00BE | keyboard | - |
| hidraw2 | 00BF | multi+vendor | INPUT report (Report ID 5) |
| hidraw3 | 00C0 | feature | FEATURE report (no Report ID) |

## Notes

- BPF program must be C (HID-BPF struct_ops not supported in Rust/Aya yet)
- Rust/Aya handles loading the compiled BPF object
- Feature reports (not input reports) - kernel polls automatically
- Dongle VID:PID = 3151:5038

## BREAKTHROUGH: Dongle Query Fix (2025-01-12)

**Root Cause:** Dongle has a delayed response buffer where GET_FEATURE returns the PREVIOUS response.

**Working Pattern (FC flush):**
1. Send command via hidapi `send_feature_report`
2. Wait 150ms
3. Send 0xFC (wireless status) as "flush" command
4. Wait 100ms
5. Read response - this returns the actual response to the original command

**Why FC works:** 0xFC appears to be a neutral "wireless status" query that doesn't overwrite
the command response buffer like F7 (battery refresh) does.

**Battery Response Format:**
| Byte | Meaning | Values |
|------|---------|--------|
| 0 | Connection state | 0x00/0x01 |
| 1 | Battery % | 0x54=84%, 0x5F=95% |
| 5 | Charging state | 0x00=not, 0x01=charging |

**FIXED (2025-01-12):**
- Updated `cli_test()` in main.rs with dongle-specific FC flush path
- Updated `query_dongle()` in hid.rs for MonsGeekDevice
- All CLI commands tested and working via 2.4GHz dongle:
  - Device ID: 2949 (0x0B85), FW: 1029
  - Profile: 2, LED: Mode 1, Polling: 8000Hz

## Future Improvements

### Make BPF bpftool Compatible
**Priority:** Low (optional improvement)
**Status:** Not started

Currently our Rust BPF object (akko-ebpf) only works with our aya-based akko-loader.
It fails with bpftool due to:
1. Multiple .ksyms sections - aya-ebpf emits separate .ksyms section per kfunc
2. BTF type mismatch - function pointers are PTR->INT'()' instead of PTR->FUNC_PROTO

**Tasks:**
- [ ] Investigate merging .ksyms sections in aya-ebpf or bpf-linker
- [ ] Research how to generate FUNC_PROTO BTF types from Rust
- [ ] Consider if this is worth the effort vs aya-only loading

## Cleanup: deprecated flash-wearing per-key SET_USERPIC streaming (2026-06-29)

Audio-reactive used to stream per-key colors at ~50Hz via
`set_per_key_colors_fast` → `SET_USERPIC` (0x0C, live framing `[layer, page,
rgb...]`). Continuous streaming over this path wears the flash. Audio now uses
the native on-device music visualizer (`SET_AUDIO_VIZ` 0x0D) instead — no flash
wear, no patched firmware. Per-key custom streaming should go through the
patched 0xE8 path (`led_stream::send_full_frame`, gated on `has_led_stream()`).

**Remaining users of the suspect framing** (reachable via
`set_all_keys_color` → `set_per_key_colors_to_layer` → `set_per_key_colors_fast`,
all in `monsgeek-keyboard/src/lib.rs`):
- [ ] `src/commands/set.rs:264` — `set` CLI "all keys to solid color" (one-shot)
- [ ] `src/tui/tabs/device_info.rs:535` — TUI device-info color action (one-shot)

These are **one-shot** writes, not continuous streams, so wear is bounded. Open
question before removing/migrating them:
- [ ] Determine (via firmware RE / Ghidra) whether the `[layer, page, rgb]`
      SET_USERPIC framing commits to flash or only to a live SRAM buffer. If it
      writes flash, migrate these one-shot callers to `upload_userpic` (the
      intentional flash-slot path) or the 0xE8 stream; if SRAM-only, they are
      fine as-is and only the *continuous* use was the problem (already fixed).

**Confirmed NOT affected:** `screen_capture` uses `SET_SCREEN_COLOR` (0x0E,
single global color) — a different command, not the per-key flash path.
`upload_userpic`/`download_userpic` are the intentional userpic flash-slot API
(`userpic` CLI command) — keep.
