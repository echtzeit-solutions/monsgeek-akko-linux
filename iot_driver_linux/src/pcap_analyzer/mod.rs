//! PCAPNG analyzer for MonsGeek USB HID protocol
//!
//! This module reads pcapng captures of USB HID traffic and decodes
//! MonsGeek vendor protocol packets using the existing transport parsers.
//!
//! # Example
//!
//! ```ignore
//! use iot_driver::pcap_analyzer::{PcapAnalyzer, OutputFormat, PacketFilter};
//!
//! let analyzer = PcapAnalyzer::new(OutputFormat::Text, PacketFilter::All);
//! analyzer.analyze_file("capture.pcapng")?;
//! ```

mod printer;
mod usb_urb;

pub use printer::{OutputFormat, PacketFilter, Printer};
pub use usb_urb::{parse_usb_packet, Direction, TransferType, UsbPacket};

use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use pcap_parser::pcapng::Block;
use pcap_parser::{create_reader, PcapBlockOwned, PcapError};

use monsgeek_transport::event_parser::{parse_usb_event, report_id};

/// Packet statistics for debugging
#[derive(Default)]
struct PacketStats {
    parse_failed: u64,
    no_hid_data: u64,
    control_no_data: u64,
    control_not_hid: u64,
    interrupt_no_data: u64,
    interrupt_out: u64,
    control_transfers: u64,
    interrupt_transfers: u64,
    bulk_transfers: u64,
    keyboard_reports: u64,
    vendor_events: u64,
    vendor_commands: u64,
    other: u64,
}

impl PacketStats {
    fn print_summary(&self) {
        eprintln!("\nPacket statistics:");
        eprintln!("  Parse failed:       {}", self.parse_failed);
        eprintln!("  No HID data:        {}", self.no_hid_data);
        eprintln!("    Control no data:  {}", self.control_no_data);
        eprintln!("    Control not HID:  {}", self.control_not_hid);
        eprintln!("    Interrupt no data:{}", self.interrupt_no_data);
        eprintln!("    Interrupt OUT:    {}", self.interrupt_out);
        eprintln!("  Control transfers:  {}", self.control_transfers);
        eprintln!("  Interrupt transfers:{}", self.interrupt_transfers);
        eprintln!("  Bulk transfers:     {}", self.bulk_transfers);
        eprintln!("  Keyboard reports:   {}", self.keyboard_reports);
        eprintln!("  Vendor events:      {}", self.vendor_events);
        eprintln!("  Vendor commands:    {}", self.vendor_commands);
        eprintln!("  Other:              {}", self.other);
    }
}

/// PCAP analyzer for MonsGeek USB HID traffic
pub struct PcapAnalyzer {
    printer: Printer,
    verbose: bool,
}

impl PcapAnalyzer {
    /// Create a new analyzer with specified output format and filter
    pub fn new(format: OutputFormat, filter: PacketFilter) -> Self {
        Self {
            printer: Printer::new(format, filter),
            verbose: false,
        }
    }

    /// Enable verbose mode to show skipped packet statistics
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Enable debug field output for events
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.printer = self.printer.with_debug(debug);
        self
    }

    /// Enable raw hex dump output
    pub fn with_hex(mut self, hex: bool) -> Self {
        self.printer = self.printer.with_hex(hex);
        self
    }

    /// Analyze a pcapng file and print decoded packets
    pub fn analyze_file(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let mut reader = create_reader(65536, file)?;
        let mut base_timestamp: Option<(u32, u32)> = None;
        let mut packet_count = 0u64;
        let mut decoded_count = 0u64;
        let mut last_incomplete_index = 0u64;

        // Statistics for verbose mode
        let mut stats = PacketStats::default();

        loop {
            // Extract data we need from the block before calling consume/refill
            // to avoid lifetime issues with borrowed data
            let result = reader.next();

            match result {
                Ok((offset, block)) => {
                    // Extract packet info before calling consume
                    let packet_info: Option<(u32, u32, Vec<u8>)> = match &block {
                        // pcapng format: EnhancedPacket blocks
                        PcapBlockOwned::NG(Block::EnhancedPacket(epb)) => {
                            Some((epb.ts_high, epb.ts_low, epb.data.to_vec()))
                        }
                        // Legacy pcap format
                        PcapBlockOwned::Legacy(lp) => {
                            Some((lp.ts_sec, lp.ts_usec, lp.data.to_vec()))
                        }
                        // Skip other block types
                        _ => None,
                    };

                    // Now we can consume since we've copied what we need
                    reader.consume(offset);

                    // Process the extracted data
                    if let Some((ts_high, ts_low, data)) = packet_info {
                        let ts = if let Some((base_high, base_low)) = base_timestamp {
                            // For pcapng: ts_high/ts_low are a 64-bit timestamp
                            // For legacy pcap: ts_sec/ts_usec
                            let base_ts = ((base_high as u64) << 32) | (base_low as u64);
                            let curr_ts = ((ts_high as u64) << 32) | (ts_low as u64);
                            (curr_ts.saturating_sub(base_ts)) as f64 / 1_000_000.0
                        } else {
                            base_timestamp = Some((ts_high, ts_low));
                            0.0
                        };

                        packet_count += 1;
                        if self.process_packet_with_stats(ts, &data, &mut stats) {
                            decoded_count += 1;
                        }
                    }
                }
                Err(PcapError::Eof) => break,
                Err(PcapError::Incomplete(_)) => {
                    // Need more data, try to refill buffer
                    // Track last incomplete to avoid infinite loops on truncated files
                    if last_incomplete_index == packet_count {
                        eprintln!(
                            "Warning: Could not read complete data block (file may be truncated)"
                        );
                        break;
                    }
                    last_incomplete_index = packet_count;
                    // Map the error to avoid lifetime issues with PcapError<&[u8]>
                    reader
                        .refill()
                        .map_err(|e| format!("Refill error: {:?}", e))?;
                    continue;
                }
                Err(e) => {
                    return Err(format!("PCAP parse error: {:?}", e).into());
                }
            }
        }

        eprintln!(
            "\n--- Analyzed {} packets, {} decoded ---",
            packet_count, decoded_count
        );

        if self.verbose {
            stats.print_summary();
        }

        Ok(())
    }

    /// Process a single packet with statistics tracking
    fn process_packet_with_stats(
        &self,
        timestamp: f64,
        raw_data: &[u8],
        stats: &mut PacketStats,
    ) -> bool {
        // Parse USB URB packet
        let packet = match parse_usb_packet(raw_data) {
            Some(p) => p,
            None => {
                stats.parse_failed += 1;
                return false;
            }
        };

        // Track transfer type
        match &packet {
            UsbPacket::Control { .. } => stats.control_transfers += 1,
            UsbPacket::Interrupt { .. } => stats.interrupt_transfers += 1,
            UsbPacket::Bulk { .. } => stats.bulk_transfers += 1,
            UsbPacket::Other { .. } => stats.other += 1,
        }

        // Handle control transfers specially - they can be HID or USB standard
        if let UsbPacket::Control { urb, setup, data } = &packet {
            if !data.is_empty() {
                // Check if it's a HID report or USB standard request
                if setup.is_set_report() || setup.is_get_report() {
                    // HID report - extract and process
                    if let Some(hid_data) = usb_urb::extract_hid_data(&packet) {
                        if !hid_data.is_empty() {
                            self.decode_and_print_with_stats(timestamp, hid_data, &packet, stats);
                            return true;
                        }
                    }
                } else {
                    // Non-HID control transfer (GET_DESCRIPTOR, etc.)
                    let is_response = urb.direction == Direction::In;
                    let descriptor_info = if setup.is_get_descriptor() {
                        Some((setup.descriptor_type_name(), setup.descriptor_index()))
                    } else {
                        None
                    };
                    self.printer.print_usb_control(
                        timestamp,
                        setup.request_name(),
                        descriptor_info,
                        data,
                        is_response,
                        urb.endpoint,
                    );
                    return true;
                }
            }
            stats.no_hid_data += 1;
            stats.control_no_data += 1;
            return false;
        }

        // Extract HID data from non-control packets
        let data = match usb_urb::extract_hid_data(&packet) {
            Some(d) if !d.is_empty() => d,
            _ => {
                stats.no_hid_data += 1;
                // Track why packets are skipped
                if let UsbPacket::Interrupt { data, urb } = &packet {
                    if data.is_empty() {
                        stats.interrupt_no_data += 1;
                    } else if urb.direction == Direction::Out {
                        stats.interrupt_out += 1;
                    }
                }
                return false;
            }
        };

        self.decode_and_print_with_stats(timestamp, data, &packet, stats);
        true
    }

    /// Decode HID data and print using the appropriate format
    fn decode_and_print_with_stats(
        &self,
        timestamp: f64,
        data: &[u8],
        packet: &UsbPacket,
        stats: &mut PacketStats,
    ) {
        if data.is_empty() {
            return;
        }

        let first_byte = data[0];
        let urb = packet.urb();

        // Determine packet type based on transfer type and data
        match packet {
            UsbPacket::Control { setup, .. } => {
                // HID Feature/Output report via control endpoint
                stats.vendor_commands += 1;
                let is_response = setup.is_get_report() && urb.direction == Direction::In;
                self.printer
                    .print_command(timestamp, first_byte, data, is_response, urb.endpoint);
            }
            UsbPacket::Interrupt { .. } => {
                // Skip standard keyboard HID reports (they'd flood the output)
                if first_byte == 0x00 && data.len() <= 8 {
                    stats.keyboard_reports += 1;
                    return;
                }

                // Vendor events have report ID 0x05
                if first_byte == report_id::USB_VENDOR_EVENT {
                    stats.vendor_events += 1;
                    let event = parse_usb_event(data);
                    self.printer.print_event(timestamp, &event, Some(data));
                } else {
                    // Other interrupt data - commands/responses or unknown
                    stats.vendor_commands += 1;
                    let is_response = first_byte & 0x80 != 0;
                    self.printer.print_command(
                        timestamp,
                        first_byte,
                        data,
                        is_response,
                        urb.endpoint,
                    );
                }
            }
            UsbPacket::Bulk { .. } => {
                // Bulk transfers - determine by direction
                stats.vendor_commands += 1;
                let is_response = urb.direction == Direction::In;
                self.printer
                    .print_command(timestamp, first_byte, data, is_response, urb.endpoint);
            }
            UsbPacket::Other { .. } => {
                // Unknown transfer type - dump it
                stats.other += 1;
                self.printer.print_unknown(timestamp, data);
            }
        }
    }
}

/// CLI entry point for pcap analysis
pub fn run_pcap_analysis(
    path: &Path,
    format: OutputFormat,
    filter: Option<&str>,
    verbose: bool,
    debug: bool,
    hex: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let filter = match filter {
        Some(f) => PacketFilter::from_str(f)?,
        None => PacketFilter::All,
    };

    let analyzer = PcapAnalyzer::new(format, filter)
        .with_verbose(verbose)
        .with_debug(debug)
        .with_hex(hex);
    analyzer.analyze_file(path)
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
