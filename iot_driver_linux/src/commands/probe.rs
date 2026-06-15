//! `probe` — self-service diagnostic report.
//!
//! Collects device identity, USB/HID descriptors, device-database match status,
//! and raw protocol responses into a GitHub-ready Markdown report so users with
//! unsupported or partially-working keyboards can paste enough information for us
//! to add support or debug. Every query is wrapped so a failure appends a
//! `timeout/error` line and the report still completes.

use super::{open_preferred_transport, resolve_device, CmdCtx, CommandResult};
use iot_driver::protocol::patch_info;
use monsgeek_transport::protocol::cmd;
use monsgeek_transport::protocol::ProtocolFamily;
use monsgeek_transport::{ChecksumType, FlowControlTransport, HidDiscovery, Transport};
use std::fmt::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Format bytes as space-separated lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Append one query's raw response (or error) as a Markdown bullet line.
fn query_line(out: &mut String, transport: &FlowControlTransport, label: &str, cmd_byte: u8) {
    match transport.query_command(cmd_byte, &[], ChecksumType::Bit7) {
        Ok(resp) => {
            let _ = writeln!(out, "- {label} (0x{cmd_byte:02X}): `{}`", hex(&resp));
        }
        Err(e) => {
            let _ = writeln!(out, "- {label} (0x{cmd_byte:02X}): timeout/error ({e})");
        }
    }
}

/// Environment / driver header.
fn section_env(out: &mut String) {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(out, "## Environment");
    let _ = writeln!(out, "- driver: iot_driver v{}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "- host: {} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "- timestamp: {epoch} (unix epoch seconds, UTC)");
    let _ = writeln!(out);
}

/// All discovered HID devices (shows unsupported/unresponsive devices too).
fn section_devices(out: &mut String) {
    let _ = writeln!(out, "## Discovered devices");
    let discovery = HidDiscovery::new();
    match discovery.probe_devices() {
        Ok(probed) if !probed.is_empty() => {
            let _ = writeln!(
                out,
                "| transport | vid:pid | responsive | device id | fw | path |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|---|");
            for p in &probed {
                let info = &p.device.info;
                let id = p
                    .device_id
                    .map(|v| format!("{v} (0x{v:04X})"))
                    .unwrap_or_else(|| "-".into());
                let ver = p
                    .version
                    .map(|v| format!("0x{v:04X}"))
                    .unwrap_or_else(|| "-".into());
                let _ = writeln!(
                    out,
                    "| {:?} | {:04X}:{:04X} | {} | {} | {} | `{}` |",
                    info.transport_type,
                    info.vid,
                    info.pid,
                    p.responsive,
                    id,
                    ver,
                    info.device_path,
                );
            }
        }
        Ok(_) => {
            let _ = writeln!(out, "_No supported (VID 0x3151) devices found._");
        }
        Err(e) => {
            let _ = writeln!(out, "_Device enumeration failed: {e}_");
        }
    }
    let _ = writeln!(out);
}

/// Decode the 0xE7 patch-info response (both response framings), like `query::info`.
fn append_patch_info(out: &mut String, transport: &FlowControlTransport) {
    match transport.query_raw(patch_info::CMD, &[], ChecksumType::Bit7) {
        Ok(resp) => {
            let _ = writeln!(out, "- patch query (0xE7) raw: `{}`", hex(&resp));
            let offsets = if resp.len() >= 6
                && resp[0] == patch_info::MAGIC_HI
                && resp[1] == patch_info::MAGIC_LO
            {
                Some((2usize, 3usize, 5usize)) // payload only
            } else if resp.len() >= 7
                && resp[1] == patch_info::MAGIC_HI
                && resp[2] == patch_info::MAGIC_LO
            {
                Some((3, 4, 6)) // echo + payload
            } else {
                None
            };
            match offsets {
                Some((ver_off, caps_off, name_off)) => {
                    let patch_ver = resp[ver_off];
                    let caps = u16::from_le_bytes([resp[caps_off], resp[caps_off + 1]]);
                    let name_end = resp.len().min(name_off + 9);
                    let name_bytes = &resp[name_off..name_end];
                    let name_len = name_bytes
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(name_bytes.len());
                    let name = String::from_utf8_lossy(&name_bytes[..name_len]);
                    let caps_list = patch_info::capability_names(caps);
                    let caps_str = if caps_list.is_empty() {
                        "none".to_string()
                    } else {
                        caps_list.join(", ")
                    };
                    let _ = writeln!(out, "- patch: {name} v{patch_ver} [{caps_str}]");
                }
                None => {
                    let _ = writeln!(out, "- patch: stock firmware (no patch magic)");
                }
            }
        }
        Err(_) => {
            let _ = writeln!(out, "- patch: stock firmware (0xE7 timeout/error)");
        }
    }
}

/// Target device deep-dive: identity, raw protocol responses, DB match, dongle heuristic.
fn section_target(out: &mut String, ctx: &CmdCtx) {
    let _ = writeln!(out, "## Target device");
    let transport = match open_preferred_transport(ctx) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(out, "_No target device opened: {e}_");
            let _ = writeln!(out);
            return;
        }
    };

    let dev = transport.device_info();
    let (vid, pid) = (dev.vid, dev.pid);
    if let Some(name) = &dev.product_name {
        let _ = writeln!(out, "- product: {name}");
    }
    let _ = writeln!(
        out,
        "- vid:pid: {vid:04X}:{pid:04X}  transport: {:?}",
        dev.transport_type
    );

    // Firmware identity (for the DB lookup below).
    let mut device_id: Option<u32> = None;
    match transport.query_command(cmd::GET_USB_VERSION, &[], ChecksumType::Bit7) {
        Ok(resp) if resp.len() >= 9 => {
            let id = u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]);
            let version = u16::from_le_bytes([resp[7], resp[8]]);
            device_id = Some(id);
            let _ = writeln!(out, "- device id: {id} (0x{id:08X})");
            let _ = writeln!(
                out,
                "- firmware: v{}.{:02} (raw 0x{version:04X})",
                version >> 8,
                version & 0xFF
            );
            let _ = writeln!(out, "- GET_USB_VERSION (0x8F): `{}`", hex(&resp));
        }
        Ok(resp) => {
            let _ = writeln!(
                out,
                "- GET_USB_VERSION (0x8F): short response `{}`",
                hex(&resp)
            );
        }
        Err(e) => {
            let _ = writeln!(out, "- GET_USB_VERSION (0x8F): timeout/error ({e})");
        }
    }

    let db_info =
        iot_driver::devices::get_device_info_with_id(device_id.map(|v| v as i32), vid, pid);
    let family = ProtocolFamily::detect(db_info.as_ref().map(|d| d.name.as_str()), pid);
    let _ = writeln!(out, "- protocol family: {family}");

    // Raw protocol capability dump (for reverse engineering unknown devices).
    let _ = writeln!(out, "\n### Raw protocol responses");
    query_line(out, &transport, "GET_FEATURE_LIST", cmd::GET_FEATURE_LIST);
    query_line(out, &transport, "GET_KBOPTION", cmd::GET_KBOPTION);
    query_line(out, &transport, "GET_PROFILE", cmd::GET_PROFILE);
    query_line(out, &transport, "GET_LEDPARAM", cmd::GET_LEDPARAM);
    query_line(out, &transport, "GET_DEBOUNCE", cmd::GET_DEBOUNCE);
    append_patch_info(out, &transport);

    // Dongle-local probe: a GET_DONGLE_INFO response means this is a receiver.
    if let Ok(Some(di)) = transport.query_dongle_info() {
        let _ = writeln!(
            out,
            "\n### Dongle detected\n- GET_DONGLE_INFO: proto v{} max_pkt {} fw {}",
            di.protocol_version, di.max_packet_size, di.firmware_version
        );
        if !monsgeek_transport::is_dongle_pid(pid) {
            let _ = writeln!(
                out,
                "- ⚠ This dongle responds on an **unrecognized PID 0x{pid:04X}**. \
                 Please report this so it can be added to the known dongle list."
            );
        }
    }

    // Device-database match — the prominent symptom report.
    let _ = writeln!(out, "\n## Device database");
    match &db_info {
        Some(info) => {
            let _ = writeln!(out, "✅ **FOUND**");
            let _ = writeln!(out, "- display name: {}", info.display_name);
            if let Some(company) = &info.company {
                let _ = writeln!(out, "- company: {company}");
            }
            let _ = writeln!(out, "- key count: {}", info.key_count);
            let _ = writeln!(out, "- magnetism: {}", info.has_magnetism);
            let _ = writeln!(out, "- side light: {}", info.has_sidelight);
            if let Some(layers) = info.layer_count {
                let _ = writeln!(out, "- layers: {layers}");
            }
            if let Some(id) = device_id {
                if let Some(def) = iot_driver::profile_registry().get_device_info_by_id(id as i32) {
                    if let Some(layout) = &def.key_layout_name {
                        let _ = writeln!(out, "- key layout: {layout}");
                    }
                    if let Some(chip) = &def.chip_family {
                        let _ = writeln!(out, "- chip family: {chip}");
                    }
                }
            }
        }
        None => {
            let _ = writeln!(out, "❌ **NOT FOUND**");
            let id_str = device_id
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into());
            let _ = writeln!(
                out,
                "> Device id {id_str} (vid {vid:04X} pid {pid:04X}) is not in the device \
                 database. This is why key count is 0 / settings reset / most features are \
                 unavailable. Please paste this report into a GitHub issue so the device can \
                 be added."
            );
        }
    }
    let _ = writeln!(out);
}

/// Dump the raw HID report descriptor of the target device (RE-critical).
///
/// Uses a fresh hidapi handle and only `get_report_descriptor` (a control
/// request); never performs an input read, which would lock the hidraw node.
fn section_report_descriptor(out: &mut String, ctx: &CmdCtx) {
    let _ = writeln!(out, "## HID report descriptor");
    let discovery = HidDiscovery::new();
    let device = match resolve_device(&discovery, ctx.device_selector()) {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(out, "_Could not resolve target device: {e}_");
            let _ = writeln!(out);
            return;
        }
    };

    let result = (|| -> Result<String, Box<dyn std::error::Error>> {
        let api = hidapi::HidApi::new()?;
        let cpath = std::ffi::CString::new(device.info.device_path.as_bytes())?;
        let hid = api.open_path(cpath.as_c_str())?;
        let mut buf = vec![0u8; 4096];
        let n = hid.get_report_descriptor(&mut buf)?;
        Ok(hex(&buf[..n]))
    })();

    match result {
        Ok(desc) if !desc.is_empty() => {
            let _ = writeln!(out, "- interface: `{}`", device.info.device_path);
            let _ = writeln!(out, "\n```\n{desc}\n```");
        }
        Ok(_) => {
            let _ = writeln!(out, "_Report descriptor was empty._");
        }
        Err(e) => {
            let _ = writeln!(out, "_Report descriptor unavailable: {e}_");
        }
    }
    let _ = writeln!(out);
}

/// Generate the diagnostic report.
pub fn run(ctx: &CmdCtx, output: Option<&Path>) -> CommandResult {
    let mut out = String::new();
    out.push_str("# MonsGeek/Akko Linux driver — diagnostic report\n\n");
    section_env(&mut out);
    section_devices(&mut out);
    section_target(&mut out, ctx);
    section_report_descriptor(&mut out, ctx);

    print!("{out}");

    if let Some(path) = output {
        std::fs::write(path, &out)?;
        eprintln!("Report written to {}", path.display());
    }

    Ok(())
}
