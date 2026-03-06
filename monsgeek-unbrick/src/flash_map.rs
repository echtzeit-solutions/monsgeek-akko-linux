use anyhow::{bail, Result};

/// AT32F405 flash page size (2KB)
pub const FLASH_PAGE_SIZE: u32 = 2048;

/// Bootloader region — NEVER write here
pub const BOOTLOADER_START: u32 = 0x0800_0000;
pub const BOOTLOADER_END: u32 = 0x0800_4FFF;

/// Firmware code region
pub const FIRMWARE_START: u32 = 0x0800_5000;

/// Config header (profile, LED, settings) — erase this for factory reset
pub const CONFIG_START: u32 = 0x0802_8000;
pub const CONFIG_SIZE: u32 = 2048;

/// Flash end (256KB)
pub const FLASH_END: u32 = 0x0804_0000;

/// DfuSe erase image: 0xFF page at CONFIG_START for factory reset
pub const CONFIG_ERASE_IMAGE: &[u8; FLASH_PAGE_SIZE as usize] = &[0xFF; FLASH_PAGE_SIZE as usize];

/// Chip ID strings at 0x08005000
pub const CHIP_ID_KEYBOARD: &[u8] = b"AT32F405 8KMKB";
pub const CHIP_ID_DONGLE: &[u8] = b"AT32F405 8K-DGKB";

/// ROM DFU bootloader USB IDs
pub const DFU_VID: u16 = 0x2E3C;
pub const DFU_PID: u16 = 0xDF11;

/// Validate that a write at `addr` of `len` bytes is safe.
/// Rejects writes to the bootloader region and out-of-bounds writes.
pub fn validate_write_address(addr: u32, len: u32) -> Result<()> {
    if len == 0 {
        bail!("write length is zero");
    }
    let end = addr
        .checked_add(len)
        .ok_or_else(|| anyhow::anyhow!("address overflow"))?;

    // Hard reject: bootloader
    if addr <= BOOTLOADER_END && end > BOOTLOADER_START {
        bail!(
            "REFUSED: write to 0x{addr:08X}..0x{end:08X} overlaps bootloader \
             (0x{BOOTLOADER_START:08X}..0x{BOOTLOADER_END:08X}). \
             This would brick the device with no recovery path."
        );
    }

    // Bounds check
    if addr < BOOTLOADER_START || end > FLASH_END {
        bail!(
            "address range 0x{addr:08X}..0x{end:08X} is outside flash \
             (0x{BOOTLOADER_START:08X}..0x{FLASH_END:08X})"
        );
    }

    Ok(())
}

/// Align address down to page boundary.
pub fn page_align(addr: u32) -> u32 {
    addr & !(FLASH_PAGE_SIZE - 1)
}

/// Iterator over page addresses that need erasing for a write at `addr` of `len` bytes.
pub fn pages_to_erase(addr: u32, len: u32) -> impl Iterator<Item = u32> {
    let start = page_align(addr);
    let end = page_align(addr + len - 1);
    (0..=((end - start) / FLASH_PAGE_SIZE)).map(move |i| start + i * FLASH_PAGE_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bootloader_write() {
        assert!(validate_write_address(0x0800_0000, 1).is_err());
        assert!(validate_write_address(0x0800_4FFF, 1).is_err());
        assert!(validate_write_address(0x0800_4000, 0x2000).is_err()); // overlaps
    }

    #[test]
    fn accepts_firmware_write() {
        assert!(validate_write_address(0x0800_5000, 0x20000).is_ok());
        assert!(validate_write_address(CONFIG_START, CONFIG_SIZE).is_ok());
    }

    #[test]
    fn rejects_out_of_bounds() {
        assert!(validate_write_address(0x0803_F800, 0x1000).is_err());
    }

    #[test]
    fn page_erase_count() {
        let pages: Vec<_> = pages_to_erase(0x0800_5000, 4096).collect();
        assert_eq!(pages, vec![0x0800_5000, 0x0800_5800]);

        let pages: Vec<_> = pages_to_erase(0x0800_5000, 2048).collect();
        assert_eq!(pages, vec![0x0800_5000]);
    }
}
