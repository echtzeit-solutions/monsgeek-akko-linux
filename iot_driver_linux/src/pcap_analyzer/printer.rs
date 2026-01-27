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
}
