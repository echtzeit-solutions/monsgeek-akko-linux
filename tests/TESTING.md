# MonsGeek M1 V5 HE Keyboard Driver Testing Protocol

This document describes the testing infrastructure for the MonsGeek M1 V5 HE keyboard Linux driver.

## Quick Start

```bash
# Build the driver
make driver

# Run build tests (no hardware needed)
./tests/run_tests.sh build

# Run all tests (hardware connected)
./tests/run_tests.sh all

# Run with verbose output
VERBOSE=1 ./tests/run_tests.sh cli

# Skip hardware tests
SKIP_HARDWARE=1 ./tests/run_tests.sh all
```

## Test Categories

### A. Build Tests (A1-A7)
Tests compilation, formatting, and unit tests. No hardware required.

```bash
./tests/run_tests.sh build
# Or via Makefile:
make test-build
```

### B. Installation Tests (B1-B7)
Tests install/uninstall targets. Requires root.

```bash
sudo ./tests/run_tests.sh install
```

### C. CLI Functional Tests (C1-C5)
Tests all CLI commands. Requires keyboard connected.

```bash
./tests/run_tests.sh cli
# Or roundtrip tests:
./tests/scripts/test_cli_roundtrip.sh
```

### D. TUI Tests (D1-D9)
Tests terminal UI. Requires keyboard + tmux.

```bash
./tests/scripts/test_tui_basic.sh
```

### E. BPF Battery Tests (E1-E8)
Tests HID-BPF battery driver. Requires root + dongle + kernel 6.12+.

```bash
sudo ./tests/scripts/test_bpf_battery.sh
```

### F. Transport Tests (F1-F5)
Tests USB/dongle/Bluetooth transport detection.

```bash
./tests/run_tests.sh transport
```

## VM Testing

For isolated testing in a clean environment:

```bash
# Create Ubuntu 25.10 VM
ISO_PATH=/path/to/ubuntu-25.10.iso ./tests/vm/setup_vm.sh create

# Configure USB passthrough
./tests/vm/setup_vm.sh usb-passthrough

# Generate guest setup script
./tests/vm/setup_vm.sh guest-setup
```

See `tests/vm/setup_vm.sh --help` for more options.

## Test Report

After running tests, fill out the template at:
`tests/TEST_REPORT_TEMPLATE.md`

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `IOT_DRIVER` | Path to driver binary | `iot_driver_linux/target/release/iot_driver` |
| `AKKO_LOADER` | Path to BPF loader | `akko-hid-bpf/target/release/akko-loader` |
| `SKIP_HARDWARE` | Skip tests requiring keyboard | `0` |
| `VERBOSE` | Show command output | `0` |

## Hardware Requirements

| Test Category | Wired | Dongle | Bluetooth |
|---------------|-------|--------|-----------|
| Build (A) | - | - | - |
| Install (B) | - | - | - |
| CLI (C) | Any | Any | Any |
| TUI (D) | Any | Any | Any |
| BPF Battery (E) | - | Required | - |
| Transport (F) | Optional | Optional | Optional |

## Known Limitations

1. **Bluetooth**: GET/SET commands may not work over BT. Events and battery work.

2. **USB Passthrough Latency**: High polling rates (8000Hz) may be affected in VMs.

3. **Charging Status**: Always shows "Discharging" due to firmware limitation.

## CI Integration

The GitHub Actions workflow (`ci.yml`) runs:
- Format check
- Clippy lints
- Unit tests
- Release build
- BPF build + kernel verifier check

Hardware tests cannot run in CI and must be performed manually.

## File Structure

```
tests/
├── run_tests.sh              # Main test runner
├── TESTING.md                # This file
├── TEST_REPORT_TEMPLATE.md   # Test report template
├── scripts/
│   ├── test_cli_roundtrip.sh # CLI setting roundtrip tests
│   ├── test_tui_basic.sh     # TUI automated tests
│   └── test_bpf_battery.sh   # BPF battery driver tests
└── vm/
    └── setup_vm.sh           # VM setup for testing
```
