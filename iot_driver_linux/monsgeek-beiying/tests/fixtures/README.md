# Beiying Phase 1 USB Capture Fixtures

## `beiying-connect.pcapng` / `beiying-connect.txt`

Captured via Linux `usbmon1` (device `38ee:0016`, USB device address `5`, bus 1)
while connecting the vendor WebHID configurator (`demo.hubx.pro` in Chromium).
Trimmed to frames `1..=47` from the raw capture (`beiying-connect.pcapng` in the
repository root during preparation) using `editcap` / `tshark -r ... -V -x`.

Important notes about the capture:
- Frames `1..=5`: USB device descriptor (`38ee:0016`, `bcdDevice = 0x0108`) and
  configuration descriptor (Interface 2 vendor HID, `wDescriptorLength = 61`,
  interrupt IN endpoint `0x83` and OUT endpoint `0x02`, 64-byte reports).
- Frames `7` & `9`: `0x82 0x01` request/reply returning the 6-byte hardware UUID
  `11 00 00 00 00 59` (`"110000000059"`).
- Frames `11` & `13`: `0x82 0x02` request/reply returning firmware version
  `08 01` (`0x0108` LE, matching Frame 1 `bcdDevice`).
- Frames `15..=41`: `0x82 0x03`, `0x82 0x04`, `0x82 0x06`, `0x82 0x08`,
  `0x82 0x09`, `0x82 0x0F`, and `0x84 0x11` capability/OS-mode queries.
- During the later part of the raw capture, background polling/daemons were active
  concurrently, and firmware replies on `0x82 0x02` occasionally returned
  `pkt[6] = 0x00` alongside `0x08 0x01` at `pkt[7..9]`.

## Vendor-tool query fixtures (`demo.hubx.pro` / `/tmp/hubx-final/hubx-base.*`)

All four `get-*` single-query fixtures below were trimmed using `tshark` from
`/tmp/hubx-base-raw.pcapng` (filtered copies `/tmp/hubx-base.pcapng` and
`/tmp/hubx-final/hubx-base.{pcapng,txt}`, matched by UUID `110000000059` / VID:PID
`38ee:0016` on Linux `usbmon1`, bus `1`, USB address `7`), captured while opening the
vendor WebHID configurator (`demo.hubx.pro` in Chromium) on the Basic Functions
(`"Funções básicas"`) page (showing **`4000 Hz`**), without clicking any configuration write
controls. Each fixture retains the USB Device/Configuration descriptors (raw Frames `1..=6`),
an `0x82 0x01` UUID handshake, and the single target command exchange.

- `get-polling-rate.pcapng` / `get-polling-rate.txt`
  - **Origin:** Trimmed from `/tmp/hubx-base-raw.pcapng` /
    `/tmp/hubx-final/hubx-base.txt` (raw Frames `1..=6`, `1157..=1160`,
    `1587..=1592`, and `3707..=3710`, matching `/tmp/hubx-final/hubx-base.pcapng`
    Frames `585..=588` at `1795130260.559533`–`260.560063`).
  - **Exchange:** Frame `15` (raw `3707`, `host -> 1.7.2`) sends `09 84 17 00 01 00 00 ... 5a`;
    Frame `17` (raw `3709`, `1.7.3 -> host`) replies `09 84 17 00 01 00 01 05 ... 54`
    (`pkt[6] = 0x01`, `pkt[7] = 0x05`, checksum `0x54`).
  - **Proven:** Matches `demo.hubx.pro` querying Basic Functions (`"Funções básicas"`) while
    displaying `4000 Hz`.

- `get-polling-rate-codes.pcapng` / `get-polling-rate-codes.txt` *(all 7 individually verified `0x84 0x17` cycles)*
  - **Origin:** Trimmed with `tshark` and concatenated in ascending Hz order via `mergecap -a`
    (`11K` `.pcapng`, `301K` `.txt`, 84 frames total, 28 `HID Data:` reports, zero `0x04 0x17` write
    packets) using the **post-reconnect read-only** `0x84 0x17` exchanges from the seven
    individually labelled `/tmp/hubx-final/hubx-poling*hz.{pcapng,txt}` captures across changing
    USB device addresses `30..=36`:
    - `125 Hz` (`0x03`, chk `0x56`): `/tmp/hubx-final/hubx-poling125hz` (`dev=29->30`; write `629`/`631`, imm. read `633`/`635`, post-reconnect read `1757`/`1759`; filtered `.pcapng` `106,107,116,117,161..164,1179..1182`) -> trimmed reply Frame `11`
    - `250 Hz` (`0x02`, chk `0x57`): `/tmp/hubx-final/hubx-poling250hz` (`dev=30->31`; write `411`/`415`, imm. read `417`/`419`, post-reconnect read `1387`/`1389`; filtered `.pcapng` `114,115,124,125,167..170,877..880`) -> trimmed reply Frame `23`
    - `500 Hz` (`0x01`, chk `0x58`): `/tmp/hubx-final/hubx-poling500hz` (`dev=31->32`; write `515`/`517`, imm. read `519`/`521`, post-reconnect read `1511`/`1513`; filtered `.pcapng` `98,99,108,109,151..154,861..864`) -> trimmed reply Frame `35`
    - `1000 Hz` (`0x00`, chk `0x59`): `/tmp/hubx-final/hubx-poling1000hz` (`dev=32->33`; write `565`/`569`, imm. read `575`/`581`, post-reconnect read `1685`/`1687`; filtered `.pcapng` `98,99,108,109,153..156,861..864`) -> trimmed reply Frame `47`
    - `2000 Hz` (`0x06`, chk `0x53`): `/tmp/hubx-final/hubx-poling2000hz` (`dev=33->34`; write `547`/`549`, imm. read `551`/`553`, post-reconnect read `1729`/`1731`; filtered `.pcapng` `98,99,102,103,147..150,927..930`) -> trimmed reply Frame `59`
    - `4000 Hz` (`0x05`, chk `0x54`): `/tmp/hubx-final/hubx-poling4000hz` (`dev=34->35`; write `909`/`911`, imm. read `913`/`915`, post-reconnect read `1785`/`1787`; filtered `.pcapng` `100,101,104,105,147..150,929..932`) -> trimmed reply Frame `71`
    - `8000 Hz` (`0x04`, chk `0x55`): `/tmp/hubx-final/hubx-poling8000hz` (`dev=35->36`; write `495`/`497`, imm. read `499`/`501`, post-reconnect read `1301`/`1303`; filtered `.pcapng` `104,105,108,109,151..154,861..864`) -> trimmed reply Frame `83`
  - **Proven & Inconsistency Note:** Every single capture independently proves both the action
    (`0x04 0x17` write -> `0x04 0x17` echo ACK on `ep 0x83` -> immediate `0x84 0x17` read ->
    USB re-enumeration) and the post-reconnect read-only `0x84 0x17` reply. Note that the earlier
    manually typed code list reported seven rates (`125..8000 Hz`) while omitting `0x05`; packet
    captures in `hubx-base` (`dev=7` + screen `4000 Hz`), `hubx-profile` (`dev=12->13`), and
    `hubx-poling4000hz` (`dev=34->35`) all prove `0x05 -> 4000 Hz`. All 7 codes (`0x00..=0x06`)
    are now mapped in `parse_polling_rate_reply`; any other code returns
    `KeyboardError::UnexpectedResponse`.

- `get-profile.pcapng` / `get-profile.txt` *(Profile 1 / `Profile(0)` -> `pkt[7] = 0x00`)*
  - **Origin:** Trimmed from `/tmp/hubx-base-raw.pcapng` /
    `/tmp/hubx-final/hubx-base.txt` (raw Frames `1..=6`, `1587..=1592`, and
    `2063..=2066`, matching `/tmp/hubx-final/hubx-base.pcapng` Frames `173..=176`
    at `1795130255.404661`–`255.405120`).
  - **Exchange:** Frame `11` (raw `2063`, `host -> 1.7.2`) sends `09 90 00 00 01 00 00 ... 65`;
    Frame `13` (raw `2065`, `1.7.3 -> host`) replies `09 90 00 00 01 00 00 00 ... 65`
    (`pkt[6] = 0x00`, `pkt[7] = 0x00`, checksum `0x65`).
  - **Proven (with `get-profile-p2`):** Identical `pkt[7] = 0x00` reply was observed across all
    seven `hubx-poling*hz` captures while Profile 1 was active (`125Hz` `779`/`781`, `250Hz`
    `555`/`557`, `500Hz` `843`/`845`, `1000Hz` `863`/`867`, `2000Hz` `719`/`721`, `4000Hz`
    `1045`/`1047`, `8000Hz` `633`/`635`). Like `0x82 0x02` (`FirmwareVersion`), `v108` leaves
    `pkt[6] = 0x00` while storing the 0-based active profile index in `pkt[7]`.

- `get-profile-p2.pcapng` / `get-profile-p2.txt` *(Profile 2 / `Profile(1)` -> `pkt[7] = 0x01`)*
  - **Origin:** Trimmed (`1.8K` `.pcapng`, `43K` `.txt`, 12 frames total, 4 `HID Data:` reports,
    zero `0x10 0x00` write frames) from `/tmp/hubx-final/hubx-trocaperfil.pcapng` (`dev=36`,
    frames `1,2,5,6,17..20,99..102`, corresponding to `/tmp/hubx-trocaperfil-raw.pcapng` /
    `/tmp/hubx-final/hubx-trocaperfil.txt` after switching Profile 1 -> Profile 2 via write
    `0x10 0x00` Frame `1047` + ACK `1049` -> read `0x90 0x00` Frames `1051`/`1053`).
  - **Exchange:** Frame `9` (filtered `99`, raw `1051`, `host -> 1.36.2`) sends
    `09 90 00 00 01 00 00 ... 65`; Frame `11` (filtered `101`, raw `1053`, `1.36.3 -> host`)
    replies `09 90 00 00 01 00 00 01 ... 64` (`pkt[6] = 0x00`, `pkt[7] = 0x01`, checksum `0x64`).
  - **Proven:** Confirms `0x90 0x00` `pkt[7]` tracks the 0-based active profile (`0x00` = Profile 1,
    `0x01` = Profile 2), cross-checked against `/tmp/hubx-profile-change-raw.pcapng`
    (`0x01, 0x00, 0x01, 0x02`). `BeiyingKeyboard::get_profile` (`parse_profile_reply`) is now
    implemented and covered by replay tests for both fixtures.

- `get-sleep-time.pcapng` / `get-sleep-time.txt` *(opaque wire bytes; unsupported on the tested wired TAC75 HE)*
  - **Origin:** Trimmed from `/tmp/hubx-base-raw.pcapng` /
    `/tmp/hubx-final/hubx-base.txt` (raw Frames `1..=6`, `1587..=1592`, and
    `3687..=3690`, matching `/tmp/hubx-final/hubx-base.pcapng` Frames `565..=568`
    at `1795130260.550413`–`260.551058`).
  - **Exchange:** Frame `11` (raw `3687`, `host -> 1.7.2`) sends `09 84 13 00 01 00 00 ... 5e`;
    Frame `13` (raw `3689`, `1.7.3 -> host`) replies `09 84 13 00 01 00 02 00 b4 ... a8`
    (`pkt[6] = 0x02`, `pkt[7..9] = [0x00, 0xb4]`, checksum `0xa8`).
  - **Status:** The owner confirms that the wired TAC75 HE tested here has no configurable
    sleep-time feature corresponding to `SleepTimeSettings`. The captured `00 b4` has no confirmed
    meaning and is deliberately not mapped to wireless idle/deep-sleep fields or LED timeout.
    `/tmp/hubx-final/hubx-sleep-mode.{pcapng,txt}` (`Fn+L` on `dev=29`) contains only `ep 0x81`
    HID key reports and heartbeat frames, with zero `0x84 0x13` or `0x04 0x13` traffic.
    `get_sleep_time` remains `KeyboardError::NotSupported` for this device.

- `get-debounce.pcapng` / `get-debounce.txt` *(zero-length reply; unsupported on the tested TAC75 HE)*
  - **Origin:** Trimmed from `/tmp/hubx-base-raw.pcapng` /
    `/tmp/hubx-final/hubx-base.txt` (raw Frames `1..=6`, `1587..=1592`, and
    `3703..=3706`, matching `/tmp/hubx-final/hubx-base.pcapng` Frames `581..=584`
    at `1795130260.557853`–`260.558467`).
  - **Exchange:** Frame `11` (raw `3703`, `host -> 1.7.2`) sends `09 84 1e 00 01 00 00 ... 53`;
    Frame `13` (raw `3705`, `1.7.3 -> host`) replies `09 84 1e 00 01 00 00 ... 53`
    (`pkt[6] = 0x00`, all payload bytes `0x00`; contrast with `0x84 0x11` and
    `0x84 0x15` returning `pkt[6] = 0x01, pkt[7] = 0x00`).
  - **Status:** The owner confirms that the tested TAC75 HE has no traditional debounce setting.
    The `0x84 0x1e` reply is empty (also across all 11 `0x84 0x1e` and 11 `0x84 0x1d` exchanges in
    `hubx-profile.txt`). Hall-effect deadzone uses distinct `0x16 0x00` / `0x96 0x00` commands and
    is not a debounce value. `BeiyingKeyboard::get_debounce` remains
    `KeyboardError::NotSupported`.

**Device-scope note:** These two unsupported getters reflect the owner's confirmation for the
wired TAC75 HE / firmware v108 tested here. Akko's public pages describe the model as wired USB-C
and document magnetic deadzone/RT Stabilizer features, but do not explicitly state the absence of
debounce or sleep-time controls. See [TAC75 HE product page](https://en.akkogear.com/product/tac75-he-the-kings-avatar-magnetic-switch-keyboard/)
and [magnetic-switch knowledge base](https://en.akkogear.com/akko-magnetic-switches-knowledge-base/).

## Audit of `/tmp/hubx-final/` and `/tmp/hubx-*-raw.pcapng` (not committed)

Kept outside the repository:

- **`/tmp/hubx-final/hubx-poling{125,250,500,1000,2000,4000,8000}hz.{pcapng,txt}`:**
  Each individual labelled capture shows write `0x04 0x17` (`ep 0x02`), write confirmation ACK
  `0x04 0x17` (`ep 0x83`), immediate `0x84 0x17` read, USB re-enumeration (`dev=29->30->...->36`),
  and post-reconnect read-only `0x84 0x17`, proving all 7 rates (`0x03=125`, `0x02=250`, `0x01=500`,
  `0x00=1000`, `0x06=2000`, `0x05=4000`, `0x04=8000 Hz`). Also clarifies the earlier manual list's
  omission of `0x05`.
- **`/tmp/hubx-final/hubx-profile.{pcapng,txt}`** (`/tmp/hubx-profile-raw.pcapng`):
  Each USB device address transition **`7 -> 8 -> 9 -> 10 -> 11 -> 12 -> 13 -> 14 -> 15`** was
  triggered by a polling-rate **write** `0x04 0x17` followed by its write ACK, immediate read, and
  post-reconnect read in sequence **`0x03 -> 0x02 -> 0x01 -> 0x00 -> 0x06 -> 0x05 -> 0x04 -> 0x03 -> 0x02`**,
  confirming `0x05` (raw Frames `39695`/`39697`/`39699`/`39701` on `dev=12` and `40061`/`40063` on
  `dev=13`) independently of `hubx-poling4000hz`.
- **`/tmp/hubx-final/hubx-trocaperfil.{pcapng,txt}`** (`/tmp/hubx-trocaperfil-raw.pcapng`, `dev=36`,
  `22:28`): Clean single-action Profile 1 -> Profile 2 capture (`0x10 0x00` write Frame `1047`, write
  ACK `1049`, `0x90 0x00` read `1051`/`1053` returning `pkt[6]=0x00, pkt[7]=0x01`, checksum `0x64`),
  trimmed into `tests/fixtures/get-profile-p2.{pcapng,txt}` (read-only `0x90 0x00` without writes).
- **`/tmp/hubx-final/hubx-profile-change.{pcapng,txt}`** (`/tmp/hubx-profile-change-raw.pcapng`,
  `dev=29`, `21:37`): Verified from on-disk timestamps and packets NOT to have been overwritten by
  deadzone writes (`0` packets of `0x16 0x00`; deadzone landed in `hubx-debounce-raw.pcapng` at
  `21:40`). Contains four `0x10 0x00` profile writes (`0x01`, `0x00`, `0x01`, `0x02`) followed by
  `0x90 0x00` replies (`pkt[6]=0x00`, `pkt[7]=0x01, 0x00, 0x01, 0x02`, Frames `2069`, `3405`,
  `4565`, `5583`), corroborating `0x90 0x00` slot numbering alongside `hubx-trocaperfil`.

## Out of scope for Phase 1 (future phases per `docs/BACKENDS.md`)

- **Phase 4 (Lighting):**
  - `/tmp/hubx-final/hubx-brilho.{pcapng,txt}` (`/tmp/hubx-brilho-raw.pcapng`, `dev=29`, `21:50`):
    `Fn` brightness shortcuts emitted 8 unsolicited device-to-host notifications
    `09 fe 0b 02 <val>` (`05, 0a, 0f, 14, 0f, 0a, 05, 00`) on `ep 0x83`; zero host writes.
  - `/tmp/hubx-final/hubx-velocidade-brilho.{pcapng,txt}` (`/tmp/hubx-velocidade-brilho-raw.pcapng`,
    `dev=29`, `21:47`): `Fn` speed shortcuts emitted 10 unsolicited notifications
    `09 fe 0b 03 <val>` (`01, 02, 03, 04, 04, 03, 02, 01, 00, 00`) on `ep 0x83`; zero host writes.
  - `/tmp/hubx-final/hubx-lado-brilho.{pcapng,txt}` (`/tmp/hubx-lado-brilho-raw.pcapng`,
    `dev=29`, `21:54`): only USB/HID heartbeat and `Ctrl+C` on `ep 0x81`; no `0x09` lighting traffic.
- **Phase 5 (Hall-Effect Deadzones & Layers):**
  - `/tmp/hubx-final/hubx-configzonamorta.{pcapng,txt}` (`/tmp/hubx-configzonamorta-raw.pcapng`,
    `dev=36`, `22:32`): `96` writes `0x16 0x00` + `96` ACKs + `96` reads `0x96 0x00` across 8 bulk
    all-key batches (random values across all selected keys; zero `0x84 0x1e` debounce packets). Not
    equated with `get_debounce`; no per-key semantics deduced.
  - `/tmp/hubx-final/hubx-debounce.{pcapng,txt}` (`/tmp/hubx-debounce-raw.pcapng`, `dev=29`, `21:40`):
    despite the filename `debounce`, contains `84` writes `0x16 0x00` + `84` ACKs + `84` reads
    `0x96 0x00` across 7 bulk deadzone batches, zero `0x84 0x1e` packets.
  - `/tmp/hubx-final/hubx-camadapadrao.{pcapng,txt}`, `hubx-camadafn.{pcapng,txt}`, and
    `hubx-camadafn1.{pcapng,txt}` (`dev=36`, `22:35`): only background heartbeat (`0x82*`/`0x84 11`)
    and `ep 0x81` `Alt+Tab`/`Ctrl+C`; layer tab switches in `demo.hubx.pro` emit no USB packets
    (layers are pre-loaded via `0x83`/`0x9a`).
- **Unrelated shortcut (`Fn+L`):**
  - `/tmp/hubx-final/hubx-sleep-mode.{pcapng,txt}` (`/tmp/hubx-sleep-mode-raw.pcapng`, `dev=29`,
    `21:57`): contains only heartbeat and `ep 0x81` `Ctrl+C` (`01000000` / `01000600`); zero `0x84 0x13`,
    `0x04 0x13`, or `0xfe` frames.

## Hardware transcript & Phase 1 status

- **Owner-confirmed unsupported getters (`get_debounce`, `get_sleep_time`):** Both remain
  `KeyboardError::NotSupported` for the tested wired TAC75 HE (`v108`), matching owner confirmation
  that this board has no traditional debounce setting or `SleepTimeSettings` timer. Hall-effect
  deadzone (`0x16 0x00` / `0x96 0x00`, Phase 5) is separate and is not mapped to debounce, and the
  opaque `0x84 0x13` reply (`00 b4`) is not mapped to `SleepTimeSettings` or LED timeout.
- **Awake hardware transcript (`/tmp/beiying-phase1-terminal-final.txt`):** Recorded with the
  local release binary on `beiying-backend` against the connected TAC75 HE (`38ee:0016`), showing
  `Firmware: v108 (raw 0x0108, dec 264)`, `Profile: 1` (UI Profile 2),
  `Polling rate: 8000 (8kHz)`, `NotSupported` for `debounce` and `sleep`, and
  `Akko Keyboard does not support this command (RongYuan-protocol keyboards only)` for `led`.
- **Expected `iot_driver list` output in Phase 1:** Even while direct Beiying queries work,
  `iot_driver list` prints `#0  RAID 75 HE           usb` followed by
  `#0 — not responding (may be asleep)` because `HidDiscovery::probe_devices()` calls
  `HidDiscovery::open_device()` (limited to `KeyboardProtocol::RongYuan` under the `docs/BACKENDS.md`
  Phase 1 seams), leaving `responsive = false` (`device_id = None`) and falling back to the first
  `38EE:0016` name in `devices.json`. Direct commands go through `open_keyboard()` ->
  `BeiyingKeyboard::open()`.
