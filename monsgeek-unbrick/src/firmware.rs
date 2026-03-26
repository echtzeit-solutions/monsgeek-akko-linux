use anyhow::{bail, Context, Result};
use std::path::Path;

/// A firmware image: data bytes and their target flash address.
pub struct FirmwareImage {
    pub address: u32,
    pub data: Vec<u8>,
}

/// Load a firmware file. Supports raw .bin and DfuSe .dfu formats.
///
/// If `include_bootloader` is true, a 256KB flash dump will include the
/// bootloader region (0x08000000-0x08004FFF) in the output. This is needed
/// to restore a board whose bootloader has been corrupted.
pub fn load_firmware(
    path: &Path,
    address: Option<u32>,
    include_bootloader: bool,
) -> Result<Vec<FirmwareImage>> {
    let data = std::fs::read(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    if data.len() < 8 {
        bail!("file too small ({} bytes)", data.len());
    }

    // Check for DfuSe prefix magic "DfuSe"
    if &data[..5] == b"DfuSe" {
        return parse_dfuse(&data);
    }

    // Full 256KB flash dump? Split into segments.
    if data.len() as u32 == crate::flash_map::FLASH_SIZE {
        return split_flash_dump(&data, include_bootloader);
    }

    // Raw .bin — require explicit or default address
    let addr = address.unwrap_or(crate::flash_map::FIRMWARE_START);
    crate::flash_map::validate_write_address(addr, data.len() as u32)?;

    Ok(vec![FirmwareImage {
        address: addr,
        data,
    }])
}

/// Split a full 256KB flash dump into writable segments, skipping the bootloader
/// (unless `include_bootloader` is true) and any trailing erased (0xFF) regions.
fn split_flash_dump(data: &[u8], include_bootloader: bool) -> Result<Vec<FirmwareImage>> {
    use crate::flash_map::*;

    let mut images = Vec::new();

    if include_bootloader {
        let boot_size = (FIRMWARE_START - BOOTLOADER_START) as usize;
        images.push(FirmwareImage {
            address: BOOTLOADER_START,
            data: data[..boot_size].to_vec(),
        });
    }

    let boot_size = (FIRMWARE_START - BOOTLOADER_START) as usize;
    let writable = &data[boot_size..];

    // Find last non-0xFF byte to avoid writing huge erased tails
    let last_used = writable
        .iter()
        .rposition(|&b| b != 0xFF)
        .map(|i| i + 1)
        .unwrap_or(0);

    if last_used == 0 && images.is_empty() {
        bail!("flash dump is entirely erased (all 0xFF) after bootloader");
    }

    if last_used > 0 {
        // Round up to page boundary
        let len = ((last_used as u32 + FLASH_PAGE_SIZE - 1)
            / FLASH_PAGE_SIZE
            * FLASH_PAGE_SIZE) as usize;
        let len = len.min(writable.len());

        validate_write_address(FIRMWARE_START, len as u32)?;

        images.push(FirmwareImage {
            address: FIRMWARE_START,
            data: writable[..len].to_vec(),
        });
    }

    Ok(images)
}

/// Parse a DfuSe container file.
/// Format: prefix(11B) + targets(variable) + suffix(16B)
fn parse_dfuse(data: &[u8]) -> Result<Vec<FirmwareImage>> {
    if data.len() < 11 {
        bail!("DfuSe file too short for prefix");
    }

    let version = data[5];
    if version != 1 {
        bail!("unsupported DfuSe version {version}");
    }

    let _dfu_image_size = u32::from_le_bytes(data[6..10].try_into().unwrap());
    let num_targets = data[10];

    let mut images = Vec::new();
    let mut offset = 11; // after prefix

    for target_idx in 0..num_targets {
        if offset + 274 > data.len() {
            bail!("DfuSe target {target_idx} header truncated");
        }

        // Target prefix: "Target" (6B) + alt_setting(1B) + named(4B) + name(255B) + size(4B) + num_elements(4B)
        if &data[offset..offset + 6] != b"Target" {
            bail!(
                "expected 'Target' magic at offset {offset}, got {:?}",
                &data[offset..offset + 6]
            );
        }

        let target_size =
            u32::from_le_bytes(data[offset + 266..offset + 270].try_into().unwrap());
        let num_elements =
            u32::from_le_bytes(data[offset + 270..offset + 274].try_into().unwrap());
        let _ = target_size; // validated implicitly by parsing elements

        offset += 274; // past target prefix

        for elem_idx in 0..num_elements {
            if offset + 8 > data.len() {
                bail!("DfuSe element {elem_idx} header truncated in target {target_idx}");
            }

            let elem_addr =
                u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            let elem_size =
                u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;

            offset += 8;

            if offset + elem_size > data.len() {
                bail!(
                    "DfuSe element data truncated: need {elem_size} bytes at offset {offset}"
                );
            }

            crate::flash_map::validate_write_address(elem_addr, elem_size as u32)?;
            images.push(FirmwareImage {
                address: elem_addr,
                data: data[offset..offset + elem_size].to_vec(),
            });

            offset += elem_size;
        }
    }

    if images.is_empty() {
        bail!("DfuSe file contains no elements");
    }

    Ok(images)
}
