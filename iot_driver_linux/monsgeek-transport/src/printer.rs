//! PrinterTransport middleware for monitoring/tracing transport operations
//!
//! This module provides a middleware that wraps any Transport implementation
//! and logs/prints all commands and responses passing through it.
//!
//! # Example
//!
//! ```ignore
//! use monsgeek_transport::{HidWiredTransport, PrinterTransport, PrinterConfig, PacketFilter};
//!
//! let transport = HidWiredTransport::open(&device)?;
//! let config = PrinterConfig::default();
//! let monitored = PrinterTransport::wrap(Arc::new(transport), config);
//! // Now all commands/responses will be printed
//! ```

use crate::protocol::cmd;
use crate::{
    try_parse_command, try_parse_response, ChecksumType, ParsedCommand, ParsedResponse,
    TimestampedEvent, Transport, TransportDeviceInfo, TransportError, VendorEvent,
};
use async_trait::async_trait;
use colored::Colorize;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::broadcast;

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

/// Configuration for the PrinterTransport
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
}

impl Default for PrinterConfig {
    fn default() -> Self {
        Self {
            show_hex: false,
            show_all_hid: false,
            filter: PacketFilter::All,
            format: OutputFormat::Text,
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
}

/// Transport middleware that prints all commands and responses
///
/// Wraps any Transport implementation and logs all traffic passing through.
pub struct PrinterTransport {
    inner: Arc<dyn Transport>,
    config: PrinterConfig,
}

impl PrinterTransport {
    /// Wrap a transport with printing middleware
    pub fn wrap(transport: Arc<dyn Transport>, config: PrinterConfig) -> Arc<dyn Transport> {
        Arc::new(Self {
            inner: transport,
            config,
        })
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

    /// Print a command being sent
    fn print_command(&self, cmd: u8, data: &[u8]) {
        if !self.should_show_command(cmd) {
            return;
        }

        // Build full packet for parsing (cmd + data)
        let mut packet = vec![cmd];
        packet.extend_from_slice(data);

        let parsed = try_parse_command(&packet);
        match &parsed {
            ParsedCommand::Unknown { cmd, data: raw } => {
                let cmd_name = cmd::name(*cmd);
                eprintln!(
                    "{} {}  0x{:02x} {} {:02x?}",
                    ">>>".cyan(),
                    "CMD".cyan().bold(),
                    cmd,
                    cmd_name.yellow(),
                    raw
                );
            }
            _ => {
                eprintln!("{} {}  {:?}", ">>>".cyan(), "CMD".cyan().bold(), parsed);
            }
        }

        if self.config.show_hex {
            eprintln!("    {}  {:02x?}", "HEX".dimmed(), packet);
        }
    }

    /// Print a response received
    fn print_response(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }

        let cmd = data[0];
        if !self.should_show_command(cmd) {
            return;
        }

        let parsed = try_parse_response(data);
        match &parsed {
            ParsedResponse::Unknown { cmd, data: raw } => {
                let cmd_name = cmd::name(*cmd);
                eprintln!(
                    "{} {}  0x{:02x} {} {} {:02x?}",
                    "<<<".green(),
                    "RSP".green().bold(),
                    cmd,
                    cmd_name.yellow(),
                    "UNKNOWN".red().bold(),
                    raw
                );
            }
            _ => {
                eprintln!("{} {}  {:?}", "<<<".green(), "RSP".green().bold(), parsed);
            }
        }

        if self.config.show_hex {
            eprintln!("    {}  {:02x?}", "HEX".dimmed(), data);
        }
    }

    /// Print an event
    fn print_event(&self, event: &VendorEvent) {
        if !self.should_show_events() {
            return;
        }

        eprintln!("{} {}  {:?}", "<<<".yellow(), "EVT".yellow().bold(), event);
    }
}

#[async_trait]
impl Transport for PrinterTransport {
    async fn send_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<(), TransportError> {
        self.print_command(cmd, data);
        self.inner.send_command(cmd, data, checksum).await
    }

    async fn send_command_with_delay(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
        delay_ms: u64,
    ) -> Result<(), TransportError> {
        self.print_command(cmd, data);
        self.inner
            .send_command_with_delay(cmd, data, checksum, delay_ms)
            .await
    }

    async fn query_command(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        self.print_command(cmd, data);
        let result = self.inner.query_command(cmd, data, checksum).await?;
        self.print_response(&result);
        Ok(result)
    }

    async fn query_raw(
        &self,
        cmd: u8,
        data: &[u8],
        checksum: ChecksumType,
    ) -> Result<Vec<u8>, TransportError> {
        self.print_command(cmd, data);
        let result = self.inner.query_raw(cmd, data, checksum).await?;
        self.print_response(&result);
        Ok(result)
    }

    async fn read_event(&self, timeout_ms: u32) -> Result<Option<VendorEvent>, TransportError> {
        let event = self.inner.read_event(timeout_ms).await?;
        if let Some(ref e) = event {
            self.print_event(e);
        }
        Ok(event)
    }

    fn device_info(&self) -> &TransportDeviceInfo {
        self.inner.device_info()
    }

    async fn is_connected(&self) -> bool {
        self.inner.is_connected().await
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.inner.close().await
    }

    async fn get_battery_status(&self) -> Result<(u8, bool, bool), TransportError> {
        self.inner.get_battery_status().await
    }

    fn subscribe_events(&self) -> Option<broadcast::Receiver<TimestampedEvent>> {
        // TODO: Could wrap the receiver to also print events
        self.inner.subscribe_events()
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
