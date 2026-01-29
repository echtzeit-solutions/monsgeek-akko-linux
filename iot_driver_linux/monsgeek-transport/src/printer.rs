//! Unified Printer for monitoring/tracing transport operations
//!
//! This module provides a printer that can work in two modes:
//!
//! 1. **Live mode**: Wraps a Transport, intercepting and printing commands/responses
//! 2. **Standalone mode**: Used for pcap analysis where packets are fed directly
//!
//! # Live Mode Example
//!
//! ```ignore
//! use monsgeek_transport::{HidWiredTransport, Printer, PrinterConfig, PacketFilter};
//!
//! let transport = HidWiredTransport::open(&device)?;
//! let config = PrinterConfig::default();
//! let monitored = Printer::wrap(Arc::new(transport), config);
//! // Now all commands/responses will be printed
//! ```
//!
//! # Standalone Mode Example
//!
//! ```ignore
//! use monsgeek_transport::{Printer, PrinterConfig};
//!
//! let printer = Printer::standalone(PrinterConfig::default());
//! // Feed packets directly
//! printer.on_command(0x8f, &[0x01, 0x02], None, None);
//! printer.on_response(&response_data, None, None);
//! ```

use crate::protocol::{cmd, magnetism as mag_const};
use crate::{
    decode_magnetism_data, try_parse_command, try_parse_response, ChecksumType, MagnetismData,
    ParsedCommand, ParsedResponse, TimestampedEvent, Transport, TransportDeviceInfo,
    TransportError, VendorEvent,
};
use async_trait::async_trait;
use crossterm::style::Stylize;
use parking_lot::Mutex;
use serde::Serialize;
use std::fmt::Write;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Format bytes with run-length encoding for sparse data
/// e.g., `[0, 0, 0, 0, 0x6c, 0x5f, 0, 0]` becomes `"5×00 6c 5f 2×00"`
fn format_sparse_hex(data: &[u8]) -> String {
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

/// Output format for the printer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

/// Packet filter for selective display
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PacketFilter {
    #[default]
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

/// Configuration for the Printer
#[derive(Debug, Clone)]
pub struct PrinterConfig {
    /// Show raw hex dump alongside decoded output
    pub show_hex: bool,
    /// Show all HID reports (keyboard, consumer, NKRO)
    pub show_all_hid: bool,
    /// Filter for selective display
    pub filter: PacketFilter,
    /// Output format
    pub format: OutputFormat,
    /// Show Debug-derived full struct fields (vs compact single-line)
    pub show_debug: bool,
}

impl Default for PrinterConfig {
    fn default() -> Self {
        Self {
            show_hex: false,
            show_all_hid: false,
            filter: PacketFilter::All,
            format: OutputFormat::Text,
            show_debug: false,
        }
    }
}

impl PrinterConfig {
    /// Create config with hex output setting
    pub fn with_hex(mut self, show: bool) -> Self {
        self.show_hex = show;
        self
    }

    /// Create config with all HID reports setting
    pub fn with_all_hid(mut self, show: bool) -> Self {
        self.show_all_hid = show;
        self
    }

    /// Create config with filter
    pub fn with_filter(mut self, filter: PacketFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Create config with debug output setting
    pub fn with_debug(mut self, show: bool) -> Self {
        self.show_debug = show;
        self
    }

    /// Create config with output format
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.format = format;
        self
    }
}

/// Tracks pending command context for stateful response parsing
#[derive(Debug, Clone, Default)]
struct CommandContext {
    /// Last GetMultiMagnetism subcmd (for parsing 0x00 responses)
    last_magnetism_subcmd: Option<u8>,
    /// Last GetMultiMagnetism page (for parsing 0x00 responses)
    last_magnetism_page: Option<u8>,
}

/// Unified printer for monitoring transport operations
///
/// Works in two modes:
/// - **Live mode**: Wraps a Transport, intercepting commands/responses
/// - **Standalone mode**: Used for pcap analysis where packets are fed directly
///
/// The core API consists of three public methods:
/// - `on_command()` - Called when a command is sent
/// - `on_response()` - Called when a response arrives
/// - `on_event()` - Called when an event arrives
pub struct Printer {
    /// Inner transport for live mode, None for standalone/pcap mode
    inner: Option<Arc<dyn Transport>>,
    config: PrinterConfig,
    context: Mutex<CommandContext>,
}

/// For backwards compatibility
pub type PrinterTransport = Printer;

impl Printer {
    /// Create a standalone printer (for pcap analysis)
    pub fn standalone(config: PrinterConfig) -> Self {
        Self {
            inner: None,
            config,
            context: Mutex::new(CommandContext::default()),
        }
    }

    /// Wrap a transport with printing middleware (for live monitoring)
    pub fn wrap(transport: Arc<dyn Transport>, config: PrinterConfig) -> Arc<dyn Transport> {
        Arc::new(Self {
            inner: Some(transport),
            config,
            context: Mutex::new(CommandContext::default()),
        })
    }

    /// Check if all HID reports should be shown
    pub fn show_all_hid(&self) -> bool {
        self.config.show_all_hid
    }

    /// Update command context for stateful response parsing
    fn update_context(&self, cmd: u8, data: &[u8]) {
        if cmd == cmd::GET_MULTI_MAGNETISM && data.len() >= 3 {
            let mut ctx = self.context.lock();
            ctx.last_magnetism_subcmd = Some(data[0]); // subcmd
            ctx.last_magnetism_page = Some(data[2]); // page
        }
    }

    /// Check if a command should be shown based on filter
    fn should_show_command(&self, cmd: u8) -> bool {
        match &self.config.filter {
            PacketFilter::All => true,
            PacketFilter::Events => false,
            PacketFilter::Commands => true,
            PacketFilter::Cmd(c) => *c == cmd || *c == (cmd | 0x80),
        }
    }

    /// Check if events should be shown
    fn should_show_events(&self) -> bool {
        matches!(
            &self.config.filter,
            PacketFilter::All | PacketFilter::Events
        )
    }

    // ========================================================================
    // PUBLIC API - Called by Transport impl (live) or directly (pcap)
    // ========================================================================

    /// Called when a command is sent (by App or Pcap)
    ///
    /// # Arguments
    /// * `cmd` - Command byte
    /// * `data` - Command data (without command byte)
    /// * `timestamp` - Optional timestamp (for pcap mode)
    /// * `endpoint` - Optional USB endpoint (for pcap mode)
    pub fn on_command(&self, cmd: u8, data: &[u8], timestamp: Option<f64>, endpoint: Option<u8>) {
        // Update context for stateful response parsing
        self.update_context(cmd, data);

        if !self.should_show_command(cmd) {
            return;
        }

        // Build full packet for parsing (cmd + data)
        let mut packet = vec![cmd];
        packet.extend_from_slice(data);

        let parsed = try_parse_command(&packet);

        match self.config.format {
            OutputFormat::Text => {
                let ts_prefix = format_timestamp(timestamp);
                let ep_prefix = format_endpoint(endpoint);

                match &parsed {
                    ParsedCommand::Unknown { cmd, data: raw } => {
                        let cmd_name = cmd::name(*cmd);
                        eprintln!(
                            "{}{} {} 0x{:02x} {} {:02x?}",
                            ts_prefix,
                            ep_prefix,
                            "CMD".cyan().bold(),
                            cmd,
                            cmd_name.yellow(),
                            raw
                        );
                    }
                    _ => {
                        if self.config.show_debug {
                            eprintln!(
                                "{}{} {} {:#?}",
                                ts_prefix,
                                ep_prefix,
                                "CMD".cyan().bold(),
                                parsed
                            );
                        } else {
                            eprintln!(
                                "{}{} {} {:?}",
                                ts_prefix,
                                ep_prefix,
                                "CMD".cyan().bold(),
                                parsed
                            );
                        }
                    }
                }

                if self.config.show_hex {
                    eprintln!("         {}   {:02x?}", "HEX".dim(), packet);
                }
            }
            OutputFormat::Json => {
                let cmd_name = cmd::name(cmd);
                let decoded = DecodedPacket::Command {
                    timestamp: timestamp.unwrap_or(0.0),
                    cmd,
                    cmd_name: cmd_name.to_string(),
                    direction: "CMD".to_string(),
                    data: format!("{:02x?}", data),
                };
                eprintln!("{}", serde_json::to_string(&decoded).unwrap());
            }
        }
    }

    /// Called when a response arrives (from Transport or Pcap)
    ///
    /// # Arguments
    /// * `data` - Response data (full packet including command echo byte)
    /// * `timestamp` - Optional timestamp (for pcap mode)
    /// * `endpoint` - Optional USB endpoint (for pcap mode)
    pub fn on_response(&self, data: &[u8], timestamp: Option<f64>, endpoint: Option<u8>) {
        if data.is_empty() {
            return;
        }

        let cmd = data[0];

        // Check if we have pending magnetism context
        // GET_MULTI_MAGNETISM responses don't echo the command byte - they're raw data
        // So we must check context FIRST, regardless of what byte the response starts with
        let ctx = self.context.lock().clone();
        if let (Some(subcmd), Some(page)) = (ctx.last_magnetism_subcmd, ctx.last_magnetism_page) {
            // Clear context after use to prevent stale matches
            {
                let mut ctx = self.context.lock();
                ctx.last_magnetism_subcmd = None;
                ctx.last_magnetism_page = None;
            }

            // Show magnetism filter if applicable
            if !self.should_show_command(cmd::GET_MULTI_MAGNETISM) {
                return;
            }

            // Decode as magnetism data
            let decoded_data = decode_magnetism_data(subcmd, data);
            let subcmd_name = crate::protocol::magnetism::name(subcmd);

            // Format based on subcmd type
            let display = match (&decoded_data, subcmd) {
                (MagnetismData::TwoByteValues(values), mag_const::CALIBRATION) => {
                    // 32 u16 values per page, so base index = page * 32
                    let base_idx = page as usize * 32;
                    let finished_keys: Vec<String> = values
                        .iter()
                        .enumerate()
                        .filter(|(_, &v)| v >= 300)
                        .map(|(i, _)| {
                            let key_idx = (base_idx + i) as u8;
                            format!(
                                "{}({})",
                                crate::protocol::matrix::key_name(key_idx),
                                key_idx
                            )
                        })
                        .collect();
                    format!(
                        "{} {{ page: {}, finished: {}/{}, keys: [{}] }}",
                        subcmd_name,
                        page,
                        finished_keys.len(),
                        values.len(),
                        finished_keys.join(", ")
                    )
                }
                _ => {
                    format!(
                        "{} {{ page: {}, data: {:?} }}",
                        subcmd_name, page, decoded_data
                    )
                }
            };

            match self.config.format {
                OutputFormat::Text => {
                    let ts_prefix = format_timestamp(timestamp);
                    let ep_prefix = format_endpoint(endpoint);
                    eprintln!(
                        "{}{} {} {}",
                        ts_prefix,
                        ep_prefix,
                        "RSP".green().bold(),
                        display
                    );

                    if self.config.show_hex {
                        eprintln!("         {}   {:02x?}", "HEX".dim(), data);
                    }
                }
                OutputFormat::Json => {
                    let decoded = DecodedPacket::Command {
                        timestamp: timestamp.unwrap_or(0.0),
                        cmd: cmd::GET_MULTI_MAGNETISM,
                        cmd_name: "GET_MULTI_MAGNETISM".to_string(),
                        direction: "RSP".to_string(),
                        data: display,
                    };
                    eprintln!("{}", serde_json::to_string(&decoded).unwrap());
                }
            }
            return;
        }

        // No magnetism context - fall through to normal parsing
        if !self.should_show_command(cmd) {
            return;
        }

        let parsed = try_parse_response(data);

        match self.config.format {
            OutputFormat::Text => {
                let ts_prefix = format_timestamp(timestamp);
                let ep_prefix = format_endpoint(endpoint);

                match &parsed {
                    ParsedResponse::Empty => {
                        eprintln!(
                            "{}{} {} {}",
                            ts_prefix,
                            ep_prefix,
                            "RSP".green().bold(),
                            "(empty)".dim()
                        );
                    }
                    ParsedResponse::Unknown { cmd, data: raw } => {
                        let cmd_name = cmd::name(*cmd);
                        eprintln!(
                            "{}{} {} 0x{:02x} {} {} {}",
                            ts_prefix,
                            ep_prefix,
                            "RSP".green().bold(),
                            cmd,
                            cmd_name.yellow(),
                            "UNKNOWN".red().bold(),
                            format_sparse_hex(raw)
                        );
                    }
                    _ => {
                        if self.config.show_debug {
                            eprintln!(
                                "{}{} {} {:#?}",
                                ts_prefix,
                                ep_prefix,
                                "RSP".green().bold(),
                                parsed
                            );
                        } else {
                            eprintln!(
                                "{}{} {} {:?}",
                                ts_prefix,
                                ep_prefix,
                                "RSP".green().bold(),
                                parsed
                            );
                        }
                    }
                }

                if self.config.show_hex {
                    eprintln!("         {}   {:02x?}", "HEX".dim(), data);
                }
            }
            OutputFormat::Json => {
                let cmd_name = cmd::name(cmd);
                let decoded = DecodedPacket::Command {
                    timestamp: timestamp.unwrap_or(0.0),
                    cmd,
                    cmd_name: cmd_name.to_string(),
                    direction: "RSP".to_string(),
                    data: format!("{:?}", parsed),
                };
                eprintln!("{}", serde_json::to_string(&decoded).unwrap());
            }
        }
    }

    /// Called when an event arrives (from Transport or Pcap)
    ///
    /// # Arguments
    /// * `event` - The parsed vendor event
    /// * `timestamp` - Optional timestamp (for pcap mode)
    /// * `raw_data` - Optional raw data for hex dump
    pub fn on_event(&self, event: &VendorEvent, timestamp: Option<f64>, raw_data: Option<&[u8]>) {
        if !self.should_show_events() {
            return;
        }

        match self.config.format {
            OutputFormat::Text => {
                let ts_prefix = format_timestamp(timestamp);

                if self.config.show_debug {
                    eprintln!("{} {} {:#?}", ts_prefix, "EVT".yellow().bold(), event);
                } else {
                    eprintln!("{} {} {:?}", ts_prefix, "EVT".yellow().bold(), event);
                }

                if self.config.show_hex {
                    if let Some(data) = raw_data {
                        eprintln!("         {}   {:02x?}", "HEX".dim(), data);
                    }
                }
            }
            OutputFormat::Json => {
                let decoded = DecodedPacket::Event {
                    timestamp: timestamp.unwrap_or(0.0),
                    event: format!("{:?}", event),
                };
                eprintln!("{}", serde_json::to_string(&decoded).unwrap());
            }
        }
    }

    /// Print a standard HID input report (keyboard, consumer, NKRO)
    ///
    /// Only prints if show_all_hid is enabled.
    pub fn on_hid_input(&self, timestamp: f64, report_id: u8, data: &[u8], endpoint: u8) {
        if !self.config.show_all_hid {
            return;
        }

        let report_type = match report_id {
            0x00 => "KEYBOARD",
            0x01 => "NKRO",
            0x02 => "MOUSE",
            0x03 => "CONSUMER",
            _ => "HID",
        };

        match self.config.format {
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
                eprintln!(
                    "{:.6} {}  EP{:02x} {} {}",
                    format!("{:.6}", timestamp).dim(),
                    "HID".blue(),
                    endpoint,
                    report_type.cyan(),
                    info
                );
                if self.config.show_hex {
                    eprintln!("         {}   {:02x?}", "HEX".dim(), data);
                }
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Event {
                    timestamp,
                    event: format!("{}:{:02x?}", report_type, data),
                };
                eprintln!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }

    /// Print an unknown/unparseable packet - IMPORTANT for discovery
    pub fn on_unknown(&self, timestamp: f64, data: &[u8]) {
        if matches!(self.config.filter, PacketFilter::Cmd(_)) {
            return;
        }

        match self.config.format {
            OutputFormat::Text => {
                eprintln!(
                    "{:.6} {} {:02x?}",
                    format!("{:.6}", timestamp).dim(),
                    "UNKNOWN".red().bold(),
                    data
                );
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Unknown {
                    timestamp,
                    data: format!("{:02x?}", data),
                };
                eprintln!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }

    /// Print a USB control transfer (non-HID)
    ///
    /// Note: Some USB captures show HID responses as generic control transfers
    /// (e.g., GET_STATUS) rather than properly tagged HID GET_REPORT responses.
    /// We detect these by checking if the data looks like a HID response.
    pub fn on_usb_control(
        &self,
        timestamp: f64,
        request_name: &str,
        descriptor_info: Option<(&str, u8)>, // (type_name, index)
        data: &[u8],
        is_response: bool,
        endpoint: u8,
    ) {
        // Check if this looks like a HID response (first byte is a known command echo)
        if is_response && !data.is_empty() {
            let first_byte = data[0];
            // Check if first byte looks like a command response
            let looks_like_hid = first_byte & 0x80 != 0 // GET command responses
                || matches!(first_byte, 0x00..=0x1F | 0x65); // SET command ACKs

            if looks_like_hid {
                // Route to on_response instead
                self.on_response(data, Some(timestamp), Some(endpoint));
                return;
            }
        }

        if matches!(
            self.config.filter,
            PacketFilter::Cmd(_) | PacketFilter::Events
        ) {
            return;
        }

        let direction = if is_response {
            "<<<".green()
        } else {
            ">>>".cyan()
        };

        match self.config.format {
            OutputFormat::Text => {
                if let Some((desc_type, index)) = descriptor_info {
                    // For descriptors, try to decode the data
                    let decoded = if desc_type == "STRING" && data.len() >= 2 {
                        decode_usb_string(data)
                    } else {
                        None
                    };

                    if let Some(s) = decoded {
                        eprintln!(
                            "{:.6} {}  EP{:02x} {} {} {}[{}] \"{}\"",
                            format!("{:.6}", timestamp).dim(),
                            "USB".magenta(),
                            endpoint,
                            direction,
                            request_name.yellow(),
                            desc_type,
                            index,
                            s.white()
                        );
                    } else {
                        eprintln!(
                            "{:.6} {}  EP{:02x} {} {} {}[{}] {:02x?}",
                            format!("{:.6}", timestamp).dim(),
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
                    // Non-descriptor control transfer
                    eprintln!(
                        "{:.6} {}  EP{:02x} {} {} {:02x?}",
                        format!("{:.6}", timestamp).dim(),
                        "USB".magenta(),
                        endpoint,
                        direction,
                        request_name.yellow(),
                        data
                    );
                }
                if self.config.show_hex && !data.is_empty() {
                    eprintln!("         {}   {:02x?}", "HEX".dim(), data);
                }
            }
            OutputFormat::Json => {
                let packet = DecodedPacket::Unknown {
                    timestamp,
                    data: format!("{} {:02x?}", request_name, data),
                };
                eprintln!("{}", serde_json::to_string(&packet).unwrap());
            }
        }
    }
}

/// Format optional timestamp prefix for output
fn format_timestamp(timestamp: Option<f64>) -> String {
    match timestamp {
        Some(ts) => format!("{:.6} ", format!("{:.6}", ts).dim()),
        None => ">>> ".cyan().to_string(),
    }
}

/// Format optional endpoint prefix for output
fn format_endpoint(endpoint: Option<u8>) -> String {
    match endpoint {
        Some(ep) => format!("EP{:02x} ", ep),
        None => String::new(),
    }
}

/// Decode USB string descriptor (UTF-16LE with length/type header)
fn decode_usb_string(data: &[u8]) -> Option<String> {
    if data.len() < 2 {
        return None;
    }

    let b_length = data[0] as usize;
    let b_descriptor_type = data[1];

    if b_descriptor_type != 0x03 || b_length < 2 || b_length > data.len() {
        return decode_utf16le(data);
    }

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

#[async_trait]
impl Transport for Printer {
    async fn send_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        self.on_command(cmd, data, None, None);
        self.inner
            .as_ref()
            .ok_or(TransportError::Disconnected)?
            .send_command(cmd, data, checksum)
            .await
    }

    async fn send_command_with_delay(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
    ) -> Result<(), TransportError> {
        self.on_command(cmd, data, None, None);
        self.inner
            .as_ref()
            .ok_or(TransportError::Disconnected)?
            .send_command_with_delay(cmd, data, checksum, delay_ms)
            .await
    }

    async fn query_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        self.on_command(cmd, data, None, None);
        let result = self
            .inner
            .as_ref()
            .ok_or(TransportError::Disconnected)?
            .query_command(cmd, data, checksum)
            .await?;
        self.on_response(&result, None, None);
        Ok(result)
    }

    async fn query_raw(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        self.on_command(cmd, data, None, None);
        let result = self
            .inner
            .as_ref()
            .ok_or(TransportError::Disconnected)?
            .query_raw(cmd, data, checksum)
            .await?;
        self.on_response(&result, None, None);
        Ok(result)
    }

    async fn read_feature_report(&self) -> Result<Vec<u8>, TransportError> {
        self.inner
            .as_ref()
            .ok_or(TransportError::Disconnected)?
            .read_feature_report()
            .await
    }

    async fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        let event = self
            .inner
            .as_ref()
            .ok_or(TransportError::Disconnected)?
            .read_event(timeout_ms)
            .await?;
        if let Some(ref e) = event {
            self.on_event(e, None, None);
        }
        Ok(event)
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        // In standalone mode, we don't have device info - panic is acceptable
        // as this should only be called in live mode
        self.inner
            .as_ref()
            .expect("device_info() called on standalone Printer")
            .device_info()
    }

    async fn is_connected(&self) -> bool {
        match &self.inner {
            Some(inner) => inner.is_connected().await,
            None => false,
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        match &self.inner {
            Some(inner) => inner.close().await,
            None => Ok(()),
        }
    }

    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        self.inner
            .as_ref()
            .ok_or(TransportError::Disconnected)?
            .get_battery_status()
            .await
    }

    fn subscribe_events(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        self.inner.as_ref()?.subscribe_events()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_filter_parse() {
        assert_eq!(PacketFilter::from_str("all").unwrap(), PacketFilter::All);
        assert_eq!(
            PacketFilter::from_str("events").unwrap(),
            PacketFilter::Events
        );
        assert_eq!(
            PacketFilter::from_str("commands").unwrap(),
            PacketFilter::Commands
        );
        assert_eq!(
            PacketFilter::from_str("cmd=0xf7").unwrap(),
            PacketFilter::Cmd(0xf7)
        );
        assert_eq!(
            PacketFilter::from_str("0x8f").unwrap(),
            PacketFilter::Cmd(0x8f)
        );
    }
}
