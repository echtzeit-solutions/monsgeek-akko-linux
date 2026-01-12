// Firmware file handling and dry-run simulation
// This module is for ANALYSIS ONLY - no actual firmware flashing

use std::fs;
use std::io::{self, Read};
use std::path::Path;
use zip::ZipArchive;

use crate::protocol::firmware_update;

/// Error types for firmware operations
#[derive(Debug)]
pub enum FirmwareError {
    IoError(io::Error),
    ZipError(zip::result::ZipError),
    InvalidFormat(String),
    FileTooSmall(usize),
    FileTooLarge(usize),
}

impl std::fmt::Display for FirmwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::ZipError(e) => write!(f, "ZIP error: {e}"),
            Self::InvalidFormat(msg) => write!(f, "Invalid format: {msg}"),
            Self::FileTooSmall(size) => write!(f, "File too small: {size} bytes"),
            Self::FileTooLarge(size) => write!(f, "File too large: {size} bytes"),
        }
    }
}

impl std::error::Error for FirmwareError {}

impl From<io::Error> for FirmwareError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}

impl From<zip::result::ZipError> for FirmwareError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::ZipError(e)
    }
}

/// Represents a parsed firmware file
#[derive(Debug, Clone)]
pub struct FirmwareFile {
    /// Raw firmware data
    pub data: Vec<u8>,
    /// File size in bytes
    pub size: usize,
    /// 32-bit checksum (sum of all bytes)
    pub checksum: u32,
    /// Number of 64-byte chunks
    pub chunk_count: usize,
    /// Original filename
    pub filename: String,
    /// Firmware type (detected from file structure)
    pub firmware_type: FirmwareType,
}

/// Type of firmware file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareType {
    /// Main USB firmware (.bin)
    Usb,
    /// RF receiver firmware
    Rf,
    /// Combined firmware package (.zip)
    Combined,
    /// Unknown/raw binary
    Unknown,
}

impl std::fmt::Display for FirmwareType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usb => write!(f, "USB"),
            Self::Rf => write!(f, "RF"),
            Self::Combined => write!(f, "Combined"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

impl FirmwareFile {
    /// Load a firmware file from disk
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, FirmwareError> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("firmware.bin")
            .to_string();

        let data = fs::read(path)?;

        if data.is_empty() {
            return Err(FirmwareError::FileTooSmall(0));
        }

        // Check if it's a ZIP file
        if data.len() >= 4 && &data[0..4] == b"PK\x03\x04" {
            return Self::load_zip(path, filename);
        }

        let size = data.len();
        let checksum = firmware_update::calculate_checksum(&data);
        let chunk_count = size.div_ceil(firmware_update::CHUNK_SIZE);

        // Detect firmware type based on size/content
        let firmware_type = Self::detect_type(&data, &filename);

        Ok(Self {
            data,
            size,
            checksum,
            chunk_count,
            filename,
            firmware_type,
        })
    }

    /// Load firmware from a ZIP archive
    fn load_zip<P: AsRef<Path>>(path: P, filename: String) -> Result<Self, FirmwareError> {
        let file = fs::File::open(path)?;
        let mut archive = ZipArchive::new(file)?;

        // Look for main firmware file
        let firmware_names = ["firmwareFile.bin", "firmware.bin", "usb_firmware.bin"];

        for name in &firmware_names {
            if let Ok(mut entry) = archive.by_name(name) {
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;

                let size = data.len();
                let checksum = firmware_update::calculate_checksum(&data);
                let chunk_count = size.div_ceil(firmware_update::CHUNK_SIZE);

                return Ok(Self {
                    data,
                    size,
                    checksum,
                    chunk_count,
                    filename,
                    firmware_type: FirmwareType::Combined,
                });
            }
        }

        // If no known firmware file, try first .bin file
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_lowercase();
            if name.ends_with(".bin") {
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;

                let size = data.len();
                let checksum = firmware_update::calculate_checksum(&data);
                let chunk_count = size.div_ceil(firmware_update::CHUNK_SIZE);

                return Ok(Self {
                    data,
                    size,
                    checksum,
                    chunk_count,
                    filename,
                    firmware_type: FirmwareType::Combined,
                });
            }
        }

        Err(FirmwareError::InvalidFormat(
            "No firmware file found in ZIP".to_string(),
        ))
    }

    /// Detect firmware type from content
    fn detect_type(data: &[u8], filename: &str) -> FirmwareType {
        let name_lower = filename.to_lowercase();

        if name_lower.contains("rf") || name_lower.contains("receiver") {
            return FirmwareType::Rf;
        }

        if name_lower.contains("usb") || name_lower.contains("main") {
            return FirmwareType::Usb;
        }

        // Detect by size - USB firmware is typically larger
        if data.len() > 100_000 {
            FirmwareType::Usb
        } else if data.len() > 10_000 {
            FirmwareType::Rf
        } else {
            FirmwareType::Unknown
        }
    }

    /// Validate the firmware file
    pub fn validate(&self) -> Result<(), FirmwareError> {
        // Minimum size check
        if self.size < 1024 {
            return Err(FirmwareError::FileTooSmall(self.size));
        }

        // Maximum size check (4MB should be more than enough)
        if self.size > 4 * 1024 * 1024 {
            return Err(FirmwareError::FileTooLarge(self.size));
        }

        // Check for empty data patterns
        if self.data.iter().all(|&b| b == 0xFF) {
            return Err(FirmwareError::InvalidFormat(
                "File contains only 0xFF bytes".to_string(),
            ));
        }

        if self.data.iter().all(|&b| b == 0x00) {
            return Err(FirmwareError::InvalidFormat(
                "File contains only 0x00 bytes".to_string(),
            ));
        }

        Ok(())
    }

    /// Get firmware data starting at USB offset
    pub fn usb_data(&self) -> Option<&[u8]> {
        if self.size > firmware_update::USB_FIRMWARE_OFFSET {
            Some(&self.data[firmware_update::USB_FIRMWARE_OFFSET..])
        } else {
            Some(&self.data)
        }
    }

    /// List contents if ZIP archive
    pub fn list_zip_contents<P: AsRef<Path>>(path: P) -> Result<Vec<String>, FirmwareError> {
        let file = fs::File::open(path)?;
        let mut archive = ZipArchive::new(file)?;

        let names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
            .collect();

        Ok(names)
    }
}

/// Represents a command in the dry-run simulation
#[derive(Debug, Clone)]
pub enum DryRunCommand {
    /// Enter bootloader mode
    EnterBootMode { mode: &'static str, bytes: Vec<u8> },
    /// Wait for device reconnection
    WaitReconnect { vid: u16, pid: u16, timeout_ms: u64 },
    /// Start firmware transfer
    StartTransfer { header: Vec<u8> },
    /// Send data chunk
    DataChunk {
        index: usize,
        offset: usize,
        size: usize,
    },
    /// Complete transfer with verification
    CompleteTransfer { header: Vec<u8>, checksum: u32 },
}

impl DryRunCommand {
    /// Format command for display
    pub fn display(&self) -> String {
        match self {
            Self::EnterBootMode { mode, bytes } => {
                format!(
                    "Enter {} boot mode: [{}] (NOT SENT)",
                    mode,
                    bytes
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
            Self::WaitReconnect {
                vid,
                pid,
                timeout_ms,
            } => {
                format!(
                    "Wait for device reconnect at {vid:04X}:{pid:04X} (timeout: {timeout_ms}ms)"
                )
            }
            Self::StartTransfer { header } => {
                format!(
                    "Start transfer: [{}]",
                    header
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
            Self::DataChunk {
                index,
                offset,
                size,
            } => {
                format!("Send chunk {index}: {size} bytes at offset 0x{offset:X}")
            }
            Self::CompleteTransfer { header, checksum } => {
                format!(
                    "Complete transfer: [{}...] checksum=0x{:08X}",
                    header
                        .iter()
                        .take(4)
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    checksum
                )
            }
        }
    }
}

/// Result of a dry-run simulation
#[derive(Debug)]
pub struct DryRunResult {
    /// Firmware file being analyzed
    pub firmware: FirmwareFile,
    /// Current device firmware version (if available)
    pub current_version: Option<String>,
    /// Device ID (if available)
    pub device_id: Option<u32>,
    /// Commands that would be sent
    pub commands: Vec<DryRunCommand>,
    /// Estimated transfer time in seconds
    pub estimated_time_secs: f32,
}

impl DryRunResult {
    /// Print the dry-run result
    pub fn print(&self, verbose: bool) {
        println!("=== DRY RUN - NO CHANGES WILL BE MADE ===");
        println!();

        println!("Firmware file: {}", self.firmware.filename);
        println!("  Type: {}", self.firmware.firmware_type);
        println!(
            "  Size: {} bytes ({} KB)",
            self.firmware.size,
            self.firmware.size / 1024
        );
        println!("  Checksum: 0x{:08X}", self.firmware.checksum);
        println!("  Chunks: {} (64 bytes each)", self.firmware.chunk_count);
        println!();

        if let Some(ref ver) = self.current_version {
            println!("Current device firmware: {ver}");
        }
        if let Some(id) = self.device_id {
            println!("Device ID: 0x{id:08X}");
        }
        println!();

        if verbose {
            println!("Commands that would be sent:");
            for (i, cmd) in self.commands.iter().enumerate() {
                println!("  {}. {}", i + 1, cmd.display());
            }
            println!();
        } else {
            println!(
                "Would send {} commands ({} data chunks)",
                self.commands.len(),
                self.commands
                    .iter()
                    .filter(|c| matches!(c, DryRunCommand::DataChunk { .. }))
                    .count()
            );
            println!("Use --verbose to see detailed command list");
            println!();
        }

        println!(
            "Estimated transfer time: {:.1} seconds",
            self.estimated_time_secs
        );
        println!();
        println!("=== DRY RUN COMPLETE - DEVICE UNCHANGED ===");
    }
}

/// Generate a dry-run simulation for USB firmware update
pub fn dry_run_usb(
    firmware: &FirmwareFile,
    current_version: Option<String>,
    device_id: Option<u32>,
) -> DryRunResult {
    let mut commands = Vec::new();

    // 1. Enter boot mode
    commands.push(DryRunCommand::EnterBootMode {
        mode: "USB",
        bytes: firmware_update::BOOT_ENTRY_USB.to_vec(),
    });

    // 2. Wait for reconnection
    commands.push(DryRunCommand::WaitReconnect {
        vid: firmware_update::BOOT_VID_PIDS[0].0,
        pid: firmware_update::BOOT_VID_PIDS[0].1,
        timeout_ms: 5000,
    });

    // Get effective data (with offset for combined firmware)
    let data = firmware.usb_data().unwrap_or(&firmware.data);
    let size = data.len() as u32;
    let chunk_count = data.len().div_ceil(firmware_update::CHUNK_SIZE) as u16;
    let checksum = firmware_update::calculate_checksum(data);

    // 3. Start transfer
    let start_header = firmware_update::build_start_header(chunk_count, size);
    commands.push(DryRunCommand::StartTransfer {
        header: start_header.to_vec(),
    });

    // 4. Data chunks
    for i in 0..chunk_count as usize {
        let offset = i * firmware_update::CHUNK_SIZE;
        let remaining = data.len() - offset;
        let chunk_size = remaining.min(firmware_update::CHUNK_SIZE);

        commands.push(DryRunCommand::DataChunk {
            index: i,
            offset,
            size: chunk_size,
        });
    }

    // 5. Complete transfer
    let complete_header = firmware_update::build_complete_header(chunk_count, checksum, size);
    commands.push(DryRunCommand::CompleteTransfer {
        header: complete_header,
        checksum,
    });

    // Estimate time: ~10ms per chunk + overhead
    let estimated_time_secs = (chunk_count as f32 * 0.01) + 2.0;

    DryRunResult {
        firmware: firmware.clone(),
        current_version,
        device_id,
        commands,
        estimated_time_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_calculation() {
        let data = [1u8, 2, 3, 4, 5];
        assert_eq!(firmware_update::calculate_checksum(&data), 15);
    }

    #[test]
    fn test_build_start_header() {
        let header = firmware_update::build_start_header(2048, 131072);
        assert_eq!(header[0], 0xBA);
        assert_eq!(header[1], 0xC0);
        // chunk_count = 2048 = 0x0800
        assert_eq!(header[2], 0x00);
        assert_eq!(header[3], 0x08);
        // size = 131072 = 0x020000
        assert_eq!(header[4], 0x00);
        assert_eq!(header[5], 0x00);
        assert_eq!(header[6], 0x02);
    }
}
