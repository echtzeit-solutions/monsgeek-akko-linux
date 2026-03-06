use anyhow::{bail, Result};
use std::ptr;
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::Usb::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;

/// GUID for WinUSB device interface
const WINUSB_GUID: GUID = GUID {
    data1: 0xDEE824EF,
    data2: 0x729B,
    data3: 0x4A0E,
    data4: [0x9C, 0x14, 0xB7, 0x11, 0x7D, 0x33, 0xA8, 0x17],
};

/// Safe wrapper around a WinUSB device handle.
pub struct WinUsbHandle {
    file: HANDLE,
    winusb: WINUSB_INTERFACE_HANDLE,
}

impl WinUsbHandle {
    /// Open a USB device by VID and PID using SetupDI enumeration + WinUSB.
    pub fn open(vid: u16, pid: u16) -> Result<Self> {
        let device_path = find_device_path(vid, pid)?;

        let file = unsafe {
            CreateFileW(
                device_path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };

        if file == INVALID_HANDLE_VALUE {
            bail!(
                "CreateFileW failed for DFU device: error {}",
                unsafe { GetLastError() }
            );
        }

        let mut winusb: WINUSB_INTERFACE_HANDLE = ptr::null_mut();
        let ok = unsafe { WinUsb_Initialize(file, &mut winusb) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            unsafe { CloseHandle(file) };
            bail!("WinUsb_Initialize failed: error {err}");
        }

        Ok(Self { file, winusb })
    }

    /// Send a control transfer (OUT direction — host to device).
    pub fn control_out(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &[u8],
    ) -> Result<()> {
        let setup = WINUSB_SETUP_PACKET {
            RequestType: request_type,
            Request: request,
            Value: value,
            Index: index,
            Length: data.len() as u16,
        };
        let mut transferred: u32 = 0;
        let ok = unsafe {
            WinUsb_ControlTransfer(
                self.winusb,
                setup,
                data.as_ptr() as *mut u8,
                data.len() as u32,
                &mut transferred,
                ptr::null(),
            )
        };
        if ok == 0 {
            bail!(
                "control_out failed (req=0x{request:02X} val=0x{value:04X}): error {}",
                unsafe { GetLastError() }
            );
        }
        Ok(())
    }

    /// Send a control transfer (IN direction — device to host).
    pub fn control_in(
        &self,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        buf: &mut [u8],
    ) -> Result<usize> {
        let setup = WINUSB_SETUP_PACKET {
            RequestType: request_type,
            Request: request,
            Value: value,
            Index: index,
            Length: buf.len() as u16,
        };
        let mut transferred: u32 = 0;
        let ok = unsafe {
            WinUsb_ControlTransfer(
                self.winusb,
                setup,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut transferred,
                ptr::null(),
            )
        };
        if ok == 0 {
            bail!(
                "control_in failed (req=0x{request:02X} val=0x{value:04X}): error {}",
                unsafe { GetLastError() }
            );
        }
        Ok(transferred as usize)
    }
}

impl Drop for WinUsbHandle {
    fn drop(&mut self) {
        unsafe {
            WinUsb_Free(self.winusb);
            CloseHandle(self.file);
        }
    }
}

/// Find the device path for a USB device matching VID:PID via SetupDI.
fn find_device_path(vid: u16, pid: u16) -> Result<Vec<u16>> {
    let needle_vid = format!("vid_{vid:04x}");
    let needle_pid = format!("pid_{pid:04x}");

    unsafe {
        let devinfo = SetupDiGetClassDevsW(
            &WINUSB_GUID,
            ptr::null(),
            ptr::null_mut(), // hwndParent
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );
        if devinfo == -1 {
            // INVALID_HANDLE_VALUE for HDEVINFO (isize)
            bail!("SetupDiGetClassDevsW failed: error {}", GetLastError());
        }

        // Ensure cleanup
        struct DevInfoGuard(isize);
        impl Drop for DevInfoGuard {
            fn drop(&mut self) {
                unsafe { SetupDiDestroyDeviceInfoList(self.0) };
            }
        }
        let _guard = DevInfoGuard(devinfo);

        let mut index: u32 = 0;
        loop {
            let mut iface_data: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
            iface_data.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;

            if SetupDiEnumDeviceInterfaces(
                devinfo,
                ptr::null(),
                &WINUSB_GUID,
                index,
                &mut iface_data,
            ) == 0
            {
                let err = GetLastError();
                if err == ERROR_NO_MORE_ITEMS {
                    break;
                }
                bail!("SetupDiEnumDeviceInterfaces failed: error {err}");
            }

            // Get required size
            let mut required_size: u32 = 0;
            SetupDiGetDeviceInterfaceDetailW(
                devinfo,
                &mut iface_data,
                ptr::null_mut(),
                0,
                &mut required_size,
                ptr::null_mut(),
            );

            // Allocate and fill detail data
            let mut buf = vec![0u8; required_size as usize];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            // cbSize must be the size of the fixed part of the struct (on 64-bit: 8)
            (*detail).cbSize =
                std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;

            if SetupDiGetDeviceInterfaceDetailW(
                devinfo,
                &mut iface_data,
                detail,
                required_size,
                ptr::null_mut(),
                ptr::null_mut(),
            ) == 0
            {
                bail!(
                    "SetupDiGetDeviceInterfaceDetailW failed: error {}",
                    GetLastError()
                );
            }

            // Extract the DevicePath (wide string after cbSize field)
            let path_ptr = &(*detail).DevicePath as *const u16;
            let path_len = {
                let mut len = 0;
                while *path_ptr.add(len) != 0 {
                    len += 1;
                }
                len
            };
            let path_slice = std::slice::from_raw_parts(path_ptr, path_len);
            let path_str = String::from_utf16_lossy(path_slice).to_lowercase();

            if path_str.contains(&needle_vid) && path_str.contains(&needle_pid) {
                // Return null-terminated wide string
                let mut wide: Vec<u16> = path_slice.to_vec();
                wide.push(0);
                return Ok(wide);
            }

            index += 1;
        }

        bail!(
            "DFU device not found (VID={vid:04X} PID={pid:04X}). \
             Is the device in DFU mode? Is the WinUSB driver installed?"
        );
    }
}
