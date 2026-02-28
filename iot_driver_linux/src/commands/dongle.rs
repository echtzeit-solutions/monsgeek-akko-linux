//! Dongle-specific command handlers.

use super::{open_preferred_transport, CommandResult};
use monsgeek_transport::{
    ChecksumType, FlowControlTransport, PrinterConfig, Transport, TransportType,
};
use std::sync::Arc;

/// Ensure the transport is connected via dongle, return it.
fn open_dongle_transport(
    printer_config: Option<PrinterConfig>,
) -> Result<Arc<FlowControlTransport>, Box<dyn std::error::Error>> {
    let transport = open_preferred_transport(printer_config)?;
    if transport.device_info().transport_type != TransportType::HidDongle {
        return Err(
            "Not connected via dongle. Use `iot_driver dongle` only with a 2.4GHz dongle.".into(),
        );
    }
    Ok(transport)
}

/// `iot_driver dongle info` — combined F0 + F7 + FB + FD view
pub fn info(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_dongle_transport(printer_config)?;

    println!("Dongle Info");
    println!("===========");

    // F0: GET_DONGLE_INFO
    match transport.query_dongle_info()? {
        Some(info) => {
            println!("  Dongle FW:      v{}", info.firmware_version);
            println!("  Protocol:       v{}", info.protocol_version);
            println!("  Max Packet:     {}", info.max_packet_size);
        }
        None => {
            println!("  Dongle FW:      (unavailable)");
        }
    }

    // FB: GET_RF_INFO
    match transport.query_rf_info()? {
        Some(rf) => {
            println!(
                "  RF Address:     {:02X}:{:02X}:{:02X}:{:02X}",
                rf.rf_address[0], rf.rf_address[1], rf.rf_address[2], rf.rf_address[3]
            );
            println!(
                "  RF FW Version:  v{}.{}",
                rf.firmware_version_major, rf.firmware_version_minor
            );
        }
        None => {
            println!("  RF Address:     (unavailable)");
        }
    }

    // FD: GET_DONGLE_ID (raw query since response doesn't echo)
    match transport.query_raw(
        monsgeek_transport::protocol::cmd::GET_DONGLE_ID,
        &[],
        ChecksumType::Bit7,
    ) {
        Ok(resp) => {
            println!(
                "  Dongle ID:      {:02X} {:02X} {:02X} {:02X}",
                resp[0], resp[1], resp[2], resp[3]
            );
        }
        Err(_) => {
            println!("  Dongle ID:      (unavailable)");
        }
    }

    // F7: GET_DONGLE_STATUS
    if let Some(status) = transport.query_dongle_status()? {
        println!();
        println!("Keyboard Status (via dongle)");
        println!("============================");
        println!("  Battery:        {}%", status.battery_level);
        println!(
            "  Charging:       {}",
            if status.charging { "Yes" } else { "No" }
        );
        println!(
            "  RF Ready:       {}",
            if status.rf_ready { "Yes" } else { "No" }
        );
        println!(
            "  Has Response:   {}",
            if status.has_response { "Yes" } else { "No" }
        );
    }

    Ok(())
}

/// `iot_driver dongle status` — detailed F7 output
pub fn status(printer_config: Option<PrinterConfig>) -> CommandResult {
    let transport = open_dongle_transport(printer_config)?;

    match transport.query_dongle_status()? {
        Some(status) => {
            println!("Dongle Status (F7)");
            println!("==================");
            println!(
                "  Has Response:   {}",
                if status.has_response { "Yes" } else { "No" }
            );
            println!("  Battery Level:  {}%", status.battery_level);
            println!(
                "  Charging:       {}",
                if status.charging { "Yes" } else { "No" }
            );
            println!(
                "  RF Ready:       {}",
                if status.rf_ready { "Yes" } else { "No" }
            );
        }
        None => {
            println!("Not connected via dongle.");
        }
    }

    Ok(())
}
