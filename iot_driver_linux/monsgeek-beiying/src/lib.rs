//! Shenzhen Beiying / BYTech (`Report ID 0x09`) keyboard backend.
//!
//! Covers the vendor-tool-verified Phase 1 getters from `docs/BACKENDS.md`
//! (`device_info`, `get_version`, `get_profile`, `get_polling_rate` for all
//! seven captured Hz wire codes `0x00..=0x06`) backed by captured USB fixtures
//! in `tests/fixtures/`. The owner-confirmed unsupported debounce and sleep-time
//! settings on the tested wired TAC75 HE (`get_debounce`, `get_sleep_time`),
//! plus all write/reset methods, return [`KeyboardError::NotSupported`].

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};
use monsgeek_keyboard::{FirmwareVersion, Keyboard, KeyboardError, SleepTimeSettings};
use monsgeek_transport::protocol::Profile;
use monsgeek_transport::{TransportDeviceInfo, TransportError};

/// HID Report ID used by Shenzhen Beiying / BYTech vendor reports on Interface 2.
///
/// Sources:
/// - `../monsgeek-transport/tests/fixtures/beiying_tac75_if2.hex` (`85 09`, Report ID 0x09,
///   `75 08 95 3F`, 63-byte Input `81 02` and Output `91 02`).
/// - `tests/fixtures/beiying-connect.txt` Frame 5 (IF2 wDescriptorLength = 61, endpoints
///   `0x83` IN / `0x02` OUT, `wMaxPacketSize = 64`) and Frames 7–47 (`HID Data: 09...`).
pub const REPORT_ID: u8 = 0x09;

/// Total HID interrupt report size on the wire, including [`REPORT_ID`].
///
/// Source: `tests/fixtures/beiying-connect.txt` Frames 7–47 (`URB length [bytes]: 64`,
/// 1-byte Report ID + 63-byte report body).
pub const REPORT_LEN: usize = 64;

/// Fixed packet header length before payload bytes (`pkt[0..7]`).
///
/// Layout observed across `tests/fixtures/beiying-connect.txt` Frames 7–47:
/// - `pkt[0]`: `REPORT_ID` (`0x09`)
/// - `pkt[1]`: command byte (e.g. `0x82`)
/// - `pkt[2]`: subcommand / parameter byte
/// - `pkt[3]`: reserved / flags byte (`0x00` in Frames 7–41)
/// - `pkt[4]`: packet count (`0x01` in Frames 7–41)
/// - `pkt[5]`: packet index (`0x00` in Frames 7–41; `0x34` only in `0x82 0x0F` Frame 35)
/// - `pkt[6]`: payload byte count (e.g. `0x06` in Frame 9, `0x00` in Frames 7 & 11)
pub const HEADER_LEN: usize = 7;

/// Trailer checksum index (`pkt[63]`).
///
/// Source: `tests/fixtures/beiying-connect.txt` Frames 7–47 (e.g. Frame 7 ends in `0x72`,
/// Frame 9 ends in `0x02`, Frame 11 ends in `0x71`, Frame 13 ends in `0x68`).
pub const CHECKSUM_INDEX: usize = REPORT_LEN - 1;

/// Maximum payload bytes per single 64-byte packet (`pkt[7..63]`).
///
/// Source: derived from `CHECKSUM_INDEX - HEADER_LEN` (`63 - 7 = 56`) in
/// `tests/fixtures/beiying-connect.txt` Frames 7–47.
pub const MAX_PAYLOAD_LEN: usize = CHECKSUM_INDEX - HEADER_LEN;

/// Length in bytes of the device UUID returned by `0x82 0x01`.
///
/// Source: `tests/fixtures/beiying-connect.txt` Frame 9 (`pkt[6] = 0x06`,
/// `pkt[7..13] = 11 00 00 00 00 59`).
pub const UUID_LEN: usize = 6;

/// Device info / capability query command.
///
/// Source: `tests/fixtures/beiying-connect.txt` Frames 7–37 (`09 82 ...`).
pub const CMD_DEVICE_QUERY: u8 = 0x82;

/// Subcommand of [`CMD_DEVICE_QUERY`] that returns the 6-byte device UUID.
///
/// Source: `tests/fixtures/beiying-connect.txt` Frames 7 (`09 82 01 ... 72`)
/// and Frame 9 (`09 82 01 00 01 00 06 11 00 00 00 00 59 ... 02`).
pub const SUBCMD_UUID: u8 = 0x01;

/// Subcommand of [`CMD_DEVICE_QUERY`] that returns the 2-byte firmware version.
///
/// Sources:
/// - `tests/fixtures/beiying-connect.txt` Frame 11 (`09 82 02 00 01 00 ... 71`)
///   and Frame 13 (`09 82 02 00 01 00 00 08 01 ... 68`, little-endian `0x0108`).
/// - Cross-checked with `tests/fixtures/beiying-connect.txt` Frame 1 USB Device
///   Descriptor (`bcdDevice = 08 01` -> `0x0108`).
pub const SUBCMD_FIRMWARE_VERSION: u8 = 0x02;

/// System parameter read command (`0x84`).
///
/// Sources:
/// - `tests/fixtures/beiying-connect.txt` Frames 39 & 41 (`0x84 0x11` OS mode).
/// - `tests/fixtures/get-polling-rate.txt` Frames 15 & 17 (`0x84 0x17`, trimmed from
///   `demo.hubx.pro` capture `/tmp/hubx-base-raw.pcapng` Frames 3707 & 3709,
///   `/tmp/hubx-base.pcapng` Frames 287 & 288).
pub const CMD_SYSTEM_PARAM_QUERY: u8 = 0x84;

/// Parameter of [`CMD_SYSTEM_PARAM_QUERY`] that returns the 1-byte USB polling
/// rate code (`0x17` = 23).
///
/// Source: `tests/fixtures/get-polling-rate.txt` Frame 15 (`09 84 17 00 01 00 ... 5a`)
/// and Frame 17 (`09 84 17 00 01 00 01 05 ... 54`: `pkt[6] = 0x01`, `pkt[7] = 0x05`).
pub const PARAM_POLLING_RATE: u8 = 0x17;

/// Active profile query command (`0x90`).
///
/// Sources:
/// - `tests/fixtures/get-profile.txt` Frames 11 & 13 (`09 90 00 00 01 00 00 ... 65`,
///   trimmed from `/tmp/hubx-base-raw.pcapng` / `/tmp/hubx-final/hubx-base.txt`
///   Frames 2063 & 2065, Profile 1 / `Profile(0)` -> `pkt[7] = 0x00`; identical
///   reply across all 7 `hubx-poling*hz` captures on Profile 1).
/// - `tests/fixtures/get-profile-p2.txt` Frames 9 & 11 (`09 90 00 00 01 00 00 01 ... 64`,
///   trimmed from `/tmp/hubx-trocaperfil-raw.pcapng` / `/tmp/hubx-final/hubx-trocaperfil.txt`
///   Frames 1051 & 1053 after Profile 1 -> Profile 2 switch write `0x10 0x00`
///   Frames 1047 & 1049; filtered `/tmp/hubx-final/hubx-trocaperfil.pcapng` Frames 99 & 101).
/// - Cross-checked against `/tmp/hubx-profile-change-raw.pcapng` /
///   `/tmp/hubx-final/hubx-profile-change.txt` Frames 2069 (`0x01`), 3405 (`0x00`),
///   4565 (`0x01`), and 5583 (`0x02`).
pub const CMD_PROFILE_QUERY: u8 = 0x90;

/// Subcommand of [`CMD_PROFILE_QUERY`] that returns the 0-based active profile slot.
///
/// Firmware `v108` leaves `pkt[6] = 0x00` and writes the 0-based profile index at
/// `pkt[7]` (same unpopulated length-byte quirk as [`SUBCMD_FIRMWARE_VERSION`]).
pub const SUBCMD_ACTIVE_PROFILE: u8 = 0x00;

/// Captured [`PARAM_POLLING_RATE`] (`0x84 0x17`) wire code for `1000 Hz`.
///
/// Sources: `tests/fixtures/get-polling-rate-codes.txt` Frame 47 (`pkt[6] = 0x01`,
/// `pkt[7] = 0x00`, checksum `0x59`); trimmed from `/tmp/hubx-final/hubx-poling1000hz.pcapng`
/// post-reconnect `dev=33` (`/tmp/hubx-final/hubx-poling1000hz.txt` Frames 1685 & 1687).
pub const POLLING_RATE_CODE_1000HZ: u8 = 0x00;

/// Captured [`PARAM_POLLING_RATE`] (`0x84 0x17`) wire code for `500 Hz`.
///
/// Sources: `tests/fixtures/get-polling-rate-codes.txt` Frame 35 (`pkt[6] = 0x01`,
/// `pkt[7] = 0x01`, checksum `0x58`); trimmed from `/tmp/hubx-final/hubx-poling500hz.pcapng`
/// post-reconnect `dev=32` (`/tmp/hubx-final/hubx-poling500hz.txt` Frames 1511 & 1513).
pub const POLLING_RATE_CODE_500HZ: u8 = 0x01;

/// Captured [`PARAM_POLLING_RATE`] (`0x84 0x17`) wire code for `250 Hz`.
///
/// Sources: `tests/fixtures/get-polling-rate-codes.txt` Frame 23 (`pkt[6] = 0x01`,
/// `pkt[7] = 0x02`, checksum `0x57`); trimmed from `/tmp/hubx-final/hubx-poling250hz.pcapng`
/// post-reconnect `dev=31` (`/tmp/hubx-final/hubx-poling250hz.txt` Frames 1387 & 1389).
pub const POLLING_RATE_CODE_250HZ: u8 = 0x02;

/// Captured [`PARAM_POLLING_RATE`] (`0x84 0x17`) wire code for `125 Hz`.
///
/// Sources: `tests/fixtures/get-polling-rate-codes.txt` Frame 11 (`pkt[6] = 0x01`,
/// `pkt[7] = 0x03`, checksum `0x56`); trimmed from `/tmp/hubx-final/hubx-poling125hz.pcapng`
/// post-reconnect `dev=30` (`/tmp/hubx-final/hubx-poling125hz.txt` Frames 1757 & 1759).
pub const POLLING_RATE_CODE_125HZ: u8 = 0x03;

/// Captured [`PARAM_POLLING_RATE`] (`0x84 0x17`) wire code for `8000 Hz`.
///
/// Sources: `tests/fixtures/get-polling-rate-codes.txt` Frame 83 (`pkt[6] = 0x01`,
/// `pkt[7] = 0x04`, checksum `0x55`); trimmed from `/tmp/hubx-final/hubx-poling8000hz.pcapng`
/// post-reconnect `dev=36` (`/tmp/hubx-final/hubx-poling8000hz.txt` Frames 1301 & 1303).
pub const POLLING_RATE_CODE_8000HZ: u8 = 0x04;

/// Captured [`PARAM_POLLING_RATE`] (`0x84 0x17`) wire code for `4000 Hz`.
///
/// Sources: `tests/fixtures/get-polling-rate.txt` Frame 17 (`hubx-base.txt` `dev=7`
/// Frames 3707 & 3709 + Basic Functions (`"Funções básicas"`) screen showing `4000 Hz`) and
/// `tests/fixtures/get-polling-rate-codes.txt` Frame 71 (`pkt[6] = 0x01`, `pkt[7] = 0x05`,
/// checksum `0x54`; trimmed from `/tmp/hubx-final/hubx-poling4000hz.pcapng` post-reconnect
/// `dev=35`, `/tmp/hubx-final/hubx-poling4000hz.txt` Frames 1785 & 1787).
pub const POLLING_RATE_CODE_4000HZ: u8 = 0x05;

/// Captured [`PARAM_POLLING_RATE`] (`0x84 0x17`) wire code for `2000 Hz`.
///
/// Sources: `tests/fixtures/get-polling-rate-codes.txt` Frame 59 (`pkt[6] = 0x01`,
/// `pkt[7] = 0x06`, checksum `0x53`); trimmed from `/tmp/hubx-final/hubx-poling2000hz.pcapng`
/// post-reconnect `dev=34` (`/tmp/hubx-final/hubx-poling2000hz.txt` Frames 1729 & 1731).
pub const POLLING_RATE_CODE_2000HZ: u8 = 0x06;

/// Default HID read timeout in milliseconds for synchronous query replies.
pub const DEFAULT_TIMEOUT_MS: i32 = 300;

const MAX_DRAIN_REPORTS: usize = 32;

/// Compute the 1-byte Beiying trailer checksum `!(sum(report[0..63]) as u8)`.
///
/// Verified against every frame in `tests/fixtures/beiying-connect.txt` Frames 7–47
/// (e.g. Frame 7: `!(0x09 + 0x82 + 0x01 + 0x01) = !0x8D = 0x72`).
#[inline]
pub fn checksum(report: &[u8; REPORT_LEN]) -> u8 {
    let sum = report[..CHECKSUM_INDEX]
        .iter()
        .fold(0u8, |acc, &b| acc.wrapping_add(b));
    !sum
}

/// Write [`checksum`] into `report[CHECKSUM_INDEX]`.
#[inline]
pub fn apply_checksum(report: &mut [u8; REPORT_LEN]) {
    report[CHECKSUM_INDEX] = checksum(report);
}

/// Return `true` iff `report` is at least 64 bytes and its trailer checksum matches.
#[inline]
pub fn verify_checksum(report: &[u8]) -> bool {
    let Some(fixed) = report.get(..REPORT_LEN) else {
        return false;
    };
    let sum = fixed[..CHECKSUM_INDEX]
        .iter()
        .fold(0u8, |acc, &b| acc.wrapping_add(b));
    fixed[CHECKSUM_INDEX] == !sum
}

/// Build a checksummed 64-byte Report ID `0x09` packet.
///
/// Packet layout matches `tests/fixtures/beiying-connect.txt` Frames 7–47.
/// Payloads exceeding [`MAX_PAYLOAD_LEN`] are rejected with [`KeyboardError::InvalidParameter`].
pub fn build_report(
    cmd: u8,
    subcmd: u8,
    b3: u8,
    total_pkts: u8,
    pkt_index: u8,
    payload: &[u8],
) -> Result<[u8; REPORT_LEN], KeyboardError> {
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(KeyboardError::InvalidParameter(format!(
            "Beiying payload length {} exceeds max {MAX_PAYLOAD_LEN}",
            payload.len()
        )));
    }
    let mut pkt = [0u8; REPORT_LEN];
    pkt[0] = REPORT_ID;
    pkt[1] = cmd;
    pkt[2] = subcmd;
    pkt[3] = b3;
    pkt[4] = total_pkts;
    pkt[5] = pkt_index;
    pkt[6] = payload.len() as u8;
    pkt[HEADER_LEN..HEADER_LEN + payload.len()].copy_from_slice(payload);
    apply_checksum(&mut pkt);
    Ok(pkt)
}

/// Build a single-packet zero-payload query (`total_pkts = 1`, `pkt_index = 0`).
///
/// Exact match with `tests/fixtures/beiying-connect.txt` Frame 7 (`0x82 0x01`)
/// and Frame 11 (`0x82 0x02`).
pub fn build_query_report(cmd: u8, subcmd: u8) -> [u8; REPORT_LEN] {
    build_report(cmd, subcmd, 0, 1, 0, &[]).expect("empty payload fits in single report")
}

/// Parse the 6-byte hardware UUID from a `0x82 0x01` reply (`tests/fixtures/beiying-connect.txt`, Frame 9).
pub fn parse_uuid_reply(report: &[u8]) -> Result<[u8; UUID_LEN], KeyboardError> {
    validate_reply_header(report, CMD_DEVICE_QUERY, SUBCMD_UUID)?;
    let declared_len = report[6] as usize;
    if declared_len != UUID_LEN {
        return Err(KeyboardError::UnexpectedResponse(format!(
            "Expected {UUID_LEN}-byte UUID payload in 0x82 0x01 reply, got {declared_len}"
        )));
    }
    let mut uuid = [0u8; UUID_LEN];
    uuid.copy_from_slice(&report[HEADER_LEN..HEADER_LEN + UUID_LEN]);
    if uuid == [0u8; UUID_LEN] {
        return Err(KeyboardError::UnexpectedResponse(
            "Beiying device returned all-zero UUID in 0x82 0x01 reply".into(),
        ));
    }
    Ok(uuid)
}

/// Parse the 2-byte little-endian firmware version from a `0x82 0x02` reply
/// (`tests/fixtures/beiying-connect.txt`, Frame 13).
///
/// Accepts `pkt[6] == 0` (observed in Frame 13: `09 82 02 00 01 00 00 08 01 ... 68`)
/// as well as `pkt[6] >= 2` (observed when the firmware populates the length byte),
/// reading `u16::from_le_bytes([report[7], report[8]])`.
pub fn parse_firmware_version_reply(report: &[u8]) -> Result<FirmwareVersion, KeyboardError> {
    validate_reply_header(report, CMD_DEVICE_QUERY, SUBCMD_FIRMWARE_VERSION)?;
    let declared_len = report[6];
    if declared_len != 0 && declared_len < 2 {
        return Err(KeyboardError::UnexpectedResponse(format!(
            "Unexpected payload length {declared_len} in 0x82 0x02 firmware reply"
        )));
    }
    let raw = u16::from_le_bytes([report[HEADER_LEN], report[HEADER_LEN + 1]]);
    if raw == 0 {
        return Err(KeyboardError::UnexpectedResponse(
            "Beiying device returned zero firmware version in 0x82 0x02 reply".into(),
        ));
    }
    Ok(FirmwareVersion::new(raw))
}

/// Parse the USB polling rate (Hz) from a `0x84 0x17` reply
/// (`tests/fixtures/get-polling-rate.txt` Frame 17 and
/// `tests/fixtures/get-polling-rate-codes.txt` Frames 11, 23, 35, 47, 59, 71, 83).
pub fn parse_polling_rate_reply(report: &[u8]) -> Result<u16, KeyboardError> {
    validate_reply_header(report, CMD_SYSTEM_PARAM_QUERY, PARAM_POLLING_RATE)?;
    let declared_len = report[6] as usize;
    if declared_len != 1 {
        return Err(KeyboardError::UnexpectedResponse(format!(
            "Expected 1-byte payload in 0x84 0x17 polling rate reply, got {declared_len}"
        )));
    }
    match report[HEADER_LEN] {
        POLLING_RATE_CODE_1000HZ => Ok(1000),
        POLLING_RATE_CODE_500HZ => Ok(500),
        POLLING_RATE_CODE_250HZ => Ok(250),
        POLLING_RATE_CODE_125HZ => Ok(125),
        POLLING_RATE_CODE_8000HZ => Ok(8000),
        POLLING_RATE_CODE_4000HZ => Ok(4000),
        POLLING_RATE_CODE_2000HZ => Ok(2000),
        other => Err(KeyboardError::UnexpectedResponse(format!(
            "Uncaptured Beiying polling rate wire code 0x{other:02X} in 0x84 0x17 reply"
        ))),
    }
}

/// Parse the 0-based active [`Profile`] from a `0x90 0x00` reply
/// (`tests/fixtures/get-profile.txt` Frame 13 and
/// `tests/fixtures/get-profile-p2.txt` Frame 11).
///
/// Accepts `pkt[6] == 0` (observed on firmware `v108` across Profile 1 `0x00`,
/// Profile 2 `0x01`, and Profile 3 `0x02`) as well as populated `pkt[6] == 1`,
/// reading the 0-based profile slot at `pkt[7]` (`report[HEADER_LEN]`).
pub fn parse_profile_reply(report: &[u8]) -> Result<Profile, KeyboardError> {
    validate_reply_header(report, CMD_PROFILE_QUERY, SUBCMD_ACTIVE_PROFILE)?;
    let declared_len = report[6];
    if declared_len != 0 && declared_len != 1 {
        return Err(KeyboardError::UnexpectedResponse(format!(
            "Unexpected payload length {declared_len} in 0x90 0x00 profile reply"
        )));
    }
    let raw = report[HEADER_LEN];
    Profile::try_from(raw).map_err(|msg| {
        KeyboardError::UnexpectedResponse(format!(
            "Invalid Beiying profile index 0x{raw:02X} in 0x90 0x00 reply: {msg}"
        ))
    })
}

fn validate_reply_header(report: &[u8], cmd: u8, subcmd: u8) -> Result<(), KeyboardError> {
    if report.len() < REPORT_LEN {
        return Err(KeyboardError::UnexpectedResponse(format!(
            "Short Beiying report: {} bytes (expected {REPORT_LEN})",
            report.len()
        )));
    }
    if !verify_checksum(report) {
        return Err(KeyboardError::Transport(TransportError::ChecksumError));
    }
    if report[0] != REPORT_ID || report[1] != cmd || report[2] != subcmd {
        return Err(KeyboardError::UnexpectedResponse(format!(
            "Unexpected Beiying reply header {:02X} {:02X} {:02X} (expected {:02X} {:02X} {:02X})",
            report[0], report[1], report[2], REPORT_ID, cmd, subcmd
        )));
    }
    if report[4] != 1 || report[5] != 0 {
        return Err(KeyboardError::UnexpectedResponse(format!(
            "Unsupported multi-packet reply framing total={} index={} for {:02X} {:02X}",
            report[4], report[5], cmd, subcmd
        )));
    }
    Ok(())
}

/// Raw 64-byte HID report I/O seam so replay tests drive the real backend logic
/// without opening `/dev/hidraw*`.
pub trait ReportIo: Send + Sync {
    /// Send one 64-byte Output report (`pkt[0] == 0x09`).
    fn write_report(&self, report: &[u8; REPORT_LEN]) -> Result<(), KeyboardError>;

    /// Read one Input report with a timeout in milliseconds; returns `0` on timeout.
    fn read_report(
        &self,
        buf: &mut [u8; REPORT_LEN],
        timeout_ms: i32,
    ) -> Result<usize, KeyboardError>;
}

/// Hardware `hidapi` implementation of [`ReportIo`].
pub struct HidApiIo {
    device: Mutex<HidDevice>,
}

impl HidApiIo {
    /// Wrap an already-opened Beiying vendor interface [`HidDevice`].
    pub fn new(device: HidDevice) -> Self {
        Self {
            device: Mutex::new(device),
        }
    }
}

impl ReportIo for HidApiIo {
    fn write_report(&self, report: &[u8; REPORT_LEN]) -> Result<(), KeyboardError> {
        let guard = self
            .device
            .lock()
            .map_err(|_| KeyboardError::Transport(TransportError::Disconnected))?;
        let written = guard.write(report).map_err(TransportError::from)?;
        if written != REPORT_LEN {
            return Err(KeyboardError::Transport(TransportError::HidError(format!(
                "Short Beiying HID write: {written} of {REPORT_LEN} bytes"
            ))));
        }
        Ok(())
    }

    fn read_report(
        &self,
        buf: &mut [u8; REPORT_LEN],
        timeout_ms: i32,
    ) -> Result<usize, KeyboardError> {
        let guard = self
            .device
            .lock()
            .map_err(|_| KeyboardError::Transport(TransportError::Disconnected))?;
        let read = guard
            .read_timeout(buf, timeout_ms)
            .map_err(TransportError::from)?;
        Ok(read)
    }
}

/// Deterministic in-memory replay I/O for fixture tests.
#[derive(Debug, Default)]
pub struct ReplayIo {
    state: Mutex<ReplayState>,
}

#[derive(Debug, Default)]
struct ReplayState {
    replies: VecDeque<Vec<u8>>,
    sent: Vec<[u8; REPORT_LEN]>,
}

impl ReplayIo {
    /// Construct a replay stream serving `replies` in FIFO order.
    pub fn new(replies: Vec<Vec<u8>>) -> Self {
        Self {
            state: Mutex::new(ReplayState {
                replies: VecDeque::from(replies),
                sent: Vec::new(),
            }),
        }
    }

    /// Return every 64-byte Output report written through [`ReportIo::write_report`].
    pub fn sent_reports(&self) -> Vec<[u8; REPORT_LEN]> {
        self.state
            .lock()
            .expect("replay mutex poisoned")
            .sent
            .clone()
    }
}

impl ReportIo for ReplayIo {
    fn write_report(&self, report: &[u8; REPORT_LEN]) -> Result<(), KeyboardError> {
        self.state
            .lock()
            .expect("replay mutex poisoned")
            .sent
            .push(*report);
        Ok(())
    }

    fn read_report(
        &self,
        buf: &mut [u8; REPORT_LEN],
        _timeout_ms: i32,
    ) -> Result<usize, KeyboardError> {
        let mut guard = self.state.lock().expect("replay mutex poisoned");
        let Some(reply) = guard.replies.pop_front() else {
            return Ok(0);
        };
        let len = reply.len().min(REPORT_LEN);
        buf[..len].copy_from_slice(&reply[..len]);
        Ok(len)
    }
}

fn drain_hid_device(dev: &HidDevice) {
    let mut discard = [0u8; REPORT_LEN];
    for _ in 0..MAX_DRAIN_REPORTS {
        match dev.read_timeout(&mut discard, 0) {
            Ok(0) | Err(_) => break,
            Ok(_) => continue,
        }
    }
}

/// Phase 1 read-only backend for Shenzhen Beiying / BYTech (`Report ID 0x09`) keyboards.
pub struct BeiyingKeyboard<I: ReportIo = HidApiIo> {
    info: TransportDeviceInfo,
    io: I,
    uuid: [u8; UUID_LEN],
}

impl BeiyingKeyboard<HidApiIo> {
    /// Open the discovered Beiying interface by `info.device_path` and execute the
    /// initial `0x82 0x01` identity handshake.
    pub fn open(info: TransportDeviceInfo) -> Result<Self, KeyboardError> {
        if info.device_path.is_empty() {
            return Err(KeyboardError::NotFound(format!(
                "Missing HID device path for {:04X}:{:04X}",
                info.vid, info.pid
            )));
        }
        let api = HidApi::new().map_err(TransportError::from)?;
        let dev_info = api
            .device_list()
            .find(|d| d.path().to_string_lossy() == info.device_path)
            .ok_or_else(|| {
                KeyboardError::NotFound(format!(
                    "Beiying interface not found at {}",
                    info.device_path
                ))
            })?;
        let dev = dev_info.open_device(&api).map_err(TransportError::from)?;
        drain_hid_device(&dev);
        Self::connect(info, HidApiIo::new(dev))
    }
}

impl<I: ReportIo> BeiyingKeyboard<I> {
    /// Perform the `0x82 0x01` connect handshake on an initialized [`ReportIo`] backend.
    pub fn connect(info: TransportDeviceInfo, io: I) -> Result<Self, KeyboardError> {
        let mut kb = Self {
            info,
            io,
            uuid: [0u8; UUID_LEN],
        };
        let reply = kb.query_report(CMD_DEVICE_QUERY, SUBCMD_UUID)?;
        kb.uuid = parse_uuid_reply(&reply)?;
        Ok(kb)
    }

    /// 6-byte device UUID reported by `0x82 0x01` during [`Self::connect`].
    pub fn uuid(&self) -> &[u8; UUID_LEN] {
        &self.uuid
    }

    /// Uppercase 12-hex-digit representation of [`Self::uuid`].
    pub fn uuid_hex(&self) -> String {
        format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            self.uuid[0], self.uuid[1], self.uuid[2], self.uuid[3], self.uuid[4], self.uuid[5]
        )
    }

    /// Access the underlying [`ReportIo`] instance (useful for verifying sent bytes in replay tests).
    pub fn io(&self) -> &I {
        &self.io
    }

    fn query_report(&self, cmd: u8, subcmd: u8) -> Result<[u8; REPORT_LEN], KeyboardError> {
        let req = build_query_report(cmd, subcmd);
        self.io.write_report(&req)?;

        let deadline = Instant::now() + Duration::from_millis(DEFAULT_TIMEOUT_MS as u64);
        let mut rx = [0u8; REPORT_LEN];
        loop {
            let remaining_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(i32::MAX as u128) as i32;
            if remaining_ms == 0 {
                return Err(KeyboardError::Timeout);
            }
            rx.fill(0);
            let n = self.io.read_report(&mut rx, remaining_ms)?;
            if n == 0 {
                return Err(KeyboardError::Timeout);
            }
            if n < REPORT_LEN || rx[0] != REPORT_ID || rx[1] != cmd || rx[2] != subcmd {
                continue;
            }
            if !verify_checksum(&rx) {
                return Err(KeyboardError::Transport(TransportError::ChecksumError));
            }
            return Ok(rx);
        }
    }
}

impl<I: ReportIo + 'static> Keyboard for BeiyingKeyboard<I> {
    fn device_info(&self) -> &TransportDeviceInfo {
        &self.info
    }

    fn get_version(&self) -> Result<FirmwareVersion, KeyboardError> {
        let reply = self.query_report(CMD_DEVICE_QUERY, SUBCMD_FIRMWARE_VERSION)?;
        parse_firmware_version_reply(&reply)
    }

    fn get_profile(&self) -> Result<Profile, KeyboardError> {
        let reply = self.query_report(CMD_PROFILE_QUERY, SUBCMD_ACTIVE_PROFILE)?;
        parse_profile_reply(&reply)
    }

    fn set_profile(&self, _profile: Profile) -> Result<(), KeyboardError> {
        Err(KeyboardError::NotSupported(
            "Beiying profile write is out of scope for Phase 1".into(),
        ))
    }

    fn get_polling_rate(&self) -> Result<u16, KeyboardError> {
        let reply = self.query_report(CMD_SYSTEM_PARAM_QUERY, PARAM_POLLING_RATE)?;
        parse_polling_rate_reply(&reply)
    }

    fn set_polling_rate(&self, _hz: u16) -> Result<(), KeyboardError> {
        Err(KeyboardError::NotSupported(
            "Beiying polling rate write is out of scope for Phase 1".into(),
        ))
    }

    fn get_debounce(&self) -> Result<u8, KeyboardError> {
        Err(KeyboardError::NotSupported(
            "The tested wired TAC75 HE has no traditional debounce setting; Hall-effect deadzone is a separate feature".into(),
        ))
    }

    fn set_debounce(&self, _ms: u8) -> Result<(), KeyboardError> {
        Err(KeyboardError::NotSupported(
            "Beiying debounce write is out of scope for Phase 1".into(),
        ))
    }

    fn get_sleep_time(&self) -> Result<SleepTimeSettings, KeyboardError> {
        Err(KeyboardError::NotSupported(
            "The tested wired TAC75 HE has no SleepTimeSettings feature; the opaque 0x84 0x13 reply is not mapped".into(),
        ))
    }

    fn set_sleep_time(&self, _settings: &SleepTimeSettings) -> Result<(), KeyboardError> {
        Err(KeyboardError::NotSupported(
            "Beiying sleep timeout write is out of scope for Phase 1".into(),
        ))
    }

    fn reset(&self) -> Result<(), KeyboardError> {
        Err(KeyboardError::NotSupported(
            "Beiying factory reset is out of scope for Phase 1".into(),
        ))
    }
}
