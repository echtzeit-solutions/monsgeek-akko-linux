//! Output formatting for pcap analyzer
//!
//! This module provides formatting for decoded USB HID packets.
//! The goal is to catch unknown commands/events we haven't implemented yet.

use crate::protocol::cmd;
use monsgeek_transport::VendorEvent;
use serde::Serialize;
use std::str::FromStr;

/// Output format for the analyzer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

/// Packet filter for selective display
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketFilter {
    All,
    Events,
    Commands,
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
        data: String,
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

    fn should_show_command(&self, cmd: u8) -> bool {
        match &self.filter {
            PacketFilter::All => true,
            PacketFilter::Events => false,
            PacketFilter::Commands => true,
            PacketFilter::Cmd(c) => *c == cmd || *c == (cmd | 0x80),
        }
    }

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
                // Use Debug for full field visibility
                println!("{:.6} EVENT {:#?}", timestamp, event);
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

    /// Print a command/response with raw data
    pub fn print_command(
        &self,
        timestamp: f64,
        cmd_byte: u8,
        data: &[u8],
        is_response: bool,
        endpoint: u8,
    ) {
        if !self.should_show_command(cmd_byte) {
            return;
        }

        let direction = if is_response { "RSP" } else { "CMD" };
        let cmd_name = cmd::name(cmd_byte);

        match self.format {
            OutputFormat::Text => {
                // Show endpoint, command name and ALL data bytes for analysis
                println!(
                    "{:.6} {} EP{:02x} 0x{:02x} {} {:02x?}",
                    timestamp, direction, endpoint, cmd_byte, cmd_name, data
                );
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Command {
                    timestamp,
                    cmd: cmd_byte,
                    cmd_name: cmd_name.to_string(),
                    direction: direction.to_string(),
                    data: format!("{:02x?}", data),
                };
                println!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }

    /// Print an unknown/unparseable packet - IMPORTANT for discovery
    pub fn print_unknown(&self, timestamp: f64, data: &[u8]) {
        if matches!(self.filter, PacketFilter::Cmd(_)) {
            return;
        }

        match self.format {
            OutputFormat::Text => {
                // Print ALL data for unknown packets - this is what we're looking for!
                println!("{:.6} UNKNOWN {:02x?}", timestamp, data);
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

    /// Print a USB control transfer (non-HID)
    pub fn print_usb_control(
        &self,
        timestamp: f64,
        request_name: &str,
        descriptor_info: Option<(&str, u8)>, // (type_name, index)
        data: &[u8],
        is_response: bool,
        endpoint: u8,
    ) {
        if matches!(self.filter, PacketFilter::Cmd(_) | PacketFilter::Events) {
            return;
        }

        let direction = if is_response { "<<<" } else { ">>>" };

        match self.format {
            OutputFormat::Text => {
                if let Some((desc_type, index)) = descriptor_info {
                    // For descriptors, try to decode the data
                    let decoded = if desc_type == "STRING" && data.len() >= 2 {
                        decode_usb_string(data)
                    } else {
                        None
                    };

                    if let Some(s) = decoded {
                        println!(
                            "{:.6} USB  EP{:02x} {} {} {}[{}] \"{}\"",
                            timestamp, endpoint, direction, request_name, desc_type, index, s
                        );
                    } else {
                        println!(
                            "{:.6} USB  EP{:02x} {} {} {}[{}] {:02x?}",
                            timestamp, endpoint, direction, request_name, desc_type, index, data
                        );
                    }
                } else {
                    // Try to decode as USB string descriptor or raw UTF-16LE
                    let decoded = if is_response && data.len() >= 4 {
                        // Try as string descriptor first (handles 2-byte header),
                        // falls back to raw UTF-16LE
                        decode_usb_string(data)
                    } else {
                        None
                    };

                    if let Some(s) = decoded {
                        println!(
                            "{:.6} USB  EP{:02x} {} {} \"{}\"",
                            timestamp, endpoint, direction, request_name, s
                        );
                    } else {
                        println!(
                            "{:.6} USB  EP{:02x} {} {} {:02x?}",
                            timestamp, endpoint, direction, request_name, data
                        );
                    }
                }
            }
            OutputFormat::Json => {
                // For JSON, include raw data and decoded if available
                let packet = DecodedPacket::Unknown {
                    timestamp,
                    data: format!("{} {:02x?}", request_name, data),
                };
                println!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }
}

/// Decode USB string descriptor (UTF-16LE with length/type header)
fn decode_usb_string(data: &[u8]) -> Option<String> {
    if data.len() < 2 {
        return None;
    }

    // USB string descriptor format:
    // Byte 0: bLength (total length including header)
    // Byte 1: bDescriptorType (0x03 for STRING)
    // Bytes 2+: Unicode string in UTF-16LE

    let b_length = data[0] as usize;
    let b_descriptor_type = data[1];

    // Check if it's a valid string descriptor
    if b_descriptor_type != 0x03 || b_length < 2 || b_length > data.len() {
        // Try decoding as raw UTF-16LE (no header)
        return decode_utf16le(data);
    }

    // Skip the 2-byte header, decode the rest as UTF-16LE
    let string_data = &data[2..b_length.min(data.len())];
    decode_utf16le(string_data)
}

/// Decode UTF-16LE bytes to String
fn decode_utf16le(data: &[u8]) -> Option<String> {
    if data.len() < 2 || !data.len().is_multiple_of(2) {
        return None;
    }

    let u16_iter = data
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]));

    // Decode UTF-16, stopping at null terminator
    let decoded: String = char::decode_utf16(u16_iter)
        .take_while(|r| r.as_ref().map(|&c| c != '\0').unwrap_or(false))
        .filter_map(|r| r.ok())
        .collect();

    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}
