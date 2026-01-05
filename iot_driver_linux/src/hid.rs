// MonsGeek/Akko HID Communication
// Shared device access code with multi-device support

use hidapi::{HidApi, HidDevice};
use std::sync::Arc;
use std::time::Duration;

use crate::devices::{self, DeviceDefinition};
use crate::hal;
use crate::profile::{profile_registry, DeviceProfile};
use crate::protocol::{self, cmd, firmware, firmware_update, rgb, timing, ChecksumType};

/// Connected device info (VID, PID, path)
#[derive(Debug, Clone)]
pub struct ConnectedDeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub path: String,
    pub definition: &'static DeviceDefinition,
}

/// Vendor event types from HID input reports
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorEventType {
    /// Key depth/magnetism data (byte[0] = 0x1B)
    KeyDepth,
    /// Magnetism reporting started (0x0F, 0x01, 0x00)
    MagnetismStart,
    /// Magnetism reporting stopped (0x0F, 0x00, 0x00)
    MagnetismStop,
    /// Profile changed (byte[0] = 0x01)
    ProfileChange,
    /// LED effect changed (byte[0] = 0x04-0x07)
    LedChange,
    /// Magnetism mode changed (byte[0] = 0x1D)
    MagnetismModeChange,
    /// Battery/wireless status from dongle (byte[0] = 0x88)
    BatteryStatus,
    /// Unknown event type
    Unknown,
}

/// Battery/power status from wireless dongle
#[derive(Debug, Clone, Copy, Default)]
pub struct BatteryInfo {
    /// Battery level 0-100 (255 = unknown/wired)
    pub level: u8,
    /// Device is online/connected
    pub online: bool,
    /// Device is charging
    pub charging: bool,
}

impl BatteryInfo {
    /// Parse battery info from dongle feature report response
    /// Discovered format from 2.4GHz dongle (VID:3151 PID:5038):
    /// - byte[0] = 0x00 (response report ID)
    /// - byte[1] = battery level (0-100)
    /// - byte[2] = charging flag (0=no, 1=yes)
    /// - byte[3] = online flag (0=disconnected, 1=connected)
    /// - bytes[4-6] = unknown flags (typically 0x01 0x01 0x01)
    pub fn from_feature_report(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        // Response starts at byte 1 (byte 0 is report ID)
        let level = data[1];
        let charging = data[2] != 0;
        let online = data[3] != 0;

        // Sanity check - battery level should be 0-100
        if level > 100 {
            return None;
        }

        Some(Self {
            level,
            online,
            charging,
        })
    }

    /// Parse battery info from vendor input event (legacy format)
    /// Format (speculative, from firmware analysis):
    /// - byte[0] = 0x88 (status marker)
    /// - byte[3] = battery level (1-100)
    /// - byte[4] = flags (bit 0 = online, bit 1 = charging)
    pub fn from_vendor_event(data: &[u8]) -> Option<Self> {
        // Skip report ID if present
        let cmd_data = if data.first() == Some(&0x05) && data.len() > 1 {
            &data[1..]
        } else {
            data
        };

        if cmd_data.len() < 5 || cmd_data[0] != 0x88 {
            return None;
        }

        Some(Self {
            level: cmd_data[3],
            online: cmd_data[4] & 0x01 != 0,
            charging: cmd_data[4] & 0x02 != 0,
        })
    }

    /// Check if this is valid battery data (not wired/unknown)
    pub fn is_valid(&self) -> bool {
        self.level <= 100
    }
}

/// Per-key trigger settings
#[derive(Debug, Clone, Default)]
pub struct TriggerSettings {
    /// Press travel (actuation point) per key, in precision units
    pub press_travel: Vec<u8>,
    /// Lift travel (release point) per key
    pub lift_travel: Vec<u8>,
    /// Rapid Trigger press sensitivity per key
    pub rt_press: Vec<u8>,
    /// Rapid Trigger lift sensitivity per key
    pub rt_lift: Vec<u8>,
    /// Key mode per key (0=Normal, 2=DKS, 3=MT, 4=TglHold, 5=TglDots, 7=Snap, +128=RT)
    pub key_modes: Vec<u8>,
    /// Bottom deadzone per key (travel below which key won't deactivate)
    pub bottom_deadzone: Vec<u8>,
    /// Top deadzone per key (travel above which key won't activate)
    pub top_deadzone: Vec<u8>,
}

/// Key mode values for per-key settings
pub mod key_mode {
    /// Normal mode - simple actuation/release points
    pub const NORMAL: u8 = 0;
    /// Dynamic Keystroke - 4-stage trigger
    pub const DKS: u8 = 2;
    /// Mod-Tap - different action for tap vs hold
    pub const MOD_TAP: u8 = 3;
    /// Toggle on hold
    pub const TOGGLE_HOLD: u8 = 4;
    /// Toggle on double-tap
    pub const TOGGLE_DOTS: u8 = 5;
    /// Snap-tap - bind to another key
    pub const SNAP_TAP: u8 = 7;
    /// Rapid Trigger flag (OR with mode)
    pub const RT_FLAG: u8 = 0x80;

    /// Check if RT is enabled for this mode
    pub fn has_rt(mode: u8) -> bool {
        mode & RT_FLAG != 0
    }

    /// Get base mode without RT flag
    pub fn base_mode(mode: u8) -> u8 {
        mode & 0x7F
    }

    /// Get mode name
    pub fn name(mode: u8) -> &'static str {
        let base = base_mode(mode);
        match base {
            NORMAL => "Normal",
            DKS => "DKS",
            MOD_TAP => "Mod-Tap",
            TOGGLE_HOLD => "TglHold",
            TOGGLE_DOTS => "TglDots",
            SNAP_TAP => "SnapTap",
            _ => "Unknown",
        }
    }
}

/// Device information read from keyboard
#[derive(Debug, Clone, Default)]
pub struct DeviceInfo {
    pub device_id: u32,
    pub version: u16,
    pub profile: u8,
    pub debounce: u8,
    pub polling_rate: u16,
    pub led_mode: u8,
    pub led_brightness: u8,
    pub led_speed: u8,
    pub led_r: u8,
    pub led_g: u8,
    pub led_b: u8,
    pub led_dazzle: bool,
    // Secondary LED (side lights)
    pub side_mode: u8,
    pub side_brightness: u8,
    pub side_speed: u8,
    pub side_r: u8,
    pub side_g: u8,
    pub side_b: u8,
    pub side_dazzle: bool,
    // Other settings
    pub fn_layer: u8,
    pub wasd_swap: bool,
    pub precision: u8,
    pub sleep_seconds: u16,
}

/// MonsGeek/Akko keyboard device wrapper
pub struct MonsGeekDevice {
    device: HidDevice,
    pub vid: u16,
    pub pid: u16,
    pub path: String,
    pub definition: &'static DeviceDefinition,
}

impl MonsGeekDevice {
    /// Find and open any supported device
    pub fn open() -> Result<Self, String> {
        let hidapi = HidApi::new().map_err(|e| format!("HID init failed: {e}"))?;

        for dev_info in hidapi.device_list() {
            let vid = dev_info.vendor_id();
            let pid = dev_info.product_id();

            // Check if this is a supported device
            if let Some(definition) = devices::find_device(vid, pid) {
                if dev_info.usage_page() == hal::USAGE_PAGE
                    && dev_info.usage() == hal::USAGE_FEATURE
                {
                    let device = dev_info
                        .open_device(&hidapi)
                        .map_err(|e| format!("Failed to open device: {e}"))?;
                    let path = dev_info.path().to_string_lossy().to_string();
                    return Ok(Self { device, vid, pid, path, definition });
                }
            }
        }
        Err("No supported MonsGeek/Akko device found".to_string())
    }

    /// Open a specific device by VID/PID
    pub fn open_device(vid: u16, pid: u16) -> Result<Self, String> {
        let definition = devices::find_device(vid, pid)
            .ok_or_else(|| format!("Device {vid:04x}:{pid:04x} not in supported list"))?;

        let hidapi = HidApi::new().map_err(|e| format!("HID init failed: {e}"))?;

        for dev_info in hidapi.device_list() {
            if dev_info.vendor_id() == vid
                && dev_info.product_id() == pid
                && dev_info.usage_page() == hal::USAGE_PAGE
                && dev_info.usage() == hal::USAGE_FEATURE
            {
                let device = dev_info
                    .open_device(&hidapi)
                    .map_err(|e| format!("Failed to open device: {e}"))?;
                let path = dev_info.path().to_string_lossy().to_string();
                return Ok(Self { device, vid, pid, path, definition });
            }
        }
        Err(format!("Device {vid:04x}:{pid:04x} not connected"))
    }

    /// Open the vendor INPUT interface for a device (for receiving key depth, events)
    /// This is separate from the main device which uses the FEATURE interface
    pub fn open_input_interface(vid: u16, pid: u16) -> Result<HidDevice, String> {
        let hidapi = HidApi::new().map_err(|e| format!("HID init failed: {e}"))?;

        for dev_info in hidapi.device_list() {
            if dev_info.vendor_id() == vid
                && dev_info.product_id() == pid
                && dev_info.usage_page() == hal::USAGE_PAGE
                && dev_info.usage() == hal::USAGE_INPUT
            {
                println!("Opening INPUT interface: {} (usage {:04x}, page {:04x})",
                    dev_info.path().to_string_lossy(),
                    dev_info.usage(),
                    dev_info.usage_page()
                );
                return dev_info
                    .open_device(&hidapi)
                    .map_err(|e| format!("Failed to open input interface: {e}"));
            }
        }
        Err(format!("Input interface for {vid:04x}:{pid:04x} not found"))
    }

    /// List all connected supported devices
    pub fn list_connected() -> Vec<ConnectedDeviceInfo> {
        let mut devices_found = Vec::new();

        if let Ok(hidapi) = HidApi::new() {
            for dev_info in hidapi.device_list() {
                let vid = dev_info.vendor_id();
                let pid = dev_info.product_id();

                if let Some(definition) = devices::find_device(vid, pid) {
                    if dev_info.usage_page() == hal::USAGE_PAGE
                        && dev_info.usage() == hal::USAGE_FEATURE
                    {
                        devices_found.push(ConnectedDeviceInfo {
                            vid,
                            pid,
                            path: dev_info.path().to_string_lossy().to_string(),
                            definition,
                        });
                    }
                }
            }
        }

        devices_found
    }

    /// Get the device definition
    pub fn definition(&self) -> &'static DeviceDefinition {
        self.definition
    }

    /// Get display name from device definition
    pub fn display_name(&self) -> &'static str {
        self.definition.display_name
    }

    /// Get key count from device definition
    pub fn key_count(&self) -> u8 {
        self.definition.key_count
    }

    /// Check if device has magnetism support
    pub fn has_magnetism(&self) -> bool {
        self.definition.has_magnetism
    }

    /// Get the device profile for this device
    /// Returns the profile from the registry, which includes matrix key mappings
    pub fn profile(&self) -> Option<Arc<dyn DeviceProfile>> {
        profile_registry().find_by_vid_pid(self.vid, self.pid)
    }

    /// Get key name for a matrix position using the device profile
    /// Falls back to "?" if profile not found or position out of range
    pub fn matrix_key_name(&self, position: u8) -> &'static str {
        // Use the builtin profile directly for static lifetime
        use crate::profile::builtin::M1V5HeProfile;

        // For now, we only have M1 V5 HE profiles
        // Return static reference from the builtin profile
        static PROFILE: std::sync::OnceLock<M1V5HeProfile> = std::sync::OnceLock::new();
        let profile = PROFILE.get_or_init(M1V5HeProfile::new);
        profile.matrix_key_name(position)
    }

    /// Send a command and receive response using retry pattern for Linux HID buffering
    pub fn query(&self, cmd: u8) -> Option<Vec<u8>> {
        self.query_with_data(cmd, &[])
    }

    /// Send a command with additional data bytes
    pub fn query_with_data(&self, cmd: u8, data: &[u8]) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; protocol::REPORT_SIZE];
        buf[0] = 0; // Report ID
        buf[1] = cmd;
        for (i, &b) in data.iter().enumerate() {
            if i + 2 < protocol::REPORT_SIZE {
                buf[i + 2] = b;
            }
        }
        // Apply Bit7 checksum
        let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
        buf[8] = (255 - (sum & 0xFF)) as u8;

        // Retry pattern for Linux HID feature report buffering
        for _ in 0..timing::QUERY_RETRIES {
            if self.device.send_feature_report(&buf).is_err() {
                continue;
            }
            std::thread::sleep(Duration::from_millis(timing::DEFAULT_DELAY_MS));

            let mut resp = vec![0u8; protocol::REPORT_SIZE];
            resp[0] = 0;
            if self.device.get_feature_report(&mut resp).is_ok() && resp[1] == cmd {
                return Some(resp);
            }
        }
        None
    }

    /// Send command without waiting for specific response
    pub fn send(&self, cmd: u8, data: &[u8], checksum: ChecksumType) -> bool {
        self.send_with_delay(cmd, data, checksum, 100)
    }

    /// Send command with custom delay (for streaming/fast updates)
    pub fn send_with_delay(&self, cmd: u8, data: &[u8], checksum: ChecksumType, delay_ms: u64) -> bool {
        let mut buf = vec![0u8; protocol::REPORT_SIZE];
        buf[0] = 0; // Report ID
        buf[1] = cmd;
        for (i, &b) in data.iter().enumerate() {
            if i + 2 < protocol::REPORT_SIZE {
                buf[i + 2] = b;
            }
        }

        // Apply checksum
        match checksum {
            ChecksumType::Bit7 => {
                let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
                buf[8] = (255 - (sum & 0xFF)) as u8;
            }
            ChecksumType::Bit8 => {
                let sum: u32 = buf[1..9].iter().map(|&b| b as u32).sum();
                buf[9] = (255 - (sum & 0xFF)) as u8;
            }
            ChecksumType::None => {}
        }

        // Debug: print first 16 bytes for SET_USERPIC page 0 (only if RUST_LOG=trace)
        if cmd == cmd::SET_USERPIC && buf[4] == 0 && std::env::var("RUST_LOG").map(|v| v.contains("trace")).unwrap_or(false) {
            eprintln!(
                "[TRACE] SET_USERPIC page 0: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} | {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
                buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]
            );
        }

        for _ in 0..timing::SEND_RETRIES {
            if self.device.send_feature_report(&buf).is_ok() {
                if delay_ms > 0 {
                    std::thread::sleep(Duration::from_millis(delay_ms));
                }
                return true;
            }
        }
        false
    }

    /// Read all device information
    pub fn read_info(&self) -> DeviceInfo {
        let mut info = DeviceInfo::default();

        // USB version / device ID
        if let Some(resp) = self.query(cmd::GET_USB_VERSION) {
            info.device_id = (resp[2] as u32)
                | ((resp[3] as u32) << 8)
                | ((resp[4] as u32) << 16)
                | ((resp[5] as u32) << 24);
            info.version = (resp[8] as u16) | ((resp[9] as u16) << 8);
        }

        // Profile
        if let Some(resp) = self.query(cmd::GET_PROFILE) {
            info.profile = resp[2];
        }

        // Debounce
        if let Some(resp) = self.query(cmd::GET_DEBOUNCE) {
            info.debounce = resp[2];
        }

        // Polling rate
        if let Some(hz) = self.get_polling_rate() {
            info.polling_rate = hz;
        }

        // LED params
        if let Some(resp) = self.query(cmd::GET_LEDPARAM) {
            info.led_mode = resp[2];
            info.led_brightness = resp[3];
            info.led_speed = resp[4];
            info.led_dazzle = (resp[5] & protocol::LED_OPTIONS_MASK) == protocol::LED_DAZZLE_ON;
            info.led_r = resp[6];
            info.led_g = resp[7];
            info.led_b = resp[8];
        }

        // Secondary LED (side lights) params
        if let Some(resp) = self.query(cmd::GET_SLEDPARAM) {
            info.side_mode = resp[2];
            info.side_brightness = resp[3];
            info.side_speed = resp[4];
            info.side_dazzle = (resp[5] & protocol::LED_OPTIONS_MASK) == protocol::LED_DAZZLE_ON;
            info.side_r = resp[6];
            info.side_g = resp[7];
            info.side_b = resp[8];
        }

        // KB options
        if let Some(resp) = self.query(cmd::GET_KBOPTION) {
            info.fn_layer = resp[3];
            info.wasd_swap = resp[6] != 0;
        }

        // Feature list (precision)
        if let Some(resp) = self.query(cmd::GET_FEATURE_LIST) {
            info.precision = resp[3];
        }

        // Sleep time
        if let Some(resp) = self.query(cmd::GET_SLEEPTIME) {
            info.sleep_seconds = (resp[2] as u16) | ((resp[3] as u16) << 8);
        }

        info
    }

    /// Set LED parameters
    /// For mode 13 (LightUserPicture), option selects which custom layer (0-3) to display.
    /// The option value is shifted left by 4 bits in the protocol.
    #[allow(clippy::too_many_arguments)]
    pub fn set_led(&self, mode: u8, brightness: u8, speed: u8, r: u8, g: u8, b: u8, dazzle: bool) -> bool {
        self.set_led_with_option(mode, brightness, speed, r, g, b, dazzle, 0)
    }

    /// Set LED parameters with explicit option byte
    /// For LightUserPicture mode (13):
    /// - layer: which custom color layer to display (0-3)
    /// - RGB should be (0, 200, 200) per official driver
    #[allow(clippy::too_many_arguments)]
    pub fn set_led_with_option(&self, mode: u8, brightness: u8, speed: u8, r: u8, g: u8, b: u8, dazzle: bool, layer: u8) -> bool {
        let (option, r_val, g_val, b_val) = if mode == 13 {
            // For LightUserPicture: option = layer << 4, RGB = (0, 200, 200)
            (layer << 4, 0u8, 200u8, 200u8)
        } else {
            let opt = if dazzle { protocol::LED_DAZZLE_ON } else { protocol::LED_DAZZLE_OFF };
            (opt, r, g, b)
        };

        let data = [
            mode,
            protocol::LED_SPEED_MAX - speed.min(protocol::LED_SPEED_MAX), // Speed is inverted
            brightness.min(protocol::LED_BRIGHTNESS_MAX),
            option,
            r_val, g_val, b_val,
        ];
        self.send(cmd::SET_LEDPARAM, &data, ChecksumType::Bit8)
    }

    /// Set secondary LED (side lights) parameters
    /// mode: 0=Off, 1=Constant, 2=Breathing, 3=Neon, 4=Wave, 5=Snake
    /// brightness: 0-4
    /// speed: 0-4
    /// dazzle: rainbow color cycling
    #[allow(clippy::too_many_arguments)]
    pub fn set_side_led(&self, mode: u8, brightness: u8, speed: u8, r: u8, g: u8, b: u8, dazzle: bool) -> bool {
        let dazzle_flag = if dazzle { protocol::LED_DAZZLE_ON } else { protocol::LED_DAZZLE_OFF };
        let data = [
            mode,
            protocol::LED_SPEED_MAX - speed.min(protocol::LED_SPEED_MAX), // Speed is inverted
            brightness.min(protocol::LED_BRIGHTNESS_MAX),
            dazzle_flag,
            r, g, b,
        ];
        self.send(cmd::SET_SLEDPARAM, &data, ChecksumType::Bit8)
    }

    /// Enable/disable magnetism (key depth) reporting
    pub fn set_magnetism_report(&self, enable: bool) -> bool {
        // Build the command buffer manually to debug
        let mut buf = vec![0u8; protocol::REPORT_SIZE];
        buf[0] = 0; // Report ID
        buf[1] = cmd::SET_MAGNETISM_REPORT; // 0x1B
        buf[2] = if enable { 1 } else { 0 };
        // Checksum: sum buf[1..8], put at buf[8]
        let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
        buf[8] = (255 - (sum & 0xFF)) as u8;

        println!("set_magnetism_report({enable}) sending: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} | {:02x}",
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]);

        match self.device.send_feature_report(&buf) {
            Ok(_) => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                true
            }
            Err(e) => {
                eprintln!("set_magnetism_report failed: {e}");
                false
            }
        }
    }

    /// Initialize per-key RGB mode (sends USERGIF start command)
    /// This may be required before sending per-key colors
    pub fn start_user_gif(&self) -> bool {
        // Official driver: _e[0] = CMD, _e[3] = 0, send with Bit7 checksum
        let data = [0, 0, 0];  // data[2] will be byte[3] in the message = 0
        if self.send(cmd::SET_USERGIF, &data, ChecksumType::Bit7) {
            // Official driver waits after start
            std::thread::sleep(std::time::Duration::from_millis(timing::ANIMATION_START_DELAY_MS));
            true
        } else {
            false
        }
    }

    /// Set per-key RGB colors (mode 25 = LightUserColor)
    /// colors: slice of (R, G, B) tuples, one per key
    pub fn set_per_key_colors(&self, colors: &[(u8, u8, u8)]) -> bool {
        self.set_per_key_colors_fast(colors, 100, 10)
    }

    /// Set per-key RGB colors with custom timing (for streaming/animation)
    /// page_delay_ms: delay after each HID report
    /// inter_delay_ms: delay between pages
    ///
    /// Format: Always sends 7 pages to match official driver:
    /// - Pages 0-5: 56 bytes of RGB data each, is_last=0
    /// - Page 6: 42 bytes of RGB data, is_last=1
    ///
    /// Total: 378 bytes = 126 key positions * 3 bytes RGB
    pub fn set_per_key_colors_fast(&self, colors: &[(u8, u8, u8)], page_delay_ms: u64, inter_delay_ms: u64) -> bool {
        self.set_per_key_colors_to_layer_internal(colors, 0, page_delay_ms, inter_delay_ms)
    }

    /// Set per-key RGB colors to a specific layer (0-3)
    pub fn set_per_key_colors_to_layer(&self, colors: &[(u8, u8, u8)], layer: u8) -> bool {
        self.set_per_key_colors_to_layer_internal(colors, layer, 50, 10)
    }

    fn set_per_key_colors_to_layer_internal(&self, colors: &[(u8, u8, u8)], layer: u8, page_delay_ms: u64, inter_delay_ms: u64) -> bool {
        // Build RGB byte array - fill with colors, pad with zeros
        let mut rgb_data = vec![0u8; rgb::TOTAL_RGB_SIZE];
        for (i, (r, g, b)) in colors.iter().enumerate() {
            if i * 3 + 2 < rgb::TOTAL_RGB_SIZE {
                rgb_data[i * 3] = *r;
                rgb_data[i * 3 + 1] = *g;
                rgb_data[i * 3 + 2] = *b;
            }
        }

        // Send exactly 7 pages to match official driver format
        for page in 0..rgb::NUM_PAGES {
            let (page_size, is_last) = if page == rgb::NUM_PAGES - 1 {
                (rgb::LAST_PAGE_SIZE, 1u8)
            } else {
                (rgb::PAGE_SIZE, 0u8)
            };

            let start = page * rgb::PAGE_SIZE;
            let end = start + page_size;

            // Build message: [profile/layer, magic, page, length, is_last, pad, pad, ...rgb_data]
            let mut data = vec![
                layer,             // layer/profile (0-3)
                rgb::MAGIC_VALUE,  // magic value
                page as u8,        // page number (0-6)
                page_size as u8,   // fixed length: 56 or 42
                is_last,           // is_last flag: 0 or 1
                0,                 // padding byte 1
                0,                 // padding byte 2
            ];
            data.extend_from_slice(&rgb_data[start..end.min(rgb_data.len())]);

            // Pad to ensure consistent message size
            while data.len() < 5 + rgb::PAGE_SIZE {
                data.push(0);
            }

            if !self.send_with_delay(cmd::SET_USERPIC, &data, ChecksumType::Bit7, page_delay_ms) {
                return false;
            }
            if inter_delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(inter_delay_ms));
            }
        }

        true
    }

    /// Read per-key colors from the device
    /// Returns the first few RGB values for debugging
    pub fn get_per_key_colors_debug(&self) -> Option<Vec<(u8, u8, u8)>> {
        // GET_USERPIC format: [cmd, profile, magic, page]
        // Try reading with raw feature report to debug the response
        let mut buf = vec![0u8; protocol::REPORT_SIZE];
        buf[0] = 0; // Report ID
        buf[1] = cmd::GET_USERPIC; // 0x8C
        buf[2] = 0;   // profile
        buf[3] = 255; // magic
        buf[4] = 0;   // page 0

        // Apply Bit7 checksum
        let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
        buf[8] = (255 - (sum & 0xFF)) as u8;

        if self.device.send_feature_report(&buf).is_err() {
            eprintln!("   Failed to send GET_USERPIC");
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));

        let mut resp = vec![0u8; protocol::REPORT_SIZE];
        resp[0] = 0;
        if let Err(e) = self.device.get_feature_report(&mut resp) {
            eprintln!("   Failed to get feature report: {e:?}");
            return None;
        }

        // Debug: print first 20 bytes of response
        eprintln!("   Response: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} | {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                 resp[0], resp[1], resp[2], resp[3], resp[4], resp[5], resp[6], resp[7], resp[8], resp[9],
                 resp[10], resp[11], resp[12], resp[13], resp[14], resp[15], resp[16], resp[17], resp[18], resp[19]);

        // Try to extract RGB values - look for actual data
        let mut colors = Vec::new();
        // The response might have RGB data starting at different offsets
        // Let's try to find where non-zero data starts
        for i in 0..10 {
            let base = 8 + i * 3;  // Try offset 8 (after checksum)
            if base + 2 < resp.len() {
                colors.push((resp[base], resp[base + 1], resp[base + 2]));
            }
        }
        Some(colors)
    }

    /// Set all keys to the same RGB color (convenience function)
    pub fn set_all_keys_color(&self, r: u8, g: u8, b: u8) -> bool {
        let key_count = self.key_count() as usize;
        let colors: Vec<(u8, u8, u8)> = vec![(r, g, b); key_count];

        // First, set LED mode to 25 (LightUserColor / Per-Key Color)
        if !self.set_led(25, 4, 0, r, g, b, false) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Then set the per-key colors
        self.set_per_key_colors(&colors)
    }

    /// Upload a multi-frame animation to the keyboard's memory
    /// The keyboard will play this animation autonomously when in LightUserColor mode (25)
    ///
    /// frames: Vec of frames, each frame is 126 RGB tuples
    /// frame_delay_ms: delay between frames in milliseconds (stored on keyboard)
    ///
    /// Protocol (SET_USERGIF = 0x12):
    /// 1. Start: [0x12, 0, 0, 0] - byte[3]=0 means "start upload"
    /// 2. For each frame, send 7 pages:
    ///    Header: [0x12, frame_idx, page, 1, total_frames, delay_lo, delay_hi, checksum]
    ///    + 56 bytes RGB data (42 for last page)
    pub fn upload_animation(&self, frames: &[Vec<(u8, u8, u8)>], frame_delay_ms: u16) -> bool {
        if frames.is_empty() || frames.len() > 255 {
            eprintln!("Animation must have 1-255 frames");
            return false;
        }

        let total_frames = frames.len() as u8;
        eprintln!("Uploading {total_frames} frame animation with {frame_delay_ms}ms delay...");

        // Step 1: Initialize upload
        if !self.start_user_gif() {
            eprintln!("Failed to initialize animation upload");
            return false;
        }

        // Step 2: Upload each frame
        for (frame_idx, frame_colors) in frames.iter().enumerate() {
            if !self.upload_animation_frame(
                frame_idx as u8,
                total_frames,
                frame_delay_ms,
                frame_colors,
            ) {
                eprintln!("Failed to upload frame {frame_idx}");
                return false;
            }
            eprint!(".");
        }
        eprintln!(" done!");

        // Step 3: Switch to LightUserColor mode to play the animation
        // Try with dazzle=true (option byte = 8) which may be required for animation playback
        self.set_led(cmd::LedMode::UserColor.as_u8(), 4, 0, 0, 0, 0, true);
        std::thread::sleep(std::time::Duration::from_millis(100));

        true
    }

    /// Upload a single frame of the animation
    fn upload_animation_frame(
        &self,
        frame_idx: u8,
        total_frames: u8,
        frame_delay_ms: u16,
        colors: &[(u8, u8, u8)],
    ) -> bool {
        // Build RGB byte array
        let mut rgb_data = vec![0u8; rgb::TOTAL_RGB_SIZE];
        for (i, (r, g, b)) in colors.iter().enumerate() {
            if i * 3 + 2 < rgb::TOTAL_RGB_SIZE {
                rgb_data[i * 3] = *r;
                rgb_data[i * 3 + 1] = *g;
                rgb_data[i * 3 + 2] = *b;
            }
        }

        // Send pages for this frame
        // Header format: [cmd, frame_idx, page, 1, total_frames, delay_lo, delay_hi]
        for page in 0..rgb::NUM_PAGES {
            let page_size = if page == rgb::NUM_PAGES - 1 { rgb::LAST_PAGE_SIZE } else { rgb::PAGE_SIZE };
            let start = page * rgb::PAGE_SIZE;
            let end = start + page_size;

            // Build message data (after cmd byte)
            let mut data = vec![
                frame_idx,                        // current frame index
                page as u8,                       // page number (0-6)
                1,                                // data flag (1 = frame data, 0 = start)
                total_frames,                     // total number of frames
                (frame_delay_ms & 0xFF) as u8,    // delay low byte
                ((frame_delay_ms >> 8) & 0xFF) as u8, // delay high byte
                0,                                // padding
            ];
            data.extend_from_slice(&rgb_data[start..end.min(rgb_data.len())]);

            if !self.send_with_delay(cmd::SET_USERGIF, &data, ChecksumType::Bit7, 5) {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        true
    }

    /// Set active profile (0-3)
    pub fn set_profile(&self, profile: u8) -> bool {
        self.send(cmd::SET_PROFILE, &[profile.min(3)], ChecksumType::Bit7)
    }

    /// Set debounce time in milliseconds
    pub fn set_debounce(&self, ms: u8) -> bool {
        self.send(cmd::SET_DEBOUNCE, &[ms], ChecksumType::Bit7)
    }

    /// Get current polling rate in Hz
    /// Returns None if query fails or response is invalid
    pub fn get_polling_rate(&self) -> Option<u16> {
        let resp = self.query(cmd::GET_REPORT)?;
        if resp[0] == cmd::GET_REPORT {
            protocol::polling_rate::decode(resp[1])
        } else {
            None
        }
    }

    /// Set polling rate in Hz
    /// Valid rates: 8000, 4000, 2000, 1000, 500, 250, 125
    pub fn set_polling_rate(&self, hz: u16) -> bool {
        if let Some(code) = protocol::polling_rate::encode(hz) {
            // Format: [cmd, 0, rate_code, ...]
            self.send(cmd::SET_REPORT, &[0, code], ChecksumType::Bit7)
        } else {
            false
        }
    }

    /// Set sleep timeout (for wireless keyboards)
    /// bt_seconds: Bluetooth sleep timeout
    /// rf_seconds: 2.4GHz sleep timeout
    pub fn set_sleep(&self, bt_seconds: u16, rf_seconds: u16) -> bool {
        let data = [
            (bt_seconds & 0xFF) as u8,
            ((bt_seconds >> 8) & 0xFF) as u8,
            (rf_seconds & 0xFF) as u8,
            ((rf_seconds >> 8) & 0xFF) as u8,
        ];
        self.send(cmd::SET_SLEEPTIME, &data, ChecksumType::Bit7)
    }

    /// Factory reset the keyboard
    pub fn reset(&self) -> bool {
        self.send(cmd::SET_RESET, &[], ChecksumType::Bit7)
    }

    /// Start/stop minimum travel calibration (released position)
    /// Procedure: start -> wait 2s -> stop
    pub fn calibrate_min(&self, start: bool) -> bool {
        self.send(cmd::SET_MAGNETISM_CAL, &[if start { 1 } else { 0 }], ChecksumType::Bit7)
    }

    /// Start/stop maximum travel calibration (fully pressed position)
    /// Procedure: start -> press all keys -> stop
    pub fn calibrate_max(&self, start: bool) -> bool {
        self.send(cmd::SET_MAGNETISM_MAX_CAL, &[if start { 1 } else { 0 }], ChecksumType::Bit7)
    }

    /// Set keyboard options
    /// fn_layer: Fn layer index (0-based)
    /// anti_mistouch: Anti-mistouch switch
    /// rt_stability: Rapid Trigger stability (0-125ms, steps of 25ms)
    /// wasd_swap: Swap WASD and arrow keys
    pub fn set_options(&self, fn_layer: u8, anti_mistouch: bool, rt_stability: u8, wasd_swap: bool) -> bool {
        // JS format: [cmd, _, fnIndex, anti_mistouch, RTStab/25, wasd_swap]
        let data = [
            0,  // byte 1 (unused)
            fn_layer,  // byte 2 - fn layer index
            if anti_mistouch { 1 } else { 0 },  // byte 3 - anti-mistouch
            rt_stability / 25,  // byte 4 - RT stability (0-5 = 0-125ms)
            if wasd_swap { 1 } else { 0 },  // byte 5 - WASD swap
        ];
        self.send(cmd::SET_KBOPTION, &data, ChecksumType::Bit7)
    }

    /// Get keyboard options
    /// Returns (os_mode, fn_layer, anti_mistouch, rt_stability, wasd_swap)
    pub fn get_options(&self) -> Option<(u8, u8, bool, u8, bool)> {
        let resp = self.query(cmd::GET_KBOPTION)?;
        // Response: [cmd, os_mode, fn_layer?, anti_mistouch?, rt_stab, wasd_swap]
        if resp.len() >= 6 {
            let os_mode = resp[1];
            let fn_layer = resp[2];
            let anti_mistouch = resp[3] != 0;
            let rt_stability = resp[4].min(5) * 25;
            let wasd_swap = resp[5] != 0;
            Some((os_mode, fn_layer, anti_mistouch, rt_stability, wasd_swap))
        } else {
            None
        }
    }

    /// Get key matrix (key remappings) for a profile
    /// profile: profile index (0-3, or combined profile*4+sonProfile for magnetism devices)
    /// num_pages: how many pages to read (based on key count)
    /// Returns raw key matrix data (4 bytes per key: type, enabled, layer, keycode)
    pub fn get_keymatrix(&self, profile: u8, num_pages: usize) -> Option<Vec<u8>> {
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            // Request format: [cmd, profile, 255 (magic), 0, page]
            let mut buf = vec![0u8; protocol::REPORT_SIZE];
            buf[0] = 0; // Report ID
            buf[1] = cmd::GET_KEYMATRIX;
            buf[2] = profile;
            buf[3] = 255;  // magic value
            buf[4] = 0;
            buf[5] = page as u8;

            // Apply checksum
            let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
            buf[8] = (255 - (sum & 0xFF)) as u8;

            for _ in 0..3 {
                if self.device.send_feature_report(&buf).is_err() {
                    continue;
                }
                std::thread::sleep(Duration::from_millis(50));

                let mut resp = vec![0u8; protocol::REPORT_SIZE];
                resp[0] = 0;
                if self.device.get_feature_report(&mut resp).is_ok() {
                    // Check if response echoes command
                    if resp[1] == cmd::GET_KEYMATRIX {
                        // Response format: [0, cmd, data...]
                        all_data.extend_from_slice(&resp[2..]);
                        break;
                    } else {
                        // Some responses may not echo command
                        all_data.extend_from_slice(&resp[1..]);
                        break;
                    }
                }
            }
        }

        if all_data.is_empty() {
            None
        } else {
            Some(all_data)
        }
    }

    /// Set a single key's mapping
    /// profile: profile index (0-3)
    /// key_index: key position in the matrix (0-based)
    /// hid_code: HID usage code for the new key
    /// enabled: whether the key is enabled (true = normal, false = disabled)
    /// layer: Fn layer (0 = base layer)
    ///
    /// Format: [cmd, profile, key_index, 0, 0, enabled, layer, checksum, code[0], code[1], code[2], code[3]]
    pub fn set_keymatrix(&self, profile: u8, key_index: u8, hid_code: u8, enabled: bool, layer: u8) -> bool {
        // Key code format for simple key: [0, 0, hid_code, 0]
        let key_data = [0u8, 0, hid_code, 0];

        let data = [
            profile,                        // byte 1: profile
            key_index,                      // byte 2: key index in matrix
            0, 0,                           // bytes 3-4: unused
            if enabled { 1 } else { 0 },    // byte 5: enabled flag
            layer,                          // byte 6: layer
            0,                              // byte 7: padding (checksum will be at byte 8)
            key_data[0], key_data[1], key_data[2], key_data[3], // bytes 8-11: key code
        ];

        self.send(cmd::SET_KEYMATRIX, &data, ChecksumType::Bit7)
    }

    /// Reset a key to its default mapping
    /// This sets the key to "disabled" which causes the firmware to use the default
    pub fn reset_key(&self, profile: u8, key_index: u8) -> bool {
        self.set_keymatrix(profile, key_index, 0, false, 0)
    }

    /// Swap two keys
    pub fn swap_keys(&self, profile: u8, key_a: u8, code_a: u8, key_b: u8, code_b: u8) -> bool {
        // Set key_a to code_b
        if !self.set_keymatrix(profile, key_a, code_b, true, 0) {
            return false;
        }
        // Set key_b to code_a
        self.set_keymatrix(profile, key_b, code_a, true, 0)
    }

    /// Set macro data for a macro slot
    /// macro_index: macro slot number (0-based)
    /// events: list of (keycode, is_down, delay_ms) tuples
    /// repeat_count: how many times to repeat the macro
    ///
    /// Format: [2-byte repeat count (LE), then 2-byte events (keycode, flags)]
    /// Flags: bit 7 = key down, bits 0-6 = delay (0-127ms)
    pub fn set_macro(&self, macro_index: u8, events: &[(u8, bool, u8)], repeat_count: u16) -> bool {
        // Build macro data
        let mut data = Vec::with_capacity(256);

        // 2-byte repeat count (little-endian)
        data.push((repeat_count & 0xFF) as u8);
        data.push((repeat_count >> 8) as u8);

        // Add events (2 bytes each)
        for &(keycode, is_down, delay) in events {
            data.push(keycode);
            let flags = if is_down {
                0x80 | (delay.min(127))  // bit 7 set = down, bits 0-6 = delay
            } else {
                delay.min(127)  // bit 7 clear = up
            };
            data.push(flags);
        }

        // Pad to at least fill first page
        while data.len() < 56 {
            data.push(0);
        }

        // Send in pages of 56 bytes
        let page_size = 56;
        let num_pages = data.len().div_ceil(page_size);

        for page in 0..num_pages {
            let start = page * page_size;
            let end = (start + page_size).min(data.len());
            let chunk = &data[start..end];
            let is_last = page == num_pages - 1;

            // Build command: [cmd, macro_index, page, chunk_len, is_last, 0, 0, checksum, data[56]]
            let mut buf = vec![0u8; protocol::REPORT_SIZE];
            buf[0] = 0; // Report ID
            buf[1] = cmd::SET_MACRO;
            buf[2] = macro_index;
            buf[3] = page as u8;
            buf[4] = chunk.len() as u8;
            buf[5] = if is_last { 1 } else { 0 };

            // Copy chunk data starting at byte 9 (after 8-byte header + checksum)
            for (i, &b) in chunk.iter().enumerate() {
                if 9 + i < buf.len() {
                    buf[9 + i] = b;
                }
            }

            // Apply Bit7 checksum
            let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
            buf[8] = (255 - (sum & 0xFF)) as u8;

            if self.device.send_feature_report(&buf).is_err() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        true
    }

    /// Create a simple text macro (types a string)
    /// Each character is sent as key-down then key-up with delay between
    /// Handles shifted characters (uppercase, symbols) automatically
    pub fn set_text_macro(&self, macro_index: u8, text: &str, delay_ms: u8, repeat: u16) -> bool {
        use crate::protocol::hid::char_to_hid;

        const LSHIFT: u8 = 0xE1;  // Left Shift HID code
        let mut events = Vec::new();

        for ch in text.chars() {
            if let Some((keycode, needs_shift)) = char_to_hid(ch) {
                if needs_shift {
                    events.push((LSHIFT, true, 0));          // Shift down
                    events.push((keycode, true, delay_ms));  // Key down
                    events.push((keycode, false, 0));        // Key up
                    events.push((LSHIFT, false, delay_ms));  // Shift up
                } else {
                    events.push((keycode, true, delay_ms));  // Key down
                    events.push((keycode, false, delay_ms)); // Key up
                }
            }
        }

        self.set_macro(macro_index, &events, repeat)
    }

    /// Get macro data for a macro slot
    /// macro_index: macro slot number (0-based)
    /// Returns raw macro data (up to 256 bytes, paginated)
    ///
    /// Format: [2-byte repeat count (LE), then 2-byte events (keycode, flags)]
    /// Flags: bit 7 = key down, bits 0-6 = delay
    pub fn get_macro(&self, macro_index: u8) -> Option<Vec<u8>> {
        let mut all_data = Vec::new();

        for page in 0..4 {
            let mut buf = vec![0u8; protocol::REPORT_SIZE];
            buf[0] = 0; // Report ID
            buf[1] = cmd::GET_MACRO;
            buf[2] = macro_index;
            buf[3] = page;

            // Apply checksum
            let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
            buf[8] = (255 - (sum & 0xFF)) as u8;

            for _ in 0..3 {
                if self.device.send_feature_report(&buf).is_err() {
                    continue;
                }
                std::thread::sleep(Duration::from_millis(50));

                let mut resp = vec![0u8; protocol::REPORT_SIZE];
                resp[0] = 0;
                if self.device.get_feature_report(&mut resp).is_ok() {
                    // Skip report ID and command echo
                    let data = if resp[1] == cmd::GET_MACRO { &resp[2..] } else { &resp[1..] };
                    all_data.extend_from_slice(data);

                    // Check for 4 consecutive zeros (end marker)
                    if data.windows(4).any(|w| w == [0, 0, 0, 0]) {
                        return Some(all_data);
                    }
                    break;
                }
            }
        }

        if all_data.is_empty() {
            None
        } else {
            Some(all_data)
        }
    }

    /// Get per-key magnetism settings (actuation, RT sensitivity, etc.)
    /// sub_cmd: 0=press travel, 1=lift travel, 2=RT press, 3=RT lift, 7=key mode
    /// num_pages: how many pages to read (2 for key modes, 2-4 for travel values depending on device)
    /// Returns concatenated response bytes from all pages
    ///
    /// NOTE: Unlike other commands, magnetism GET doesn't echo the command byte.
    /// The device returns data directly starting at byte 1 of the response.
    pub fn get_magnetism(&self, sub_cmd: u8, num_pages: usize) -> Option<Vec<u8>> {
        // Magnetism command format: [cmd, sub_cmd, flag=1, page]
        let mut all_data = Vec::new();

        for page in 0..num_pages {
            let data = [sub_cmd, 1, page as u8]; // sub_cmd, flag, page
            if let Some(resp) = self.query_magnetism(cmd::GET_MULTI_MAGNETISM, &data) {
                // Response format: [report_id=0, data...]
                // Data starts directly at byte 1 (no cmd echo)
                all_data.extend_from_slice(&resp[1..]);
            } else {
                // If page fails, fill with zeros (64 bytes of data per page)
                all_data.extend(std::iter::repeat_n(0u8, 64));
            }
        }
        Some(all_data)
    }

    /// Query magnetism command (doesn't check for cmd echo in response)
    fn query_magnetism(&self, cmd: u8, data: &[u8]) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; protocol::REPORT_SIZE];
        buf[0] = 0; // Report ID
        buf[1] = cmd;
        for (i, &b) in data.iter().enumerate() {
            if i + 2 < protocol::REPORT_SIZE {
                buf[i + 2] = b;
            }
        }
        // Apply Bit7 checksum
        let sum: u32 = buf[1..8].iter().map(|&b| b as u32).sum();
        buf[8] = (255 - (sum & 0xFF)) as u8;

        // Retry pattern for Linux HID
        for _ in 0..3 {
            if self.device.send_feature_report(&buf).is_err() {
                continue;
            }
            std::thread::sleep(Duration::from_millis(50));

            let mut resp = vec![0u8; protocol::REPORT_SIZE];
            resp[0] = 0;
            if self.device.get_feature_report(&mut resp).is_ok() {
                // Accept any non-zero response (device returns data directly, not cmd echo)
                if resp.iter().skip(1).any(|&b| b != 0) {
                    return Some(resp);
                }
            }
        }
        None
    }

    /// Get all trigger settings for display
    /// Returns per-key settings: travel, RT sensitivity, modes, and deadzones
    /// Travel values are 16-bit (2 bytes per key), modes are 8-bit (1 byte per key)
    pub fn get_all_triggers(&self) -> Option<TriggerSettings> {
        // Key modes use 1 byte per key, need 2 pages for up to ~120 keys
        let modes = self.get_magnetism(protocol::magnetism::KEY_MODE, 2)?;

        // Travel values use 2 bytes per key (16-bit little-endian), need 2 pages for ~60 keys
        // (each page has ~60 bytes of data = ~30 key values)
        let press = self.get_magnetism(protocol::magnetism::PRESS_TRAVEL, 2)?;
        let lift = self.get_magnetism(protocol::magnetism::LIFT_TRAVEL, 2)?;
        let rt_press = self.get_magnetism(protocol::magnetism::RT_PRESS, 2)?;
        let rt_lift = self.get_magnetism(protocol::magnetism::RT_LIFT, 2)?;

        // Deadzones - may fail on older firmware, use empty vecs as fallback
        let bottom_dz = self.get_magnetism(protocol::magnetism::BOTTOM_DEADZONE, 2)
            .unwrap_or_default();
        let top_dz = self.get_magnetism(protocol::magnetism::TOP_DEADZONE, 2)
            .unwrap_or_default();

        Some(TriggerSettings {
            press_travel: press,
            lift_travel: lift,
            rt_press,
            rt_lift,
            key_modes: modes,
            bottom_deadzone: bottom_dz,
            top_deadzone: top_dz,
        })
    }

    /// Set per-key magnetism settings for all keys (u8 values, legacy)
    /// sub_cmd: 0=press travel, 1=lift travel, 2=RT press, 3=RT lift, 7=key mode
    /// values: array of values for each key (up to 60 keys per call)
    pub fn set_magnetism(&self, sub_cmd: u8, values: &[u8]) -> bool {
        let mut data = vec![sub_cmd];
        data.extend_from_slice(values);
        self.send(cmd::SET_MULTI_MAGNETISM, &data, ChecksumType::Bit7)
    }

    /// Set per-key magnetism settings for all keys (u16 values, 2 bytes per key)
    /// sub_cmd: 0=press travel, 1=lift travel, 2=RT press, 3=RT lift
    /// values: 16-bit values in little-endian format, sent in pages
    ///
    /// Format: [cmd, sub_cmd, flag=1, page, commit, data...]
    /// - flag=1 for bulk mode
    /// - commit=1 on last page
    pub fn set_magnetism_u16(&self, sub_cmd: u8, values: &[u16]) -> bool {
        // Convert u16 values to bytes (little-endian)
        let key_count = self.key_count() as usize;
        let bytes: Vec<u8> = values.iter()
            .take(key_count)
            .flat_map(|&v| v.to_le_bytes())
            .collect();

        // Send in pages (56 bytes per page as per JS source)
        let page_size = 56;
        let num_pages = bytes.len().div_ceil(page_size);
        let mut success = true;

        for (page, chunk) in bytes.chunks(page_size).enumerate() {
            let is_last = page == num_pages - 1;
            // JS format: [cmd, sub_cmd, flag, page, commit, 0, 0, checksum, data[56]]
            // The header is 8 bytes total: checksum at byte 7, data starts at byte 8
            // Since send() puts cmd at buf[1] and checksum at buf[8], we need:
            // - sub_cmd at buf[2]
            // - flag at buf[3]
            // - page at buf[4]
            // - commit at buf[5]
            // - 0 at buf[6]
            // - 0 at buf[7]
            // - (checksum at buf[8] - placed by send())
            // - data starts at buf[9]
            // So we need 3 padding bytes after commit to reserve space for checksum
            let mut data = vec![
                sub_cmd,
                1,                           // flag = 1 for bulk mode
                page as u8,                  // page number
                if is_last { 1 } else { 0 }, // commit flag on last page
                0, 0, 0,                     // padding: 2 zeros + checksum placeholder
            ];
            data.extend_from_slice(chunk);
            if !self.send(cmd::SET_MULTI_MAGNETISM, &data, ChecksumType::Bit7) {
                success = false;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        success
    }

    /// Set actuation point for all keys (in precision units, e.g. 20 = 2.0mm at precision 0)
    pub fn set_actuation_all(&self, travel: u8) -> bool {
        let values = vec![travel; self.key_count() as usize];
        self.set_magnetism(protocol::magnetism::PRESS_TRAVEL, &values)
    }

    /// Set actuation point for all keys (u16 version for newer firmware)
    pub fn set_actuation_all_u16(&self, travel: u16) -> bool {
        let values = vec![travel; self.key_count() as usize];
        self.set_magnetism_u16(protocol::magnetism::PRESS_TRAVEL, &values)
    }

    /// Set release point for all keys
    pub fn set_release_all(&self, travel: u8) -> bool {
        let values = vec![travel; self.key_count() as usize];
        self.set_magnetism(protocol::magnetism::LIFT_TRAVEL, &values)
    }

    /// Set release point for all keys (u16 version)
    pub fn set_release_all_u16(&self, travel: u16) -> bool {
        let values = vec![travel; self.key_count() as usize];
        self.set_magnetism_u16(protocol::magnetism::LIFT_TRAVEL, &values)
    }

    /// Set Rapid Trigger press sensitivity for all keys
    pub fn set_rt_press_all(&self, sensitivity: u8) -> bool {
        let values = vec![sensitivity; self.key_count() as usize];
        self.set_magnetism(protocol::magnetism::RT_PRESS, &values)
    }

    /// Set Rapid Trigger press sensitivity for all keys (u16 version)
    pub fn set_rt_press_all_u16(&self, sensitivity: u16) -> bool {
        let values = vec![sensitivity; self.key_count() as usize];
        self.set_magnetism_u16(protocol::magnetism::RT_PRESS, &values)
    }

    /// Set Rapid Trigger release sensitivity for all keys
    pub fn set_rt_lift_all(&self, sensitivity: u8) -> bool {
        let values = vec![sensitivity; self.key_count() as usize];
        self.set_magnetism(protocol::magnetism::RT_LIFT, &values)
    }

    /// Set Rapid Trigger release sensitivity for all keys (u16 version)
    pub fn set_rt_lift_all_u16(&self, sensitivity: u16) -> bool {
        let values = vec![sensitivity; self.key_count() as usize];
        self.set_magnetism_u16(protocol::magnetism::RT_LIFT, &values)
    }

    /// Enable/disable Rapid Trigger for all keys
    pub fn set_rapid_trigger_all(&self, enable: bool) -> bool {
        let mode = if enable {
            protocol::magnetism::MODE_RAPID_TRIGGER
        } else {
            protocol::magnetism::MODE_NORMAL
        };
        let values = vec![mode; self.key_count() as usize];
        self.set_magnetism(protocol::magnetism::KEY_MODE, &values)
    }

    /// Set key modes for all keys (per-key u8 values)
    pub fn set_key_modes(&self, modes: &[u8]) -> bool {
        self.set_magnetism(protocol::magnetism::KEY_MODE, modes)
    }

    /// Set mode for a single key (modifies the modes array in place and sends)
    /// Returns true if successful, updates the modes array
    pub fn set_single_key_mode(&self, modes: &mut [u8], key_index: usize, mode: u8) -> bool {
        if key_index >= modes.len() {
            return false;
        }
        modes[key_index] = mode;
        self.set_key_modes(modes)
    }

    /// Read HID input report (non-blocking, for key depth data)
    pub fn read_input(&self, timeout_ms: i32) -> Option<Vec<u8>> {
        let mut buf = [0u8; protocol::INPUT_REPORT_SIZE];
        match self.device.read_timeout(&mut buf, timeout_ms) {
            Ok(len) if len > 0 => Some(buf[..len].to_vec()),
            Ok(_) => None, // timeout or zero-length read
            Err(e) => {
                tracing::debug!("HID read error: {e:?}");
                None
            }
        }
    }

    /// Read HID input report with verbose debugging
    pub fn read_input_verbose(&self, timeout_ms: i32) -> (Option<Vec<u8>>, String) {
        let mut buf = [0u8; protocol::INPUT_REPORT_SIZE];
        match self.device.read_timeout(&mut buf, timeout_ms) {
            Ok(len) if len > 0 => {
                (Some(buf[..len].to_vec()), format!("OK: {len} bytes"))
            }
            Ok(len) => {
                (None, format!("Timeout/empty: {len} bytes"))
            }
            Err(e) => {
                (None, format!("Error: {e:?}"))
            }
        }
    }

    /// Read vendor event (non-blocking input report with event classification)
    ///
    /// Vendor events include:
    /// - byte[0] = 0x1B (27): Key depth/magnetism data
    /// - byte[0..3] = [0x0F, 0x01, 0x00]: Magnetism reporting started
    /// - byte[0..3] = [0x0F, 0x00, 0x00]: Magnetism reporting stopped
    /// - byte[0] = 0x01 (1): Profile change
    /// - byte[0] = 0x04-0x07: LED effect change
    /// - byte[0] = 0x1D (29): Magnetism mode change
    pub fn read_vendor_event(&self, timeout_ms: i32) -> Option<Vec<u8>> {
        self.read_input(timeout_ms)
    }

    /// Classify a vendor event by its first byte(s)
    /// Handles both formats: with report ID prefix (0x05) and without
    pub fn classify_vendor_event(data: &[u8]) -> VendorEventType {
        if data.is_empty() {
            return VendorEventType::Unknown;
        }

        // Handle report ID prefix (Linux HID includes 0x05 as first byte)
        let cmd_data = if data[0] == 0x05 && data.len() > 1 {
            &data[1..]
        } else {
            data
        };

        if cmd_data.is_empty() {
            return VendorEventType::Unknown;
        }

        match cmd_data[0] {
            0x1B => VendorEventType::KeyDepth,
            0x0F if cmd_data.len() >= 3 => {
                if cmd_data[1] == 0x01 && cmd_data[2] == 0x00 {
                    VendorEventType::MagnetismStart
                } else if cmd_data[1] == 0x00 && cmd_data[2] == 0x00 {
                    VendorEventType::MagnetismStop
                } else {
                    VendorEventType::Unknown
                }
            }
            0x01 => VendorEventType::ProfileChange,
            0x04..=0x07 => VendorEventType::LedChange,
            0x1D => VendorEventType::MagnetismModeChange,
            0x88 => VendorEventType::BatteryStatus,
            _ => VendorEventType::Unknown,
        }
    }

    /// Get precision factor for converting raw values to mm
    /// Based on firmware version (from JS source):
    /// - Version undefined: 10 (0.1mm)
    /// - Version 768-1279: 100 (0.01mm)
    /// - Version >= 1280: 200 (0.005mm)
    pub fn precision_factor_from_version(version: u16) -> f32 {
        if version >= firmware::PRECISION_HIGH_VERSION {
            firmware::PRECISION_HIGH_FACTOR
        } else if version >= firmware::PRECISION_MID_VERSION {
            firmware::PRECISION_MID_FACTOR
        } else {
            firmware::PRECISION_LOW_FACTOR
        }
    }

    /// Get precision string from firmware version
    pub fn precision_str_from_version(version: u16) -> &'static str {
        if version >= firmware::PRECISION_HIGH_VERSION {
            "0.005mm"
        } else if version >= firmware::PRECISION_MID_VERSION {
            "0.01mm"
        } else {
            "0.1mm"
        }
    }

    /// Legacy precision factor (deprecated, use precision_factor_from_version)
    pub fn precision_factor(precision: u8) -> f32 {
        match precision {
            0 => 10.0,   // 0.1mm
            1 => 20.0,   // 0.05mm
            2 => 100.0,  // 0.01mm
            _ => 10.0,
        }
    }

    /// Legacy precision string (deprecated, use precision_str_from_version)
    pub fn precision_str(precision: u8) -> &'static str {
        match precision {
            0 => "0.1mm",
            1 => "0.05mm",
            2 => "0.01mm",
            _ => "unknown",
        }
    }

    /// Check if device is in bootloader mode (by VID/PID)
    pub fn is_boot_mode(&self) -> bool {
        firmware_update::is_boot_mode(self.vid, self.pid)
    }

    /// Format firmware version as human-readable string (e.g., "1.0.8")
    pub fn format_version(version: u16) -> String {
        let major = (version >> 8) & 0xF;
        let minor = (version >> 4) & 0xF;
        let patch = version & 0xF;
        format!("{major}.{minor}.{patch}")
    }

    /// Get device API ID for firmware checks
    /// Queries the device directly for its ID
    pub fn get_api_device_id(&self) -> Option<u32> {
        // Query the device directly for its ID rather than using VID/PID mapping
        let info = self.read_info();
        if info.device_id != 0 {
            Some(info.device_id)
        } else {
            // Fallback to VID/PID mapping if device query fails
            crate::firmware_api::device_ids::from_vid_pid(self.vid, self.pid)
        }
    }

    /// Send audio visualizer frequency band data
    /// Uses the keyboard's built-in music reactive mode (0x0D command)
    /// `bands` must be 16 values, each 0-6 representing frequency intensity
    pub fn set_audio_viz_bands(&self, bands: &[u8; 16]) -> bool {
        use crate::protocol::{cmd, audio_viz, ChecksumType};
        // Build data: 6 bytes padding + 16 bytes band levels
        let mut data = [0u8; 22];
        // Bytes 0-5 are padding (zeros)
        // Bytes 6-21 are the 16 frequency bands (after checksum at position 7)
        for (i, &level) in bands.iter().enumerate() {
            data[6 + i] = level.min(audio_viz::MAX_LEVEL);
        }
        // Use Bit7 checksum (checksum at byte 7 of message = byte 8 with report ID)
        self.send_with_delay(cmd::SET_AUDIO_VIZ, &data, ChecksumType::Bit7, 5)
    }

    /// Send audio visualizer data from FFT magnitudes
    /// `magnitudes` should be normalized 0.0-1.0 floats
    pub fn set_audio_viz_fft(&self, magnitudes: &[f32]) -> bool {
        use crate::protocol::audio_viz;
        let bands = audio_viz::magnitudes_to_bands(magnitudes);
        self.set_audio_viz_bands(&bands)
    }
}
