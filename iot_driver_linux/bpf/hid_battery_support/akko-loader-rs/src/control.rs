// SPDX-License-Identifier: GPL-2.0
//! PID file and stop/status control for akko-loader

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process;
use std::thread;
use std::time::Duration;

const PID_FILE: &str = "/tmp/akko-loader.pid";
const STOP_FILE: &str = "/tmp/akko-loader.stop";

/// Write PID file for --stop/--status commands
pub fn write_pid_file() -> Result<()> {
    let mut file = fs::File::create(PID_FILE).context("Failed to create PID file")?;
    writeln!(file, "{}", process::id())?;

    // Make world-readable so --status works without sudo
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(PID_FILE, fs::Permissions::from_mode(0o644))?;
    }

    Ok(())
}

/// Read PID from file
pub fn read_pid_file() -> Option<u32> {
    fs::read_to_string(PID_FILE)
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Check if stop file exists (non-root stop mechanism)
pub fn check_stop_file() -> bool {
    Path::new(STOP_FILE).exists()
}

/// Clean up PID and stop files on exit
pub fn cleanup_files() {
    let _ = fs::remove_file(PID_FILE);
    let _ = fs::remove_file(STOP_FILE);
}

/// Check if a process is running
fn process_running(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Kill previous loader instances
pub fn kill_previous_loaders() -> Result<()> {
    let my_pid = process::id();

    let proc_dir = Path::new("/proc");
    if !proc_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(proc_dir)?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Parse PID from directory name
        let pid: u32 = match name_str.parse() {
            Ok(p) if p > 0 && p != my_pid => p,
            _ => continue,
        };

        // Read cmdline
        let cmdline_path = entry.path().join("cmdline");
        let cmdline = match fs::read_to_string(&cmdline_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check if it's an akko loader
        if cmdline.contains("akko") && cmdline.contains("loader") {
            tracing::info!("Killing previous loader (PID {})", pid);
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }

    // Wait for them to exit
    thread::sleep(Duration::from_millis(300));

    Ok(())
}

/// Stop command - create stop file to signal loader
pub fn do_stop() -> Result<()> {
    let pid = match read_pid_file() {
        Some(p) => p,
        None => {
            eprintln!("No loader running (PID file not found)");
            return Ok(());
        }
    };

    if !process_running(pid) {
        eprintln!("Loader not running (stale PID file)");
        let _ = fs::remove_file(PID_FILE);
        return Ok(());
    }

    // Create stop file - loader will see this and exit
    fs::File::create(STOP_FILE).context("Failed to create stop file")?;

    eprintln!("Signaling loader (PID {pid}) to stop...");

    // Wait for loader to exit (up to 5 seconds)
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if !process_running(pid) {
            eprintln!("Loader stopped");
            let _ = fs::remove_file(STOP_FILE);
            return Ok(());
        }
    }

    eprintln!("Loader did not stop in time, sending SIGTERM...");
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    let _ = fs::remove_file(STOP_FILE);

    Ok(())
}

/// Status command - show loader state
pub fn do_status() -> Result<()> {
    println!("Akko Loader Status:");

    let pid = match read_pid_file() {
        Some(p) => p,
        None => {
            println!("  Status: not running (no PID file)");
            return Ok(());
        }
    };

    if !process_running(pid) {
        println!("  Status: not running (stale PID file, PID was {pid})");
        return Ok(());
    }

    println!("  Status: running");
    println!("  PID: {pid}");

    // Show battery if available
    let ps_dir = Path::new("/sys/class/power_supply");
    if let Ok(entries) = fs::read_dir(ps_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.contains("3151") {
                let capacity_path = entry.path().join("capacity");
                if let Ok(cap) = fs::read_to_string(&capacity_path) {
                    println!("  Battery: {}%", cap.trim());
                }
            }
        }
    }

    Ok(())
}
