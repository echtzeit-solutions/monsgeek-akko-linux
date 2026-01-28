//! Output formatting for pcap analyzer
//!
//! This module provides formatting for decoded USB HID packets.
//! Uses monsgeek-transport parsers as the single source of truth.
//! The key value is highlighting UNKNOWN packets for protocol discovery.

use crate::protocol::cmd;
use colored::Colorize;
use monsgeek_transport::{
    try_parse_command, try_parse_response, ParsedCommand, ParsedResponse, VendorEvent,
};
use serde::Serialize;
use std::fmt::Write;
use std::str::FromStr;

/// Format bytes with run-length encoding for sparse data
/// e.g., `[0, 0, 0, 0, 0x6c, 0x5f, 0, 0]` becomes `"5×00 6c 5f 2×00"`
pub fn format_sparse_hex(data: &[u8]) -> String {
    if data.is_empty() {
        return String::from("[]");
    }

    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let byte = data[i];
        let mut count = 1;

        // Count consecutive identical bytes
        while i + count < data.len() && data[i + count] == byte && count < 255 {
            count += 1;
        }

        if !result.is_empty() {
            result.push(' ');
        }

        if count >= 3 {
            // Use run-length encoding for 3+ consecutive bytes
            let _ = write!(result, "{}×{:02x}", count, byte);
        } else {
            // Write individual bytes
            for j in 0..count {
                if j > 0 {
                    result.push(' ');
                }
                let _ = write!(result, "{:02x}", data[i + j]);
            }
        }

        i += count;
    }

    result
}

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
    /// Show Debug-derived fields for events
    show_debug: bool,
    /// Show raw hex dump alongside decoded output
    show_hex: bool,
    /// Show standard HID reports (keyboard, consumer, NKRO)
    show_all_hid: bool,
}

impl Printer {
    pub fn new(format: OutputFormat, filter: PacketFilter) -> Self {
        Self {
            format,
            filter,
            show_debug: false,
            show_hex: false,
            show_all_hid: false,
        }
    }

    /// Enable debug field output
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.show_debug = debug;
        self
    }

    /// Enable raw hex dump output
    pub fn with_hex(mut self, hex: bool) -> Self {
        self.show_hex = hex;
        self
    }

    /// Enable showing all HID reports (keyboard, consumer, NKRO)
    pub fn with_all_hid(mut self, all: bool) -> Self {
        self.show_all_hid = all;
        self
    }

    /// Check if all HID reports should be shown
    pub fn show_all_hid(&self) -> bool {
        self.show_all_hid
    }

    /// Print a standard HID input report (keyboard, consumer, NKRO)
    pub fn print_hid_input(&self, timestamp: f64, report_id: u8, data: &[u8], endpoint: u8) {
        if !self.show_all_hid {
            return;
        }

        let report_type = match report_id {
            0x00 => "KEYBOARD",
            0x01 => "NKRO",
            0x02 => "MOUSE",
            0x03 => "CONSUMER",
            _ => "HID",
        };

        match self.format {
            OutputFormat::Text => {
                // Format based on report type
                let info = match report_id {
                    0x00 => {
                        // Standard keyboard: modifier, reserved, keycodes[6]
                        if data.len() >= 8 {
                            let mods = data[1];
                            let keycodes: Vec<u8> =
                                data[3..8].iter().copied().filter(|&k| k != 0).collect();
                            format!("mod=0x{:02x} keys={:?}", mods, keycodes)
                        } else {
                            format!("{:02x?}", data)
                        }
                    }
                    0x03 => {
                        // Consumer control: 2-byte usage code
                        if data.len() >= 3 {
                            let usage = u16::from_le_bytes([data[1], data[2]]);
                            format!("usage=0x{:04x}", usage)
                        } else {
                            format!("{:02x?}", data)
                        }
                    }
                    _ => format!("{:02x?}", data),
                };
                println!(
                    "{:.6} {}  EP{:02x} {} {}",
                    format!("{:.6}", timestamp).dimmed(),
                    "HID".blue(),
                    endpoint,
                    report_type.cyan(),
                    info
                );
                if self.show_hex {
                    println!("         {}   {:02x?}", "HEX".dimmed(), data);
                }
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Event {
                    timestamp,
                    event: format!("{}:{:02x?}", report_type, data),
                };
                println!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
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

    /// Print a vendor event
    pub fn print_event(&self, timestamp: f64, event: &VendorEvent, raw_data: Option<&[u8]>) {
        if !self.should_show_events() {
            return;
        }

        match self.format {
            OutputFormat::Text => {
                if self.show_debug {
                    // Full Debug output with all fields
                    println!(
                        "{} {} {:#?}",
                        format!("{:.6}", timestamp).dimmed(),
                        "EVENT".yellow().bold(),
                        event
                    );
                } else {
                    // Compact single-line format
                    println!(
                        "{} {} {:?}",
                        format!("{:.6}", timestamp).dimmed(),
                        "EVENT".yellow().bold(),
                        event
                    );
                }
                if self.show_hex {
                    if let Some(data) = raw_data {
                        println!("         {}   {:02x?}", "HEX".dimmed(), data);
                    }
                }
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
    ///
    /// For responses (cmd & 0x80 != 0), uses monsgeek-transport parsers.
    /// Unknown responses are flagged prominently for protocol discovery.
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

        let (direction, dir_color): (&str, fn(&str) -> colored::ColoredString) = if is_response {
            ("RSP", |s| s.green().bold())
        } else {
            ("CMD", |s| s.cyan().bold())
        };

        match self.format {
            OutputFormat::Text => {
                if is_response {
                    // Use transport parsers for responses - single source of truth
                    let parsed = try_parse_response(data);
                    match &parsed {
                        ParsedResponse::Empty => {
                            // Empty/stale buffer - dimmed output
                            println!(
                                "{} {} EP{:02x} {}",
                                format!("{:.6}", timestamp).dimmed(),
                                dir_color(direction),
                                endpoint,
                                "(empty)".dimmed()
                            );
                        }
                        ParsedResponse::Unknown { cmd, data: raw } => {
                            // Flag for investigation - use compressed hex
                            let cmd_name = cmd::name(*cmd);
                            println!(
                                "{} {} EP{:02x} 0x{:02x} {} {} {}",
                                format!("{:.6}", timestamp).dimmed(),
                                dir_color(direction),
                                endpoint,
                                cmd,
                                cmd_name.yellow(),
                                "UNKNOWN".red().bold(),
                                format_sparse_hex(raw)
                            );
                        }
                        _ => {
                            // Known response - use Debug derive
                            if self.show_debug {
                                println!(
                                    "{} {} EP{:02x} {:#?}",
                                    format!("{:.6}", timestamp).dimmed(),
                                    dir_color(direction),
                                    endpoint,
                                    parsed
                                );
                            } else {
                                println!(
                                    "{} {} EP{:02x} {:?}",
                                    format!("{:.6}", timestamp).dimmed(),
                                    dir_color(direction),
                                    endpoint,
                                    parsed
                                );
                            }
                        }
                    }
                } else {
                    // Commands - try to parse as typed struct
                    let parsed = try_parse_command(data);
                    match &parsed {
                        ParsedCommand::Unknown { cmd, data: raw } => {
                            let cmd_name = cmd::name(*cmd);
                            println!(
                                "{} {} EP{:02x} 0x{:02x} {} {:02x?}",
                                format!("{:.6}", timestamp).dimmed(),
                                dir_color(direction),
                                endpoint,
                                cmd,
                                cmd_name.yellow(),
                                raw
                            );
                        }
                        _ => {
                            if self.show_debug {
                                println!(
                                    "{} {} EP{:02x} {:#?}",
                                    format!("{:.6}", timestamp).dimmed(),
                                    dir_color(direction),
                                    endpoint,
                                    parsed
                                );
                            } else {
                                println!(
                                    "{} {} EP{:02x} {:?}",
                                    format!("{:.6}", timestamp).dimmed(),
                                    dir_color(direction),
                                    endpoint,
                                    parsed
                                );
                            }
                        }
                    }
                }

                if self.show_hex {
                    println!("         {}   {:02x?}", "HEX".dimmed(), data);
                }
            }
            OutputFormat::Json => {
                let cmd_name = cmd::name(cmd_byte);
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
                println!(
                    "{} {} {:02x?}",
                    format!("{:.6}", timestamp).dimmed(),
                    "UNKNOWN".red().bold(),
                    data
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

    /// Print a USB control transfer (non-HID)
    ///
    /// Note: Some USB captures show HID responses as generic control transfers
    /// (e.g., GET_STATUS) rather than properly tagged HID GET_REPORT responses.
    /// We detect these by checking if the data looks like a HID response.
    pub fn print_usb_control(
        &self,
        timestamp: f64,
        request_name: &str,
        descriptor_info: Option<(&str, u8)>, // (type_name, index)
        data: &[u8],
        is_response: bool,
        endpoint: u8,
    ) {
        // Check if this looks like a HID response (first byte is a known command echo)
        // This handles captures where HID responses come through as generic control transfers
        if is_response && !data.is_empty() {
            let first_byte = data[0];
            // Check if first byte looks like a command response (has bit 7 set for GET commands,
            // or matches known command bytes)
            let looks_like_hid = first_byte & 0x80 != 0 // GET command responses
                || matches!(first_byte, 0x00..=0x1F | 0x65); // SET command ACKs

            if looks_like_hid {
                // Route to print_command instead
                self.print_command(timestamp, first_byte, data, true, endpoint);
                return;
            }
        }

        if matches!(self.filter, PacketFilter::Cmd(_) | PacketFilter::Events) {
            return;
        }

        let direction = if is_response {
            "<<<".green()
        } else {
            ">>>".cyan()
        };

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
                            "{} {}  EP{:02x} {} {} {}[{}] \"{}\"",
                            format!("{:.6}", timestamp).dimmed(),
                            "USB".magenta(),
                            endpoint,
                            direction,
                            request_name.yellow(),
                            desc_type,
                            index,
                            s.bright_white()
                        );
                    } else {
                        println!(
                            "{} {}  EP{:02x} {} {} {}[{}] {:02x?}",
                            format!("{:.6}", timestamp).dimmed(),
                            "USB".magenta(),
                            endpoint,
                            direction,
                            request_name.yellow(),
                            desc_type,
                            index,
                            data
                        );
                    }
                } else {
                    // Non-descriptor control transfer - just show hex
                    println!(
                        "{} {}  EP{:02x} {} {} {:02x?}",
                        format!("{:.6}", timestamp).dimmed(),
                        "USB".magenta(),
                        endpoint,
                        direction,
                        request_name.yellow(),
                        data
                    );
                }
                // Show raw hex dump on separate line if enabled
                if self.show_hex && !data.is_empty() {
                    println!("         {}   {:02x?}", "HEX".dimmed(), data);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_sparse_hex() {
        // Empty
        assert_eq!(format_sparse_hex(&[]), "[]");

        // Single byte
        assert_eq!(format_sparse_hex(&[0x6c]), "6c");

        // Two identical (no compression)
        assert_eq!(format_sparse_hex(&[0x00, 0x00]), "00 00");

        // Three identical (compressed)
        assert_eq!(format_sparse_hex(&[0x00, 0x00, 0x00]), "3×00");

        // Mixed
        assert_eq!(
            format_sparse_hex(&[0x00, 0x00, 0x00, 0x6c, 0x5f, 0x00, 0x00]),
            "3×00 6c 5f 00 00"
        );

        // Long run
        let zeros = vec![0u8; 32];
        assert_eq!(format_sparse_hex(&zeros), "32×00");

        // Sparse with data in middle
        let mut data = vec![0u8; 10];
        data[4] = 0x6c;
        data[5] = 0x5f;
        assert_eq!(format_sparse_hex(&data), "4×00 6c 5f 4×00");
    }
}
