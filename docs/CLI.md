# CLI Reference

Complete command reference for `iot_driver`.

## Global Flags

These flags work with any command:

| Flag | Description |
|------|-------------|
| `--monitor` | Enable transport monitoring (prints all HID commands/responses) |
| `--file <FILE>` | Use pcap file instead of real device (passive replay) |
| `--hex` | Show raw hex dump alongside decoded output |
| `--filter <FILTER>` | Filter output: `all`, `events`, `commands`, or `cmd=0xNN` |
| `--all` | Include standard HID reports (keyboard, consumer, NKRO) |

**Examples:**
```bash
iot_driver --monitor info           # Trace HID traffic while getting info
iot_driver --file capture.pcap      # Replay pcap file (no device needed)
iot_driver --hex --filter cmd=0x87 led  # Show 0x87 commands in hex
```

## Query Commands

Commands that read device state without modifying it.

### info

Get device ID and firmware version.

```bash
iot_driver info
```

**Aliases:** `version`, `ver`, `v`

### profile

Get current profile (0-3).

```bash
iot_driver profile
```

**Aliases:** `prof`, `p`

### led

Get LED settings (mode, brightness, speed, color).

```bash
iot_driver led
```

**Aliases:** `light`, `l`

### debounce

Get debounce time in milliseconds.

```bash
iot_driver debounce
```

**Aliases:** `deb`, `d`

### rate

Get polling rate in Hz.

```bash
iot_driver rate
```

**Aliases:** `poll`, `hz`

### options

Get keyboard options (Fn layer, WASD swap, etc.).

```bash
iot_driver options
```

**Aliases:** `opts`, `opt`, `o`

### features

Get supported features and precision.

```bash
iot_driver features
```

**Aliases:** `feat`, `f`

### sleep

Get sleep time settings (idle + deep sleep for BT and 2.4GHz).

```bash
iot_driver sleep
```

**Aliases:** `s`

### all

Show all device information.

```bash
iot_driver all
```

**Aliases:** `a`

### battery

Get battery status (for 2.4GHz wireless dongles).

```bash
iot_driver battery           # Normal output
iot_driver battery -q        # Quiet (percentage only)
iot_driver battery -w        # Watch mode (updates every 1s)
iot_driver battery -w 5      # Watch mode (updates every 5s)
iot_driver battery --vendor  # Use vendor HID (skip kernel power_supply)
iot_driver battery --hex     # Show full vendor response
```

**Aliases:** `bat`, `b`

## Set Commands

Commands that modify device settings.

### set-profile

Set active profile.

```bash
iot_driver set-profile 1    # Profile 0-3
```

**Aliases:** `sp`

### set-debounce

Set debounce time in milliseconds.

```bash
iot_driver set-debounce 5   # 0-50 ms
```

**Aliases:** `sd`

### set-rate

Set polling rate.

```bash
iot_driver set-rate 8000    # 8kHz
iot_driver set-rate 1000hz  # 1kHz
iot_driver set-rate 1k      # 1kHz
```

Supported: 125, 250, 500, 1000, 2000, 4000, 8000 Hz

**Aliases:** `sr`, `setpoll`

### set-led

Set LED mode and parameters.

```bash
iot_driver set-led <mode> [brightness] [speed] [r] [g] [b]
```

- `mode`: 0-25 or name (`breathing`, `wave`, `rainbow`, etc.)
- `brightness`: 0-4 (default: 4)
- `speed`: 0-4 (default: 2)
- `r`, `g`, `b`: 0-255 (default: 255)

```bash
iot_driver set-led wave              # Wave mode, defaults
iot_driver set-led 4 4 3             # Mode 4, brightness 4, speed 3
iot_driver set-led constant 4 0 255 0 128  # Purple constant color
```

**Aliases:** `sl`

### set-sleep

Set sleep time settings.

```bash
iot_driver set-sleep --idle 2m --deep 28m        # Both BT and 2.4GHz
iot_driver set-sleep --idle-bt 3m                # BT only
iot_driver set-sleep --deep-24g off              # Disable 2.4GHz deep sleep
iot_driver set-sleep --uniform 2m,28m            # Set idle,deep for all
```

Values: seconds (`120`), minutes (`2m`), hours (`1h`), or `off`/`0`

**Aliases:** `ss`

### reset

Factory reset keyboard.

```bash
iot_driver reset
```

### calibrate

Run min + max calibration.

```bash
iot_driver calibrate
```

**Aliases:** `cal`

## Trigger Commands

Commands for per-key trigger settings.

### triggers

Show current trigger settings.

```bash
iot_driver triggers
```

**Aliases:** `gt`

### set-actuation

Set actuation point for all keys.

```bash
iot_driver set-actuation 2.0    # 2.0mm actuation point
```

**Aliases:** `sa`

### set-rt

Enable/disable Rapid Trigger or set sensitivity.

```bash
iot_driver set-rt on           # Enable with default sensitivity
iot_driver set-rt on 0.3       # Enable with 0.3mm sensitivity
iot_driver set-rt 0.2          # Enable with 0.2mm sensitivity
iot_driver set-rt off          # Disable Rapid Trigger
```

**Aliases:** `rapid-trigger`, `rt`

### set-release

Set release point for all keys.

```bash
iot_driver set-release 1.5     # 1.5mm release point
```

**Aliases:** `srl`

### set-bottom-deadzone

Set bottom deadzone for all keys.

```bash
iot_driver set-bottom-deadzone 0.2
```

**Aliases:** `sbd`

### set-top-deadzone

Set top deadzone for all keys.

```bash
iot_driver set-top-deadzone 0.1
```

**Aliases:** `std`

### set-key-trigger

Set trigger settings for a specific key.

```bash
iot_driver set-key-trigger 42 --actuation 1.5
iot_driver set-key-trigger 42 --release 2.0
iot_driver set-key-trigger 42 --mode rt
iot_driver set-key-trigger 42 --actuation 1.0 --mode dks
```

Modes: `normal`, `rt`, `dks`, `snaptap`

**Aliases:** `skt`

## Color Commands

### set-color-all

Set all keys to a single color.

```bash
iot_driver set-color-all 255 0 0        # Red
iot_driver set-color-all 0 255 0 -l 1   # Green on layer 1
```

**Aliases:** `color-all`, `sc`

## Remapping Commands

### remap

Remap a key.

```bash
iot_driver remap A B              # A outputs B
iot_driver remap CapsLock Escape  # CapsLock -> Escape
iot_driver remap 42 0x29 -l 1     # By matrix index, layer 1
```

**Aliases:** `set-key`

### reset-key

Reset a key to default.

```bash
iot_driver reset-key A
iot_driver reset-key CapsLock -l 1
```

**Aliases:** `rk`

### swap

Swap two keys.

```bash
iot_driver swap A S
iot_driver swap CapsLock Escape -l 0
```

### keymatrix

Show key matrix mappings.

```bash
iot_driver keymatrix       # Layer 0
iot_driver keymatrix 1     # Layer 1
```

**Aliases:** `km`

## Macro Commands

### macro

Get macro for a key.

```bash
iot_driver macro F1
```

**Aliases:** `get-macro`

### set-macro

Set a text macro for a key.

```bash
iot_driver set-macro F1 "Hello World"
iot_driver set-macro F2 "git status"
```

**Aliases:** `set-text-macro`

### clear-macro

Clear macro from a key.

```bash
iot_driver clear-macro F1
```

## Animation Commands

### gif

Upload GIF animation to keyboard memory.

```bash
iot_driver gif animation.gif
iot_driver gif image.gif tile      # Tile mode
iot_driver gif --test              # Test rainbow animation
iot_driver gif --test --frames 30 --delay 100
```

Mapping modes: `scale` (default), `tile`, `center`, `direct`

### gif-stream

Stream GIF animation in real-time.

```bash
iot_driver gif-stream animation.gif
iot_driver gif-stream animation.gif --loop
iot_driver gif-stream video.gif center
```

### mode

Set LED mode by name or number.

```bash
iot_driver mode wave
iot_driver mode breathing
iot_driver mode 4
iot_driver mode user-picture -l 2   # Layer 2 for per-key modes
```

### modes

List all available LED modes.

```bash
iot_driver modes
```

## Demo Commands

Real-time animation demos (stream from host).

### rainbow

Real-time rainbow sweep animation.

```bash
iot_driver rainbow
```

### checkerboard

Checkerboard pattern demo.

```bash
iot_driver checkerboard
```

**Aliases:** `checker`

### sweep

Sweeping line animation demo.

```bash
iot_driver sweep
```

### red

Set all keys to red (demo).

```bash
iot_driver red
```

### wave

Real-time wave animation demo.

```bash
iot_driver wave
```

## Audio Commands

### audio

Run audio reactive LED mode.

```bash
iot_driver audio                    # Spectrum mode (default)
iot_driver audio -m solid           # Solid color pulse
iot_driver audio -m gradient        # Gradient effect
iot_driver audio --hue 180          # Base hue (0-360)
iot_driver audio --sensitivity 1.5  # Sensitivity (0.5-2.0)
```

Modes: `spectrum`, `solid`, `gradient`

### audio-test

List available audio capture devices.

```bash
iot_driver audio-test
```

### audio-levels

Show real-time audio levels (terminal visualizer).

```bash
iot_driver audio-levels
```

## Screen Commands

### screen

Run screen color reactive LED mode (requires `screen-capture` feature).

```bash
iot_driver screen           # 2 FPS default
iot_driver screen -f 10     # 10 FPS
```

**Aliases:** `screencolor`

## Debug Commands

### test-transport

Test transport abstraction layer.

```bash
iot_driver test-transport
```

**Aliases:** `tt`

### depth

Monitor real-time key depth (magnetism).

```bash
iot_driver depth            # Normal output
iot_driver depth -r         # Raw hex bytes
iot_driver depth -z         # Show zero-depth reports
iot_driver depth -v         # Verbose status
```

**Aliases:** `keydepth`

## Firmware Commands

Firmware tools (dry-run only, no actual flashing).

### firmware info

Show current device firmware version.

```bash
iot_driver firmware info
```

**Aliases:** `fw i`

### firmware validate

Validate a firmware file.

```bash
iot_driver firmware validate firmware.bin
iot_driver firmware validate firmware.zip
```

**Aliases:** `fw val`

### firmware dry-run

Simulate firmware update (no actual flashing).

```bash
iot_driver firmware dry-run firmware.bin
iot_driver firmware dry-run firmware.bin -v   # Verbose
```

**Aliases:** `fw dr`

### firmware check

Check for firmware updates from MonsGeek server.

```bash
iot_driver firmware check
iot_driver firmware check --device-id 12345
```

**Aliases:** `fw chk`

### firmware download

Download firmware from MonsGeek server.

```bash
iot_driver firmware download
iot_driver firmware download -o my-firmware.zip
iot_driver firmware download --device-id 12345
```

**Aliases:** `fw dl`

## Utility Commands

### list

List all HID devices.

```bash
iot_driver list
```

**Aliases:** `ls`

### raw

Send raw command byte (for debugging).

```bash
iot_driver raw 8f          # Send 0x8F command
iot_driver raw 87          # Send 0x87 command
```

**Aliases:** `cmd`, `hex`

### serve

Run gRPC server on port 3814 (compatible with app.monsgeek.com).

```bash
iot_driver serve
```

**Aliases:** `server`

### tui

Run interactive terminal UI.

```bash
iot_driver tui
```

### joystick

Run joystick mapper (maps magnetic keys to virtual joystick axes).

```bash
iot_driver joystick
iot_driver joystick -c ~/.config/monsgeek/joystick.toml
iot_driver joystick --headless
```

**Aliases:** `joy`

## LED Mode Reference

| # | Mode | Description |
|---|------|-------------|
| 0 | Off | LEDs disabled |
| 1 | Constant | Static color |
| 2 | Breathing | Pulsing effect |
| 3 | Neon | Neon glow |
| 4 | Wave | Color wave |
| 5 | Ripple | Ripple from keypress |
| 6 | Raindrop | Raindrops |
| 7 | Snake | Snake pattern |
| 8 | Reactive | React to keypress (stay lit) |
| 9 | Converge | Converging pattern |
| 10 | Sine Wave | Sine wave |
| 11 | Kaleidoscope | Kaleidoscope |
| 12 | Line Wave | Line wave |
| 13 | User Picture | Custom per-key (4 layers) |
| 14 | Laser | Laser effect |
| 15 | Circle Wave | Circular wave |
| 16 | Rainbow | Rainbow/dazzle |
| 17 | Rain Down | Rain downward |
| 18 | Meteor | Meteor shower |
| 19 | Reactive Off | React briefly |
| 20 | Music Patterns | Audio reactive patterns |
| 21 | Screen Sync | Ambient screen color |
| 22 | Music Bars | Audio reactive bars |
| 23 | Train | Train pattern |
| 24 | Fireworks | Fireworks |
| 25 | Per-Key Color | Dynamic animation (GIF) |

## Per-Key Mode Reference

| Mode | Description |
|------|-------------|
| Normal | Standard actuation/release points |
| Rapid Trigger (RT) | Dynamic actuation based on movement |
| DKS | Dynamic Keystroke (4-stage trigger) |
| Mod-Tap | Different action for tap vs hold |
| Toggle Hold | Toggle on hold |
| Toggle Dots | Toggle on double-tap |
| Snap-Tap | SOCD resolution (bind to another key) |
