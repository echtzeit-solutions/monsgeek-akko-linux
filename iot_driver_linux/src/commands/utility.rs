//! Utility command handlers.

use super::{format_command_response, open_preferred_transport, CmdCtx, CommandResult};
use monsgeek_transport::{format_device_list, ChecksumType, HidDiscovery, Transport};

/// List supported devices with probe results (replaces raw HID dump)
pub fn list() -> CommandResult {
    let discovery = HidDiscovery::new();
    let resolve_name = |device_id: Option<u32>, vid: u16, pid: u16| -> Option<String> {
        iot_driver::devices::get_device_info_with_id(device_id.map(|id| id as i32), vid, pid)
            .map(|info| info.display_name)
    };

    let labeled = discovery.list_labeled_devices(resolve_name)?;
    if labeled.is_empty() {
        println!("No supported devices found.");
        return Ok(());
    }

    let labels: Vec<_> = labeled.iter().map(|(_, l)| l.clone()).collect();
    print!("{}", format_device_list(&labels));

    // Show transport details for each device
    for (probed, label) in &labeled {
        if !probed.responsive {
            println!("  #{} — not responding (may be asleep)", label.index);
        }
    }

    Ok(())
}

/// Send a raw command and print response
pub fn raw(cmd_str: &str, ctx: &CmdCtx) -> CommandResult {
    let cmd = u8::from_str_radix(cmd_str, 16)?;

    let transport = open_preferred_transport(ctx)?;
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
pub async fn tui(device_selector: Option<String>) -> CommandResult {
    iot_driver::tui::run(device_selector).await?;
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
