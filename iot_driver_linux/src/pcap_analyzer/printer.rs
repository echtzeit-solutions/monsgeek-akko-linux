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
    Event {
        timestamp: f64,
        event: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
    #[serde(rename = "command")]
    Command {
        timestamp: f64,
        cmd: u8,
        cmd_name: String,
        direction: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
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

    /// Print a vendor event
    pub fn print_event(&self, timestamp: f64, event: &VendorEvent) {
        if !self.should_show_events() {
            return;
        }

        match self.format {
            OutputFormat::Text => self.print_event_text(timestamp, event),
            OutputFormat::Json => self.print_event_json(timestamp, event),
        }
    }

    fn print_event_text(&self, ts: f64, event: &VendorEvent) {
        match event {
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            } => {
                let depth_mm = *depth_raw as f32 / 100.0;
                println!(
                    "{:.6} EVENT KeyDepth key={} depth={:.2}mm (raw={})",
                    ts, key_index, depth_mm, depth_raw
                );
            }
            VendorEvent::BatteryStatus {
                level,
                charging,
                online,
            } => {
                println!(
                    "{:.6} EVENT Battery {}% charging={} online={}",
                    ts, level, charging, online
                );
            }
            VendorEvent::MagnetismStart => {
                println!("{:.6} EVENT MagnetismStart", ts);
            }
            VendorEvent::MagnetismStop => {
                println!("{:.6} EVENT MagnetismStop", ts);
            }
            VendorEvent::ProfileChange { profile } => {
                println!("{:.6} EVENT ProfileChange profile={}", ts, profile);
            }
            VendorEvent::Wake => {
                println!("{:.6} EVENT Wake", ts);
            }
            VendorEvent::SettingsAck { started } => {
                let status = if *started { "started" } else { "completed" };
                println!("{:.6} EVENT SettingsAck {}", ts, status);
            }
            VendorEvent::LedEffectMode { effect_id } => {
                println!("{:.6} EVENT LedEffectMode effect={}", ts, effect_id);
            }
            VendorEvent::LedEffectSpeed { speed } => {
                println!("{:.6} EVENT LedEffectSpeed speed={}", ts, speed);
            }
            VendorEvent::BrightnessLevel { level } => {
                println!("{:.6} EVENT BrightnessLevel level={}", ts, level);
            }
            VendorEvent::LedColor { color } => {
                println!("{:.6} EVENT LedColor color={}", ts, color);
            }
            VendorEvent::WinLockToggle { locked } => {
                println!("{:.6} EVENT WinLock locked={}", ts, locked);
            }
            VendorEvent::WasdSwapToggle { swapped } => {
                println!("{:.6} EVENT WasdSwap swapped={}", ts, swapped);
            }
            VendorEvent::BacklightToggle => {
                println!("{:.6} EVENT BacklightToggle", ts);
            }
            VendorEvent::FnLayerToggle { layer } => {
                println!("{:.6} EVENT FnLayerToggle layer={}", ts, layer);
            }
            VendorEvent::DialModeToggle => {
                println!("{:.6} EVENT DialModeToggle", ts);
            }
            VendorEvent::UnknownKbFunc { category, action } => {
                println!(
                    "{:.6} EVENT UnknownKbFunc category={} action={}",
                    ts, category, action
                );
            }
            VendorEvent::Unknown(data) => {
                println!("{:.6} EVENT Unknown {:02x?}", ts, data);
            }
        }
    }

    fn print_event_json(&self, ts: f64, event: &VendorEvent) {
        let (event_name, details) = match event {
            VendorEvent::KeyDepth {
                key_index,
                depth_raw,
            } => (
                "KeyDepth".to_string(),
                Some(serde_json::json!({
                    "key_index": key_index,
                    "depth_raw": depth_raw,
                    "depth_mm": *depth_raw as f32 / 100.0
                })),
            ),
            VendorEvent::BatteryStatus {
                level,
                charging,
                online,
            } => (
                "BatteryStatus".to_string(),
                Some(serde_json::json!({
                    "level": level,
                    "charging": charging,
                    "online": online
                })),
            ),
            VendorEvent::MagnetismStart => ("MagnetismStart".to_string(), None),
            VendorEvent::MagnetismStop => ("MagnetismStop".to_string(), None),
            VendorEvent::ProfileChange { profile } => (
                "ProfileChange".to_string(),
                Some(serde_json::json!({ "profile": profile })),
            ),
            VendorEvent::Wake => ("Wake".to_string(), None),
            VendorEvent::SettingsAck { started } => (
                "SettingsAck".to_string(),
                Some(serde_json::json!({ "started": started })),
            ),
            VendorEvent::Unknown(data) => (
                "Unknown".to_string(),
                Some(serde_json::json!({ "raw": format!("{:02x?}", data) })),
            ),
            _ => (format!("{:?}", event), None),
        };

        let packet = DecodedPacket::Event {
            timestamp: ts,
            event: event_name,
            details,
        };
        println!("{}", serde_json::to_string(&packet).unwrap());
    }

    /// Print a command/response
    pub fn print_command(&self, timestamp: f64, cmd_byte: u8, data: &[u8], is_response: bool) {
        if !self.should_show_command(cmd_byte) {
            return;
        }

        match self.format {
            OutputFormat::Text => self.print_command_text(timestamp, cmd_byte, data, is_response),
            OutputFormat::Json => self.print_command_json(timestamp, cmd_byte, data, is_response),
        }
    }

    fn print_command_text(&self, ts: f64, cmd_byte: u8, data: &[u8], is_response: bool) {
        let direction = if is_response { "RSP " } else { "CMD " };
        let cmd_name = cmd::name(cmd_byte);

        // Show first 16 bytes of data (excluding command byte)
        let data_preview = if data.len() > 1 {
            let end = std::cmp::min(data.len(), 17);
            format!(" data={:02x?}", &data[1..end])
        } else {
            String::new()
        };

        println!(
            "{:.6} {} 0x{:02x} {}{}",
            ts, direction, cmd_byte, cmd_name, data_preview
        );
    }

    fn print_command_json(&self, ts: f64, cmd_byte: u8, data: &[u8], is_response: bool) {
        let packet = DecodedPacket::Command {
            timestamp: ts,
            cmd: cmd_byte,
            cmd_name: cmd::name(cmd_byte).to_string(),
            direction: if is_response {
                "response".to_string()
            } else {
                "command".to_string()
            },
            data: if data.len() > 1 {
                Some(format!("{:02x?}", &data[1..]))
            } else {
                None
            },
        };
        println!("{}", serde_json::to_string(&packet).unwrap());
    }

    /// Print an unknown/unparseable packet
    pub fn print_unknown(&self, timestamp: f64, data: &[u8]) {
        if matches!(self.filter, PacketFilter::Cmd(_)) {
            return; // Don't show unknown packets when filtering by command
        }

        match self.format {
            OutputFormat::Text => {
                let preview_len = std::cmp::min(data.len(), 32);
                println!(
                    "{:.6} UNKNOWN: {:02x?}{}",
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
