# MonsGeek M1 V5 HE Driver Test Report

## Environment

| Field | Value |
|-------|-------|
| Date | YYYY-MM-DD |
| Tester | Name |
| OS | Ubuntu 25.10 |
| Kernel | X.Y.Z |
| VM Type | QEMU/KVM / VirtualBox / Native |
| Hardware | Wired / Dongle / BT |

## Test Summary

| Category | Passed | Failed | Skipped | Total |
|----------|--------|--------|---------|-------|
| A. Build | /7 | | | 7 |
| B. Install | /7 | | | 7 |
| C. CLI | /N | | | N |
| D. TUI | /9 | | | 9 |
| E. BPF | /8 | | | 8 |
| F. Transport | /5 | | | 5 |
| **Total** | | | | |

## Detailed Results

### A. Build Tests

| ID | Test | Result | Notes |
|----|------|--------|-------|
| A1 | Clean build | [ ] Pass [ ] Fail | |
| A2 | Debug build | [ ] Pass [ ] Fail | |
| A3 | BPF loader build | [ ] Pass [ ] Fail | |
| A4 | eBPF build | [ ] Pass [ ] Fail [ ] Skip | |
| A5 | Format check | [ ] Pass [ ] Fail | |
| A6 | Clippy lint | [ ] Pass [ ] Fail | |
| A7 | Unit tests | [ ] Pass [ ] Fail | |

### B. Installation Tests

| ID | Test | Result | Notes |
|----|------|--------|-------|
| B1 | Install driver | [ ] Pass [ ] Fail | |
| B2 | Install udev | [ ] Pass [ ] Fail | |
| B3 | Udev reload | [ ] Pass [ ] Fail | |
| B4 | Install BPF | [ ] Pass [ ] Fail | |
| B5 | Install systemd | [ ] Pass [ ] Fail | |
| B6 | Full install | [ ] Pass [ ] Fail | |
| B7 | Uninstall | [ ] Pass [ ] Fail | |

### C. CLI Functional Tests

#### C.1 Device Discovery

| ID | Test | Result | Notes |
|----|------|--------|-------|
| C1.1 | List devices | [ ] Pass [ ] Fail | |
| C1.2 | Device info | [ ] Pass [ ] Fail | |
| C1.3 | All info | [ ] Pass [ ] Fail | |

#### C.2 Settings Get

| ID | Test | Result | Notes |
|----|------|--------|-------|
| C2.1 | Get profile | [ ] Pass [ ] Fail | |
| C2.2 | Get LED | [ ] Pass [ ] Fail | |
| C2.3 | Get debounce | [ ] Pass [ ] Fail | |
| C2.4 | Get polling rate | [ ] Pass [ ] Fail | |
| C2.5 | Get options | [ ] Pass [ ] Fail | |
| C2.6 | Get sleep | [ ] Pass [ ] Fail | |
| C2.7 | Get triggers | [ ] Pass [ ] Fail | |
| C2.8 | Get features | [ ] Pass [ ] Fail | |

#### C.3 Settings Set

| ID | Test | Result | Notes |
|----|------|--------|-------|
| C3.1 | Set profile | [ ] Pass [ ] Fail | |
| C3.2 | Set debounce | [ ] Pass [ ] Fail | |
| C3.3 | Set polling rate | [ ] Pass [ ] Fail | |
| C3.4 | Set LED mode | [ ] Pass [ ] Fail | |
| C3.5 | Set sleep | [ ] Pass [ ] Fail | |
| C3.6 | Set actuation | [ ] Pass [ ] Fail | |
| C3.7 | Set RT on | [ ] Pass [ ] Fail | |
| C3.8 | Set RT off | [ ] Pass [ ] Fail | |

#### C.4 LED Animations

| ID | Test | Result | Notes |
|----|------|--------|-------|
| C4.1 | Rainbow | [ ] Pass [ ] Fail | |
| C4.2 | GIF upload | [ ] Pass [ ] Fail | |
| C4.3 | Audio reactive | [ ] Pass [ ] Fail | |

#### C.5 Battery (Dongle Only)

| ID | Test | Result | Notes |
|----|------|--------|-------|
| C5.1 | Get battery | [ ] Pass [ ] Fail [ ] N/A | |
| C5.2 | Battery watch | [ ] Pass [ ] Fail [ ] N/A | |
| C5.3 | Battery quiet | [ ] Pass [ ] Fail [ ] N/A | |

### D. TUI Functional Tests

| ID | Test | Result | Notes |
|----|------|--------|-------|
| D1 | Launch TUI | [ ] Pass [ ] Fail | |
| D2 | Tab navigation | [ ] Pass [ ] Fail | |
| D3 | Device info tab | [ ] Pass [ ] Fail | |
| D4 | LED settings tab | [ ] Pass [ ] Fail | |
| D5 | Key depth tab | [ ] Pass [ ] Fail | |
| D6 | Triggers tab | [ ] Pass [ ] Fail | |
| D7 | Options tab | [ ] Pass [ ] Fail | |
| D8 | Macros tab | [ ] Pass [ ] Fail | |
| D9 | Exit TUI | [ ] Pass [ ] Fail | |

### E. BPF Battery Driver Tests

| ID | Test | Result | Notes |
|----|------|--------|-------|
| E1 | Load BPF | [ ] Pass [ ] Fail | |
| E2 | Verify pin | [ ] Pass [ ] Fail | |
| E3 | Power supply | [ ] Pass [ ] Fail | |
| E4 | Read capacity | [ ] Pass [ ] Fail | |
| E5 | Desktop integration | [ ] Pass [ ] Fail | |
| E6 | Unload BPF | [ ] Pass [ ] Fail | |
| E7 | Systemd auto-load | [ ] Pass [ ] Fail | |
| E8 | Systemd auto-unload | [ ] Pass [ ] Fail | |

### F. Transport Layer Tests

| ID | Test | Result | Notes |
|----|------|--------|-------|
| F1 | Wired discovery | [ ] Pass [ ] Fail [ ] N/A | |
| F2 | Dongle discovery | [ ] Pass [ ] Fail [ ] N/A | |
| F3 | BT discovery | [ ] Pass [ ] Fail [ ] N/A | |
| F4 | Auto-select | [ ] Pass [ ] Fail | |
| F5 | BT battery | [ ] Pass [ ] Fail [ ] N/A | |

## Issues Found

### Issue 1: [Title]
- **Test ID**:
- **Severity**: Critical / High / Medium / Low
- **Description**:
- **Steps to Reproduce**:
  1.
- **Expected**:
- **Actual**:
- **Workaround**:

## Known Limitations

| Issue | Impact | Status |
|-------|--------|--------|
| BT commands limited | GET/SET may not work | Events + battery work |
| Charging status unavailable | Always shows "Discharging" | FW limitation |

## Notes

-
