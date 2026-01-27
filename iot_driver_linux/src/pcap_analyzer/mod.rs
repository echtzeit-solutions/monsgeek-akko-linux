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

/// PCAP analyzer for MonsGeek USB HID traffic
pub struct PcapAnalyzer {
    printer: Printer,
}

impl PcapAnalyzer {
    /// Create a new analyzer with specified output format and filter
    pub fn new(format: OutputFormat, filter: PacketFilter) -> Self {
        Self {
            printer: Printer::new(format, filter),
        }
    }

    /// Analyze a pcapng file and print decoded packets
    pub fn analyze_file(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let mut reader = create_reader(65536, file)?;
        let mut base_timestamp: Option<(u32, u32)> = None;
        let mut packet_count = 0u64;
        let mut decoded_count = 0u64;
        let mut last_incomplete_index = 0u64;

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
                        if self.process_packet(ts, &data) {
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
        Ok(())
    }

    /// Process a single packet and return true if it was a HID packet
    fn process_packet(&self, timestamp: f64, raw_data: &[u8]) -> bool {
        // Parse USB URB packet
        let packet = match parse_usb_packet(raw_data) {
            Some(p) => p,
            None => return false,
        };

        // Extract HID data from the packet
        let data = match usb_urb::extract_hid_data(&packet) {
            Some(d) if !d.is_empty() => d,
            _ => return false,
        };

        self.decode_and_print(timestamp, data, &packet);
        true
    }

    /// Decode HID data and print using the appropriate format
    fn decode_and_print(&self, timestamp: f64, data: &[u8], packet: &UsbPacket) {
        if data.is_empty() {
            return;
        }

        let first_byte = data[0];
        let urb = packet.urb();

        // Determine packet type based on transfer type and data
        match packet {
            UsbPacket::Control { setup, .. } => {
                // Feature/Output report via control endpoint - these are vendor commands/responses
                let is_response = setup.is_get_report() && urb.direction == Direction::In;
                self.printer
                    .print_command(timestamp, first_byte, data, is_response);
            }
            UsbPacket::Interrupt { .. } => {
                // Interrupt endpoint - process vendor events (report ID 0x05)
                if first_byte == report_id::USB_VENDOR_EVENT {
                    let event = parse_usb_event(data);
                    self.printer.print_event(timestamp, &event);
                } else if urb.direction == Direction::In && first_byte != 0x00 {
                    // Other non-keyboard interrupt data - might be vendor responses
                    let is_response = first_byte & 0x80 != 0;
                    self.printer
                        .print_command(timestamp, first_byte, data, is_response);
                }
                // Silently ignore keyboard HID reports (first byte 0x00)
            }
            UsbPacket::Bulk { .. } => {
                // Bulk transfers - determine by direction
                let is_response = urb.direction == Direction::In;
                self.printer
                    .print_command(timestamp, first_byte, data, is_response);
            }
            UsbPacket::Other { .. } => {
                // Unknown transfer type
            }
        }
    }
}

/// CLI entry point for pcap analysis
pub fn run_pcap_analysis(
    path: &Path,
    format: OutputFormat,
    filter: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let filter = match filter {
        Some(f) => PacketFilter::from_str(f)?,
        None => PacketFilter::All,
    };

    let analyzer = PcapAnalyzer::new(format, filter);
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
