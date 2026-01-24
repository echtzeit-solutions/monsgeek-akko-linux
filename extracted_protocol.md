# MonsGeek M1 V5 HE Protocol Extraction

## Communication Architecture

```
┌─────────────────────────────┐
│   Web App / Electron App    │  (akko_cloud.js - React UI)
│   HIDDeviceWrapper class    │  Uses WebHID API in browser
└──────────────┬──────────────┘
               │ WebHID (browser) or gRPC (electron)
               ▼
┌─────────────────────────────┐
│      IOTConnector           │  gRPC client to iot_driver
│  http://127.0.0.1:3814      │
└──────────────┬──────────────┘
               │ gRPC-Web
               ▼
┌─────────────────────────────┐
│     iot_driver.exe          │  Native Rust HID helper
│     (hidapi + tonic)        │
└──────────────┬──────────────┘
               │ HID Feature Reports
               ▼
┌─────────────────────────────┐
│   MonsGeek M1 V5 HE         │  VID=0x3151 PID=0x5030
│   Interface 2, Usage 0x02   │  UsagePage 0xFFFF
└─────────────────────────────┘
```

## HID Interface Parameters

```
Vendor ID:      0x3151 (12625)
Product ID:     0x5030 (20528)
Usage Page:     0xFFFF (65535) - Vendor defined
Usage:          0x02
Interface:      2
Report Size:    64 bytes
Report ID:      0
```

## Checksum Types

From JS code (line 1159643):
```javascript
var CheckSum = (g => (
    g[g.Bit7 = 0] = "Bit7",   // Checksum at byte 7
    g[g.Bit8 = 1] = "Bit8",   // Checksum at byte 8
    g[g.None = 2] = "None"    // No checksum
))(CheckSum || {});
```

### Checksum Algorithm (line 1159656-1167):

**Bit7 mode (most common):**
```javascript
// Pad message to 8 bytes minimum
const dt = d.length < 8 ? [...d, ...new Array(8 - d.length).fill(0)] : [...d];
// Calculate checksum over bytes 0-6
const sum = dt.slice(0, 7).reduce((a, b) => a + b, 0);
// Store inverted checksum at byte 7
dt[7] = 255 - (sum & 255);
```

**Bit8 mode:**
```javascript
const sum = dt.slice(0, 8).reduce((a, b) => a + b, 0);
dt[8] = 255 - (sum & 255);
```

## WebHID API Usage (HIDDeviceWrapper)

**setFeature (send command):** (line 1154324)
```javascript
async setFeature(deviceKey, reportId, data) {
    const device = this.devices.get(deviceKey);
    // Pad to 64 bytes
    const buffer = new Array(64).fill(0);
    data.forEach((byte, i) => { if (i < 64) buffer[i] = byte; });
    await device.sendFeatureReport(reportId, new Uint8Array(buffer));
}
```

**getFeature (read response):** (line 1154332)
```javascript
async getFeature(deviceKey, reportId, offset = 0) {
    const device = this.devices.get(deviceKey);
    const dataView = await device.receiveFeatureReport(reportId);
    return new Uint8Array(dataView.buffer.slice(offset));
}
```

## gRPC Protocol (IOTConnector)

**sendMsg** - Send with device type awareness:
```javascript
g.sendMsg = (msg, devicePath, checkSumType, dangleDevType) => {
    const request = SendMsg.create();
    request.msg = msg;
    request.devicePath = devicePath;
    request.checkSumType = checkSumType;
    request.dangleDevType = dangleDevType;
    return g.client.sendMsg(request);
}
```

**sendRawFeature** - Direct HID send:
```javascript
g.sendRawMsg = (msg, devicePath, checkSumType) => {
    const request = SendMsg.create();
    request.msg = msg;
    request.devicePath = devicePath;
    request.checkSumType = checkSumType;
    request.dangleDevType = DangleDevType.DangleDevTypeNone;
    return g.client.sendRawFeature(request);
}
```

## Command/Response Pattern

The `commonMsg` function (line 1159733):
```javascript
async commonMsg(data, checksumType, sendDelay = 10, readDelay = 10) {
    // 1. Send feature report
    await this.sendMsg(data, checksumType, sendDelay);
    // 2. Read response
    return await this.readMsg(readDelay);
}
```

**Response validation:**
- Response byte 0 echoes the command ID
- Response byte 1 = 0xAA (170) indicates success

## Command IDs (FEA_CMD_*)

### SET Commands (Write operations)
| Command | ID (dec) | ID (hex) | Description |
|---------|----------|----------|-------------|
| FEA_CMD_SET_RESERT | 1 | 0x01 | Reset keyboard |
| FEA_CMD_SET_REPORT | 3 | 0x03 | Set report mode |
| FEA_CMD_SET_PROFILE | 4 | 0x04 | Set active profile |
| FEA_CMD_SET_DEBOUNCE | 6 | 0x06 | Set debounce time |
| FEA_CMD_SET_LEDPARAM | 7 | 0x07 | Set LED parameters |
| FEA_CMD_SET_SLEDPARAM | 8 | 0x08 | Set side LED params |
| FEA_CMD_SET_KBOPTION | 9 | 0x09 | Set KB options |
| FEA_CMD_SET_KEYMATRIX | 10 | 0x0A | Set key mapping |
| FEA_CMD_SET_MACRO | 11 | 0x0B | Set macro |
| FEA_CMD_SET_USERPIC | 12 | 0x0C | Set user picture |
| FEA_CMD_SET_FN | 16 | 0x10 | Set Fn layer |
| FEA_CMD_SET_SLEEPTIME | 17 | 0x11 | Set sleep timeout |
| FEA_CMD_SET_USERGIF | 18 | 0x12 | Set user GIF |
| FEA_CMD_SET_CMD_AUTOOSEN | 23 | 0x17 | Set auto-calibration |
| FEA_CMD_SET_MAGNETISM_REPOR | 27 | 0x1B | Magnetism report |
| FEA_CMD_SET_MAGNETISM_CAL | 28 | 0x1C | Magnetism calibration |
| FEA_CMD_SET_MAGNETISM_MAXIMUM_CALIBRATION | 30 | 0x1E | Max calibration |
| FEA_CMD_SET_OLEDOPTION | 34 | 0x22 | Set OLED options |
| FEA_CMD_SETTFTLCDDATA | 37 | 0x25 | Set TFT LCD data |
| FEA_CMD_SET_OLEDLUANGAGE | 39 | 0x27 | Set OLED language |
| FEA_CMD_SET_OLEDCLOCK | 40 | 0x28 | Set OLED clock |
| FEA_CMD_SET_SCREEN_24BITDATA | 41 | 0x29 | Set screen 24-bit |
| FEA_CMD_SET_SKU | 80 | 0x50 | Set SKU |
| FEA_CMD_SET_MULTI_MAGNETISM | 101 | 0x65 | Set multi-magnetism |
| FEA_CMD_SET_FLASHCHIPERASSE | 172 | 0xAC | Flash chip erase |

### GET Commands (Read operations)
| Command | ID (dec) | ID (hex) | Description |
|---------|----------|----------|-------------|
| FEA_CMD_GET_RF_VERSION | 128 | 0x80 | Get RF/firmware version |
| FEA_CMD_GET_REPORT | 131 | 0x83 | Get report mode |
| FEA_CMD_GET_PROFILE | 132 | 0x84 | Get active profile |
| FEA_CMD_GET_LEDONOFF | 133 | 0x85 | Get LED on/off state |
| FEA_CMD_GET_DEBOUNCE | 134 | 0x86 | Get debounce time |
| FEA_CMD_GET_LEDPARAM | 135 | 0x87 | Get LED parameters |
| FEA_CMD_GET_SLEDPARAM | 136 | 0x88 | Get side LED params |
| FEA_CMD_GET_KBOPTION | 137 | 0x89 | Get KB options |
| FEA_CMD_GET_KEYMATRIX | 138 | 0x8A | Get key mapping |
| FEA_CMD_GET_MACRO | 139 | 0x8B | Get macro |
| FEA_CMD_GET_USERPIC | 140 | 0x8C | Get user picture |
| FEA_CMD_GET_USB_VERSION | 143 | 0x8F | Get USB version/device ID |
| FEA_CMD_GET_FN | 144 | 0x90 | Get Fn layer |
| FEA_CMD_GET_SLEEPTIME | 145 | 0x91 | Get sleep timeout |
| FEA_CMD_GET_CMD_AUTOOSEN | 151 | 0x97 | Get auto-calibration |
| FEA_CMD_GETTFTLCDDATA | 165 | 0xA5 | Get TFT LCD data |
| FEA_CMD_GET_SCREEN_24BITDATA | 169 | 0xA9 | Get screen 24-bit |
| FEA_CMD_GETOLED_VERSION | 173 | 0xAD | Get OLED version |
| FEA_CMD_GETMLED_VERSION | 174 | 0xAE | Get main LED version |
| FEA_CMD_GET_SKU | 208 | 0xD0 | Get SKU |
| FEA_CMD_GET_MULTI_MAGNETISM | 229 | 0xE5 | Get multi-magnetism |
| FEA_CMD_GET_FEATURE_LIST | 230 | 0xE6 | Get feature list |

## Example Command Flows

### Get Device ID (line 1159746)
```javascript
async getCommonDeviceId() {
    // Command 0x8F (143) = FEA_CMD_GET_USB_VERSION
    const response = await this.commonMsg(new Uint8Array([143]), CheckSum.Bit7);
    if (response && response[0] === 143) {
        // Device ID is little-endian uint32 at bytes 1-4
        const deviceId = new DataView(response.buffer).getUint32(1, true);
        return deviceId;
    }
}
```

### Get Firmware Version (line 1213780)
```javascript
async getUSBVersion() {
    const msg = new Uint8Array(1);
    msg[0] = this.FEA_CMD_GET_USB_VERSION;  // 143 = 0x8F
    const response = await this.commonMsg(msg, CheckSum.Bit7);
    if (response !== undefined) {
        // Version is 16-bit at bytes 7-8 (little-endian)
        return (response[8] << 8) | response[7];
    }
}
```

### Get RF Version (line 1213787)
```javascript
async getRFVersion() {
    const msg = new Uint8Array(1);
    msg[0] = this.FEA_CMD_GET_RF_VERSION;  // 128 = 0x80
    const response = await this.commonMsg(msg, CheckSum.Bit7);
    if (response !== undefined) {
        // Version is 16-bit at bytes 1-2 (little-endian)
        const version = (response[2] << 8) | response[1];
        if (version !== 0) return version;
    }
}
```

### Set Profile (line 1213823)
```javascript
async setCurrentProfile(profileIndex) {
    const msg = new Uint8Array(64);
    msg[0] = this.FEA_CMD_SET_PROFILE;  // 4 = 0x04
    msg[1] = profileIndex;
    const response = await this.commonMsg(msg, CheckSum.Bit7);
    if (response && response[0] === this.FEA_CMD_SET_PROFILE && response[1] === 0xAA) {
        await this.vendorSleep();
        return true;
    }
    return false;
}
```

## Raw HID Communication

Based on the code, the communication uses HID Feature Reports:

1. **Send:** `device.sendFeatureReport(reportId=0, data[64])`
2. **Receive:** `device.receiveFeatureReport(reportId=0)`

**NOT** input/output reports, but **feature reports** which are bidirectional.

## Key Insight: Report ID

The WebHID wrapper always uses Report ID 0:
```javascript
hidDevice.setFeature(this.deviceKey, 0, data)  // reportId = 0
hidDevice.getFeature(this.deviceKey, 0)        // reportId = 0
```

This maps to:
- `send_feature_report(&[0, ...data])` in hidapi (prepend report ID)
- `get_feature_report(&mut buf)` with `buf[0] = 0`

## Magnetic Switch Specific Commands

For HE (Hall Effect) keyboards:
| Command | ID | Description |
|---------|-----|-------------|
| FEA_CMD_GET_MULTI_MAGNETISM | 229 (0xE5) | Get actuation points per key |
| FEA_CMD_SET_MULTI_MAGNETISM | 101 (0x65) | Set actuation points per key |
| FEA_CMD_SET_MAGNETISM_CAL | 28 (0x1C) | Calibrate magnetic sensors |
| FEA_CMD_SET_MAGNETISM_MAXIMUM_CALIBRATION | 30 (0x1E) | Max calibration |

## Delays and Timing

From the code:
- Default vendor sleep: 100ms after write commands
- Bluetooth needs extra delays: 60ms send, 100ms read
- Some operations need 2000ms (reset), 1000ms (flash operations)

## Bluetooth (BLE) Protocol

**IMPORTANT LIMITATIONS**: The Bluetooth HID protocol is severely limited compared to USB:

### What Works
- **Vendor Events**: Fn key notifications are received
- **Battery**: Via standard BLE Battery Service (UUID 0x180F)
- **Keyboard Input**: Standard HID keyboard works normally

### What Doesn't Work
- **GET Commands**: ATT writes succeed but keyboard doesn't send notification response
- **SET Commands**: ATT writes succeed but keyboard ignores them

### Technical Investigation (Jan 2026)

**GATT Structure:**
| Characteristic | Report ID | Type | Flags | Notes |
|---------------|-----------|------|-------|-------|
| char0032 | 6 | Input | read, notify | Vendor responses (65 bytes) |
| char0039 | 6 | Output | write | Vendor commands (65 bytes) |
| char0036 | 1 | Output | write | Keyboard LED output |

**Protocol Analysis:**

1. **Vendor protocol is transported over GATT (HOGP)** using Report characteristics.
2. **Commands are written to the vendor Output Report** and responses arrive via **notifications**
   on the vendor Input Report.
3. In USBPcap "BT over USB" captures, the leading `0x02` and the `0x82` endpoint are **HCI ACL**
   transport details, **not HID report IDs**.
4. The actual vendor payload is framed with a leading marker byte:
   - **0x55**: command/response channel
   - **0x66**: event channel
5. The checksum is the same Bit7/Bit8 algorithm as USB, but it applies starting at the `cmd`
   byte (i.e. skipping the 0x55 marker).

**Windows USB Capture Analysis:**
```
# Windows BT electron app (works):
OUT (HCI ACL, ep 0x02): ... L2CAP(CID=0x0004 ATT) ATT.WriteCommand(handle=0x003a, value[65])
  value starts with: 55 8f ...

IN  (HCI ACL, ep 0x82): ... ATT.HandleValueNotification(handle=0x0033, value[65])
  value starts with: 55 8f 85 0b ... (device id 0x0b85)

# Summary:
# - Write vendor commands to handle 0x003a (Output report)
# - Read responses from notifications on handle 0x0033 (Input report)
```

**Possible causes:**
- Linux HOGP usage was sending the wrong on-wire framing (missing 0x55 marker / wrong checksum offset).
- Direct GATT access can be blocked while the kernel HOGP driver is bound, depending on system config.

### BLE Event Format

Events use a different format than USB:
```
USB:       [Report ID 0x05] [type] [value] ...
Bluetooth: [Report ID 0x06] [0x66] [type] [value] ...
```

The `0x66` byte is a BLE-specific notification marker.

### BLE Device Identification
```
VID:        0x3151
PID:        0x5027 (M1 V5 HE Bluetooth)
Bus Type:   Bluetooth (0x0005)
Usage Page: 0xFF55 (vendor)
Usage:      0x0202 (vendor)
Report ID:  6
```

### Accessing Battery Level

Battery is NOT available via vendor commands over BLE. Use:
```bash
bluetoothctl info F4:EE:25:AF:3A:38 | grep "Battery Percentage"
```

Or via D-Bus:
```
org.bluez /org/bluez/hci0/dev_XX_XX_XX_XX_XX_XX
org.bluez.Battery1 interface -> Percentage property
```

## Linux-Specific: HID Feature Report Buffering

**CRITICAL DISCOVERY**: On Linux with hidraw, there's a response delay/buffering behavior:

1. After `HIDIOCSFEATURE` (send), the response isn't immediately available
2. `HIDIOCGFEATURE` (read) returns the **previous** buffered response
3. Need to retry read 2-3 times (with ~50-100ms delays) to get the actual response

**Working pattern:**
```python
def query(fd, cmd):
    for attempt in range(3):
        send_feature(fd, [cmd])
        time.sleep(0.1)
        resp = read_feature(fd)
        if resp[1] == cmd:  # Command echo matches
            return resp
    return None
```

This differs from WebHID which appears to be synchronous.

## Verified Device Data (MonsGeek M1 V5 HE)

| Field | Value |
|-------|-------|
| Device ID | 2949 (0x0B85) |
| Firmware Version | 1029 (v10.29) |
| Default Profile | 0 |
| LED Mode | 1 (Constant/Static) |
| LED Brightness | 2 |
| LED Speed | 4 |
| LED Color | RGB(7, 255, 255) |
| Precision | 0.1mm (factor=10) |

## LED Effect Modes (LightList)

The LED mode byte in GET_LEDPARAM/SET_LEDPARAM maps to these effects:

| Mode | Internal Name | Description |
|------|---------------|-------------|
| 0 | LightOff | Off |
| 1 | LightAlwaysOn | Constant/Static |
| 2 | LightBreath | Breathing |
| 3 | LightNeon | Neon/Spectrum Cycle |
| 4 | LightWave | Wave |
| 5 | LightRipple | Ripple |
| 6 | LightRaindrop | Raindrop/Star Dots |
| 7 | LightSnake | Snake/Flow |
| 8 | LightPressAction | Reactive (light on press) |
| 9 | LightConverage | Converge |
| 10 | LightSineWave | Sine Wave |
| 11 | LightKaleidoscope | Kaleidoscope |
| 12 | LightLineWave | Line Wave |
| 13 | LightUserPicture | Custom Picture |
| 14 | LightLaser | Laser |
| 15 | LightCircleWave | Circle Wave |
| 16 | LightDazzing | Dazzle/Rainbow |
| 17 | LightRainDown | Rain Down |
| 18 | LightMeteor | Meteor |
| 19 | LightPressActionOff | Reactive (light off on press) |
| 20 | LightMusicFollow3 | Music Reactive 3 |
| 21 | LightScreenColor | Screen Color Sync |
| 22 | LightMusicFollow2 | Music Reactive 2 |
| 23 | LightTrain | Train |
| 24 | LightFireWorks | Fireworks |
| 25 | LightUserColor | Custom Color Per Key |

### LED Parameter Structure (GET_LEDPARAM 0x87 / SET_LEDPARAM 0x07)

**Response format:**
```
Byte 1: Mode (0-25, see table above)
Byte 2: Speed (inverted: MAXSPEED - actual, where MAXSPEED=4)
Byte 3: Brightness (0-4)
Byte 4: Options (high nibble = option, low nibble = dazzle flag)
Byte 5: Red
Byte 6: Green
Byte 7: Blue
```

**Options byte decoding:**
- `option = byte[4] >> 4` (direction/variant)
- `dazzle = (byte[4] & 0x0F) == 8` (rainbow color cycle)
- `normal = (byte[4] & 0x0F) == 7` (single color)

## Live Key Depth Monitoring (Real-time Magnetism)

This feature enables real-time reporting of key press depth for all keys.

### Enable/Disable Command (SET_MAGNETISM_REPOR 0x1B)

**Request:**
```
Byte 0: 0x1B (27)
Byte 1: 1 = start reporting, 0 = stop reporting
Byte 7: Checksum (Bit7)
```

### Input Report Format

When enabled, the keyboard sends HID INPUT reports (not feature reports):

```
Byte 0: Report ID
Byte 1: 0x1B (27) - indicates magnetism data
Byte 2: Key depth value (low byte)
Byte 3: Key depth value (high byte, if 16-bit precision)
```

**Decoding depth value:**
```javascript
// For precision factor 10 (0.1mm resolution):
depth_mm = value[2] / 10.0;

// For precision factor 100 (0.01mm resolution):
depth_mm = ((value[3] << 8) | value[2]) / 100.0;
```

The precision factor is device-specific and can be queried via `get磁轴行程步进倍数()`.

### Other Vendor Input Reports

The keyboard sends various status updates via input reports:

| Bytes 1-3 | Event |
|-----------|-------|
| [15, 1, 0] | Calibration started |
| [15, 0, 0] | Calibration stopped |
| [27, *, *] | Key depth data |
| [128, 0, 0] | Microphone event |
| [13, 0, 0] | Reset |
| [19, 0, 0] | Sleep mode change |
| [44, 0, 0] | Screen clear complete |
| [4-7, *, *] | LED effect changed |
| [8-11, *, *] | Side LED effect changed |
| [1, *, *] | Profile changed |
| [29, *, *] | Magnetic mode changed |

## Multi-Magnetism / Trigger Settings (GET/SET_MULTI_MAGNETISM)

### Command Structure

**GET_MULTI_MAGNETISM (0xE5):**
```
Byte 0: 0xE5 (229)
Byte 1: Sub-command (data type)
Byte 2: 1 (read mode)
Byte 3: Page index (for paginated data)
Byte 7: Checksum
```

**SET_MULTI_MAGNETISM (0x65):**
```
Byte 0: 0x65 (101)
Byte 1: Sub-command (data type)
Byte 2: 0 (write mode) or 1 (batch write)
Byte 3+: Data payload
Byte 7: Checksum
```

### Sub-commands

| Sub-cmd | Description | Format | Notes |
|---------|-------------|--------|-------|
| 0 | Press travel (actuation point) | 16-bit LE per key | In precision units |
| 1 | Lift travel (release point) | 16-bit LE per key | In precision units |
| 2 | RT press travel | 16-bit LE per key | Rapid Trigger press |
| 3 | RT lift travel | 16-bit LE per key | Rapid Trigger release |
| 4 | DKS travel | 16-bit LE per key | Dynamic Keystroke |
| 5 | Mod-Tap time | 8-bit per key | Value × 10 = ms |
| 6 | Bottom dead zone | 16-bit LE per key | In precision units |
| 7 | Key mode flags | Variable | Per-key feature flags |
| 9 | Snap Tap enable | 8-bit per key | Anti-SOCD |
| 10 | DKS trigger modes | 4 bytes per key | Actions at 4 depths |
| 251 | Top dead zone | 16-bit LE per key | FW >= 1024 |
| 252 | Switch type | 16-bit LE per key | If replaceable |
| 254 | Calibration values | 16-bit LE per key | Raw sensor data |

### Key Mode Options

Each key can be configured with one of these modes:

| Mode | Description |
|------|-------------|
| `normal` | Standard actuation with press/release points |
| `dks` | Dynamic Keystroke - different actions at 4 depth levels |
| `mt` | Mod-Tap - tap for one key, hold for modifier |
| `tgl_hold` | Toggle on tap, hold for momentary |
| `snap` | Snap Tap - anti-SOCD for gaming |

### Data Structure Example

**Normal mode per-key data:**
```javascript
{
    travel: 20,           // Actuation point (2.0mm)
    liftTravel: 20,       // Release point (2.0mm)
    deadZoneTravel: 0,    // Bottom dead zone
    topDeadZoneTravel: 0, // Top dead zone
    fire: false,          // Rapid Trigger enabled
    firePressTravel: 3,   // RT press sensitivity (0.3mm)
    fireLiftTravel: 3     // RT release sensitivity (0.3mm)
}
```

**DKS mode per-key data:**
```javascript
{
    dynamicTravel: 10,    // DKS activation travel
    triggerModes: [1, 2, 0, 3]  // Actions at 4 depth levels
}
```

### Travel Value Conversion

All travel values are stored in precision units:
```javascript
// Reading: divide by precision factor
travel_mm = raw_value / precision_factor;

// Writing: multiply by precision factor
raw_value = travel_mm * precision_factor;

// Example with precision=10 (0.1mm):
// raw=20 → 2.0mm actuation point
// raw=3  → 0.3mm RT sensitivity
```

## Calibration

### Minimum Travel Calibration (0x1C)

Calibrates the "released" position of all keys:
```
Byte 0: 0x1C (28)
Byte 1: 1 = start, 0 = stop
```

### Maximum Travel Calibration (0x1E)

Calibrates the "fully pressed" position of all keys:
```
Byte 0: 0x1E (30)
Byte 1: 1 = start, 0 = stop
```

### Calibration Procedure

1. Send minimum calibration start (0x1C, 1)
2. Wait 2000ms (keys should be released)
3. Send minimum calibration stop (0x1C, 0)
4. Send maximum calibration start (0x1E, 1)
5. User presses all keys fully
6. Send maximum calibration stop (0x1E, 0)
