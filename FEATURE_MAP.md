# Akko Cloud Driver - Linux Port Feature Map

This document tracks the implementation progress of features for the Linux port of the Akko Cloud keyboard/mouse driver.

**Status Legend:**
- ⬜ Not Started
- 🟡 In Progress
- ✅ Complete
- ❌ Won't Implement
- 🔍 Needs Investigation

---

## 1. Core HID Protocol

### 1.1 Device Connection & Management

| Feature | Status | Reference | Notes |
|---------|--------|-----------|-------|
| WebHID device enumeration | ⬜ | `HIDDeviceWrapper.autoConnect()` | Linux: use hidraw |
| Device connect/disconnect | ⬜ | `HIDDeviceWrapper.connect()`, `disconnect()` | |
| Multiple device support | ⬜ | `HIDDeviceWrapper.devices: Map` | |
| Reconnect saved devices | ⬜ | `HIDDeviceWrapper.reconnectSavedDevices()` | |
| Device identification (whoAmI) | ⬜ | `HIDInterface.whoAmI()` | CMD: `0x8F` |

**Related Constants:**
```javascript
FEATURE_REPORT_SIZE = 64
STORAGE_KEY = "HID_DEVICE_INFO"
```

### 1.2 HID Communication

| Feature | Status | Reference | Notes |
|---------|--------|-----------|-------|
| Send feature report | ⬜ | `HIDInterface.sendFeature()` | |
| Read feature report | ⬜ | `HIDInterface.readFeature()` | |
| Checksum Bit7 | ⬜ | `CheckSum.Bit7` | sum bytes 0-6, invert, store in byte 7 |
| Checksum Bit8 | ⬜ | `CheckSum.Bit8` | sum bytes 0-7, invert, store in byte 8 |
| No checksum | ⬜ | `CheckSum.None` | |
| Bluetooth timing | ⬜ | `sendMsg()` | Extra 60ms delay for BT |
| Vendor sleep/block | ⬜ | `vendorSleep()`, `block` | Prevents concurrent access |

**Related Structs:**
```rust
enum CheckSum {
    Bit7 = 0,
    Bit8 = 1,
    None = 2,
}
```

---

## 2. Keyboard Features

### 2.1 Basic Configuration

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Reset device | ⬜ | - | `0x01` | `FEA_CMD_SET_RESERT` |
| Get USB version | ⬜ | `0x8F` | - | Returns firmware version |
| Get RF version | ⬜ | `0x80` | - | Wireless firmware |
| Get MLED version | ⬜ | `0xAE` | - | LED controller version |
| Get OLED version | ⬜ | `0xAD` | - | Display firmware |
| Current profile | ⬜ | `0x84` | `0x04` | 0-indexed profile number |
| Debounce time | ⬜ | `0x86` | `0x06` | Milliseconds |
| Sleep time | ⬜ | `0x91` | `0x11` | BT/2.4G sleep timers |

**Sleep Time Struct:**
```rust
struct SleepTime {
    time_bt: u16,      // BT idle timeout (seconds)
    time_24: u16,      // 2.4G idle timeout
    deep_time_bt: u16, // BT deep sleep timeout
    deep_time_24: u16, // 2.4G deep sleep timeout
}
```

### 2.2 Key Matrix / Remapping

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get key matrix | ⬜ | `0x8A` | - | Full keyboard layout |
| Set key matrix | ⬜ | - | `0x0A` | Bulk key config |
| Set single key | ⬜ | - | `0x0A` | `setKeyConfigSimple()` |
| Fn layer matrix | ⬜ | `0x90` | `0x10` | Fn key bindings |
| Key config parsing | ⬜ | `matrixToConfigValues()` | |

**Key Matrix Format:**
```rust
// Each key is 4 bytes: [byte0, byte1, keycode, modifier]
// Matrix size = num_keys * 4
struct KeyConfig {
    original: u32,     // Original HID keycode
    index: u8,         // Position in matrix (0-127)
    config_type: ConfigType,
}

enum ConfigType {
    Keyboard,          // [0, modifier, keycode, 0]
    Combo,             // [0, modifier, key1, key2]
    Mouse,             // [1, 0, mouse_code, 0]
    Macro,             // [9, macro_type, macro_index, 0]
    Function,          // [3, 0, func_code_lo, func_code_hi]
    Forbidden,         // [0, 0, 0, 0]
    Gamepad,           // [21, type, code, 0]
    ControlRecoil,     // [22, method, gun_index, 0]
    Snap,              // [22, number, keycode, 0]
}
```

### 2.3 Macros

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get macro | ⬜ | `0x8B` | - | Up to 4 pages per macro |
| Set macro | ⬜ | - | `0x0B` | |
| Parse macro buffer | ⬜ | `buffToMacroEvents()` | |

**Macro Format:**
```rust
struct Macro {
    repeat_count: u16,  // Bytes 0-1, little endian
    events: Vec<MacroEvent>,
}

enum MacroEvent {
    Keyboard { action: KeyAction, keycode: u8 },
    MouseButton { action: KeyAction, button: MouseKey },
    MouseMove { dx: i8, dy: i8 },
    Delay { ms: u16 },
}

enum KeyAction { Down, Up }

// Macro byte encoding:
// Keyboard: [keycode, delay_or_action]
//   - delay_or_action & 0x80 = down, else up
//   - delay_or_action & 0x7F = delay if < 128, else next 2 bytes
// Mouse move: [0xF9, delay_short, dx, dy] or [0xF9, 0, dx, dy, delay_lo, delay_hi]
```

### 2.4 Lighting - Main

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get light settings | ⬜ | `0x87` | - | |
| Set light settings | ⬜ | - | `0x07` | Uses Bit8 checksum |
| Custom light picture | ⬜ | `0x8C` | `0x0C` | Per-key RGB |

**Light Effects:**
```rust
enum LightEffect {
    LightOff = 0,
    LightAlwaysOn = 1,
    LightBreath = 2,
    LightNeon = 3,
    LightWave = 4,
    LightRipple = 5,
    LightRaindrop = 6,
    LightSnake = 7,
    LightPressAction = 8,
    LightConverage = 9,
    LightSineWave = 10,
    LightKaleidoscope = 11,
    LightLineWave = 12,
    LightUserPicture = 13,
    LightLaser = 14,
    LightCircleWave = 15,
    LightDazzing = 16,
    LightRainDown = 17,
    LightMeteor = 18,
    LightPressActionOff = 19,
    LightMusicFollow3 = 20,
    LightScreenColor = 21,
    LightMusicFollow2 = 22,
    LightTrain = 23,
    LightFireWorks = 24,
    LightUserColor = 25,
}

struct LightSetting {
    effect: LightEffect,
    speed: u8,        // 0-4, inverted (4 = slowest)
    brightness: u8,   // 0-4
    option: u8,       // Direction/variant (effect-specific)
    rgb: u32,         // RGB color (0xRRGGBB)
    dazzle: bool,     // Rainbow mode
}
```

### 2.5 Lighting - Side LEDs

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get side light settings | ⬜ | `0x88` | - | |
| Set side light settings | ⬜ | - | `0x08` | Uses Bit8 checksum |

**Side Light Effects:** `LightOff`, `LightAlwaysOn`, `LightBreath`, `LightNeon`, `LightWave`, `LightSnake`, `LightMusicFollow2`

---

## 3. Magnetic/Hall Effect Keyboard Features

### 3.1 Analog Key Settings

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get magnetism info | ⬜ | `0xE5` | - | Multi-page read |
| Set magnetism info | ⬜ | - | `0x65` | |
| Calibration | ⬜ | - | `0x1C` | `FEA_CMD_SET_MAGNETISM_CAL` |
| Max calibration | ⬜ | - | `0x1E` | Full travel calibration |

**Magnetism Sub-Commands (via `0xE5`/`0x65`):**
| Sub-cmd | Description |
|---------|-------------|
| 0 | Actuation travel (per-key) |
| 1 | Lift travel (per-key) |
| 2 | Rapid trigger press travel |
| 3 | Rapid trigger lift travel |
| 4 | DKS travel values |
| 5 | MT (Mod-Tap) times |
| 6 | Deadzone values |
| 7 | Key mode flags |
| 9 | Snap tap values |
| 10 | DKS keycodes |
| 251 | Top deadzone |
| 252 | Switch type |

**Key Mode Struct:**
```rust
struct MagnetismKeyMode {
    original: u32,
    index: u8,
    pos: u8,
    option: MagnetismOption,
    fire: bool,  // Rapid trigger enabled
}

enum MagnetismOption {
    Normal = 0,
    Dks = 2,      // Dynamic Keystroke
    Mt = 3,       // Mod-Tap
    TglHold = 4,  // Toggle on hold
    TglDots = 5,  // Toggle on double-tap
    Snap = 7,     // Snap tap
}
```

**Travel Settings:**
```rust
struct TravelSettings {
    travel: Range,      // Actuation point (0.1-3.4mm, step 0.01)
    fire_press: Range,  // RT press sensitivity
    fire_lift: Range,   // RT release sensitivity
    deadzone: Range,    // Dead zone (0-1mm)
}

struct Range {
    min: f32,
    max: f32,
    step: f32,
    default: f32,
}
```

---

## 4. Mouse Features

### 4.1 Basic Mouse Configuration

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get USB version | ⬜ | `0x8F` | - | |
| Get RF version | ⬜ | `0x80` | - | |
| Reset device | ⬜ | - | `0x02` | |
| Current profile | ⬜ | `0x85` | `0x05` | |

### 4.2 Mouse Options (DPI, Polling Rate)

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get mouse options | ⬜ | `0xD3` | - | `FEA_CMD_MOUSE_GET_OPTIONPARAM0` |
| Set mouse options | ⬜ | - | `0x53` | |
| Get extended options | ⬜ | `0xD4` | - | `OPTIONPARAM1` |
| Set extended options | ⬜ | - | `0x54` | |
| Set report rate | ⬜ | - | `0x04` | |

**Mouse Option Struct:**
```rust
struct MouseOption {
    dpi_list: [u16; 8],      // Up to 8 DPI presets
    current_dpi_index: u8,
    lift_off_distance: u8,   // LOD setting
    debounce: u8,
    angle_snap: bool,
    ripple_control: bool,
    motion_sync: bool,
    report_rate: ReportRate,
}

enum ReportRate {
    Hz125 = 8,
    Hz250 = 4,
    Hz500 = 2,
    Hz1000 = 1,
    Hz2000 = 132,
    Hz4000 = 130,
    Hz8000 = 129,
}
```

### 4.3 Mouse Button Remapping

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get button matrix | ⬜ | `0xD0` | - | `FEA_CMD_MOUSE_GET_KEYMATRIX` |
| Set button matrix | ⬜ | - | `0x50` | |
| Get Fn matrix | ⬜ | `0xD1` | - | |
| Set Fn matrix | ⬜ | - | `0x51` | |

**Mouse Keys:**
```rust
enum MouseKey {
    Left = -1000,
    Right = -999,
    Middle = -998,
    Forward = -997,
    Back = -996,
    Dpi = -995,
    WheelForward = -994,
    WheelBack = -993,
    WheelLeft = -992,
    WheelRight = -991,
    ScrollUp = -988,
    ScrollDown = -987,
}

// Mouse key matrix encoding:
// [1, 0, 0xF0, 0] = Left
// [1, 0, 0xF1, 0] = Right
// [1, 0, 0xF2, 0] = Middle
// [1, 0, 0xF3, 0] = Back
// [1, 0, 0xF4, 0] = Forward
// [20, 0, 0, 0]   = DPI cycle
```

### 4.4 Mouse Lighting

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get light settings | ⬜ | `0x87` | - | Same as keyboard |
| Set light settings | ⬜ | - | `0x07` | |

### 4.5 Recoil Control (Gaming)

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get gun config | ⬜ | `0xE0` | - | `FEA_CMD_GET_CONTROLRECOIL` |
| Set gun config | ⬜ | - | `0x60` | |

---

## 5. Display Features (OLED/TFT)

### 5.1 OLED Display

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Get OLED version | ⬜ | `0xAD` | - | |
| Set clock | ⬜ | - | `0x28` | Sync system time |
| Set language | ⬜ | - | `0x27` | |
| Set sys info | ⬜ | - | `0x22` | CPU/RAM/disk stats |
| Set weather | ⬜ | - | `0x2A` | |
| Custom image | ⬜ | `0xA0` | `0x20` | OLED picture |
| Custom GIF | ⬜ | `0xA4` | `0x24` | Animated |

### 5.2 TFT/LCD Display

| Feature | Status | Command GET | Command SET | Notes |
|---------|--------|-------------|-------------|-------|
| Send image data | ⬜ | `0xA5` | `0x25` | 16-bit RGB |
| Send 24-bit image | ⬜ | `0xA9` | `0x29` | 24-bit RGB |
| Flash erase | ⬜ | `0xAC` | `0x2C` | Clear display storage |

**TFT Image Struct:**
```rust
struct TFTImageData {
    current_frame: u8,
    frame_num: u8,
    frame_delay: u8,  // Animation delay (ms)
    data: Vec<u8>,    // RGB pixel data
    left: u16,
    top: u16,
    right: u16,
    bottom: u16,
}
```

---

## 6. Special Function Keys

**Function Key Mappings:**
```rust
// Format: [byte0, byte1, byte2, byte3]
const SPECIAL_FUNCTIONS: &[(&str, [u8; 4])] = &[
    // Media
    ("prev_track", [3, 0, 182, 0]),
    ("next_track", [3, 0, 181, 0]),
    ("stop", [3, 0, 183, 0]),
    ("play_pause", [3, 0, 205, 0]),
    ("mute", [3, 0, 226, 0]),
    ("vol_down", [3, 0, 234, 0]),
    ("vol_up", [3, 0, 233, 0]),

    // System
    ("calculator", [3, 0, 146, 1]),
    ("mail", [3, 0, 138, 1]),
    ("my_computer", [3, 0, 148, 1]),
    ("search", [3, 0, 33, 2]),
    ("home", [3, 0, 35, 2]),
    ("brightness_down", [3, 0, 112, 0]),
    ("brightness_up", [3, 0, 111, 0]),

    // Keyboard
    ("fn", [10, 1, 0, 0]),
    ("fn_right", [10, 1, 1, 0]),
    ("fn_lock", [10, 13, 0, 0]),
    ("lock_screen", [0, 0, 227, 15]),

    // Gaming
    ("rapid_fire", [11, 0, 0, 0]),
    ("sniper_key", [22, 5, 0, 0]),
    ("recoil_toggle", [22, 6, 255, 0]),
    ("gun_switch_up", [22, 1, 0, 0]),
    ("gun_switch_down", [22, 2, 0, 0]),

    // Mouse DPI
    ("dpi_loop", [20, 0, 0, 0]),
    ("dpi_up", [20, 0, 1, 0]),
    ("dpi_down", [20, 0, 2, 0]),
    ("dpi_shift", [20, 0, 4, 0]),

    // Profile
    ("profile_loop", [8, 0, 3, 0]),
    ("profile_up", [8, 0, 1, 0]),
    ("profile_down", [8, 0, 2, 0]),
];
```

---

## 7. Firmware Update

| Feature | Status | Reference | Notes |
|---------|--------|-----------|-------|
| Enter bootloader | ⬜ | `getDeviceIsBoot()` | Check boot mode |
| OLED bootloader | ⬜ | `FEA_CMD_SET_OLEDBOOTLOADER` | |
| Flash chip erase | ⬜ | `FEA_CMD_SET_FLASHCHIPERASSE` | |

---

## 8. Device Database

### 8.1 Supported Keyboards (Sample)

| Device ID | Name | VID | PID | Chip | Features |
|-----------|------|-----|-----|------|----------|
| 2116 | TITAN68HE | 0x3151 | 0x5029 | RY5088 | Magnetic, Side LED |
| 2227 | VK MAG75 Pro | 0x373A | 0xA206 | RY5088 | Magnetic |
| 2268 | X65HE | 0x3151 | 0x502D | RY5088 | Magnetic |

### 8.2 Supported Mice (Sample)

| Device ID | Name | VID | PID | Sensor | Max DPI |
|-----------|------|-----|-----|--------|---------|
| 2249 | X1 | 0x3151 | 0x5032 | PAW3395 | 40000 |
| 1893 | R2 | 0x3151 | 0x4026 | PAW3950 | 42000 |
| 1643 | R3 | 0x3151 | 0x4026 | PAW3395 | 26000 |

---

## 9. Implementation Notes

### 9.1 Linux-Specific Considerations

- **Device Access:** Use `/dev/hidraw*` instead of WebHID
- **Permissions:** Requires udev rules for non-root access
- **Bluetooth:** May need BlueZ integration
- **IOT SDK:** Not applicable (Windows-only Electron feature)

### 9.2 File References

| File | Purpose |
|------|---------|
| `classes/common/HIDInterface.js` | Base protocol implementation |
| `classes/common/CommonKBRY5088.js` | Keyboard protocol |
| `classes/common/CommonMsPan1080.js` | Mouse protocol |
| `extracted/*.json` | Key matrices, i18n, config data |

### 9.3 Testing Commands

```bash
# List HID devices
ls -la /dev/hidraw*

# Get device info
udevadm info -a /dev/hidrawX

# Test basic communication (requires implementation)
./iot_driver --device /dev/hidrawX --cmd 0x8F
```

---

## Progress Summary

| Category | Total | Complete | Progress |
|----------|-------|----------|----------|
| Core HID | 7 | 0 | 0% |
| Keyboard Basic | 8 | 0 | 0% |
| Key Remapping | 5 | 0 | 0% |
| Macros | 3 | 0 | 0% |
| Lighting | 4 | 0 | 0% |
| Magnetic Keys | 4 | 0 | 0% |
| Mouse Basic | 4 | 0 | 0% |
| Mouse Options | 5 | 0 | 0% |
| Display | 8 | 0 | 0% |
| **Total** | **48** | **0** | **0%** |

---

*Last updated: 2025-12-30*
*Generated from Akko Cloud v4.370.2.17*
