# Keyboard backends

`iot_driver` talks to keyboards through backends. A backend is one vendor
protocol: it owns its own I/O and implements the protocol-neutral
`monsgeek_keyboard::Keyboard` trait (`iot_driver_linux/monsgeek-keyboard/src/backend.rs`).

Today there is one backend, `RyKeyboard`, for the RongYuan (RY5088 / YiChip)
protocol. This document is the plan and the rules for adding a second one. The
first candidate is the Shenzhen Beiying / BYTech protocol (Report ID `0x09`),
used by some Akko boards such as the TAC75 HE that share USB IDs with RongYuan
models (`38ee:0016`). The maintainers have no Beiying hardware, so every phase
below is gated on evidence the contributor can produce and the maintainers can
check without the board.

## How the layers fit

```
CLI / TUI command
   │  &dyn Keyboard            ← generic commands (info, profile, rate, …)
   │  &RyKeyboard              ← RY-only commands, via with_ry_keyboard
   ▼
open_keyboard(ctx)             ← the CLI chooses a backend here
   │  resolve_device()         ← discovery lists and selects (--device)
   ├── RyKeyboard  ── FlowControlTransport ── RY framing (Feature reports, Bit7/Bit8)
   └── <NewBackend> ── its own I/O          ── its own framing
```

`open_keyboard` (`iot_driver_linux/src/commands/mod.rs`) selects the device
first and opens it second. A new backend adds its arm between those two steps,
branching on how discovery classified the selected device. The TUI
(`src/tui/mod.rs`, connect and reconnect) still builds a `RyKeyboard` directly;
it moves to the factory together with the TUI migration below, not before.

The trait borrows two plain types from `monsgeek-transport`: `Profile` and
`TransportDeviceInfo`. A backend depends on that crate for those types only.

- `monsgeek_transport::Transport` and `FlowControlTransport` are the **RY** wire
  layer (command byte, checksum types, flow control). A new backend does not
  implement them and does not add methods to them.
- Commands that only need the `Keyboard` trait take `&dyn Keyboard`. Commands
  for features only one protocol has take that backend's concrete type and are
  reached through `with_ry_keyboard` / `open_ry_keyboard`, or the new backend's
  equivalent, which downcast once and report other boards as unsupported.
- `<dyn Keyboard>::downcast_ref` is the only downcast. It is for a command that
  shows backend-specific detail (see `commands::query::info`), never for routing
  shared behaviour around the trait.

## Rules for every backend PR

These apply to humans and coding agents alike. A PR that breaks one will be
asked to change before review.

1. **Stay out of RY code.** Touch RongYuan files only at the seams named in
   phase 1. No new methods or hooks on `Transport`, `FlowControlTransport`,
   `Printer` or `RyKeyboard`, and no `if backend == X` branches inside shared
   code.
2. **Never fabricate.** An operation the backend cannot do yet returns
   `KeyboardError::NotSupported`. No placeholder values, no fake progress, no
   `Ok(())` for a write that was not sent.
3. **Cite every protocol fact.** Each command ID, offset, encoding or constant
   carries a comment naming its source: a capture fixture (file and frame) or a
   vendor binary (file, version, address). Don't call something "verified"
   unless there's a capture or a test to back it.
4. **Units at the boundary.** Trait methods take and return Hz, ms, seconds,
   µm, profile numbers. Wire codes stay inside the backend.
5. **Layouts are data.** Key layouts and per-model capabilities go in `data/`
   (keyed by what the device reports about itself), not in Rust tables.
6. **One phase per PR**, each green on `make test`, `make check` and
   `make fmt-check` (the same commands CI runs).
7. **Never slice a `str` by byte index**; use `char_indices`, `split`,
   `strip_prefix` or byte slices of `&[u8]`.

## Testing without the maintainers' hardware

Each phase ships three kinds of evidence:

- **Fixtures.** Captures of the vendor's own tool doing the operation, checked
  in under `iot_driver_linux/<backend crate>/tests/fixtures/`, trimmed to the
  relevant exchange. Wireshark + USBPcap on Windows or `usbmon` on Linux
  (WebHID in Chromium) both work; keep the original `.pcapng` and a small text
  rendering of the request/reply pairs.
- **Replay tests.** Put the backend's raw I/O behind a small trait (write one
  report, read one report with a timeout) with a hidapi implementation and a
  replay implementation that serves the fixture's replies. Tests then drive the
  real backend code against captured bytes: request bytes must match exactly and
  parsed values must match what the vendor tool showed.
- **A hardware transcript** pasted into the PR: the `iot_driver` commands the
  phase enables, run on the real board, with their output.

## Phases

### Phase 0: backend seam (done)

`Keyboard` trait, `RyKeyboard` behind it, the `open_keyboard` factory, generic
CLI commands on `&dyn Keyboard`, and a fake-backend test proving those commands
need nothing RY-specific.

### Phase 1: discovery and read-only queries

- New crate `iot_driver_linux/monsgeek-beiying` (workspace member) with the
  framing, checksum and multi-packet code, the replay I/O trait, and a
  `BeiyingKeyboard` implementing `Keyboard`.
- Getters: `device_info`, `get_version`, `get_profile`, `get_polling_rate`,
  `get_debounce`, `get_sleep_time`. Every setter returns `NotSupported`.
- Discovery seams you may change, and only these:
  - `HidDiscovery::list_devices` (`monsgeek-transport/src/discovery.rs`): add a
    branch that recognises a Beiying interface. Decide that from its HID report
    descriptor read from sysfs, never by opening the device, because VID:PID
    alone is ambiguous. RY acceptance stays the existing usage-page/usage check,
    unchanged; the descriptor only decides whether an interface is Beiying.
  - `DiscoveredDevice` (`monsgeek-transport/src/types.rs`): add a field saying
    which protocol discovery recognised. RY devices get the RY value.
  - `HidDiscovery::open_device`: return an error for a non-RY device instead of
    opening an RY transport on it.
  - `open_keyboard`: the new arm, matching on that field.
- Add a test with a captured RY descriptor next to the Beiying one, showing
  each is classified as its own protocol.
- RY-only commands on a Beiying board print the "does not support this
  command" message from `open_ry_keyboard`; nothing else changes for them.
- Gate: fixture of the vendor tool's connect handshake and each query; replay
  tests; transcript of `iot_driver info`, `profile`, `rate`, `debounce`,
  `sleep`.

### Phase 2: settings writes

- Setters for the phase-1 settings. Validate in plain units and reject what the
  device cannot represent (e.g. a single sleep timeout cannot hold separate
  Bluetooth and 2.4 GHz values; reject non-uniform `SleepTimeSettings`).
- Gate: fixture of the vendor tool making each change; replay tests; transcript
  showing `set-*` followed by the matching read returning the new value, and the
  value surviving a replug.

### Phase 3: layout as data

- Key positions and names in a new file under `data/` (for example
  `data/beiying_layouts.json`) keyed by the device's self-reported identity.
  Not in `devices.json` or `device_matrices.json`: `scripts/update-device-db.sh`
  regenerates those from upstream and would drop hand-written entries. One
  model's table must not be applied to every board of the protocol.
- Gate: the data file plus a test that resolves it from a fixture handshake.

### Phase 4: lighting

- Propose the trait extension in the PR description first: an optional
  lighting capability (effects listed by the backend, not RY's `LedMode`
  numbering) and a realtime per-key frame method keyed by the phase-3 layout.
- Once it exists, `audio_reactive` and `screen_capture` can drive any backend
  that has it. Moving those modules onto the capability is part of this phase.
- Gate: fixtures for effect changes and one streamed frame; replay tests;
  transcript or a short video.

### Phase 5: keymaps and Hall-effect settings

- Needs a key-identity type shared by both backends (RY uses matrix positions).
  Propose it before implementing.
- Actuation, rapid trigger, deadzones and live travel monitoring follow the
  same fixture → replay → transcript gate.

### Phase 6: calibration and factory reset

- Last, because they are the operations that can leave a board unusable.
  Calibration must report real progress from the device. Reset must name the
  exact command and scope from a capture of the vendor tool.

### TUI

The TUI still builds and holds an `Arc<RyKeyboard>`. Once phase 2 lands, its
connect path can move to `open_keyboard` and its device-info tab to
`Arc<dyn Keyboard>`; other tabs follow their phase.

## Not covered

A companion service that replaces a vendor's Windows helper (for example the
web configurator's localhost bridge) is outside this plan.
