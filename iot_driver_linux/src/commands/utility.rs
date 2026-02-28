//! Utility command handlers.

use super::{format_command_response, open_preferred_transport, CommandResult};
use hidapi::HidApi;
use monsgeek_transport::{ChecksumType, PrinterConfig, Transport};

/// List all HID devices
pub fn list(hidapi: &HidApi) -> CommandResult {
    println!("All HID devices:");
    for device_info in hidapi.device_list() {
        println!(
            "  VID={:04x} PID={:04x} usage={:04x} page={:04x} if={} path=...",
            device_info.vendor_id(),
            device_info.product_id(),
            device_info.usage(),
            device_info.usage_page(),
            device_info.interface_number(),
        );
    }
    Ok(())
}

/// Send a raw command and print response
pub fn raw(cmd_str: &str, printer_config: Option<PrinterConfig>) -> CommandResult {
    let cmd = u8::from_str_radix(cmd_str, 16)?;

    let transport = open_preferred_transport(printer_config)?;
    let info = transport.device_info();
    println!(
        "Device: VID={:04X} PID={:04X} type={:?}",
        info.vid, info.pid, info.transport_type
    );
    println!(
        "Sending command 0x{:02x} ({})...",
        cmd,
        iot_driver::protocol::cmd::name(cmd)
    );

    let resp = transport.query_command(cmd, &[], ChecksumType::Bit7)?;
    format_command_response(cmd, &resp);
    Ok(())
}

/// Run the TUI
pub async fn tui() -> CommandResult {
    iot_driver::tui::run().await?;
    Ok(())
}

/// Launch the joystick mapper
pub fn joystick(config: Option<std::path::PathBuf>, headless: bool) -> CommandResult {
    let mut cmd = std::process::Command::new("monsgeek-joystick");
    if let Some(config_path) = config {
        cmd.arg("--config").arg(config_path);
    }
    if headless {
        cmd.arg("--headless");
    }
    let status = cmd.status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("Joystick mapper exited with status: {}", s);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("monsgeek-joystick binary not found. Run: cargo build -p monsgeek-joystick");
        }
        Err(e) => {
            eprintln!("Failed to run joystick mapper: {}", e);
        }
    }
    Ok(())
}
