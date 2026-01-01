// MonsGeek/Akko HID Communication
// Shared device access code with multi-device support

use hidapi::{HidApi, HidDevice};
use std::time::Duration;

use crate::devices::{self, DeviceDefinition};
use crate::protocol::{self, cmd, ChecksumType};

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
    /// Unknown event type
    Unknown,
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
    /// Key mode per key (0=Normal, 1=RT, 2=DKS, etc.)
    pub key_modes: Vec<u8>,
}

/// Device information read from keyboard
#[derive(Debug, Clone, Default)]
pub struct DeviceInfo {
    pub device_id: u32,
    pub version: u16,
    pub profile: u8,
    pub debounce: u8,
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
        let hidapi = HidApi::new().map_err(|e| format!("HID init failed: {}", e))?;

        for dev_info in hidapi.device_list() {
            let vid = dev_info.vendor_id();
            let pid = dev_info.product_id();

            // Check if this is a supported device
            if let Some(definition) = devices::find_device(vid, pid) {
                if dev_info.usage_page() == protocol::USAGE_PAGE
                    && dev_info.usage() == protocol::USAGE
                {
                    let device = dev_info
                        .open_device(&hidapi)
                        .map_err(|e| format!("Failed to open device: {}", e))?;
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
            .ok_or_else(|| format!("Device {:04x}:{:04x} not in supported list", vid, pid))?;

        let hidapi = HidApi::new().map_err(|e| format!("HID init failed: {}", e))?;

        for dev_info in hidapi.device_list() {
            if dev_info.vendor_id() == vid
                && dev_info.product_id() == pid
                && dev_info.usage_page() == protocol::USAGE_PAGE
                && dev_info.usage() == protocol::USAGE
            {
                let device = dev_info
                    .open_device(&hidapi)
                    .map_err(|e| format!("Failed to open device: {}", e))?;
                let path = dev_info.path().to_string_lossy().to_string();
                return Ok(Self { device, vid, pid, path, definition });
            }
        }
        Err(format!("Device {:04x}:{:04x} not connected", vid, pid))
    }

    /// List all connected supported devices
    pub fn list_connected() -> Vec<ConnectedDeviceInfo> {
        let mut devices_found = Vec::new();

        if let Ok(hidapi) = HidApi::new() {
            for dev_info in hidapi.device_list() {
                let vid = dev_info.vendor_id();
                let pid = dev_info.product_id();

                if let Some(definition) = devices::find_device(vid, pid) {
                    if dev_info.usage_page() == protocol::USAGE_PAGE
                        && dev_info.usage() == protocol::USAGE
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
        for _ in 0..5 {
            if self.device.send_feature_report(&buf).is_err() {
                continue;
            }
            std::thread::sleep(Duration::from_millis(100));

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

        for _ in 0..3 {
            if self.device.send_feature_report(&buf).is_ok() {
                std::thread::sleep(Duration::from_millis(100));
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
    pub fn set_led(&self, mode: u8, brightness: u8, speed: u8, r: u8, g: u8, b: u8, dazzle: bool) -> bool {
        let data = [
            mode,
            protocol::LED_SPEED_MAX - speed.min(protocol::LED_SPEED_MAX), // Speed is inverted
            brightness.min(protocol::LED_BRIGHTNESS_MAX),
            if dazzle { protocol::LED_DAZZLE_ON } else { protocol::LED_DAZZLE_OFF },
            r, g, b,
        ];
        self.send(cmd::SET_LEDPARAM, &data, ChecksumType::Bit8)
    }

    /// Set secondary LED (side lights) parameters
    /// mode: 0=Off, 1=Constant, 2=Breathing, 3=Neon, 4=Wave, 5=Snake
    /// brightness: 0-4
    /// speed: 0-4
    /// dazzle: rainbow color cycling
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
        self.send(cmd::SET_MAGNETISM_REPORT, &[if enable { 1 } else { 0 }], ChecksumType::Bit7)
    }

    /// Set per-key RGB colors (mode 25 = LightUserColor)
    /// colors: slice of (R, G, B) tuples, one per key
    pub fn set_per_key_colors(&self, colors: &[(u8, u8, u8)]) -> bool {
        let key_count = self.key_count() as usize;

        // Build RGB byte array (3 bytes per key)
        let mut rgb_data: Vec<u8> = Vec::with_capacity(key_count * 3);
        for i in 0..key_count {
            let (r, g, b) = colors.get(i).copied().unwrap_or((0, 0, 0));
            rgb_data.push(r);
            rgb_data.push(g);
            rgb_data.push(b);
        }

        // Send in pages of 56 bytes each
        // Format: [cmd, profile, 255, page, len, is_last, ...data]
        let page_size = 56;
        let num_pages = (rgb_data.len() + page_size - 1) / page_size;

        for page in 0..num_pages {
            let start = page * page_size;
            let end = (start + page_size).min(rgb_data.len());
            let chunk = &rgb_data[start..end];
            let is_last = page == num_pages - 1;

            let mut data = vec![
                0,                              // profile (0 = current)
                255,                            // magic value
                page as u8,                     // page number
                chunk.len() as u8,              // bytes in this page
                if is_last { 1 } else { 0 },    // is_last flag
            ];
            data.extend_from_slice(chunk);

            // Pad to 56 bytes of RGB data
            while data.len() < 5 + page_size {
                data.push(0);
            }

            if !self.send(cmd::SET_USERPIC, &data, ChecksumType::Bit7) {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        true
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

    /// Set active profile (0-3)
    pub fn set_profile(&self, profile: u8) -> bool {
        self.send(cmd::SET_PROFILE, &[profile.min(3)], ChecksumType::Bit7)
    }

    /// Set debounce time in milliseconds
    pub fn set_debounce(&self, ms: u8) -> bool {
        self.send(cmd::SET_DEBOUNCE, &[ms], ChecksumType::Bit7)
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

    /// Get macro data for a macro slot
    /// macro_index: macro slot number (0-based)
    /// Returns raw macro data (up to 256 bytes, paginated)
    ///
    /// Format: [2-byte length (LE), then macro events (4 bytes each)]
    /// Macro event: [type, delay_low, delay_high/keycode, modifier]
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
                all_data.extend(std::iter::repeat(0u8).take(64));
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
    /// Returns (press_travel, lift_travel, rt_press, rt_lift, key_modes) for each key
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

        Some(TriggerSettings {
            press_travel: press,
            lift_travel: lift,
            rt_press: rt_press,
            rt_lift: rt_lift,
            key_modes: modes,
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
        let num_pages = (bytes.len() + page_size - 1) / page_size;
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

    /// Read HID input report (non-blocking, for key depth data)
    pub fn read_input(&self, timeout_ms: i32) -> Option<Vec<u8>> {
        let mut buf = [0u8; protocol::INPUT_REPORT_SIZE];
        match self.device.read_timeout(&mut buf, timeout_ms) {
            Ok(len) if len > 0 => Some(buf[..len].to_vec()),
            _ => None,
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
    pub fn classify_vendor_event(data: &[u8]) -> VendorEventType {
        if data.is_empty() {
            return VendorEventType::Unknown;
        }
        match data[0] {
            0x1B => VendorEventType::KeyDepth,
            0x0F if data.len() >= 3 => {
                if data[1] == 0x01 && data[2] == 0x00 {
                    VendorEventType::MagnetismStart
                } else if data[1] == 0x00 && data[2] == 0x00 {
                    VendorEventType::MagnetismStop
                } else {
                    VendorEventType::Unknown
                }
            }
            0x01 => VendorEventType::ProfileChange,
            0x04..=0x07 => VendorEventType::LedChange,
            0x1D => VendorEventType::MagnetismModeChange,
            _ => VendorEventType::Unknown,
        }
    }

    /// Get precision factor for converting raw values to mm
    /// Based on firmware version (from JS source):
    /// - Version undefined: 10 (0.1mm)
    /// - Version 768-1279: 100 (0.01mm)
    /// - Version >= 1280: 200 (0.005mm)
    pub fn precision_factor_from_version(version: u16) -> f32 {
        if version >= 1280 {
            200.0  // 0.005mm precision
        } else if version >= 768 {
            100.0  // 0.01mm precision
        } else {
            10.0   // 0.1mm precision
        }
    }

    /// Get precision string from firmware version
    pub fn precision_str_from_version(version: u16) -> &'static str {
        if version >= 1280 {
            "0.005mm"
        } else if version >= 768 {
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
}
