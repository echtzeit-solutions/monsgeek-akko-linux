use crate::winusb::WinUsbHandle;
use anyhow::{bail, Result};
use std::io::Write;

/// DFU class request codes
const DFU_DNLOAD: u8 = 1;
const DFU_UPLOAD: u8 = 2;
const DFU_GETSTATUS: u8 = 3;
const DFU_CLRSTATUS: u8 = 4;

/// DFU request type: class, interface, OUT
const DFU_OUT: u8 = 0x21;
/// DFU request type: class, interface, IN
const DFU_IN: u8 = 0xA1;

/// DfuSe special command prefixes (sent as DNLOAD block 0)
const DFUSE_CMD_SET_ADDRESS: u8 = 0x21;
const DFUSE_CMD_ERASE_PAGE: u8 = 0x41;

/// DFU transfer size (matches AT32 ROM DFU expectation)
const TRANSFER_SIZE: usize = 2048;

/// DFU states
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum DfuState {
    AppIdle = 0,
    AppDetach = 1,
    DfuIdle = 2,
    DfuDnloadSync = 3,
    DfuDnbusy = 4,
    DfuDnloadIdle = 5,
    DfuManifestSync = 6,
    DfuManifest = 7,
    DfuManifestWaitReset = 8,
    DfuUploadIdle = 9,
    DfuError = 10,
    Unknown = 0xFF,
}

impl From<u8> for DfuState {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::AppIdle,
            1 => Self::AppDetach,
            2 => Self::DfuIdle,
            3 => Self::DfuDnloadSync,
            4 => Self::DfuDnbusy,
            5 => Self::DfuDnloadIdle,
            6 => Self::DfuManifestSync,
            7 => Self::DfuManifest,
            8 => Self::DfuManifestWaitReset,
            9 => Self::DfuUploadIdle,
            10 => Self::DfuError,
            _ => Self::Unknown,
        }
    }
}

pub struct DfuStatus {
    pub status: u8,
    pub poll_timeout_ms: u32,
    pub state: DfuState,
}

pub struct DfuSeDevice {
    handle: WinUsbHandle,
    iface: u16,
}

impl DfuSeDevice {
    /// Open the AT32F405 ROM DFU device.
    pub fn open() -> Result<Self> {
        let handle =
            WinUsbHandle::open(crate::flash_map::DFU_VID, crate::flash_map::DFU_PID)?;
        Ok(Self { handle, iface: 0 })
    }

    /// Get DFU status (6-byte response).
    pub fn get_status(&self) -> Result<DfuStatus> {
        let mut buf = [0u8; 6];
        let n = self
            .handle
            .control_in(DFU_IN, DFU_GETSTATUS, 0, self.iface, &mut buf)?;
        if n < 6 {
            bail!("GETSTATUS returned only {n} bytes (expected 6)");
        }
        Ok(DfuStatus {
            status: buf[0],
            poll_timeout_ms: u32::from(buf[1])
                | (u32::from(buf[2]) << 8)
                | (u32::from(buf[3]) << 16),
            state: DfuState::from(buf[4]),
        })
    }

    /// Clear error status.
    pub fn clear_status(&self) -> Result<()> {
        self.handle
            .control_out(DFU_OUT, DFU_CLRSTATUS, 0, self.iface, &[])
    }

    /// Wait for device to be ready, polling GETSTATUS and respecting bwPollTimeout.
    /// Returns the final status.
    pub fn wait_ready(&self) -> Result<DfuStatus> {
        loop {
            let st = self.get_status()?;
            match st.state {
                DfuState::DfuDnbusy | DfuState::DfuDnloadSync | DfuState::DfuManifestSync => {
                    if st.poll_timeout_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(
                            st.poll_timeout_ms.into(),
                        ));
                    }
                    continue;
                }
                DfuState::DfuError => {
                    self.clear_status()?;
                    bail!("DFU device reported error (bStatus={})", st.status);
                }
                _ => return Ok(st),
            }
        }
    }

    /// DfuSe: set address pointer (for subsequent upload/download).
    pub fn set_address(&self, addr: u32) -> Result<()> {
        let mut cmd = [0u8; 5];
        cmd[0] = DFUSE_CMD_SET_ADDRESS;
        cmd[1..5].copy_from_slice(&addr.to_le_bytes());
        self.handle
            .control_out(DFU_OUT, DFU_DNLOAD, 0, self.iface, &cmd)?;
        self.wait_ready()?;
        Ok(())
    }

    /// DfuSe: erase a single flash page.
    pub fn erase_page(&self, addr: u32) -> Result<()> {
        let mut cmd = [0u8; 5];
        cmd[0] = DFUSE_CMD_ERASE_PAGE;
        cmd[1..5].copy_from_slice(&addr.to_le_bytes());
        self.handle
            .control_out(DFU_OUT, DFU_DNLOAD, 0, self.iface, &cmd)?;
        self.wait_ready()?;
        Ok(())
    }

    /// DfuSe: write data to flash at the given address.
    /// Erases required pages first, then writes in TRANSFER_SIZE chunks.
    pub fn write_data(&self, addr: u32, data: &[u8]) -> Result<()> {
        let len = data.len() as u32;
        crate::flash_map::validate_write_address(addr, len)?;

        // Erase pages
        let pages: Vec<u32> = crate::flash_map::pages_to_erase(addr, len).collect();
        let total_pages = pages.len();
        for (i, page_addr) in pages.into_iter().enumerate() {
            print!("\r  erasing page {}/{total_pages} (0x{page_addr:08X})...", i + 1);
            std::io::stdout().flush().ok();
            self.erase_page(page_addr)?;
        }
        println!("\r  erased {total_pages} pages.                        ");

        // Set address and write in chunks
        self.set_address(addr)?;

        let total_chunks = (data.len() + TRANSFER_SIZE - 1) / TRANSFER_SIZE;
        for (i, chunk) in data.chunks(TRANSFER_SIZE).enumerate() {
            // DfuSe: block number starts at 2 for data
            let block = (i as u16) + 2;
            print!("\r  writing block {}/{total_chunks}...", i + 1);
            std::io::stdout().flush().ok();
            self.handle
                .control_out(DFU_OUT, DFU_DNLOAD, block, self.iface, chunk)?;
            self.wait_ready()?;
        }
        println!("\r  wrote {} bytes.                        ", data.len());

        Ok(())
    }

    /// DfuSe: read data from flash at the given address.
    pub fn read_data(&self, addr: u32, len: usize) -> Result<Vec<u8>> {
        self.set_address(addr)?;

        // First UPLOAD triggers the address set; we need to do a status check
        // The AT32 ROM DFU expects set_address then UPLOAD starting at block 2
        let mut result = Vec::with_capacity(len);
        let total_chunks = (len + TRANSFER_SIZE - 1) / TRANSFER_SIZE;

        for i in 0..total_chunks {
            let block = (i as u16) + 2;
            let remaining = len - result.len();
            let this_size = remaining.min(TRANSFER_SIZE);
            let mut buf = vec![0u8; this_size];
            let n = self
                .handle
                .control_in(DFU_IN, DFU_UPLOAD, block, self.iface, &mut buf)?;
            result.extend_from_slice(&buf[..n]);
            if n < this_size {
                break; // short read
            }
        }

        Ok(result)
    }
}
