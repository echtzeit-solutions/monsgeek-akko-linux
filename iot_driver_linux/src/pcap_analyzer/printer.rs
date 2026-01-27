//! Output formatting for pcap analyzer
//!
//! This module provides formatting for decoded USB HID packets,
//! supporting both human-readable text and JSON output.

use crate::protocol::cmd;
use monsgeek_transport::VendorEvent;
use serde::Serialize;
use std::str::FromStr;

/// Output format for the analyzer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Human-readable text output
    #[default]
    Text,
    /// JSON output (one object per line)
    Json,
}

/// Packet filter for selective display
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketFilter {
    /// Show all packets
    All,
    /// Show only events (input reports)
    Events,
    /// Show only commands (output reports)
    Commands,
    /// Show only packets matching a specific command byte
    Cmd(u8),
}

impl FromStr for PacketFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "all" | "" => Ok(Self::All),
            "events" | "event" => Ok(Self::Events),
            "commands" | "cmd" | "cmds" => Ok(Self::Commands),
            s if s.starts_with("cmd=") || s.starts_with("0x") => {
                let hex_str = s.strip_prefix("cmd=").unwrap_or(s);
                let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                u8::from_str_radix(hex_str, 16)
                    .map(Self::Cmd)
                    .map_err(|e| format!("Invalid command byte: {}", e))
            }
            _ => Err(format!("Unknown filter: {}", s)),
        }
    }
}

/// A decoded packet for JSON output
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum DecodedPacket {
    #[serde(rename = "event")]
    Event { timestamp: f64, event: String },
    #[serde(rename = "command")]
    Command {
        timestamp: f64,
        cmd: u8,
        cmd_name: String,
        direction: String,
        decoded: String,
    },
    #[serde(rename = "unknown")]
    Unknown { timestamp: f64, data: String },
}

/// Output printer
pub struct Printer {
    format: OutputFormat,
    filter: PacketFilter,
}

impl Printer {
    pub fn new(format: OutputFormat, filter: PacketFilter) -> Self {
        Self { format, filter }
    }

    /// Check if a command byte should be shown based on filter
    fn should_show_command(&self, cmd: u8) -> bool {
        match &self.filter {
            PacketFilter::All => true,
            PacketFilter::Events => false,
            PacketFilter::Commands => true,
            PacketFilter::Cmd(c) => *c == cmd || *c == (cmd | 0x80),
        }
    }

    /// Check if events should be shown
    fn should_show_events(&self) -> bool {
        matches!(&self.filter, PacketFilter::All | PacketFilter::Events)
    }

    /// Print a vendor event using Debug formatting
    pub fn print_event(&self, timestamp: f64, event: &VendorEvent) {
        if !self.should_show_events() {
            return;
        }

        match self.format {
            OutputFormat::Text => {
                // Use Debug formatting for human-readable output
                println!("{:.6} EVENT {:?}", timestamp, event);
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Event {
                    timestamp,
                    event: format!("{:?}", event),
                };
                println!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }

    /// Print a command/response
    pub fn print_command(&self, timestamp: f64, cmd_byte: u8, data: &[u8], is_response: bool) {
        if !self.should_show_command(cmd_byte) {
            return;
        }

        let direction = if is_response { "RSP" } else { "CMD" };
        let cmd_name = cmd::name(cmd_byte);
        let decoded = decode_command(cmd_byte, data);

        match self.format {
            OutputFormat::Text => {
                println!(
                    "{:.6} {} 0x{:02x} {} {}",
                    timestamp, direction, cmd_byte, cmd_name, decoded
                );
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Command {
                    timestamp,
                    cmd: cmd_byte,
                    cmd_name: cmd_name.to_string(),
                    direction: direction.to_string(),
                    decoded,
                };
                println!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }

    /// Print an unknown/unparseable packet
    pub fn print_unknown(&self, timestamp: f64, data: &[u8]) {
        if matches!(self.filter, PacketFilter::Cmd(_)) {
            return;
        }

        match self.format {
            OutputFormat::Text => {
                let preview_len = std::cmp::min(data.len(), 32);
                println!(
                    "{:.6} UNKNOWN {:02x?}{}",
                    timestamp,
                    &data[..preview_len],
                    if data.len() > preview_len { "..." } else { "" }
                );
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Unknown {
                    timestamp,
                    data: format!("{:02x?}", data),
                };
                println!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }
}

/// Decode command data into human-readable format
fn decode_command(cmd_byte: u8, data: &[u8]) -> String {
    if data.len() <= 1 {
        return String::new();
    }
    let payload = &data[1..]; // Skip command byte

    match cmd_byte {
        // SET_SCREEN_COLOR / SET_CALIBRATION_LED (0x0e)
        0x0e => {
            if payload.len() >= 5 {
                let mode = payload[0];
                let key = payload[1];
                let r = payload[2];
                let g = payload[3];
                let b = payload[4];
                format!("mode={} key={} rgb=({},{},{})", mode, key, r, g, b)
            } else {
                format!("data={:02x?}", payload)
            }
        }

        // Keyboard HID report (0x00)
        0x00 => {
            if payload.len() >= 7 {
                let modifier = payload[0];
                let keys: Vec<u8> = payload[2..].iter().copied().filter(|&k| k != 0).collect();
                if keys.is_empty() && modifier == 0 {
                    "release".to_string()
                } else {
                    format!("mod=0x{:02x} keys={:02x?}", modifier, keys)
                }
            } else {
                format!("data={:02x?}", payload)
            }
        }

        // SET_PROFILE (0x05) / GET_PROFILE response (0x85)
        0x05 | 0x85 => {
            if !payload.is_empty() {
                format!("profile={}", payload[0])
            } else {
                String::new()
            }
        }

        // SET_LEDPARAM (0x06) / GET_LEDPARAM response (0x86)
        0x06 | 0x86 => {
            if payload.len() >= 4 {
                format!(
                    "effect={} speed={} brightness={} color={}",
                    payload[0], payload[1], payload[2], payload[3]
                )
            } else {
                format!("data={:02x?}", payload)
            }
        }

        // SET_MAGNETISM_REPORT (0x0f)
        0x0f => {
            if !payload.is_empty() {
                let enabled = payload[0] != 0;
                format!("enabled={}", enabled)
            } else {
                String::new()
            }
        }

        // GET_SETTINGS response (0x8f)
        0x8f => {
            format!("settings={:02x?}", payload)
        }

        // GET_DEVICE_INFO response (0x84)
        0x84 => {
            if payload.len() >= 8 {
                format!(
                    "ver={}.{}.{} fw={}.{}.{}",
                    payload[0], payload[1], payload[2], payload[3], payload[4], payload[5]
                )
            } else {
                format!("data={:02x?}", payload)
            }
        }

        // BATTERY commands (0xf7, 0x88)
        0xf7 | 0x88 => {
            if payload.len() >= 2 {
                let level = payload[0];
                let flags = payload[1];
                let charging = flags & 0x02 != 0;
                let online = flags & 0x01 != 0;
                format!("level={}% charging={} online={}", level, charging, online)
            } else if !payload.is_empty() {
                format!("level={}%", payload[0])
            } else {
                String::new()
            }
        }

        // SET_MULTI_MAGNETISM (0x65) / GET_MULTI_MAGNETISM (0xe5)
        0x65 | 0xe5 => {
            if payload.len() >= 2 {
                let count = payload[0];
                format!("keys={} data={:02x?}", count, &payload[1..])
            } else {
                format!("data={:02x?}", payload)
            }
        }

        // SET_TRIGGER (0x67)
        0x67 => {
            if payload.len() >= 3 {
                let key = payload[0];
                let act = payload[1];
                let rel = payload[2];
                format!("key={} actuation={} release={}", key, act, rel)
            } else {
                format!("data={:02x?}", payload)
            }
        }

        // Default: show raw hex
        _ => {
            let end = std::cmp::min(payload.len(), 16);
            if payload.is_empty() {
                String::new()
            } else if payload.len() > 16 {
                format!("data={:02x?}...", &payload[..end])
            } else {
                format!("data={:02x?}", payload)
            }
        }
    }
}
