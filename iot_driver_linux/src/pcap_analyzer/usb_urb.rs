//! USB URB packet parsing for USBPcap captures
//!
//! This module parses USB Request Block (URB) packets as captured by USBPcap on Windows.
//! The format is documented in the USBPcap project.

/// Direction of USB transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Host to device (OUT)
    Out,
    /// Device to host (IN)
    In,
}

/// USB transfer type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    /// Isochronous transfer
    Isochronous,
    /// Interrupt transfer
    Interrupt,
    /// Control transfer
    Control,
    /// Bulk transfer
    Bulk,
}

impl TransferType {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Isochronous),
            1 => Some(Self::Interrupt),
            2 => Some(Self::Control),
            3 => Some(Self::Bulk),
            _ => None,
        }
    }
}

/// USB function codes from USBPcap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum UsbFunction {
    /// URB_FUNCTION_SELECT_CONFIGURATION
    SelectConfiguration = 0x0000,
    /// URB_FUNCTION_SELECT_INTERFACE
    SelectInterface = 0x0001,
    /// URB_FUNCTION_BULK_OR_INTERRUPT_TRANSFER
    BulkOrInterruptTransfer = 0x0009,
    /// URB_FUNCTION_CONTROL_TRANSFER
    ControlTransfer = 0x0008,
    /// URB_FUNCTION_CLASS_DEVICE
    ClassDevice = 0x001A,
    /// URB_FUNCTION_CLASS_INTERFACE
    ClassInterface = 0x001B,
    /// URB_FUNCTION_CLASS_ENDPOINT
    ClassEndpoint = 0x001C,
    /// URB_FUNCTION_CLASS_OTHER
    ClassOther = 0x001F,
    /// Other/unknown function
    Unknown(u16),
}

impl From<u16> for UsbFunction {
    fn from(v: u16) -> Self {
        match v {
            0x0000 => Self::SelectConfiguration,
            0x0001 => Self::SelectInterface,
            0x0008 => Self::ControlTransfer,
            0x0009 => Self::BulkOrInterruptTransfer,
            0x001A => Self::ClassDevice,
            0x001B => Self::ClassInterface,
            0x001C => Self::ClassEndpoint,
            0x001F => Self::ClassOther,
            other => Self::Unknown(other),
        }
    }
}

/// Parsed USB URB header
#[derive(Debug, Clone)]
pub struct UsbUrb {
    /// Header length in bytes
    pub header_len: u16,
    /// IRP ID for correlation
    pub irp_id: u64,
    /// USBD status
    pub status: u32,
    /// URB function
    pub function: UsbFunction,
    /// Transfer direction
    pub direction: Direction,
    /// USB bus number
    pub bus: u16,
    /// USB device address
    pub device: u16,
    /// Endpoint number
    pub endpoint: u8,
    /// Transfer type
    pub transfer_type: TransferType,
    /// Data length (after header)
    pub data_len: u32,
}

/// Control transfer setup packet
#[derive(Debug, Clone)]
pub struct ControlSetup {
    /// bmRequestType: direction, type, recipient
    pub bm_request_type: u8,
    /// bRequest: specific request code
    pub b_request: u8,
    /// wValue: request-specific value
    pub w_value: u16,
    /// wIndex: interface or endpoint
    pub w_index: u16,
    /// wLength: data length
    pub w_length: u16,
}

/// HID report types
#[allow(dead_code)]
pub mod hid_report_type {
    pub const INPUT: u8 = 1;
    pub const OUTPUT: u8 = 2;
    pub const FEATURE: u8 = 3;
}

impl ControlSetup {
    /// Check if this is a SET_REPORT request (bRequest = 9)
    pub fn is_set_report(&self) -> bool {
        self.b_request == 9
    }

    /// Check if this is a GET_REPORT request (bRequest = 1)
    pub fn is_get_report(&self) -> bool {
        self.b_request == 1
    }

    /// Check if this is a Feature report (report type 3)
    pub fn is_feature_report(&self) -> bool {
        self.report_type() == hid_report_type::FEATURE
    }

    /// Get the report type (from high byte of wValue)
    pub fn report_type(&self) -> u8 {
        (self.w_value >> 8) as u8
    }

    /// Get the report ID (from low byte of wValue)
    pub fn report_id(&self) -> u8 {
        (self.w_value & 0xFF) as u8
    }
}

/// Result of parsing a USB packet from pcap
#[derive(Debug, Clone)]
pub enum UsbPacket {
    /// Control transfer (setup + optional data)
    Control {
        urb: UsbUrb,
        setup: ControlSetup,
        data: Vec<u8>,
    },
    /// Interrupt transfer
    Interrupt { urb: UsbUrb, data: Vec<u8> },
    /// Bulk transfer
    Bulk { urb: UsbUrb, data: Vec<u8> },
    /// Other/unparsed packet
    Other { urb: UsbUrb },
}

impl UsbPacket {
    /// Get the URB from any packet type
    pub fn urb(&self) -> &UsbUrb {
        match self {
            Self::Control { urb, .. } => urb,
            Self::Interrupt { urb, .. } => urb,
            Self::Bulk { urb, .. } => urb,
            Self::Other { urb } => urb,
        }
    }

    /// Get the data payload if present
    pub fn data(&self) -> Option<&[u8]> {
        match self {
            Self::Control { data, .. } if !data.is_empty() => Some(data),
            Self::Interrupt { data, .. } if !data.is_empty() => Some(data),
            Self::Bulk { data, .. } if !data.is_empty() => Some(data),
            _ => None,
        }
    }
}

/// Parse USBPcap URB header from raw bytes
///
/// USBPcap header format (27-28 bytes minimum):
/// ```text
/// Offset  Size  Field
/// 0       2     headerLen
/// 2       8     irpId
/// 10      4     status
/// 14      2     function
/// 16      1     info (bit 0 = direction: 0=OUT, 1=IN)
/// 17      2     bus
/// 19      2     device
/// 21      1     endpoint
/// 22      1     transferType (0=iso, 1=int, 2=ctrl, 3=bulk)
/// 23      4     dataLength
/// ```
pub fn parse_urb_header(raw: &[u8]) -> Option<UsbUrb> {
    if raw.len() < 27 {
        return None;
    }

    let header_len = u16::from_le_bytes([raw[0], raw[1]]);
    if header_len < 27 || raw.len() < header_len as usize {
        return None;
    }

    let irp_id = u64::from_le_bytes([
        raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8], raw[9],
    ]);
    let status = u32::from_le_bytes([raw[10], raw[11], raw[12], raw[13]]);
    let function = u16::from_le_bytes([raw[14], raw[15]]);
    let info = raw[16];
    let bus = u16::from_le_bytes([raw[17], raw[18]]);
    let device = u16::from_le_bytes([raw[19], raw[20]]);
    let endpoint = raw[21];
    let transfer_type_byte = raw[22];
    let data_len = u32::from_le_bytes([raw[23], raw[24], raw[25], raw[26]]);

    let transfer_type = TransferType::from_byte(transfer_type_byte)?;
    let direction = if info & 0x01 != 0 {
        Direction::In
    } else {
        Direction::Out
    };

    Some(UsbUrb {
        header_len,
        irp_id,
        status,
        function: function.into(),
        direction,
        bus,
        device,
        endpoint,
        transfer_type,
        data_len,
    })
}

/// Parse control setup packet (8 bytes)
fn parse_control_setup(raw: &[u8]) -> Option<ControlSetup> {
    if raw.len() < 8 {
        return None;
    }

    Some(ControlSetup {
        bm_request_type: raw[0],
        b_request: raw[1],
        w_value: u16::from_le_bytes([raw[2], raw[3]]),
        w_index: u16::from_le_bytes([raw[4], raw[5]]),
        w_length: u16::from_le_bytes([raw[6], raw[7]]),
    })
}

/// Parse a complete USB packet from pcap data
pub fn parse_usb_packet(raw: &[u8]) -> Option<UsbPacket> {
    let urb = parse_urb_header(raw)?;
    let header_len = urb.header_len as usize;

    match urb.transfer_type {
        TransferType::Control => {
            // Control transfers have 8-byte setup packet after header
            if raw.len() < header_len + 8 {
                return Some(UsbPacket::Other { urb });
            }

            let setup = parse_control_setup(&raw[header_len..header_len + 8])?;
            let data_start = header_len + 8;
            // USBPcap data_len includes the setup packet, so actual data is data_len - 8
            let data_len = urb.data_len as usize;
            let actual_data_len = data_len.saturating_sub(8);

            let data = if raw.len() >= data_start + actual_data_len && actual_data_len > 0 {
                raw[data_start..data_start + actual_data_len].to_vec()
            } else {
                Vec::new()
            };

            Some(UsbPacket::Control { urb, setup, data })
        }
        TransferType::Interrupt => {
            let data_len = urb.data_len as usize;
            let data = if raw.len() >= header_len + data_len && data_len > 0 {
                raw[header_len..header_len + data_len].to_vec()
            } else {
                Vec::new()
            };

            Some(UsbPacket::Interrupt { urb, data })
        }
        TransferType::Bulk => {
            let data_len = urb.data_len as usize;
            let data = if raw.len() >= header_len + data_len && data_len > 0 {
                raw[header_len..header_len + data_len].to_vec()
            } else {
                Vec::new()
            };

            Some(UsbPacket::Bulk { urb, data })
        }
        TransferType::Isochronous => Some(UsbPacket::Other { urb }),
    }
}

/// Extract HID report data from a USB packet
///
/// For control transfers (SET/GET_REPORT), extracts report data,
/// skipping the report ID byte (first byte) for HID class requests.
/// Only extracts data from the correct direction:
/// - SET_REPORT: OUT (submit) packet contains command data
/// - GET_REPORT: IN (complete) packet contains response data
///
/// For interrupt/bulk transfers, extracts the data directly.
pub fn extract_hid_data(packet: &UsbPacket) -> Option<&[u8]> {
    match packet {
        UsbPacket::Control { urb, setup, data } => {
            // For HID SET_REPORT/GET_REPORT, filter by direction:
            // - SET_REPORT data is in OUT packets (host sends command)
            // - GET_REPORT data is in IN packets (device sends response)
            let is_set_with_data =
                setup.is_set_report() && urb.direction == Direction::Out && data.len() > 1;
            let is_get_with_data =
                setup.is_get_report() && urb.direction == Direction::In && data.len() > 1;

            if is_set_with_data || is_get_with_data {
                // Skip the report ID byte (first byte) to get to the command byte
                Some(&data[1..])
            } else if !data.is_empty() && data.len() > 1 {
                // Return raw data for non-HID control transfers
                Some(data)
            } else {
                None
            }
        }
        UsbPacket::Interrupt { data, .. } | UsbPacket::Bulk { data, .. } => {
            if !data.is_empty() {
                Some(data)
            } else {
                None
            }
        }
        UsbPacket::Other { urb } => {
            // For Other packets, we don't have parsed data but return None
            // The raw data is in the packet but we can't extract it here
            let _ = urb; // silence unused warning
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_urb() {
        // Minimal 27-byte URB header
        let mut raw = vec![0u8; 27];
        raw[0] = 27; // header len low byte
        raw[1] = 0; // header len high byte
        raw[22] = 1; // interrupt transfer

        let urb = parse_urb_header(&raw).unwrap();
        assert_eq!(urb.header_len, 27);
        assert_eq!(urb.transfer_type, TransferType::Interrupt);
    }

    #[test]
    fn test_control_setup_parse() {
        // SET_REPORT, report type 3 (feature), report ID 0
        let setup_data = [0x21, 0x09, 0x00, 0x03, 0x02, 0x00, 0x41, 0x00];
        let setup = parse_control_setup(&setup_data).unwrap();

        assert!(setup.is_set_report());
        assert_eq!(setup.report_type(), 3); // Feature report
        assert_eq!(setup.report_id(), 0);
        assert_eq!(setup.w_index, 2); // Interface 2
        assert_eq!(setup.w_length, 65); // 65 bytes
    }
}
