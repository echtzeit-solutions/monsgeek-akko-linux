# Beiying: Phase 1 Capture and Evidence Guide

This guide describes how to collect reproducible evidence for Phase 1 of
`docs/BACKENDS.md`. It does not document an inferred protocol: every wire byte
must come from captures of the vendor tool and be confirmed by a hardware
transcript on the real keyboard.

## Deliverables

The maintainer requires three distinct pieces of evidence:

1. **Original USB capture** (`.pcapng`) of the vendor tool performing the
   handshake and each read query.
2. **Text rendering** (`.txt`) of the relevant frames with request and reply
   bytes visible. Preserve the original `.pcapng` alongside it.
3. **Hardware transcript** showing the output of the `iot_driver` commands
   enabled by the phase.

UI screenshots alone do not replace the USB capture or the hardware transcript.
Do not invent bytes, field names, or values that are not visible in the
evidence. Do not run write, save, apply, reset, or calibrate commands during
read-only capture collection.

## Operations to Capture

Use the official Beiying vendor tool and record a separate capture for each
action, starting with the keyboard connected. Do not use the RongYuan utility:
USB VID/PID can be shared across different protocols.

| Suggested file | Action in vendor tool | Expected evidence |
| --- | --- | --- |
| `connect.pcapng` / `.txt` | Open/connect to the keyboard | Full handshake |
| `get-version.pcapng` / `.txt` | Query firmware version | Request and reply |
| `get-profile.pcapng` / `.txt` | Query active profile | Request and reply |
| `get-polling-rate.pcapng` / `.txt` | Query polling rate | Request and reply |
| `get-debounce.pcapng` / `.txt` | Tested TAC75 HE v108 has no traditional debounce | Keep as `NotSupported`; do not map deadzone |
| `get-sleep-time.pcapng` / `.txt` | Tested wired TAC75 HE has no `SleepTimeSettings` | Keep as `NotSupported`; `0x00b4` remains opaque |

Run each available query once and stop capturing immediately afterward to keep
unrelated USB traffic minimal. Record the tool name and version, device model
string shown by the program, firmware version, and host operating system. For
the tested wired TAC75 HE (`v108`), the owner confirmed that neither
traditional debounce nor a `SleepTimeSettings` timer exists on this keyboard; do
not substitute deadzone writes/reads or LED timeouts to simulate those getters,
and do not interpret the opaque `00 b4` reply from `0x84 0x13` as sleep time.

## Windows: Wireshark with USBPcap (recommended)

Install Wireshark with the USBPcap component and open an Administrator
terminal. Installation paths vary; use the actual path to `USBPcapCMD.exe` and
create a capture directory outside the repository:

```powershell
New-Item -ItemType Directory -Force C:\beiying-captures
& 'C:\Program Files\USBPcap\USBPcapCMD.exe' --help
& 'C:\Program Files\USBPcap\USBPcapCMD.exe' --devices
```

If `--devices` is unavailable in the installed version, run `USBPcapCMD.exe`
without arguments and follow the interactive root-hub prompt. Choose the root
hub to which the keyboard is attached and confirm the capture flags with
`--help` (typically `-d` for the USBPcap device and `-o` for the output file).
Example (replace `USBPcap1` with the listed device):

```powershell
& 'C:\Program Files\USBPcap\USBPcapCMD.exe' -d '\\.\USBPcap1' -o 'C:\beiying-captures\connect.pcap'
```

Once capturing starts, perform only the single action from the table and press
`Ctrl+C` in the terminal. Repeat with the next filename. Open each capture in
Wireshark and save a **pcapng** copy (`File > Save As`, format
`Wireshark/... - pcapng`), while preserving the original `.pcap` if USBPcap
produced that format.

To generate a detailed text rendering in PowerShell with TShark installed:

```powershell
& 'C:\Program Files\Wireshark\tshark.exe' -r 'C:\beiying-captures\connect.pcapng' -V -x | Out-File -Encoding utf8 'C:\beiying-captures\connect.txt'
```

Repeat for each capture name. `-V -x` includes protocol dissection and raw hex
bytes. The capture may contain traffic from other devices on the same bus; in
the trimmed evidence copy, retain only the keyboard frames and complete
transaction exchange without altering or discarding the original capture.

## Linux: Alternative with `usbmon` and `tshark`

USBPcap on Windows is often simpler when running a Windows vendor utility. When
using the vendor web configurator on Linux, `usbmon` captures software-level
URBs (note that `usbmon` records host-controller URBs rather than electrical
bus traces):

```bash
sudo modprobe usbmon
tshark -D
```

Identify the `usbmonN` interface for the keyboard's USB bus and start a
separate capture per operation (press `Ctrl+C` to stop):

```bash
sudo tshark -i usbmonN -w /tmp/connect.pcapng
```

Perform only the single operation in the tool, press `Ctrl+C`, and generate the
verbose text rendering:

```bash
tshark -r /tmp/connect.pcapng -V -x > /tmp/connect.txt
```

Inspect submission and completion URBs in Wireshark, preserve the `.pcapng`
alongside the `.txt`, and never check in unfiltered bus captures containing
other USB devices.

## Repository Layout

Check trimmed, single-operation fixtures into:

```text
iot_driver_linux/monsgeek-beiying/tests/fixtures/
```

Include both `.pcapng` and `.txt` for each captured operation. Keep large raw
captures outside the repository and document where they were stored and how the
fixtures were trimmed. Each fixture must retain the necessary connect
handshake and complete request/reply exchange with frame numbers, direction,
timestamps, and full hex bytes—never hand-edited or synthetic bytes.

Check fixture sizes before adding them:

```bash
du -h /path/to/beiying-captures/*
```

Fixtures must drive replay tests against the real backend implementation so
both outgoing request bytes and parsed values are verified continuously in CI.

## Real-Hardware Transcript

After implementing the Phase 1 queries, run the Phase 1 CLI commands on the
connected keyboard and capture both stdout and stderr:

```bash
{
  iot_driver info
  iot_driver profile
  iot_driver rate
  iot_driver debounce
  iot_driver sleep
} 2>&1 | tee /tmp/beiying-phase1-terminal.txt
```

Use the locally built binary and the CLI syntax from the local checkout; pass
`--device <id>` when required. Record the keyboard model, firmware version, OS,
and local checkout branch/commit alongside the transcript.

## Current Phase 1 Status (`beiying-backend`)

- **Passive sysfs discovery (`iot_driver_linux/monsgeek-transport/src/discovery.rs`):**
  Classifies `0xFF60:0x0061` / `Report ID 0x09` interfaces by reading the kernel
  HID report descriptor from sysfs without opening `/dev/hidraw*` or sending
  RongYuan probes. Tested against captured RongYuan (`rongyuan_m1_v5_if2.hex`)
  and Beiying (`beiying_tac75_if2.hex`) descriptors in
  `iot_driver_linux/monsgeek-transport/tests/fixtures/`.
- **Phase 1 getters (`iot_driver_linux/monsgeek-beiying`):**
  All implemented getters are backed by `demo.hubx.pro` WebHID captures in
  `iot_driver_linux/monsgeek-beiying/tests/fixtures/`:
  - **Handshake `0x82 0x01` and firmware version `get_version` (`0x82 0x02`):**
    Hardware UUID `110000000059` and firmware `0x0108` (`v108`) verified in
    `tests/fixtures/beiying-connect.{pcapng,txt}`.
  - **Polling rate `get_polling_rate` (`0x84 0x17`, all 7 rates proven):**
    - `0x03 -> 125 Hz` (`/tmp/hubx-final/hubx-poling125hz.{pcapng,txt}`, `dev=29->30`,
      write `0x04 0x17` `629`/`631`, post-reconnect read `1757`/`1759`, chk `0x56`)
    - `0x02 -> 250 Hz` (`/tmp/hubx-final/hubx-poling250hz.{pcapng,txt}`, `dev=30->31`,
      write `411`/`415`, post-reconnect read `1387`/`1389`, chk `0x57`)
    - `0x01 -> 500 Hz` (`/tmp/hubx-final/hubx-poling500hz.{pcapng,txt}`, `dev=31->32`,
      write `515`/`517`, post-reconnect read `1511`/`1513`, chk `0x58`)
    - `0x00 -> 1000 Hz` (`/tmp/hubx-final/hubx-poling1000hz.{pcapng,txt}`, `dev=32->33`,
      write `565`/`569`, post-reconnect read `1685`/`1687`, chk `0x59`)
    - `0x06 -> 2000 Hz` (`/tmp/hubx-final/hubx-poling2000hz.{pcapng,txt}`, `dev=33->34`,
      write `547`/`549`, post-reconnect read `1729`/`1731`, chk `0x53`)
    - `0x05 -> 4000 Hz` (`/tmp/hubx-final/hubx-poling4000hz.{pcapng,txt}`, `dev=34->35`,
      write `909`/`911`, post-reconnect read `1785`/`1787`, chk `0x54`; also confirmed in
      `tests/fixtures/get-polling-rate.{pcapng,txt}` / `hubx-base` `dev=7` on the
      Basic Functions (`"Funções básicas"`) screen showing `4000 Hz`, and in
      `hubx-profile` `dev=12->13`)
    - `0x04 -> 8000 Hz` (`/tmp/hubx-final/hubx-poling8000hz.{pcapng,txt}`, `dev=35->36`,
      write `495`/`497`, post-reconnect read `1301`/`1303`, chk `0x55`)
    - **Inconsistency note:** An earlier manually typed mapping listed seven
      polling rates (`125..8000 Hz`) while omitting wire code `0x05`. The raw
      and filtered captures (`hubx-base`, `hubx-profile`, and
      `hubx-poling4000hz`) confirm the complete `0x00..=0x06` mapping including
      `0x05 -> 4000 Hz`.
    - All seven post-reconnect read-only `0x84 0x17` exchanges (zero `0x04 0x17`
      write packets) are trimmed into
      `tests/fixtures/get-polling-rate-codes.{pcapng,txt}` (84 frames, replies
      at Frames `11`, `23`, `35`, `47`, `59`, `71`, and `83`). Uncaptured codes
      (`>= 0x07`) return `KeyboardError::UnexpectedResponse`.
  - **Active profile `get_profile` (`0x90 0x00`, 0-based index):**
    - `Profile 1` (`Profile(0)`, `pkt[7] = 0x00`, chk `0x65`):
      `tests/fixtures/get-profile.{pcapng,txt}` (trimmed from `hubx-base` Frames
      `2063`/`2065`, identical across all seven `hubx-poling*hz` captures on
      Profile 1).
    - `Profile 2` (`Profile(1)`, `pkt[7] = 0x01`, chk `0x64`):
      `tests/fixtures/get-profile-p2.{pcapng,txt}` (read-only slice trimmed from
      `/tmp/hubx-final/hubx-trocaperfil.pcapng`, `dev=36`, after Profile 1 ->
      Profile 2 switch write `0x10 0x00` raw Frame `1047` + ACK `1049` and read
      `0x90 0x00` Frames `1051`/`1053`; corroborated by
      `/tmp/hubx-profile-change-raw.pcapng` / `/tmp/hubx-final/hubx-profile-change.txt`,
      where `0x90 0x00` returned `0x01, 0x00, 0x01, 0x02` with zero deadzone
      `0x16 0x00` writes).
    - Like `0x82 0x02`, firmware `v108` leaves `pkt[6] = 0x00` and writes the
      0-based profile index at `pkt[7]`; `parse_profile_reply` accepts
      `pkt[6] == 0 || pkt[6] == 1`.
- **Owner-confirmed unsupported settings on the tested wired TAC75 HE (`v108`):**
  The keyboard owner confirmed that this wired TAC75 HE (`v108`) does not have
  traditional debounce or `SleepTimeSettings` features. This confirmation is
  specific to the tested device and firmware—not inferred solely from protocol
  status bytes—and Akko's public pages describe USB-C wired operation, magnetic
  switches, deadzone, and RT Stabilizer without explicitly stating the absence
  of traditional debounce or sleep-time settings
  ([TAC75 HE product page](https://en.akkogear.com/product/tac75-he-the-kings-avatar-magnetic-switch-keyboard/),
  [Akko magnetic-switch knowledge base](https://en.akkogear.com/akko-magnetic-switches-knowledge-base/)).
  - `get_debounce` remains `KeyboardError::NotSupported`: `0x84 0x1e`
    (`tests/fixtures/get-debounce.{pcapng,txt}`) and `0x84 0x1d` return
    zero-length replies (`pkt[6] = 0x00`). Hall-effect deadzone (`0x16 0x00` /
    `0x96 0x00`) is a distinct Phase 5 feature and must not be equated with
    debounce.
  - `get_sleep_time` remains `KeyboardError::NotSupported`: `0x84 0x13`
    (`tests/fixtures/get-sleep-time.{pcapng,txt}`) returns `00 b4`, which has
    no confirmed mapping to `SleepTimeSettings` (`idle_bt`, `deep_bt`,
    `idle_24g`, `deep_24g`) or LED timeout, and `/tmp/hubx-final/hubx-sleep-mode.{pcapng,txt}`
    (`Fn+L`) contains only `ep 0x81` HID key reports and heartbeat frames with
    zero `0x84 0x13` or `0x04 0x13` traffic. Do not interpret `00 b4` as sleep
    time.

### Out of Scope for Phase 1 (Future Phases per `docs/BACKENDS.md`)

- **Phase 4 (Lighting):**
  - `/tmp/hubx-final/hubx-brilho.{pcapng,txt}`: unsolicited device-to-host
    notifications `09 fe 0b 02 <val>` (`05, 0a, 0f, 14, 0f, 0a, 05, 00`) on
    `ep 0x83`; zero host writes.
  - `/tmp/hubx-final/hubx-velocidade-brilho.{pcapng,txt}`: unsolicited
    notifications `09 fe 0b 03 <val>` (`01, 02, 03, 04, 04, 03, 02, 01, 00, 00`)
    on `ep 0x83`.
  - `/tmp/hubx-final/hubx-lado-brilho.{pcapng,txt}`: heartbeat/`Ctrl+C` on
    `ep 0x81` only.
- **Phase 5 (Hall-Effect Deadzones & Layers):**
  - `/tmp/hubx-final/hubx-configzonamorta.{pcapng,txt}`
    (`/tmp/hubx-configzonamorta-raw.pcapng`, `dev=36`, `22:32`): 96 write packets
    `0x16 0x00` + 96 ACKs + 96 read packets `0x96 0x00` across 8 bulk all-key
    batches with randomized values (zero `0x84 0x1e` packets; not debounce).
  - `/tmp/hubx-final/hubx-debounce.{pcapng,txt}`
    (`/tmp/hubx-debounce-raw.pcapng`, `dev=29`, `21:40`): despite the filename,
    contains 84 writes `0x16 0x00` + 84 ACKs + 84 reads `0x96 0x00` across 7
    bulk deadzone batches (zero `0x84 0x1e` packets).
  - `/tmp/hubx-final/hubx-camadapadrao.{pcapng,txt}`, `hubx-camadafn.{pcapng,txt}`,
    and `hubx-camadafn1.{pcapng,txt}` (`dev=36`, `22:35`): heartbeat
    (`0x82*`/`0x84 11`) and `ep 0x81` `Alt+Tab`/`Ctrl+C` only; switching layer
    tabs in `demo.hubx.pro` sends no USB traffic (layers are pre-loaded via
    `0x83`/`0x9a`).

## Recorded Phase 1 Hardware Transcript

Recorded on the host with the local `beiying-backend` release binary and the
awake TAC75 HE (`38ee:0016`) connected (saved verbatim at
`/tmp/beiying-phase1-terminal-final.txt`):

```text
=== branch e commit ===
beiying-backend
22d0b1711ecb1ceb881f3ca8d3f3d7240edbe84d
=== teclado ===
Bus 001 Device 036: ID 38ee:0016 BY Tech Akko Keyboard
=== binário local ===
327810846b3a8955af656e8e012944cf6bbafb0a5323a63615ae932eb745e0de  /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver

$ /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver list

#0  RAID 75 HE           usb     
  #0 — not responding (may be asleep)
[exit: 0]

$ /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver --device 0 info

Device:    Akko Keyboard
  VID/PID:  38EE:0016  type=HidWired
Firmware:  v108 (raw 0x0108, dec 264)
[exit: 0]

$ /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver --device 0 profile

Profile: 1
[exit: 0]

$ /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver --device 0 rate

Polling rate: 8000 (8kHz)
[exit: 0]

$ /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver --device 0 debounce

Error: NotSupported("The tested wired TAC75 HE has no traditional debounce setting; Hall-effect deadzone is a separate feature")
[exit: 1]

$ /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver --device 0 sleep

Failed to get sleep settings: Feature not supported: The tested wired TAC75 HE has no SleepTimeSettings feature; the opaque 0x84 0x13 reply is not mapped
[exit: 0]

$ /home/juninho/Downloads/monsgeek-akko-linux/iot_driver_linux/target/release/iot_driver --device 0 led

Akko Keyboard does not support this command (RongYuan-protocol keyboards only)
[exit: 0]
```

**Notes on the transcript:**

- Direct Beiying commands succeed against the real board (`38EE:0016` /
  `v108`), reporting 0-based `Profile: 1` (UI Profile 2) and
  `Polling rate: 8000 (8kHz)`, while `debounce` and `sleep` cleanly report
  `NotSupported` and the RongYuan-only `led` command reports
  `Akko Keyboard does not support this command (RongYuan-protocol keyboards only)`.
- `iot_driver list` still prints `#0  RAID 75 HE           usb` and
  `#0 — not responding (may be asleep)` even though all direct Beiying queries
  work. That is expected under the Phase 1 discovery seam in `docs/BACKENDS.md`:
  `list` runs `HidDiscovery::probe_devices()`, which calls
  `HidDiscovery::open_device()` to send a RongYuan `GET_USB_VERSION` probe.
  Because Phase 1 intentionally limits `HidDiscovery::open_device()` to
  `KeyboardProtocol::RongYuan`, the Beiying device is marked `responsive = false`
  with `device_id = None`, causing `list` to fall back to the first matching
  `38EE:0016` name in `devices.json` (`RAID 75 HE`) and print `not responding`.
  Direct commands route through `open_keyboard()`, which branches on
  `KeyboardProtocol::Beiying` and opens `BeiyingKeyboard::open()`.

## PR Checklist

Target a **separate PR per phase** against `echtzeit-solutions/monsgeek-akko-linux:beiying-backend`
(do not append Phase 1 to PR #25 and do not target `refactor/keyboard-backend-trait` or `master`):

- [x] Official vendor tool (`demo.hubx.pro` WebHID) identified; no RongYuan tool used for fixtures.
- [x] Handshake (`beiying-connect`), `get_polling_rate` (all 7 rates `125..8000 Hz`), and `get_profile` (`Profile 1` and `Profile 2`) have trimmed read-only fixtures in `tests/fixtures/`.
- [x] Every checked-in fixture retains readable request/reply pairs with zero write packets.
- [x] `get_debounce` and `get_sleep_time` documented as unsupported on the tested wired TAC75 HE (`v108`) and kept at `NotSupported` without mapping deadzones or `00 b4`.
- [x] Hardware transcript recorded on the awake TAC75 HE (`/tmp/beiying-phase1-terminal-final.txt`) covering `list`, `info`, `profile`, `rate`, `debounce`, `sleep`, and RongYuan-only `led`.
- [x] Zero write operations executed against the keyboard by `iot_driver`.
- [x] Every implemented protocol constant and parser cites a checked-in fixture file and frame number per `docs/BACKENDS.md`.

## Tool References

- Wireshark USB capture and `usbmon`: <https://wiki.wireshark.org/CaptureSetup/USB>
- TShark flags (`-D`, `-i`, `-w`, `-r`, `-V`, `-x`):
  <https://www.wireshark.org/docs/man-pages/tshark.html>
- USBPcap tour and Windows usage: <https://desowin.org/usbpcap/tour.html>
- USBPcap source and documentation: <https://github.com/desowin/usbpcap>
