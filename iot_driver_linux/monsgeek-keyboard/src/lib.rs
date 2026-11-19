//! High-level keyboard interface for MonsGeek/Akko keyboards
//!
//! This crate provides a convenient API for interacting with keyboard features
//! on top of any transport layer (HID wired, dongle, Bluetooth, etc.)

pub mod backend;
pub mod error;
pub mod hid_codes;
pub mod led;
pub mod magnetism;
pub mod memory;
pub mod settings;
pub mod sync;

pub use backend::Keyboard;
pub use error::KeyboardError;
pub use led::{LedMode, LedParams, RgbColor};
pub use magnetism::{
    DksAction, DksBinding, DksConfig, DksPhase, KeyDepthEvent, KeyMode, KeyTriggerSettings,
    KeyTriggerSettingsDetail, ModeByte, TravelDepth, TriggerSettings,
};
pub use settings::{
    BatteryInfo, FeatureList, FirmwareVersion, KeyboardOptions, PollingRate, Precision,
    RT_STABILITY_MAX, RT_STABILITY_STEP_MS, SleepTimeSettings, os_mode_name,
};
pub use sync::list_keyboards;

pub use monsgeek_transport::protocol::ProtocolFamily;

/// Information about firmware patches applied to the keyboard
#[derive(Debug, Clone)]
pub struct PatchInfo {
    pub version: u8,
    pub capabilities: u16,
    pub name: String,
}

impl PatchInfo {
    pub fn has_led_stream(&self) -> bool {
        self.capabilities & 0x02 != 0
    }

    pub fn has_anim_engine(&self) -> bool {
        self.capabilities & 0x40 != 0
    }

    pub fn has_peek_poke(&self) -> bool {
        self.capabilities & 0x80 != 0
    }
}

/// Status of a single animation definition slot.
#[derive(Debug, Clone)]
pub struct AnimDefStatus {
    pub id: DefId,
    pub num_kf: u8,
    pub flags: u8,
    pub priority: i8,
    pub key_count: u8,
    pub duration_ticks: u16,
}

impl AnimDefStatus {
    pub fn is_one_shot(&self) -> bool {
        self.flags & 0x01 != 0
    }
    pub fn is_rainbow(&self) -> bool {
        self.flags & 0x04 != 0
    }
}

/// Animation engine status from firmware query.
#[derive(Debug, Clone)]
pub struct AnimStatus {
    pub active_count: u8,
    pub frame_count: u32,
    pub overlay_active: bool,
    pub defs: Vec<AnimDefStatus>,
}

// Macro parsing
// (MacroEvent struct and parse_macro_events fn are defined after RyKeyboard impl)

// Re-export VendorEvent and TimestampedEvent for use by consumers (TUI notification handling)
pub use monsgeek_transport::{TimestampedEvent, VendorEvent};

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use monsgeek_transport::protocol::{
    CommandTable, DefId, INPUT_REPORT_SIZE, KeymatrixLayer, Layer, LedPos, MacroSlot, Profile,
    StripIdx, cmd, magnetism as mag_cmd,
};
use monsgeek_transport::{ChecksumType, FlowControlTransport, Transport, TransportDeviceInfo};
// Typed commands
use monsgeek_transport::command::{
    GetFnData, GetKeyMatrixData, GetMacroData, GetMultiMagnetismData, HidCommand,
    LedParamsResponse as TransportLedParamsResponse, QueryLedParams, SetFnData, SetKeyMatrixData,
    SetMacroCommand, SetMagnetismReport, SetMultiMagnetismCommand, SetMultiMagnetismHeader,
};
use zerocopy::IntoBytes;

/// Wire step for the Mod-Tap decision time: the firmware stores `ms / 10` in a
/// single byte, so times are quantized to 10 ms (0–2550 ms).
const MODTAP_TIME_STEP_MS: u16 = 10;

/// Frame offset of the polling-rate code in SET_REPORT/GET_REPORT: `[cmd, 0, code, ...]`.
/// The byte directly after the command is reserved and stays zero — reading or writing
/// there instead makes every rate look like the 8 kHz code 0.
const POLLING_RATE_FRAME_OFFSET: usize = 2;

/// Sentinel partner index meaning "this key has no Snap-Tap pair".
///
/// NOTE: pending firmware confirmation on v407 — `0xFF` is the conventional
/// unbound marker and is out of the valid key-index range.
pub const SNAPTAP_UNBOUND: u8 = 0xFF;

/// Settle time after the final ("simple", flag=0) per-key SET_MULTI_MAGNETISM
/// write of a batch before the firmware answers GET_MULTI_MAGNETISM correctly.
/// Reading sooner returns the *whole* trigger table shifted/garbled — not just
/// the written key. Measured floor is ~200 ms; this carries margin.
const MAGNETISM_SETTLE_MS: u64 = 250;

/// Keyboard speaking the RongYuan (RY5088 / YiChip) vendor protocol over any of
/// its transports (wired, dongle, Bluetooth).
///
/// Provides convenient methods for keyboard features like LED control,
/// key mapping, trigger settings, etc.
pub struct RyKeyboard {
    transport: Arc<FlowControlTransport>,
    key_count: u8,
    has_magnetism: bool,
    /// Key names indexed by matrix position. Empty string = no physical key at that position.
    matrix_key_names: Vec<String>,
    /// Factory HID keycode per matrix position, from the device database. Empty when
    /// unknown, in which case callers fall back to deriving it from the position name.
    matrix_defaults: Vec<u8>,
    /// Matrix positions that are non-analog (GPIO/encoder, not magnetic switches).
    non_analog_positions: Vec<u8>,
    /// Polling rates this model accepts, from the device database.
    /// Empty means unknown, in which case no restriction is applied.
    polling_rates: Vec<u16>,
    /// Profile every keymatrix / Fn read and write operates on.
    ///
    /// Resolved at connect from the board's own active profile, or forced by the
    /// caller. Keeping it here rather than passing it per call means a keymap read
    /// and the write that follows it can't disagree. Atomic because the interface is
    /// shared behind an `Arc` and the profile changes mid-session when the user
    /// switches profiles.
    active_profile: AtomicU8,
    /// Command table for the active protocol family. The `ProtocolFamily` itself
    /// is not kept — it is only ever consulted to pick this table.
    commands: &'static CommandTable,
}

/// Userpic flash slots: the firmware keeps them in one 2 KB sector at 384 bytes each.
const USERPIC_SLOTS: u8 = 5;

/// RGB bytes a userpic carries — one triple for each of the 126 matrix positions
/// (21 columns × 6 rows) the SET_USERPIC pages cover.
pub const USERPIC_BYTES: usize = 126 * 3;

fn check_userpic_slot(slot: u8) -> Result<(), KeyboardError> {
    if slot < USERPIC_SLOTS {
        Ok(())
    } else {
        Err(KeyboardError::InvalidParameter(format!(
            "Userpic slot must be 0-{}",
            USERPIC_SLOTS - 1
        )))
    }
}

/// Which config to write into DKS slot 0.
///
/// Slot 0 doubles as keymatrix layer 0 — the key's *base* output once it leaves DKS
/// mode — and the base layer has no ROM fallback, so writing it empty silences the
/// key. When the caller leaves slot 0 empty, keep whatever the key emits today.
fn resolve_slot0(requested: [u8; 4], current: [u8; 4]) -> [u8; 4] {
    if requested == [0; 4] {
        current
    } else {
        requested
    }
}

impl RyKeyboard {
    /// Create a new keyboard interface
    ///
    /// # Arguments
    /// * `transport` - Flow-controlled transport layer
    /// * `key_count` - Number of keys on the keyboard
    /// * `has_magnetism` - Whether the keyboard has Hall Effect switches
    /// * `protocol` - Protocol family (RY5088 or YiChip)
    pub fn new(
        transport: Arc<FlowControlTransport>,
        key_count: u8,
        has_magnetism: bool,
        protocol: ProtocolFamily,
    ) -> Self {
        Self {
            transport,
            key_count,
            has_magnetism,
            matrix_key_names: Vec::new(),
            matrix_defaults: Vec::new(),
            non_analog_positions: Vec::new(),
            polling_rates: Vec::new(),
            active_profile: AtomicU8::new(0),
            commands: protocol.commands(),
        }
    }

    /// Set the profile that keymatrix and Fn operations target.
    ///
    /// Does not switch the keyboard — see [`set_profile`](Self::set_profile) for that.
    /// This selects which profile's stored keymap is read and written.
    pub fn set_active_profile(&self, profile: Profile) {
        self.active_profile.store(profile.get(), Ordering::Relaxed);
    }

    /// Profile that keymatrix and Fn operations target.
    pub fn active_profile(&self) -> Profile {
        Profile::try_from(self.active_profile.load(Ordering::Relaxed)).unwrap_or_default()
    }

    /// Pages this interface has read, for progress reporting.
    ///
    /// Sample it either side of a long paged load to show how far along it is —
    /// see [`FlowControlTransport::pages_read`].
    pub fn pages_read(&self) -> u64 {
        self.transport.pages_read()
    }

    /// Set matrix key names from a device profile.
    pub fn set_matrix_key_names(&mut self, names: Vec<String>) {
        self.matrix_key_names = names;
    }

    /// Set the factory keycode per matrix position from the device database.
    pub fn set_matrix_defaults(&mut self, defaults: Vec<u8>) {
        self.matrix_defaults = defaults;
    }

    /// Factory HID keycode for a matrix position, or `None` when this device's
    /// layout is not in the database.
    ///
    /// Deriving it from the position's name only works when the board matches the
    /// generic matrix; boards that differ (the Womier SK75 has LMeta where the
    /// generic table has LAlt) need their own table, or every such key reads as
    /// customised.
    pub fn matrix_default(&self, position: usize) -> Option<u8> {
        self.matrix_defaults
            .get(position)
            .copied()
            .filter(|&c| c != 0)
    }

    /// Get the display name for a matrix position.
    /// Returns empty string for positions with no physical key.
    pub fn matrix_key_name(&self, position: usize) -> &str {
        self.matrix_key_names
            .get(position)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Set non-analog matrix positions (GPIO/encoder keys that can't be calibrated).
    pub fn set_non_analog_positions(&mut self, positions: Vec<u8>) {
        self.non_analog_positions = positions;
    }

    /// Restrict which polling rates may be set, capping at the model's maximum.
    /// Leave unset for devices missing from the database to keep them unrestricted.
    pub fn set_polling_rates(&mut self, rates: Vec<u16>) {
        self.polling_rates = rates;
    }

    /// Check if a matrix position is non-analog (GPIO/encoder, not a magnetic switch).
    pub fn is_non_analog(&self, position: usize) -> bool {
        self.non_analog_positions.contains(&(position as u8))
    }

    /// Get the matrix size (number of positions including empty slots).
    /// Uses matrix_key_names length if populated, otherwise falls back to key_count.
    pub fn matrix_size(&self) -> usize {
        if self.matrix_key_names.is_empty() {
            self.key_count as usize
        } else {
            self.matrix_key_names.len()
        }
    }

    /// Open a specific discovered device with metadata from the device database.
    pub fn open_device(
        device: &monsgeek_transport::DiscoveredDevice,
        key_count: u8,
        has_magnetism: bool,
        protocol: ProtocolFamily,
    ) -> Result<Self, KeyboardError> {
        let transport = monsgeek_transport::open_device_sync(device)?;
        Ok(Self::new(transport, key_count, has_magnetism, protocol))
    }

    /// Get the underlying transport
    pub fn transport(&self) -> &Arc<FlowControlTransport> {
        &self.transport
    }

    /// How many matrix positions to scan, i.e. one past the highest position the
    /// board uses.
    ///
    /// **Not** the number of keys. The matrix is sparse: it has gaps, and on this
    /// family the tail holds an encoder and a couple of pseudo-entries. Callers walk
    /// `0..matrix_positions()` and filter by name — see `keymap::build_key_rows` or
    /// `commands::triggers::calibrate`. For a count to show a user, take the named
    /// or analog positions from the matrix database instead.
    pub fn matrix_positions(&self) -> u8 {
        self.key_count
    }

    /// Check if keyboard has magnetism (Hall Effect) support
    pub fn has_magnetism(&self) -> bool {
        self.has_magnetism
    }

    /// Check if using wireless transport
    pub fn is_wireless(&self) -> bool {
        self.transport.device_info().is_wireless()
    }

    /// Check if connected via dongle
    pub fn is_dongle(&self) -> bool {
        self.transport.device_info().is_dongle()
    }

    // === Device Info ===

    /// Get device ID (unique identifier)
    pub fn get_device_id(&self) -> Result<u32, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)?;

        if resp.len() < 5 || resp[0] != cmd::GET_USB_VERSION {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid device ID response".into(),
            ));
        }

        let device_id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
        Ok(device_id)
    }

    /// Get battery info (dongle/wireless only)
    ///
    /// For dongle connections, this sends F7 to refresh and reads the cached
    /// value from feature report 0x05. For wired connections, returns full battery.
    pub fn get_battery(&self) -> Result<BatteryInfo, KeyboardError> {
        let (level, online, idle) = self.transport.get_battery_status()?;
        Ok(BatteryInfo {
            level,
            online,
            charging: false, // Not available via dongle protocol
            idle,
        })
    }

    // === LED Control ===

    /// Get current LED parameters
    pub fn get_led_params(&self) -> Result<LedParams, KeyboardError> {
        let resp: TransportLedParamsResponse = self.transport.query(&QueryLedParams::default())?;
        Ok(LedParams::from_transport_response(&resp))
    }

    /// Set LED mode
    pub fn set_led_mode(&self, mode: LedMode) -> Result<(), KeyboardError> {
        let mut params = self.get_led_params()?;
        params.mode = mode;
        self.set_led_params(&params)
    }

    /// Set LED parameters
    pub fn set_led_params(&self, params: &LedParams) -> Result<(), KeyboardError> {
        self.transport.send(&params.to_transport_cmd())?;
        Ok(())
    }

    // === Keyboard Options ===

    /// Get keyboard options (OS mode, Fn layer, etc.)
    pub fn get_kb_options(&self) -> Result<KeyboardOptions, KeyboardError> {
        let cmd_byte = self.commands.get_kboption.ok_or_else(|| {
            KeyboardError::NotSupported("KB options not available on this device".into())
        })?;
        let resp = self
            .transport
            .query_command(cmd_byte, &[], ChecksumType::Bit7)?;

        if resp.len() < 6 || resp[0] != cmd_byte {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid KB options response".into(),
            ));
        }

        Ok(KeyboardOptions::from_bytes(&resp[1..]))
    }

    /// Set keyboard options
    pub fn set_kb_options(&self, options: &KeyboardOptions) -> Result<(), KeyboardError> {
        let cmd_byte = self.commands.set_kboption.ok_or_else(|| {
            KeyboardError::NotSupported("KB options not available on this device".into())
        })?;
        self.transport
            .send_command(cmd_byte, &options.to_bytes(), ChecksumType::Bit7)?;

        Ok(())
    }

    // === Feature List ===

    /// Get device feature list (precision, capabilities)
    pub fn get_feature_list(&self) -> Result<FeatureList, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd::GET_FEATURE_LIST, &[], ChecksumType::Bit7)?;

        if resp.is_empty() || resp[0] != cmd::GET_FEATURE_LIST {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid feature list response".into(),
            ));
        }

        Ok(FeatureList::from_bytes(&resp[1..]))
    }

    /// Get precision level for travel/trigger settings
    ///
    /// This method tries to get precision from the feature list first.
    /// If the keyboard doesn't support the feature list command (returns invalid response),
    /// it falls back to inferring precision from the firmware version.
    ///
    /// This is the recommended way to get precision - consumers should use this
    /// instead of calling get_feature_list() or get_version() directly for precision.
    pub fn get_precision(&self) -> Result<settings::Precision, KeyboardError> {
        // Try feature list first
        if let Ok(features) = self.get_feature_list()
            && let Some(precision) = features.precision()
        {
            return Ok(precision);
        }

        // Fall back to firmware version
        let version = self.get_version()?;
        Ok(version.precision())
    }

    // === Side LED (Sidelight) ===

    /// Get side LED parameters
    pub fn get_side_led_params(&self) -> Result<LedParams, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd::GET_SLEDPARAM, &[], ChecksumType::Bit7)?;

        if resp.len() < 8 || resp[0] != cmd::GET_SLEDPARAM {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid side LED params response".into(),
            ));
        }

        // Protocol format: [cmd, mode, speed, brightness, option, r, g, b]
        // Note: Side LED speed is NOT inverted (unlike main LED)
        Ok(LedParams {
            mode: LedMode::from_u8(resp[1]).unwrap_or(LedMode::Off),
            speed: resp[2],
            brightness: resp[3],
            color: RgbColor::new(resp[5], resp[6], resp[7]),
            direction: resp.get(4).copied().unwrap_or(0), // Option byte (dazzle info)
        })
    }

    /// Set side LED parameters
    pub fn set_side_led_params(&self, params: &LedParams) -> Result<(), KeyboardError> {
        // Protocol format: [mode, speed, brightness, option, r, g, b]
        // Note: Side LED speed is NOT inverted (unlike main LED)
        let data = [
            params.mode as u8,
            params.speed.min(led::SPEED_MAX),
            params.brightness.min(led::BRIGHTNESS_MAX),
            params.direction, // Option byte (dazzle info)
            params.color.r,
            params.color.g,
            params.color.b,
        ];

        self.transport
            .send_command(cmd::SET_SLEDPARAM, &data, ChecksumType::Bit8)?;

        Ok(())
    }

    // === Per-Key RGB ===

    /// Set all keys to a single color (for per-key RGB mode)
    pub fn set_all_keys_color(&self, color: RgbColor, led_layer: u8) -> Result<(), KeyboardError> {
        let colors = vec![(color.r, color.g, color.b); self.matrix_size()];
        self.set_per_key_colors_to_layer(&colors, led_layer)
    }

    // === Userpic (Flash-Based Per-Key Colors, Mode 13) ===

    /// Upload a userpic to a flash slot (0-4).
    ///
    /// `data` holds one RGB triple per matrix position (column-major, `col * 6 + row`),
    /// at most [`USERPIC_BYTES`] of it; the rest of the slot is written black.
    ///
    /// Sent as SET_USERPIC pages of 56 bytes, the last one short (42 bytes).
    pub fn upload_userpic(&self, slot: u8, data: &[u8]) -> Result<(), KeyboardError> {
        let cmd_byte = self.commands.set_userpic.ok_or_else(|| {
            KeyboardError::NotSupported("User pictures not available on this device".into())
        })?;
        check_userpic_slot(slot)?;
        if data.len() > USERPIC_BYTES {
            return Err(KeyboardError::InvalidParameter(format!(
                "Userpic data is {} bytes, a slot holds {USERPIC_BYTES}",
                data.len()
            )));
        }
        let mut slot_data = [0u8; USERPIC_BYTES];
        slot_data[..data.len()].copy_from_slice(data);

        const PAGE_SIZE: usize = 56;
        let last_page = USERPIC_BYTES.div_ceil(PAGE_SIZE) - 1;
        for (page, chunk) in slot_data.chunks(PAGE_SIZE).enumerate() {
            // [slot, 0xFF, page, data_size, last_flag, 0, 0, rgb...]
            let mut payload = vec![
                slot,
                0xFF,
                page as u8,
                chunk.len() as u8,
                u8::from(page == last_page),
                0,
                0,
            ];
            payload.extend_from_slice(chunk);
            self.transport
                .send_command(cmd_byte, &payload, ChecksumType::Bit7)?;

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    /// Download a userpic from a flash slot (0-4).
    ///
    /// Returns [`USERPIC_BYTES`] bytes in the layout [`upload_userpic`](Self::upload_userpic)
    /// takes. Uses the GET_USERPIC (0x8C) block read protocol.
    pub fn download_userpic(&self, slot: u8) -> Result<Vec<u8>, KeyboardError> {
        let cmd_byte = self.commands.get_userpic.ok_or_else(|| {
            KeyboardError::NotSupported("User pictures not available on this device".into())
        })?;
        check_userpic_slot(slot)?;

        let mut data = Vec::with_capacity(USERPIC_BYTES.next_multiple_of(INPUT_REPORT_SIZE));
        for block in 0..USERPIC_BYTES.div_ceil(INPUT_REPORT_SIZE) as u8 {
            let query = [slot, 0xFF, block];
            let resp = self.transport.query_page(
                cmd_byte,
                &query,
                ChecksumType::Bit7,
                INPUT_REPORT_SIZE,
            )?;
            data.extend_from_slice(&resp);
        }

        data.truncate(USERPIC_BYTES);
        Ok(data)
    }

    // === Magnetism / Hall Effect ===

    /// Start magnetism (key depth) reporting
    pub fn start_magnetism_report(&self) -> Result<(), KeyboardError> {
        if !self.has_magnetism {
            return Err(KeyboardError::NotSupported(
                "Device does not have Hall Effect switches".into(),
            ));
        }
        self.transport.send(&SetMagnetismReport::enable())?;
        Ok(())
    }

    /// Stop magnetism (key depth) reporting
    pub fn stop_magnetism_report(&self) -> Result<(), KeyboardError> {
        if !self.has_magnetism {
            return Ok(());
        }
        self.transport.send(&SetMagnetismReport::disable())?;
        Ok(())
    }

    /// Read a key depth event
    ///
    /// Returns None on timeout
    pub fn read_key_depth(
        &self,
        timeout_ms: u32,
        precision_factor: f64,
    ) -> Result<Option<KeyDepthEvent>, KeyboardError> {
        match self.transport.read_event(timeout_ms)? {
            Some(VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            }) => Ok(Some(KeyDepthEvent {
                key_index,
                depth_raw,
                depth_mm: depth_raw as f32 / precision_factor as f32,
            })),
            _ => Ok(None),
        }
    }

    /// Get trigger settings for a specific key.
    ///
    /// Per-key config lives in the multi-magnetism table (subcmd 0x00 actuation,
    /// 0x01 release, 0x07 mode). The legacy single-key command (`GET_KEY_MAGNETISM_MODE`
    /// 0x9D) is a no-op on the RY5088 — the vendor web app stubs it out — so we
    /// read the bulk table and index it.
    pub fn get_key_trigger(&self, key_index: u8) -> Result<KeyTriggerSettings, KeyboardError> {
        let all = self.get_all_triggers()?;
        let idx = key_index as usize;
        let mode_byte = ModeByte::from_u8(all.key_modes.get(idx).copied().unwrap_or(0));
        Ok(KeyTriggerSettings {
            key_index,
            actuation: all.press_travel.get(idx).copied().unwrap_or(0),
            deactuation: all.lift_travel.get(idx).copied().unwrap_or(0),
            mode: mode_byte.base,
            rapid_trigger: mode_byte.rapid_trigger,
        })
    }

    /// Set trigger settings for a specific key.
    ///
    /// Writes actuation (subcmd 0x00), release (0x01) and mode (0x07) via the
    /// per-key "simple" multi-magnetism form, exactly as the vendor web app does.
    /// The old `SET_KEY_MAGNETISM_MODE` (0x1D) command is a no-op on the RY5088
    /// (it belongs to a different chip family), so writes through it never landed.
    pub fn set_key_trigger(&self, settings: &KeyTriggerSettings) -> Result<(), KeyboardError> {
        if !self.has_magnetism {
            return Err(KeyboardError::NotSupported(
                "Device does not have Hall Effect switches".into(),
            ));
        }

        let key = settings.key_index;
        self.set_magnetism_simple(
            mag_cmd::PRESS_TRAVEL,
            key,
            false,
            &settings.actuation.to_le_bytes(),
        )?;
        self.set_magnetism_simple(
            mag_cmd::LIFT_TRAVEL,
            key,
            false,
            &settings.deactuation.to_le_bytes(),
        )?;
        let mode = ModeByte::new(settings.mode, settings.rapid_trigger).to_u8();
        self.set_magnetism_simple(mag_cmd::KEY_MODE, key, true, &[mode])?;
        Ok(())
    }

    /// Query magnetism data for a specific sub-command
    ///
    /// Magnetism queries use a multi-page protocol:
    /// - Send: [sub_cmd, flag=1, page]
    /// - Response doesn't echo command, data starts at byte 0
    fn get_magnetism(&self, sub_cmd: u8, num_pages: usize) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            let query = GetMultiMagnetismData::paged(sub_cmd, page as u8);
            // Propagate rather than padding with zeros: a fabricated page reads back
            // as a whole block of keys with 0.00 mm actuation, which is indis-
            // tinguishable from a real setting. Callers that treat a sub-command as
            // optional (deadzones on older firmware) already handle the error.
            let resp = self.transport.query_page(
                cmd::GET_MULTI_MAGNETISM,
                query.as_bytes(),
                ChecksumType::Bit7,
                INPUT_REPORT_SIZE,
            )?;
            all_data.extend_from_slice(&resp);
        }

        Ok(all_data)
    }

    /// Get all trigger settings
    pub fn get_all_triggers(&self) -> Result<TriggerSettings, KeyboardError> {
        if !self.has_magnetism {
            return Err(KeyboardError::NotSupported(
                "Device does not have Hall Effect switches".into(),
            ));
        }

        // Calculate pages needed based on key count (64 bytes per page)
        let pages_u8 = (self.key_count as usize).div_ceil(64); // 1 byte per key
        let pages_u16 = (self.key_count as usize * 2).div_ceil(64); // 2 bytes per key

        // Key modes use 1 byte per key
        let modes = self.get_magnetism(mag_cmd::KEY_MODE, pages_u8)?;

        let kc = self.key_count as usize;

        // Travel values use 2 bytes per key (16-bit little-endian)
        let press = self.get_magnetism(mag_cmd::PRESS_TRAVEL, pages_u16)?;
        let lift = self.get_magnetism(mag_cmd::LIFT_TRAVEL, pages_u16)?;
        let rt_press = self.get_magnetism(mag_cmd::RT_PRESS, pages_u16)?;
        let rt_lift = self.get_magnetism(mag_cmd::RT_LIFT, pages_u16)?;

        // Deadzones - may fail on older firmware
        let bottom_dz = self
            .get_magnetism(mag_cmd::BOTTOM_DEADZONE, pages_u16)
            .unwrap_or_default();
        let top_dz = self
            .get_magnetism(mag_cmd::TOP_DEADZONE, pages_u16)
            .unwrap_or_default();

        Ok(TriggerSettings {
            key_count: kc,
            press_travel: TriggerSettings::decode_u16_values(&press, kc),
            lift_travel: TriggerSettings::decode_u16_values(&lift, kc),
            rt_press: TriggerSettings::decode_u16_values(&rt_press, kc),
            rt_lift: TriggerSettings::decode_u16_values(&rt_lift, kc),
            key_modes: modes,
            bottom_deadzone: TriggerSettings::decode_u16_values(&bottom_dz, kc),
            top_deadzone: TriggerSettings::decode_u16_values(&top_dz, kc),
        })
    }

    // === Bulk Trigger Setters ===

    /// Set magnetism values for all keys (u16 version, used by newer firmware)
    ///
    /// Sends values in pages of 56 bytes each.
    /// Format: [sub_cmd, flag=1, page, commit, 0, 0, 0, data...]
    fn set_magnetism_u16(&self, sub_cmd: u8, values: &[u16]) -> Result<(), KeyboardError> {
        // Convert u16 values to bytes (little-endian)
        let bytes: Vec<u8> = values
            .iter()
            .take(self.key_count as usize)
            .flat_map(|&v| v.to_le_bytes())
            .collect();

        // Send in pages (56 bytes per page)
        const PAGE_SIZE: usize = 56;
        let num_pages = bytes.len().div_ceil(PAGE_SIZE);

        for (page, chunk) in bytes.chunks(PAGE_SIZE).enumerate() {
            let is_last = page == num_pages - 1;
            let cmd = SetMultiMagnetismCommand {
                header: SetMultiMagnetismHeader::paged(sub_cmd, page as u8, is_last),
                payload: chunk.to_vec(),
            };

            self.transport.send_with_delay(&cmd, 30)?;
        }

        Ok(())
    }

    /// Set magnetism values for all keys (u8 version, legacy)
    fn set_magnetism_u8(&self, sub_cmd: u8, values: &[u8]) -> Result<(), KeyboardError> {
        let mut data = vec![sub_cmd];
        data.extend_from_slice(&values[..self.key_count as usize]);
        self.transport
            .send_command(cmd::SET_MULTI_MAGNETISM, &data, ChecksumType::Bit7)?;
        Ok(())
    }

    /// Set the actuation point for all keys.
    pub fn set_actuation_all(&self, travel: TravelDepth) -> Result<(), KeyboardError> {
        let values = vec![travel.raw(); self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::PRESS_TRAVEL, &values)
    }

    /// Set the release point for all keys.
    pub fn set_release_all(&self, travel: TravelDepth) -> Result<(), KeyboardError> {
        let values = vec![travel.raw(); self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::LIFT_TRAVEL, &values)
    }

    /// Set Rapid Trigger press sensitivity for all keys.
    pub fn set_rt_press_all(&self, travel: TravelDepth) -> Result<(), KeyboardError> {
        let values = vec![travel.raw(); self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::RT_PRESS, &values)
    }

    /// Set Rapid Trigger release sensitivity for all keys.
    pub fn set_rt_lift_all(&self, travel: TravelDepth) -> Result<(), KeyboardError> {
        let values = vec![travel.raw(); self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::RT_LIFT, &values)
    }

    /// Enable/disable the Rapid-Trigger flag (`0x80`) for all keys, preserving
    /// each key's base mode (read-modify-write of the KEY_MODE bytes).
    pub fn set_rapid_trigger_all(&self, enable: bool) -> Result<(), KeyboardError> {
        let kc = self.key_count as usize;
        let mut modes = self.get_magnetism(mag_cmd::KEY_MODE, kc.div_ceil(64))?;
        modes.resize(kc, 0);
        for m in &mut modes {
            if enable {
                *m |= ModeByte::RT_FLAG;
            } else {
                *m &= !ModeByte::RT_FLAG;
            }
        }
        self.set_magnetism_u8(mag_cmd::KEY_MODE, &modes)
    }

    /// Set the base mode (`0x80` RT flag cleared) for all keys.
    pub fn set_mode_all(&self, mode: ModeByte) -> Result<(), KeyboardError> {
        let values = vec![mode.to_u8(); self.key_count as usize];
        self.set_magnetism_u8(mag_cmd::KEY_MODE, &values)
    }

    /// Write a single key's bytes for a magnetism sub-command (the "simple",
    /// non-paged form used by the webapp: `flag=0`, `page=key_index`,
    /// `commit=is_final`). Mirrors `_sendMagnetismInfoSimpleCMD` in the vendor
    /// web app exactly.
    fn set_magnetism_simple(
        &self,
        sub_cmd: u8,
        key_index: u8,
        is_final: bool,
        payload: &[u8],
    ) -> Result<(), KeyboardError> {
        let pkt = SetMultiMagnetismCommand {
            header: SetMultiMagnetismHeader::per_key(sub_cmd, key_index, is_final),
            payload: payload.to_vec(),
        };
        // After the final write of a batch (commit=1) the firmware needs to settle
        // before it will answer GET_MULTI_MAGNETISM correctly. Read it back too
        // soon and every key comes back shifted/garbage (the whole trigger table
        // reads wrong, not just this key). ~200ms is the observed floor; use margin.
        // Mirrors the vendor web app's `vendorSleep()` after each simple-write batch.
        let settle_ms = if is_final { MAGNETISM_SETTLE_MS } else { 30 };
        self.transport.send_with_delay(&pkt, settle_ms)?;
        Ok(())
    }

    // === DKS (Dynamic Keystroke) ===

    /// Read the 512-byte DKS trigger-modes blob (GET subcmd 0x0A, 8 pages).
    pub fn get_dks_trigger_modes_blob(&self) -> Result<Vec<u8>, KeyboardError> {
        self.get_magnetism(mag_cmd::DKS_MODES, 8)
    }

    /// Read per-key DKS activation travel values (GET subcmd 0x04, u16 LE per key).
    pub fn get_dks_travels(&self) -> Result<Vec<u16>, KeyboardError> {
        let kc = self.key_count as usize;
        let raw = self.get_magnetism(mag_cmd::DKS_TRAVEL, kc.div_ceil(32))?;
        Ok(raw
            .chunks(2)
            .take(kc)
            .map(|c| u16::from_le_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
            .collect())
    }

    /// Read the 4-byte key config stored on a keymatrix sub-layer (0–3).
    ///
    /// DKS modify-key combos live on layers 0–3 (`setKeyConfigSimple` in the
    /// vendor webapp). `profile` is the keyboard profile (usually 0).
    pub fn get_key_config_at_layer(
        &self,
        profile: Profile,
        layer: KeymatrixLayer,
        key_index: u8,
    ) -> Result<[u8; 4], KeyboardError> {
        let matrix = self.get_keymatrix(profile, layer, 8)?;
        let off = key_index as usize * 4;
        if off + 4 > matrix.len() {
            return Err(KeyboardError::InvalidParameter(format!(
                "key_index {key_index} out of range for keymatrix"
            )));
        }
        Ok([
            matrix[off],
            matrix[off + 1],
            matrix[off + 2],
            matrix[off + 3],
        ])
    }

    /// Read the full DKS configuration for one key.
    pub fn get_dks_config(&self, key_index: u8) -> Result<DksConfig, KeyboardError> {
        let idx = key_index as usize;
        let travels = self.get_dks_travels()?;
        let travel_raw = travels.get(idx).copied().unwrap_or(0);
        let blob = self.get_dks_trigger_modes_blob()?;
        let modes = DksConfig::trigger_modes_from_blob(&blob, idx);
        // Raw bytes: a DKS slot may hold any action, and decoding to a key-only
        // type here would silently drop macros/consumer usages on the next write.
        let mut configs = [[0u8; 4]; 4];
        for (slot, config) in KeymatrixLayer::ALL.iter().zip(configs.iter_mut()) {
            *config = self.get_key_config_at_layer(self.active_profile(), *slot, key_index)?;
        }
        Ok(DksConfig::from_parts(travel_raw, modes, configs))
    }

    /// Write DKS trigger-point travel (触发点行程, u16 raw) for one key (SET subcmd 0x04).
    pub fn set_dks_trigger_point_travel_raw(
        &self,
        key_index: u8,
        trigger_point_travel_raw: u16,
    ) -> Result<(), KeyboardError> {
        let bytes = trigger_point_travel_raw.to_le_bytes();
        self.set_magnetism_simple(mag_cmd::DKS_TRAVEL, key_index, true, &bytes)
    }

    /// Write four packed binding-row bytes for one key (SET subcmd 0x08).
    pub fn set_dks_trigger_modes(
        &self,
        key_index: u8,
        modes: [u8; 4],
    ) -> Result<(), KeyboardError> {
        self.set_magnetism_simple(mag_cmd::DKS_TRIGGER_MODES_SET, key_index, true, &modes)
    }

    /// Write a key's 4-byte config to keymatrix layer 0–3.
    ///
    /// Layers 0/1 are the key's Base and Layer1 outputs; in DKS mode the firmware
    /// reinterprets all four as the key's four output slots. This is SET_KEYMATRIX
    /// (0x0A) only — the Fn layer is a *separate* store, see
    /// [`set_fn_config`](Self::set_fn_config).
    ///
    /// `commit` is the keymatrix "enabled" byte, which is really the firmware's
    /// flash-dirty/save bit (it does not gate output). Set it on only the last write
    /// of a batch: one commit persists all the preceding RAM writes, mirroring the
    /// vendor webapp and avoiding repeated flash saves. A commit stalls the vendor
    /// pipeline, so it settles before returning.
    pub fn set_keymatrix_config(
        &self,
        profile: Profile,
        key_index: u8,
        layer: KeymatrixLayer,
        config: [u8; 4],
        commit: bool,
    ) -> Result<(), KeyboardError> {
        let pkt = SetKeyMatrixData::new(profile.get(), key_index, layer.get(), commit, config)?;
        if commit {
            self.transport.send_command_with_delay(
                self.commands.set_keymatrix,
                &pkt.to_data(),
                ChecksumType::Bit7,
                MAGNETISM_SETTLE_MS,
            )?;
        } else {
            self.transport.send_command(
                self.commands.set_keymatrix,
                &pkt.to_data(),
                ChecksumType::Bit7,
            )?;
        }
        Ok(())
    }

    /// Apply a full DKS configuration: mode, travel, trigger modes, and four combos.
    ///
    /// Sets the key's base mode to DKS (preserving any existing RT flag unless
    /// `rapid_trigger` is `Some`). Combos are written to keymatrix layers 0–3.
    pub fn set_dks_config(
        &self,
        key_index: u8,
        config: &DksConfig,
        rapid_trigger: Option<bool>,
    ) -> Result<(), KeyboardError> {
        let mut trigger = self.get_key_trigger(key_index)?;
        trigger.mode = KeyMode::DynamicKeystroke;
        if let Some(rt) = rapid_trigger {
            trigger.rapid_trigger = rt;
        }
        self.set_key_trigger(&trigger)?;

        let travel_bytes = config.trigger_point_travel_raw.to_le_bytes();
        self.set_magnetism_simple(mag_cmd::DKS_TRAVEL, key_index, false, &travel_bytes)?;

        let modes = config.trigger_modes();
        self.set_magnetism_simple(mag_cmd::DKS_TRIGGER_MODES_SET, key_index, true, &modes)?;

        // Binding 0 occupies keymatrix layer 0 — the key's *base* output when it
        // returns to Normal mode. Writing it empty stores keycode 0 and silences the
        // key (no ROM fallback for the base layer). If the caller left binding 0
        // empty, preserve the key's current layer-0 output instead of zeroing it.
        let mut configs: [[u8; 4]; 4] = std::array::from_fn(|i| config.bindings[i].config);
        configs[0] = resolve_slot0(
            configs[0],
            self.get_key_config_at_layer(self.active_profile(), KeymatrixLayer::BASE, key_index)?,
        );

        for (binding, config) in configs.into_iter().enumerate() {
            let slot = KeymatrixLayer::dks_slot(binding as u8).expect("4 bindings");
            // Persist once, on the final binding (commit = flash-dirty + settle).
            let commit = binding == 3;
            self.set_keymatrix_config(self.active_profile(), key_index, slot, config, commit)?;
        }
        Ok(())
    }

    // === Mod-Tap ===

    /// Read the Mod-Tap tap-vs-hold decision time (ms) for every key.
    ///
    /// Note: only the timing lives in the magnetism protocol; the tap/hold
    /// keycodes are configured through the normal keymap for that key.
    pub fn get_modtap_times(&self) -> Result<Vec<u16>, KeyboardError> {
        let kc = self.key_count as usize;
        let raw = self.get_magnetism(mag_cmd::MODTAP_TIME, kc.div_ceil(64))?;
        Ok(raw
            .into_iter()
            .take(kc)
            .map(|b| b as u16 * MODTAP_TIME_STEP_MS)
            .collect())
    }

    /// Set the Mod-Tap decision time (ms, rounded to the 10 ms wire step) for a
    /// single key.
    pub fn set_modtap_time(&self, key_index: u8, ms: u16) -> Result<(), KeyboardError> {
        let steps = (ms / MODTAP_TIME_STEP_MS).min(u8::MAX as u16) as u8;
        self.set_magnetism_simple(mag_cmd::MODTAP_TIME, key_index, true, &[steps])
    }

    // === Snap Tap (SOCD) ===

    /// Read each key's Snap-Tap partner index. `SNAPTAP_UNBOUND` means the key
    /// is not part of a pair.
    pub fn get_snaptap_binds(&self) -> Result<Vec<u8>, KeyboardError> {
        let kc = self.key_count as usize;
        let mut raw = self.get_magnetism(mag_cmd::SNAPTAP_ENABLE, kc.div_ceil(64))?;
        raw.truncate(kc);
        Ok(raw)
    }

    /// Bind two keys as a Snap-Tap (SOCD) pair. The binding is bidirectional, so
    /// both directions are written (matching the vendor app).
    pub fn set_snaptap_pair(&self, key_a: u8, key_b: u8) -> Result<(), KeyboardError> {
        self.set_magnetism_simple(mag_cmd::SNAPTAP_ENABLE, key_a, false, &[key_b])?;
        self.set_magnetism_simple(mag_cmd::SNAPTAP_ENABLE, key_b, true, &[key_a])
    }

    /// Clear a key's Snap-Tap binding, also clearing its partner's
    /// back-reference so the pair is fully dissolved.
    pub fn clear_snaptap(&self, key_index: u8) -> Result<(), KeyboardError> {
        let binds = self.get_snaptap_binds()?;
        let partner = binds
            .get(key_index as usize)
            .copied()
            .unwrap_or(SNAPTAP_UNBOUND);
        let partner_valid = partner != SNAPTAP_UNBOUND && (partner as usize) < binds.len();
        // If there is a partner, the second write carries the final/commit flag.
        self.set_magnetism_simple(
            mag_cmd::SNAPTAP_ENABLE,
            key_index,
            !partner_valid,
            &[SNAPTAP_UNBOUND],
        )?;
        if partner_valid {
            self.set_magnetism_simple(mag_cmd::SNAPTAP_ENABLE, partner, true, &[SNAPTAP_UNBOUND])?;
        }
        Ok(())
    }

    /// Set the bottom deadzone for all keys — the distance from the bottom of
    /// travel that is ignored.
    pub fn set_bottom_deadzone_all(&self, travel: TravelDepth) -> Result<(), KeyboardError> {
        let values = vec![travel.raw(); self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::BOTTOM_DEADZONE, &values)
    }

    /// Set the top deadzone for all keys — the distance from the top of travel
    /// that is ignored.
    pub fn set_top_deadzone_all(&self, travel: TravelDepth) -> Result<(), KeyboardError> {
        let values = vec![travel.raw(); self.key_count as usize];
        self.set_magnetism_u16(mag_cmd::TOP_DEADZONE, &values)
    }

    // === Extended LED Control ===

    /// Set LED mode with full parameters
    ///
    /// # Arguments
    /// * `mode` - LED mode (0-22)
    /// * `brightness` - Brightness level (0-4)
    /// * `speed` - Animation speed (0-4)
    /// * `r`, `g`, `b` - RGB color values
    /// * `dazzle` - Enable rainbow color cycling
    #[allow(clippy::too_many_arguments)]
    pub fn set_led(
        &self,
        mode: u8,
        brightness: u8,
        speed: u8,
        r: u8,
        g: u8,
        b: u8,
        dazzle: bool,
    ) -> Result<(), KeyboardError> {
        self.set_led_with_option(mode, brightness, speed, r, g, b, dazzle, 0)
    }

    /// Set LED mode with layer option (for UserPicture mode)
    ///
    /// For mode 13 (UserPicture):
    /// - `layer`: which custom color layer to display (0-3)
    /// - RGB values are ignored, using (0, 200, 200) per protocol
    #[allow(clippy::too_many_arguments)]
    pub fn set_led_with_option(
        &self,
        mode: u8,
        brightness: u8,
        speed: u8,
        r: u8,
        g: u8,
        b: u8,
        dazzle: bool,
        userpic_slot: u8,
    ) -> Result<(), KeyboardError> {
        let (option, r_val, g_val, b_val) = if mode == 13 {
            // For UserPicture mode: option = slot << 4, RGB = (0, 200, 200)
            (userpic_slot << 4, 0u8, 200u8, 200u8)
        } else {
            let opt = if dazzle {
                led::DAZZLE_ON
            } else {
                led::DAZZLE_OFF
            };
            (opt, r, g, b)
        };

        let data = [
            mode,
            led::SPEED_MAX - speed.min(led::SPEED_MAX), // Speed is inverted in protocol
            brightness.min(led::BRIGHTNESS_MAX),
            option,
            r_val,
            g_val,
            b_val,
        ];

        self.transport
            .send_command(cmd::SET_LEDPARAM, &data, ChecksumType::Bit8)?;

        Ok(())
    }

    /// Select a music-visualizer LED mode (MusicBars / MusicPatterns) with a
    /// style variant and color choice.
    ///
    /// These modes carry the **style** in the upper nibble of the option byte,
    /// which neither [`set_led_with_option`] nor [`set_led_params`] can express.
    /// The option **low nibble** selects the color source: `4` = solid custom
    /// RGB (from the color bytes); otherwise the firmware runs its built-in
    /// rainbow hue cycle. Speed has no effect on this effect, so the speed byte
    /// is fixed. The host then streams band levels via `SET_AUDIO_VIZ` (0x0D).
    ///
    /// [`set_led_with_option`]: Self::set_led_with_option
    /// [`set_led_params`]: Self::set_led_params
    pub fn set_music_viz_mode(
        &self,
        mode: u8,
        style: u8,
        brightness: u8,
        color: Option<(u8, u8, u8)>,
    ) -> Result<(), KeyboardError> {
        let (low, r, g, b) = match color {
            Some((r, g, b)) => (4u8, r, g, b),  // solid custom color
            None => (led::DAZZLE_OFF, 0, 0, 0), // built-in rainbow cycle
        };
        let data = [
            mode,
            0, // speed (inverted) — ignored by this effect
            brightness.min(led::BRIGHTNESS_MAX),
            (style << 4) | low,
            r,
            g,
            b,
        ];
        self.transport
            .send_command(cmd::SET_LEDPARAM, &data, ChecksumType::Bit8)?;
        Ok(())
    }

    /// Stream per-key colors for real-time effects
    ///
    /// # Arguments
    /// * `colors` - Tuple of (r, g, b) for each key (126 keys)
    /// * `repeat` - Number of times to send (for reliability)
    /// * `led_layer` - Which LED bank to update (0-3). Not a key layer.
    pub fn set_per_key_colors_fast(
        &self,
        colors: &[(u8, u8, u8)],
        repeat: u8,
        led_layer: u8,
    ) -> Result<(), KeyboardError> {
        const CHUNK_SIZE: usize = 18; // 18 keys per chunk (54 bytes RGB)

        // Pad colors to full matrix size
        let matrix_size = self.matrix_size();
        let mut full_colors = vec![(0u8, 0u8, 0u8); matrix_size];
        let len = colors.len().min(matrix_size);
        full_colors[..len].copy_from_slice(&colors[..len]);

        for _ in 0..repeat.max(1) {
            for (chunk_idx, chunk) in full_colors.chunks(CHUNK_SIZE).enumerate() {
                let mut data = vec![0u8; 56]; // layer + page + 54 RGB bytes
                data[0] = led_layer;
                data[1] = chunk_idx as u8;
                for (i, &(r, g, b)) in chunk.iter().enumerate() {
                    data[2 + i * 3] = r;
                    data[2 + i * 3 + 1] = g;
                    data[2 + i * 3 + 2] = b;
                }

                self.transport.send_command_with_delay(
                    cmd::SET_USERPIC,
                    &data,
                    ChecksumType::Bit8,
                    5,
                )?;
            }
        }

        Ok(())
    }

    /// Store per-key colors to a specific LED bank (not a key layer).
    pub fn set_per_key_colors_to_layer(
        &self,
        colors: &[(u8, u8, u8)],
        led_layer: u8,
    ) -> Result<(), KeyboardError> {
        self.set_per_key_colors_fast(colors, 1, led_layer)
    }

    // === Calibration ===

    /// Start/stop minimum position calibration (keys released)
    pub fn calibrate_min(&self, start: bool) -> Result<(), KeyboardError> {
        self.transport.send_command(
            cmd::SET_MAGNETISM_CAL,
            &[if start { 1 } else { 0 }],
            ChecksumType::Bit7,
        )?;
        Ok(())
    }

    /// Start/stop maximum position calibration (keys pressed)
    pub fn calibrate_max(&self, start: bool) -> Result<(), KeyboardError> {
        self.transport.send_command(
            cmd::SET_MAGNETISM_MAX_CAL,
            &[if start { 1 } else { 0 }],
            ChecksumType::Bit7,
        )?;
        Ok(())
    }

    /// Get calibration progress for a page of keys (32 keys per page)
    ///
    /// During max calibration, polls the keyboard for per-key calibration values.
    /// Values >= 300 indicate the key has been calibrated (pressed to bottom).
    ///
    /// # Arguments
    /// * `page` - Page number (0-3, each page has 32 keys)
    ///
    /// # Returns
    /// Vector of 16-bit calibration values for up to 32 keys
    pub fn get_calibration_progress(&self, page: u8) -> Result<Vec<u16>, KeyboardError> {
        let query = GetMultiMagnetismData::paged(mag_cmd::CALIBRATION, page);
        let response = self.transport.query_raw(
            cmd::GET_MULTI_MAGNETISM,
            query.as_bytes(),
            ChecksumType::Bit7,
        )?;

        // Decode 16-bit LE values from response (64 bytes = 32 values)
        let mut values = Vec::with_capacity(32);
        for chunk in response.chunks(2) {
            if chunk.len() == 2 {
                values.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
        }
        Ok(values)
    }

    // === Raw Commands (for CLI compatibility) ===

    /// Send a raw command and get response
    pub fn query_raw_cmd(&self, cmd_byte: u8) -> Result<Vec<u8>, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd_byte, &[], ChecksumType::Bit7)?;
        Ok(resp)
    }

    /// Send raw command with data
    pub fn query_raw_cmd_data(&self, cmd_byte: u8, data: &[u8]) -> Result<Vec<u8>, KeyboardError> {
        let resp = self
            .transport
            .query_command(cmd_byte, data, ChecksumType::Bit7)?;
        Ok(resp)
    }

    /// Send raw command without expecting response
    pub fn send_raw_cmd(&self, cmd_byte: u8, data: &[u8]) -> Result<(), KeyboardError> {
        self.transport
            .send_command(cmd_byte, data, ChecksumType::Bit7)?;
        Ok(())
    }

    /// Send a raw command with **no** inter-command flow-control delay, for
    /// high-rate streaming (audio visualizer, screen color). Plain
    /// [`send_raw_cmd`] sleeps `DEFAULT_DELAY_MS` (100ms) after every send —
    /// fine for config commands, but it caps streaming at ~10Hz. The device
    /// renders these every main-loop pass (hundreds of Hz), so the only pacing
    /// should be the caller's frame loop.
    ///
    /// [`send_raw_cmd`]: Self::send_raw_cmd
    pub fn send_raw_cmd_fast(&self, cmd_byte: u8, data: &[u8]) -> Result<(), KeyboardError> {
        self.transport
            .send_command_with_delay(cmd_byte, data, ChecksumType::Bit7, 0)?;
        Ok(())
    }

    // === Key Matrix (Key Remapping) ===

    /// Read a keymatrix layer for a profile.
    ///
    /// Both coordinates are explicit on purpose: an earlier `get_keymatrix(profile,
    /// pages)` wrapper defaulted the layer to 0, and every caller read its surviving
    /// argument as a layer — so `keymatrix 1` and the keymap loader silently worked
    /// on profile 1 instead of layer 1.
    ///
    /// # Arguments
    /// * `profile` - Profile index (0-3)
    /// * `layer` - Keymatrix layer 0-3. Layers 0/1 are the key's Base and Layer1
    ///   outputs; in DKS mode the firmware reinterprets all four as output slots.
    ///   The Fn layer is a separate store — see [`get_fn_keymatrix`](Self::get_fn_keymatrix).
    /// * `num_pages` - Number of pages to read (8 for full 126-key matrix)
    ///
    /// # Returns
    /// Raw key matrix data (4 bytes per key: `[config_type, b1, b2, b3]`)
    pub fn get_keymatrix(
        &self,
        profile: Profile,
        layer: KeymatrixLayer,
        num_pages: usize,
    ) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            let query = GetKeyMatrixData {
                profile: profile.get(),
                magic: 0xFF,
                page: page as u8,
                layer: layer.get(),
            };

            // `?`, never a skip, and a length-checked page. Callers index this buffer
            // by `key_index * 4`, so dropping — or shortening — 64 bytes mid-stream
            // slides every later key's binding onto the wrong key, and it still looks
            // like a successful read.
            let resp = self.transport.query_page(
                self.commands.get_keymatrix,
                query.as_bytes(),
                ChecksumType::Bit7,
                INPUT_REPORT_SIZE,
            )?;
            all_data.extend_from_slice(&resp);
        }

        if all_data.is_empty() {
            Err(KeyboardError::UnexpectedResponse(
                "No keymatrix data".into(),
            ))
        } else {
            Ok(all_data)
        }
    }

    /// Read the Fn layer key matrix using GET_FN (0x90).
    ///
    /// Unlike [`get_keymatrix`](Self::get_keymatrix) which reads base remaps via GET_KEYMATRIX (0x8A),
    /// this reads the actual Fn layer bindings (media keys, LED controls, etc.)
    /// via the dedicated GET_FN command.
    ///
    /// # Arguments
    /// * `profile` - Profile index (0-3)
    /// * `sys` - OS mode: 0=Windows, 1=Mac
    /// * `num_pages` - Number of pages to read (8 for full matrix)
    pub fn get_fn_keymatrix(
        &self,
        profile: Profile,
        sys: u8,
        num_pages: usize,
    ) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            let query = GetFnData {
                sys,
                profile: profile.get(),
                magic: 0xFF,
                page: page as u8,
            };
            // Same offset hazard as `get_keymatrix`.
            let resp = self.transport.query_page(
                cmd::GET_FN,
                query.as_bytes(),
                ChecksumType::Bit7,
                INPUT_REPORT_SIZE,
            )?;
            all_data.extend_from_slice(&resp);
        }

        if all_data.is_empty() {
            Err(KeyboardError::UnexpectedResponse(
                "GET_FN returned no data".into(),
            ))
        } else {
            Ok(all_data)
        }
    }

    /// Write a key's 4-byte config to the Fn layer (SET_FN, 0x10).
    ///
    /// The Fn layer is a separate store from the keymatrix; an all-zero entry there
    /// is transparent fall-through rather than silence.
    pub fn set_fn_config(
        &self,
        profile: Profile,
        key_index: u8,
        config: [u8; 4],
    ) -> Result<(), KeyboardError> {
        self.transport
            .send(&SetFnData::new(0, profile.get(), key_index, config)?)?;
        Ok(())
    }

    /// Set a single key's mapping (base layer only).
    ///
    /// For layer-aware remapping, use [`set_keymatrix_config`](Self::set_keymatrix_config)
    /// or [`set_fn_config`](Self::set_fn_config).
    pub fn set_keymatrix(
        &self,
        profile: Profile,
        key_index: u8,
        hid_code: u8,
        enabled: bool,
        layer: KeymatrixLayer,
    ) -> Result<(), KeyboardError> {
        let pkt = SetKeyMatrixData::new(
            profile.get(),
            key_index,
            layer.get(),
            enabled,
            [0, 0, hid_code, 0],
        )?;
        self.transport.send_command(
            self.commands.set_keymatrix,
            &pkt.to_data(),
            ChecksumType::Bit7,
        )?;
        Ok(())
    }

    /// Clear a key's entry on an *overlay* layer, where an all-zero config means
    /// transparent fall-through to the base layer.
    ///
    /// Not valid for keymatrix layer 0: the base layer has no ROM fallback, so an
    /// all-zero entry there silences the key. Reset it by writing the position's
    /// factory keycode instead.
    pub fn reset_key(&self, layer: Layer, key_index: u8) -> Result<(), KeyboardError> {
        match layer {
            Layer::Base => Err(KeyboardError::InvalidParameter(
                "base layer has no ROM fallback; write the factory keycode instead".into(),
            )),
            Layer::Fn => self.set_fn_config(self.active_profile(), key_index, [0, 0, 0, 0]),
            Layer::Layer1 => self.set_keymatrix_config(
                self.active_profile(),
                key_index,
                KeymatrixLayer::try_from(1).expect("1 is in range"),
                [0, 0, 0, 0],
                true,
            ),
        }
    }

    /// Swap two keys
    pub fn swap_keys(
        &self,
        profile: Profile,
        key_a: u8,
        code_a: u8,
        key_b: u8,
        code_b: u8,
    ) -> Result<(), KeyboardError> {
        // Set key_a to code_b
        self.set_keymatrix(profile, key_a, code_b, true, KeymatrixLayer::BASE)?;
        // Set key_b to code_a
        self.set_keymatrix(profile, key_b, code_a, true, KeymatrixLayer::BASE)
    }

    // === Macros ===

    /// Get macro data for a macro slot
    ///
    /// # Arguments
    /// * `macro_index` - Macro slot number (0-based)
    ///
    /// # Returns
    /// Raw macro data: [2-byte repeat count (LE), then 2-byte events (keycode, flags)]
    pub fn get_macro(&self, macro_index: MacroSlot) -> Result<Vec<u8>, KeyboardError> {
        let mut all_data = Vec::new();

        for page in 0..4u8 {
            let query = GetMacroData {
                macro_index: macro_index.get(),
                page,
            };

            // A skipped page shifts the rest of the macro, and the shifted bytes then
            // parse as a plausible-but-wrong keystroke sequence — so propagate.
            let resp = self.transport.query_page(
                cmd::GET_MACRO,
                query.as_bytes(),
                ChecksumType::Bit7,
                INPUT_REPORT_SIZE,
            )?;

            // Skip command echo if present (some transports may add it)
            let start = if !resp.is_empty() && resp[0] == cmd::GET_MACRO {
                1
            } else {
                0
            };
            if resp.len() > start {
                all_data.extend_from_slice(&resp[start..]);
            }

            // Check for 4 consecutive zeros (end marker)
            if resp[start..].windows(4).any(|w| w == [0, 0, 0, 0]) {
                break;
            }
        }

        if all_data.is_empty() {
            Err(KeyboardError::UnexpectedResponse("No macro data".into()))
        } else if all_data.iter().all(|&b| b == 0xFF) {
            // Uninitialized slot — treat as empty
            Ok(vec![0, 0]) // repeat_count=0, no events
        } else {
            Ok(all_data)
        }
    }

    /// Set macro data for a macro slot
    ///
    /// # Arguments
    /// * `macro_index` - Macro slot number (0-based)
    /// * `events` - List of (keycode, is_down, delay_ms) tuples with u16 delay
    /// * `repeat_count` - How many times to repeat the macro
    ///
    /// Events use variable-length encoding:
    /// - Short delay (0-127ms): 2 bytes `[keycode, direction_bit | delay]`
    /// - Long delay (128+ms): 4 bytes `[keycode, direction_bit, delay_lo, delay_hi]`
    pub fn set_macro(
        &self,
        macro_index: MacroSlot,
        events: &[(u8, bool, u16)],
        repeat_count: u16,
    ) -> Result<(), KeyboardError> {
        // Build macro data
        let mut macro_data = Vec::with_capacity(256);

        // 2-byte repeat count (little-endian)
        macro_data.push((repeat_count & 0xFF) as u8);
        macro_data.push((repeat_count >> 8) as u8);

        // Add events with variable-length encoding
        // Short format (1-127ms): 2 bytes [keycode, direction_bit | delay]
        // Long format (0ms or 128+ms): 4 bytes [keycode, direction_bit, delay_lo, delay_hi]
        // Note: 0ms uses long format to avoid ambiguity with the parser
        // (the parser treats low-7-bits==0 as long format indicator)
        for &(keycode, is_down, delay) in events {
            macro_data.push(keycode);
            if (1..=127).contains(&delay) {
                // Short format
                let flags = if is_down {
                    0x80 | (delay as u8)
                } else {
                    delay as u8
                };
                macro_data.push(flags);
            } else {
                // Long format (0ms or 128+ms)
                let flags = if is_down { 0x80 } else { 0x00 };
                macro_data.push(flags);
                macro_data.push((delay & 0xFF) as u8);
                macro_data.push((delay >> 8) as u8);
            }
        }

        // Pad to at least fill first page
        while macro_data.len() < 56 {
            macro_data.push(0);
        }

        // Send in pages of 56 bytes
        const PAGE_SIZE: usize = 56;
        let num_pages = macro_data.len().div_ceil(PAGE_SIZE);

        for page in 0..num_pages {
            let start = page * PAGE_SIZE;
            let end = (start + PAGE_SIZE).min(macro_data.len());
            let chunk = &macro_data[start..end];
            let is_last = page == num_pages - 1;

            let macro_cmd =
                SetMacroCommand::new(macro_index.get(), page as u8, is_last, chunk.to_vec())?;

            self.transport.send_command_with_delay(
                self.commands.set_macro,
                &macro_cmd.to_data(),
                ChecksumType::Bit7,
                30,
            )?;
        }

        Ok(())
    }

    /// Set a text macro (convenience method)
    ///
    /// # Arguments
    /// * `macro_index` - Macro slot number (0-based)
    /// * `text` - Text to type
    /// * `delay_ms` - Delay between keystrokes in ms
    /// * `repeat` - How many times to repeat
    pub fn set_text_macro(
        &self,
        macro_index: MacroSlot,
        text: &str,
        delay_ms: u16,
        repeat: u16,
    ) -> Result<(), KeyboardError> {
        use crate::hid_codes::char_to_hid;

        const LSHIFT: u8 = 0xE1; // Left Shift HID code
        let mut events = Vec::new();

        for ch in text.chars() {
            if let Some((keycode, needs_shift)) = char_to_hid(ch) {
                if needs_shift {
                    events.push((LSHIFT, true, 0u16)); // Shift down
                    events.push((keycode, true, delay_ms)); // Key down
                    events.push((keycode, false, 0u16)); // Key up
                    events.push((LSHIFT, false, delay_ms)); // Shift up
                } else {
                    events.push((keycode, true, delay_ms)); // Key down
                    events.push((keycode, false, delay_ms)); // Key up
                }
            }
        }

        self.set_macro(macro_index, &events, repeat)
    }

    /// Assign a macro to a key on any layer.
    ///
    /// * `macro_type` - 0=repeat by count, 1=toggle, 2=hold to repeat
    pub fn assign_macro_to_key(
        &self,
        layer: Layer,
        key_index: u8,
        macro_index: MacroSlot,
        macro_type: u8,
    ) -> Result<(), KeyboardError> {
        let config = [9, macro_type, macro_index.get(), 0];
        match layer.keymatrix_layer() {
            Some(km) => {
                self.set_keymatrix_config(self.active_profile(), key_index, km, config, true)
            }
            None => self.set_fn_config(self.active_profile(), key_index, config),
        }
    }

    /// Remove macro assignment from a key, restoring default behavior.
    pub fn unassign_macro_from_key(
        &self,
        layer: Layer,
        key_index: u8,
    ) -> Result<(), KeyboardError> {
        self.reset_key(layer, key_index)
    }

    // === Connection ===

    /// Check if the keyboard is still connected
    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    /// Close the connection
    pub fn close(&self) -> Result<(), KeyboardError> {
        self.transport.close()?;
        Ok(())
    }

    // === Patch Features ===

    /// Stream a page of per-key RGB data to the LED frame buffer (patched firmware)
    ///
    /// Writes 18 keys of RGB data directly to the WS2812 frame buffer without
    /// touching flash. Call `stream_led_commit()` after sending all pages to
    /// update the LEDs.
    ///
    /// Uses zero delay — the firmware handles 0xE8 instantly (memcpy to frame
    /// buffer), so the default 100ms flow-control delay is unnecessary and would
    /// limit throughput to ~1.4 FPS.
    ///
    /// # Arguments
    /// * `page` - Page index (0-6, each page = 18 keys)
    /// * `rgb_data` - RGB data (up to 54 bytes = 18 keys × 3 bytes)
    pub fn stream_led_page(&self, page: u8, rgb_data: &[u8]) -> Result<(), KeyboardError> {
        let mut data = vec![0u8; 55]; // page + 54 RGB bytes
        data[0] = page;
        let len = rgb_data.len().min(54);
        data[1..1 + len].copy_from_slice(&rgb_data[..len]);
        self.transport
            .send_command_with_delay(cmd::LED_STREAM, &data, ChecksumType::None, 0)?;
        Ok(())
    }

    /// Commit streamed LED data — copies frame buffer to DMA buffer for display
    pub fn stream_led_commit(&self) -> Result<(), KeyboardError> {
        self.transport
            .send_command_with_delay(cmd::LED_STREAM, &[0xFF], ChecksumType::None, 0)?;
        Ok(())
    }

    /// Send sparse overlay update: set specific LEDs by matrix index.
    ///
    /// Each entry is `(matrix_idx, r, g, b)` where `matrix_idx = row*16 + col`.
    /// The firmware maps matrix index to strip index via `static_led_pos_tbl`.
    /// Max 13 entries per packet; larger slices are chunked automatically.
    pub fn stream_led_sparse(&self, entries: &[(u8, u8, u8, u8)]) -> Result<(), KeyboardError> {
        for chunk in entries.chunks(13) {
            let mut data = vec![0u8; 2 + chunk.len() * 4]; // page + count + entries
            data[0] = 0xFD;
            data[1] = chunk.len() as u8;
            for (i, &(idx, r, g, b)) in chunk.iter().enumerate() {
                data[2 + i * 4] = idx;
                data[2 + i * 4 + 1] = r;
                data[2 + i * 4 + 2] = g;
                data[2 + i * 4 + 3] = b;
            }
            self.transport.send_command_with_delay(
                cmd::LED_STREAM,
                &data,
                ChecksumType::None,
                0,
            )?;
        }
        Ok(())
    }

    /// Release LED streaming — signals end of streaming session
    pub fn stream_led_release(&self) -> Result<(), KeyboardError> {
        self.transport
            .send_command_with_delay(cmd::LED_STREAM, &[0xFE], ChecksumType::None, 0)?;
        Ok(())
    }

    // ── On-device animation engine (0xEA) ────────────────────────────
    // Uses typed HidCommand/HidResponse from monsgeek-transport::command.

    /// Define an animation on the firmware.
    ///
    /// `keyframes` is a slice of `(t_ticks, color_rgb565, easing)` tuples.
    /// If more than 4 keyframes, sends a DEF_EXT packet automatically.
    pub fn anim_define(
        &self,
        def_id: DefId,
        flags: u8,
        priority: i8,
        duration_ticks: u16,
        keyframes: &[(u16, u16, u8)],
    ) -> Result<(), KeyboardError> {
        use monsgeek_transport::command::{AnimDefine, AnimDefineExt};
        let num_kf = keyframes.len().min(8) as u8;
        // Use query_command (not send) — ensures dongle relay completes
        self.transport.query_command(
            cmd::ANIM_CMD,
            &AnimDefine {
                def_id,
                num_kf,
                flags,
                priority,
                duration_ticks,
                keyframes: keyframes.to_vec(),
            }
            .to_data(),
            ChecksumType::None,
        )?;
        if num_kf > 4 {
            self.transport.query_command(
                cmd::ANIM_CMD,
                &AnimDefineExt {
                    def_id,
                    keyframes: keyframes[4..num_kf as usize].to_vec(),
                }
                .to_data(),
                ChecksumType::None,
            )?;
        }
        Ok(())
    }

    /// Assign keys to an animation definition.
    ///
    /// `keys` is a slice of `(LedPos, phase_offset)` pairs — the *grid* space.
    /// [`Self::anim_query_keys`] reads back the *strip* space; the firmware
    /// converts, so the two are deliberately not the same type.
    /// Chunked automatically for packets > 29 entries.
    pub fn anim_assign(&self, def_id: DefId, keys: &[(LedPos, u8)]) -> Result<(), KeyboardError> {
        use monsgeek_transport::command::AnimAssign;
        for chunk in keys.chunks(29) {
            self.transport.query_command(
                cmd::ANIM_CMD,
                &AnimAssign {
                    def_id,
                    keys: chunk.iter().map(|&(p, off)| (p.get(), off)).collect(),
                }
                .to_data(),
                ChecksumType::None,
            )?;
        }
        Ok(())
    }

    /// Cancel a specific animation definition and release its keys.
    pub fn anim_cancel(&self, def_id: DefId) -> Result<(), KeyboardError> {
        self.transport.query_command(
            cmd::ANIM_CMD,
            &monsgeek_transport::command::AnimCancel { def_id }.to_data(),
            ChecksumType::None,
        )?;
        Ok(())
    }

    /// Clear all animations and overlay.
    pub fn anim_clear(&self) -> Result<(), KeyboardError> {
        self.transport.query_command(
            cmd::ANIM_CMD,
            &monsgeek_transport::command::AnimClear.to_data(),
            ChecksumType::None,
        )?;
        Ok(())
    }

    /// Query animation engine status.
    ///
    /// Returns `None` if the firmware doesn't support the animation engine.
    pub fn anim_query(&self) -> Result<Option<AnimStatus>, KeyboardError> {
        use monsgeek_transport::command::{AnimQuery, AnimQueryResponse};
        match self
            .transport
            .query::<AnimQuery, AnimQueryResponse>(&AnimQuery)
        {
            Ok(r) => Ok(Some(AnimStatus {
                active_count: r.active_count,
                frame_count: r.frame_count,
                overlay_active: r.overlay_active,
                // A slot id outside 0-7 is firmware the driver does not
                // understand; drop the entry rather than address a wrong slot.
                defs: r
                    .defs
                    .into_iter()
                    .filter_map(|d| {
                        Some(AnimDefStatus {
                            id: DefId::try_from(d.id).ok()?,
                            num_kf: d.num_kf,
                            flags: d.flags,
                            priority: d.priority,
                            key_count: d.key_count,
                            duration_ticks: d.duration_ticks,
                        })
                    })
                    .collect(),
            })),
            Err(monsgeek_transport::TransportError::InvalidResponse { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Query key assignments for an animation definition slot.
    ///
    /// Returns `(StripIdx, phase_offset)` pairs for all keys assigned to the def.
    ///
    /// Note the asymmetry with [`Self::anim_assign`], which takes [`LedPos`]:
    /// the firmware stores its key table in strip order. Round-tripping needs a
    /// deliberate conversion through the firmware's `static_led_pos_tbl`.
    pub fn anim_query_keys(&self, def_id: DefId) -> Result<Vec<(StripIdx, u8)>, KeyboardError> {
        use monsgeek_transport::command::{AnimQueryKeys, AnimQueryKeysResponse};
        match self
            .transport
            .query::<AnimQueryKeys, AnimQueryKeysResponse>(&AnimQueryKeys { def_id })
        {
            Ok(r) => Ok(r
                .keys
                .into_iter()
                .map(|(i, off)| (StripIdx::new(i), off))
                .collect()),
            Err(_) => Ok(Vec::new()),
        }
    }

    /// Query patch info from modded firmware
    ///
    /// Returns `Some(PatchInfo)` if the keyboard is running patched firmware,
    /// `None` if it's running stock firmware (response doesn't contain the
    /// expected magic bytes).
    pub fn get_patch_info(&self) -> Result<Option<PatchInfo>, KeyboardError> {
        let resp = self
            .transport
            .query_raw(cmd::GET_PATCH_INFO, &[], ChecksumType::Bit7)?;

        // Response layout: resp[0]=cmd echo (0xE7), resp[1..2]=magic,
        // resp[3]=ver, resp[4..5]=caps, resp[6..]=name.
        // (GET_REPORT returns from lp_class_report_buf = cmd_buf+2,
        //  handler writes magic at cmd_buf[3..4], so resp[1..2])
        if resp.len() < 8 || resp[1] != 0xCA || resp[2] != 0xFE {
            return Ok(None);
        }
        let version = resp[3];
        let capabilities = u16::from_le_bytes([resp[4], resp[5]]);
        let name_end = resp.len().min(14);
        let name_bytes = &resp[6..name_end];
        let name_len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());
        let name = String::from_utf8_lossy(&name_bytes[..name_len]).to_string();
        Ok(Some(PatchInfo {
            version,
            capabilities,
            name,
        }))
    }

    /// Query dongle patch info via HID Feature Report ID 8.
    ///
    /// Returns `Some(PatchInfo)` if the dongle is running patched firmware,
    /// `None` if it's stock or the transport doesn't support it (wired/BLE).
    pub fn get_dongle_patch_info(&self) -> Result<Option<PatchInfo>, KeyboardError> {
        let Some(buf) = self.transport.inner().get_dongle_patch_info()? else {
            return Ok(None);
        };
        // buf[0] = report ID 8, buf[1..2] = magic, buf[3] = ver,
        // buf[4..5] = caps LE16, buf[6..] = name
        if buf.len() < 8 || buf[1] != 0xCA || buf[2] != 0xFE {
            return Ok(None);
        }
        let version = buf[3];
        let capabilities = u16::from_le_bytes([buf[4], buf[5]]);
        let name_end = buf.len().min(14);
        let name_bytes = &buf[6..name_end];
        let name_len = name_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_bytes.len());
        let name = String::from_utf8_lossy(&name_bytes[..name_len]).to_string();
        Ok(Some(PatchInfo {
            version,
            capabilities,
            name,
        }))
    }

    /// Subscribe to timestamped vendor events via broadcast channel
    ///
    /// Returns a receiver for asynchronous vendor event notifications.
    /// Events are pushed from a dedicated reader thread with near-zero latency
    /// when data arrives. Each event includes a timestamp (seconds since transport
    /// was opened) for accurate timing in visualizations.
    ///
    /// Returns None if event subscriptions are not supported (no input endpoint).
    pub fn subscribe_events(&self) -> Option<tokio::sync::broadcast::Receiver<TimestampedEvent>> {
        self.transport.subscribe_events()
    }
}

impl Keyboard for RyKeyboard {
    fn device_info(&self) -> &TransportDeviceInfo {
        self.transport.device_info()
    }

    /// Get firmware version
    fn get_version(&self) -> Result<FirmwareVersion, KeyboardError> {
        // Use GET_USB_VERSION which returns device_id and version
        let resp = self
            .transport
            .query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7)?;

        if resp.len() < 9 || resp[0] != cmd::GET_USB_VERSION {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid version response".into(),
            ));
        }

        // GET_USB_VERSION response (after report ID stripped):
        // [0] = cmd echo, [1..5] = device_id, [7..9] = version
        let raw = u16::from_le_bytes([resp[7], resp[8]]);
        Ok(FirmwareVersion::new(raw))
    }

    /// Get the profile the keyboard is currently switched to.
    fn get_profile(&self) -> Result<Profile, KeyboardError> {
        let cmd = self.commands.get_profile;
        let resp = self.transport.query_command(cmd, &[], ChecksumType::Bit7)?;
        if resp.is_empty() || resp[0] != cmd {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid profile response".into(),
            ));
        }
        Profile::try_from(resp[1]).map_err(KeyboardError::InvalidParameter)
    }

    /// Switch the keyboard to a profile.
    fn set_profile(&self, profile: Profile) -> Result<(), KeyboardError> {
        self.transport.send_command(
            self.commands.set_profile,
            &[profile.get()],
            ChecksumType::Bit7,
        )?;
        Ok(())
    }

    /// RY5088-only, uses GET_REPORT.
    fn get_polling_rate(&self) -> Result<u16, KeyboardError> {
        let cmd_byte = self.commands.get_report.ok_or_else(|| {
            KeyboardError::NotSupported("Polling rate not available on this device".into())
        })?;
        let resp = self
            .transport
            .query_command(cmd_byte, &[], ChecksumType::Bit7)?;
        if resp.len() < POLLING_RATE_FRAME_OFFSET + 1 || resp[0] != cmd_byte {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid polling rate response".into(),
            ));
        }
        let code = resp[POLLING_RATE_FRAME_OFFSET];
        PollingRate::from_protocol(code)
            .map(PollingRate::to_hz)
            .ok_or_else(|| {
                KeyboardError::UnexpectedResponse(format!("Unknown polling rate: 0x{code:02X}"))
            })
    }

    /// RY5088-only, uses SET_REPORT.
    fn set_polling_rate(&self, hz: u16) -> Result<(), KeyboardError> {
        let cmd_byte = self.commands.set_report.ok_or_else(|| {
            KeyboardError::NotSupported("Polling rate not available on this device".into())
        })?;
        let rate = PollingRate::from_hz(hz).ok_or_else(|| {
            KeyboardError::InvalidParameter(format!(
                "{hz} Hz is not a polling rate; valid rates are 125, 250, 500, 1000, 2000, 4000, 8000"
            ))
        })?;
        if !self.polling_rates.is_empty() && !self.polling_rates.contains(&hz) {
            let max = self.polling_rates.iter().max().copied().unwrap_or(0);
            return Err(KeyboardError::NotSupported(format!(
                "{hz} Hz is above this device's maximum of {max} Hz"
            )));
        }
        // Payload starts one byte after the command, so pad to reach the code's slot.
        self.transport
            .send_command(cmd_byte, &[0, rate as u8], ChecksumType::Bit7)?;
        Ok(())
    }

    /// Get debounce time in milliseconds
    fn get_debounce(&self) -> Result<u8, KeyboardError> {
        let cmd_byte = self.commands.get_debounce;
        let resp = self
            .transport
            .query_command(cmd_byte, &[], ChecksumType::Bit7)?;
        if resp.is_empty() || resp[0] != cmd_byte {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid debounce response".into(),
            ));
        }
        Ok(resp[1])
    }

    /// Set debounce time in milliseconds (0-50)
    fn set_debounce(&self, ms: u8) -> Result<(), KeyboardError> {
        if ms > 50 {
            return Err(KeyboardError::InvalidParameter(
                "Debounce must be 0-50ms".into(),
            ));
        }
        self.transport
            .send_command(self.commands.set_debounce, &[ms], ChecksumType::Bit7)?;
        Ok(())
    }

    /// Get sleep time settings for all wireless modes (RY5088-only)
    ///
    /// Returns idle and deep sleep timeouts for both Bluetooth and 2.4GHz.
    /// All values are in seconds.
    fn get_sleep_time(&self) -> Result<SleepTimeSettings, KeyboardError> {
        let cmd_byte = self.commands.get_sleeptime.ok_or_else(|| {
            KeyboardError::NotSupported("Sleep time not available on this device".into())
        })?;
        let resp = self
            .transport
            .query_command(cmd_byte, &[], ChecksumType::Bit7)?;
        if resp.len() < 16 || resp[0] != cmd_byte {
            return Err(KeyboardError::UnexpectedResponse(
                "Invalid sleep time response".into(),
            ));
        }
        Ok(SleepTimeSettings {
            idle_bt: u16::from_le_bytes([resp[8], resp[9]]),
            idle_24g: u16::from_le_bytes([resp[10], resp[11]]),
            deep_bt: u16::from_le_bytes([resp[12], resp[13]]),
            deep_24g: u16::from_le_bytes([resp[14], resp[15]]),
        })
    }

    /// Set sleep time settings for all wireless modes (RY5088-only)
    ///
    /// Sets idle and deep sleep timeouts for both Bluetooth and 2.4GHz.
    /// All values are in seconds. Set to 0 to disable a particular timeout.
    fn set_sleep_time(&self, settings: &SleepTimeSettings) -> Result<(), KeyboardError> {
        let cmd_byte = self.commands.set_sleeptime.ok_or_else(|| {
            KeyboardError::NotSupported("Sleep time not available on this device".into())
        })?;
        // Build data with same layout as SetSleepTime::to_data()
        let mut data = vec![0u8; 15];
        data[7..9].copy_from_slice(&settings.idle_bt.to_le_bytes());
        data[9..11].copy_from_slice(&settings.idle_24g.to_le_bytes());
        data[11..13].copy_from_slice(&settings.deep_bt.to_le_bytes());
        data[13..15].copy_from_slice(&settings.deep_24g.to_le_bytes());
        self.transport
            .send_command(cmd_byte, &data, ChecksumType::Bit7)?;
        Ok(())
    }

    /// Factory reset the keyboard
    fn reset(&self) -> Result<(), KeyboardError> {
        self.transport
            .send_command(self.commands.set_reset, &[], ChecksumType::Bit7)?;
        Ok(())
    }
}

/// A single parsed macro event
#[derive(Debug, Clone)]
pub struct MacroEvent {
    pub keycode: u8,
    pub is_down: bool,
    pub delay_ms: u16,
}

/// Parse raw macro data into repeat count and structured events.
///
/// Input `data` should be the full macro data (starting with 2-byte LE repeat count).
/// Events use variable-length encoding:
/// - Short delay (0-127ms): 2 bytes `[keycode, direction_bit | delay]`
/// - Long delay (128+ms): 4 bytes `[keycode, direction_bit, delay_lo, delay_hi]`
///
/// Returns `(repeat_count, events)`. Stops on `[0, 0]` end marker or end of data.
pub fn parse_macro_events(data: &[u8]) -> (u16, Vec<MacroEvent>) {
    if data.len() < 2 {
        return (0, Vec::new());
    }

    let repeat_count = u16::from_le_bytes([data[0], data[1]]);
    let mut events = Vec::new();
    let mut pos = 2;

    while pos + 1 < data.len() {
        let keycode = data[pos];
        let flags = data[pos + 1];

        // End marker: [0, 0]
        if keycode == 0 && flags == 0 {
            break;
        }

        let is_down = (flags & 0x80) != 0;
        let delay_low_bits = flags & 0x7F;

        if delay_low_bits == 0 && pos + 3 < data.len() {
            // Long format: direction-only byte followed by 16-bit LE delay
            let delay_ms = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
            events.push(MacroEvent {
                keycode,
                is_down,
                delay_ms,
            });
            pos += 4;
        } else {
            // Short format: delay encoded in low 7 bits
            events.push(MacroEvent {
                keycode,
                is_down,
                delay_ms: delay_low_bits as u16,
            });
            pos += 2;
        }
    }

    (repeat_count, events)
}

#[cfg(test)]
mod tests {
    use super::resolve_slot0;

    /// DKS slot 0 doubles as keymatrix layer 0, which has no ROM fallback. Leaving it
    /// empty must preserve whatever the key emits today rather than silencing it —
    /// including non-key actions such as a bound macro, which the old
    /// `DksCombo::from_config_bytes(..).unwrap_or_default()` read silently discarded.
    #[test]
    fn dks_slot0_preserves_current_output_when_left_empty() {
        assert_eq!(resolve_slot0([0; 4], [9, 0, 3, 0]), [9, 0, 3, 0]);
        assert_eq!(resolve_slot0([0; 4], [0, 0, 0x06, 0]), [0, 0, 0x06, 0]);
        // An explicit request always wins.
        assert_eq!(
            resolve_slot0([0, 0xE0, 0x06, 0], [0, 0, 0x04, 0]),
            [0, 0xE0, 0x06, 0]
        );
        // Nothing to preserve.
        assert_eq!(resolve_slot0([0; 4], [0; 4]), [0; 4]);
    }
}
