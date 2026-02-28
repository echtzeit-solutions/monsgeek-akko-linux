//! Shared event parsing for vendor notifications
//!
//! This module consolidates the event parsing logic used across all transport
//! backends (wired, dongle, bluetooth). The keyboard sends notifications via
//! HID input reports when settings change (Fn key combos, profile changes, etc.)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hidapi::HidDevice;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::types::{TimestampedEvent, VendorEvent};

/// Notification type constants for vendor events.
///
/// These correspond to the first byte of the notification payload
/// (after skipping any report ID).
///
/// **Note:** Notification bytes are a separate namespace from command bytes.
/// The same numeric value can mean different things depending on the USB
/// endpoint: command bytes (on the feature endpoint) are defined in
/// `protocol::cmd`, while notification bytes (on the input endpoint) are
/// defined here. For example, `0x05` is `SET_LEDONOFF` as a command but
/// `LED_EFFECT_SPEED` as a notification.
pub mod notif {
    /// Wake notification (all zeros)
    pub const WAKE: u8 = 0x00;
    /// Profile changed via Fn+F9..F12
    pub const PROFILE_CHANGE: u8 = 0x01;
    /// Keyboard function (Win lock, WASD swap, etc.)
    pub const KB_FUNC: u8 = 0x03;
    /// LED effect mode changed
    pub const LED_EFFECT_MODE: u8 = 0x04;
    /// LED effect speed changed
    pub const LED_EFFECT_SPEED: u8 = 0x05;
    /// Brightness level changed
    pub const BRIGHTNESS_LEVEL: u8 = 0x06;
    /// LED color changed
    pub const LED_COLOR: u8 = 0x07;
    /// Settings acknowledgment (start/complete)
    pub const SETTINGS_ACK: u8 = 0x0F;
    /// Key depth report (magnetism)
    pub const KEY_DEPTH: u8 = 0x1B;
    /// Battery status notification
    pub const BATTERY_STATUS: u8 = 0x88;
}

/// USB report ID constants
pub mod report_id {
    /// Mouse report ID (keyboard's built-in mouse function)
    pub const MOUSE: u8 = 0x02;
    /// Vendor event report ID (USB wired/dongle)
    pub const USB_VENDOR_EVENT: u8 = 0x05;
}

// BLE constants (VENDOR_REPORT_ID, CMDRESP_MARKER, EVENT_MARKER) live in
// protocol::ble — imported here to avoid duplication.
use crate::protocol::ble;

/// Parse keyboard function notification (type 0x03)
///
/// The payload format is:
/// - Byte 0: notification type (0x03)
/// - Byte 1: category (context-dependent)
/// - Byte 2: action code
fn parse_kb_func(payload: &[u8]) -> VendorEvent {
    let category = payload.get(1).copied().unwrap_or(0);
    let action = payload.get(2).copied().unwrap_or(0);

    match action {
        0x01 => VendorEvent::WinLockToggle {
            locked: category != 0,
        },
        0x03 => VendorEvent::WasdSwapToggle {
            swapped: category == 8, // category 8 = swapped, category 0 = normal
        },
        0x08 => VendorEvent::FnLayerToggle {
            layer: category, // 0 = default, 1 = alternate
        },
        0x09 => VendorEvent::BacklightToggle,
        0x11 => VendorEvent::DialModeToggle,
        _ => VendorEvent::UnknownKbFunc { category, action },
    }
}

/// Parse vendor event from USB input report (EP2 notifications)
///
/// Used by both wired and dongle transports. The format is identical.
///
/// Report formats:
/// - Report ID 0x02: Mouse report [02, buttons, 00, X_lo, X_hi, Y_lo, Y_hi, wheel_lo, wheel_hi]
/// - Report ID 0x05: Vendor event [05, type, value, ...]
///
/// Vendor notification types (Report ID 0x05):
/// - 0x00: Wake (all zeros)
/// - 0x01: Profile changed (data = profile 0-3)
/// - 0x03: KB function (category, action)
/// - 0x04: LED effect mode (effect ID 1-20)
/// - 0x05: LED effect speed (0-4)
/// - 0x06: Brightness level (0-4)
/// - 0x07: LED color (0-7)
/// - 0x0F: Settings ACK (0=done, 1=start)
/// - 0x1B: Key depth (magnetism report)
/// - 0x88: Battery status (from dongle)
pub fn parse_usb_event(data: &[u8]) -> VendorEvent {
    if data.is_empty() {
        return VendorEvent::Unknown(data.to_vec());
    }

    // Handle mouse reports (Report ID 0x02)
    // Format: [02, buttons, 00, X_lo, X_hi, Y_lo, Y_hi, wheel_lo, wheel_hi]
    if data[0] == report_id::MOUSE && data.len() >= 7 {
        let buttons = data[1];
        let x = i16::from_le_bytes([data[3], data[4]]);
        let y = i16::from_le_bytes([data[5], data[6]]);
        let wheel = if data.len() >= 9 {
            i16::from_le_bytes([data[7], data[8]])
        } else {
            0
        };
        return VendorEvent::MouseReport {
            buttons,
            x,
            y,
            wheel,
        };
    }

    // Skip report ID if present (0x05)
    let payload = if data[0] == report_id::USB_VENDOR_EVENT && data.len() > 1 {
        &data[1..]
    } else {
        data
    };

    parse_event_payload(payload, data)
}

/// Parse vendor event from Bluetooth HID input report
///
/// Bluetooth format differs from USB:
/// - USB:       [05, type, value, ...]
/// - Bluetooth: [06, 66, type, value, ...]
///
/// The 0x66 byte is a BLE-specific event marker.
pub fn parse_ble_event(data: &[u8]) -> VendorEvent {
    if data.len() < 3 {
        return VendorEvent::Unknown(data.to_vec());
    }

    // Bluetooth vendor events: [06, 66, type, value, ...]
    let payload =
        if data[0] == ble::VENDOR_REPORT_ID && data[1] == ble::EVENT_MARKER && data.len() > 2 {
            &data[2..] // Skip report ID (06) and BLE marker (66)
        } else if data[0] == report_id::USB_VENDOR_EVENT && data.len() > 1 {
            // Fallback: USB-style events [05, type, value, ...]
            &data[1..]
        } else if data[0] == ble::VENDOR_REPORT_ID && data.len() > 1 {
            // Alternate: just report ID 6 without 0x66 marker
            &data[1..]
        } else {
            return VendorEvent::Unknown(data.to_vec());
        };

    parse_event_payload(payload, data)
}

/// Shared event subsystem for all transport backends.
///
/// Manages the broadcast channel, event reader thread, and shutdown flag.
/// All transports delegate `read_event()`, `subscribe_events()`, and `Drop`
/// to this struct, eliminating ~80 lines of duplication.
pub struct EventSubsystem {
    /// Broadcast sender for timestamped vendor events (if input device available)
    event_tx: Option<broadcast::Sender<TimestampedEvent>>,
    /// Shutdown flag for event reader thread
    shutdown: Arc<AtomicBool>,
}

/// Broadcast channel capacity for vendor events
const EVENT_CHANNEL_CAPACITY: usize = 256;

impl EventSubsystem {
    /// Create a new event subsystem, optionally spawning an event reader thread.
    ///
    /// If `input_device` is `Some`, spawns a background thread that reads HID
    /// input reports and broadcasts parsed events to subscribers.
    pub fn new<F>(input_device: Option<HidDevice>, parser: F, config: EventReaderConfig) -> Self
    where
        F: Fn(&[u8]) -> VendorEvent + Send + 'static,
    {
        let shutdown = Arc::new(AtomicBool::new(false));
        let event_tx = input_device.map(|input| {
            let (tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
            let tx_clone = tx.clone();
            let shutdown_clone = shutdown.clone();

            std::thread::Builder::new()
                .name(format!("{}-event-reader", config.name))
                .spawn(move || {
                    run_event_reader_loop(input, tx_clone, shutdown_clone, parser, config);
                })
                .expect("Failed to spawn event reader thread");

            tx
        });

        Self { event_tx, shutdown }
    }

    /// Poll for a single event with timeout.
    pub fn read_event(
        &self,
        timeout_ms: u32,
    ) -> Result<Option<VendorEvent>, crate::error::TransportError> {
        if let Some(ref tx) = self.event_tx {
            let mut rx = tx.subscribe();
            let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
            loop {
                match rx.try_recv() {
                    Ok(timestamped) => return Ok(Some(timestamped.event)),
                    Err(broadcast::error::TryRecvError::Empty) => {
                        if Instant::now() >= deadline {
                            return Ok(None);
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                    Err(broadcast::error::TryRecvError::Closed) => return Ok(None),
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Subscribe to the event broadcast channel.
    pub fn subscribe(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        self.event_tx.as_ref().map(|tx| tx.subscribe())
    }
}

impl Drop for EventSubsystem {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// Configuration for the event reader loop
#[derive(Clone)]
pub struct EventReaderConfig {
    /// Read timeout in milliseconds (for checking shutdown flag when idle)
    pub read_timeout_ms: i32,
    /// Sleep duration on error before retrying
    pub error_sleep_ms: u64,
    /// Name prefix for debug logging
    pub name: &'static str,
}

impl EventReaderConfig {
    /// Configuration for USB wired transport
    pub fn usb() -> Self {
        Self {
            read_timeout_ms: 5,
            error_sleep_ms: 100,
            name: "HID",
        }
    }

    /// Configuration for 2.4GHz dongle transport
    pub fn dongle() -> Self {
        Self {
            read_timeout_ms: 5,
            error_sleep_ms: 100,
            name: "Dongle",
        }
    }

    /// Configuration for Bluetooth transport
    pub fn bluetooth() -> Self {
        Self {
            read_timeout_ms: 10,
            error_sleep_ms: 100,
            name: "Bluetooth",
        }
    }
}

/// Generic event reader loop for all transport backends
///
/// Reads from a HID input device and broadcasts timestamped events to subscribers.
/// The loop runs until the shutdown flag is set.
///
/// # Arguments
/// * `input_device` - HID device to read from
/// * `tx` - Broadcast sender for timestamped events
/// * `shutdown` - Atomic flag to signal shutdown
/// * `parser` - Function to parse raw bytes into VendorEvent
/// * `config` - Configuration options
///
/// # Example
/// ```ignore
/// run_event_reader_loop(
///     device,
///     tx,
///     shutdown,
///     parse_usb_event,
///     EventReaderConfig::usb(),
/// );
/// ```
pub fn run_event_reader_loop<F>(
    input_device: HidDevice,
    tx: broadcast::Sender<TimestampedEvent>,
    shutdown: Arc<AtomicBool>,
    parser: F,
    config: EventReaderConfig,
) where
    F: Fn(&[u8]) -> VendorEvent,
{
    debug!("{} event reader thread started", config.name);
    let mut buf = [0u8; 64];
    let start_time = Instant::now();

    while !shutdown.load(Ordering::Relaxed) {
        // Read with short timeout - wakes immediately on data
        // Timeout only affects how often we check shutdown flag when idle
        match input_device.read_timeout(&mut buf, config.read_timeout_ms) {
            Ok(len) if len > 0 => {
                // Capture timestamp immediately after read
                let timestamp = start_time.elapsed().as_secs_f64();
                debug!(
                    "{} event reader got {} bytes at {:.3}s: {:02X?}",
                    config.name,
                    len,
                    timestamp,
                    &buf[..len.min(16)]
                );
                let event = parser(&buf[..len]);
                let timestamped = TimestampedEvent::new(timestamp, event);
                // Send to all subscribers (ignores if no receivers)
                let _ = tx.send(timestamped);
            }
            Ok(_) => {
                // Timeout, no data - loop continues to check shutdown
            }
            Err(e) => {
                // Log error but keep trying (device might recover)
                warn!("{} event reader error: {}", config.name, e);
                // Brief sleep to avoid spinning on persistent errors
                std::thread::sleep(Duration::from_millis(config.error_sleep_ms));
            }
        }
    }

    debug!("{} event reader thread exiting", config.name);
}

/// Parse the notification payload (common to USB and BLE after framing removed)
fn parse_event_payload(payload: &[u8], original_data: &[u8]) -> VendorEvent {
    if payload.is_empty() {
        return VendorEvent::Unknown(original_data.to_vec());
    }

    let notif_type = payload[0];
    let value = payload.get(1).copied().unwrap_or(0);

    match notif_type {
        // Wake notification: type 0x00 with all zeros
        notif::WAKE if payload.iter().skip(1).all(|&b| b == 0) => VendorEvent::Wake,

        // Profile changed via Fn+F9..F12
        notif::PROFILE_CHANGE => VendorEvent::ProfileChange { profile: value },

        // Keyboard function notification (Win lock, WASD swap, etc.)
        notif::KB_FUNC => parse_kb_func(payload),

        // LED effect mode changed via Fn+Home/PgUp/End/PgDn
        notif::LED_EFFECT_MODE => VendorEvent::LedEffectMode { effect_id: value },

        // LED effect speed changed via Fn+←/→
        notif::LED_EFFECT_SPEED => VendorEvent::LedEffectSpeed { speed: value },

        // Brightness level changed via Fn+↑/↓
        notif::BRIGHTNESS_LEVEL => VendorEvent::BrightnessLevel { level: value },

        // LED color changed via Fn+\
        notif::LED_COLOR => VendorEvent::LedColor { color: value },

        // Settings ACK: value=1 means change started, value=0 means change complete
        notif::SETTINGS_ACK => VendorEvent::SettingsAck {
            started: value != 0,
        },

        // Key depth report (magnetism)
        notif::KEY_DEPTH if payload.len() >= 5 => {
            let depth_raw = u16::from_le_bytes([payload[1], payload[2]]);
            let key_index = payload[3];
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            }
        }

        // Battery status (from dongle)
        notif::BATTERY_STATUS if payload.len() >= 5 => VendorEvent::BatteryStatus {
            level: payload[3],
            charging: payload[4] & 0x02 != 0,
            online: payload[4] & 0x01 != 0,
        },

        _ => VendorEvent::Unknown(original_data.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_profile_change() {
        // USB format: [05, 01, 02] = report ID 5, type 1 (profile), profile 2
        let event = parse_usb_event(&[0x05, 0x01, 0x02]);
        assert!(matches!(event, VendorEvent::ProfileChange { profile: 2 }));
    }

    #[test]
    fn test_parse_brightness() {
        // USB format without report ID
        let event = parse_usb_event(&[0x06, 0x03]);
        assert!(matches!(event, VendorEvent::BrightnessLevel { level: 3 }));
    }

    #[test]
    fn test_parse_ble_event() {
        // BLE format: [06, 66, 01, 01] = report ID 6, marker 0x66, type 1, profile 1
        let event = parse_ble_event(&[0x06, 0x66, 0x01, 0x01]);
        assert!(matches!(event, VendorEvent::ProfileChange { profile: 1 }));
    }

    #[test]
    fn test_parse_win_lock() {
        // Win lock toggle: type 0x03, category 1, action 0x01 (locked)
        let event = parse_usb_event(&[0x03, 0x01, 0x01]);
        assert!(matches!(event, VendorEvent::WinLockToggle { locked: true }));

        // Unlocked
        let event = parse_usb_event(&[0x03, 0x00, 0x01]);
        assert!(matches!(
            event,
            VendorEvent::WinLockToggle { locked: false }
        ));
    }

    #[test]
    fn test_parse_battery_status() {
        // Battery: type 0x88, [0, 0, level, flags]
        let event = parse_usb_event(&[0x88, 0x00, 0x00, 0x55, 0x03]);
        match event {
            VendorEvent::BatteryStatus {
                level,
                charging,
                online,
            } => {
                assert_eq!(level, 0x55);
                assert!(charging); // bit 1 set
                assert!(online); // bit 0 set
            }
            _ => panic!("Expected BatteryStatus"),
        }
    }

    #[test]
    fn test_parse_key_depth() {
        // Key depth: type 0x1B, [depth_lo, depth_hi, key_index, ...]
        let event = parse_usb_event(&[0x1B, 0x64, 0x00, 0x0A, 0x00]);
        match event {
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            } => {
                assert_eq!(key_index, 0x0A);
                assert_eq!(depth_raw, 0x0064); // 100 in little-endian
            }
            _ => panic!("Expected KeyDepth"),
        }
    }

    #[test]
    fn test_parse_mouse_report() {
        // Mouse report: [02, buttons, 00, X_lo, X_hi, Y_lo, Y_hi, wheel_lo, wheel_hi]
        // Example: small leftward movement (X=-1, Y=0)
        let event = parse_usb_event(&[0x02, 0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00]);
        match event {
            VendorEvent::MouseReport {
                buttons,
                x,
                y,
                wheel,
            } => {
                assert_eq!(buttons, 0);
                assert_eq!(x, -1); // 0xffff as i16
                assert_eq!(y, 0);
                assert_eq!(wheel, 0);
            }
            _ => panic!("Expected MouseReport, got {:?}", event),
        }

        // Example with button press and movement
        let event = parse_usb_event(&[0x02, 0x01, 0x00, 0x05, 0x00, 0xfb, 0xff, 0x00, 0x00]);
        match event {
            VendorEvent::MouseReport {
                buttons,
                x,
                y,
                wheel,
            } => {
                assert_eq!(buttons, 1); // left button
                assert_eq!(x, 5);
                assert_eq!(y, -5); // 0xfffb as i16
                assert_eq!(wheel, 0);
            }
            _ => panic!("Expected MouseReport"),
        }
    }
}
