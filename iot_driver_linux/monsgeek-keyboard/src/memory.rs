//! Peek and poke over the vendor channel (patched firmware, commands 0xEB / 0xEC).
//!
//! The wire format and the list of addresses the firmware will touch live next
//! to its handlers, in `patch/memaccess.h`. One peek request returns at most
//! [`PEEK_MAX_LEN`] bytes; [`KeyboardInterface::peek`] splits longer reads.
//!
//! Every request carries a nonce that the firmware echoes. A debugging tool
//! that silently shows the previous command's answer is worse than one that
//! fails, so a reply with the wrong nonce or address is discarded and the
//! request sent again.

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

use monsgeek_transport::ChecksumType;
use monsgeek_transport::protocol::cmd;

use crate::{KeyboardError, KeyboardInterface};

/// Most bytes one peek request can return.
pub const PEEK_MAX_LEN: usize = 48;

/// Tag the firmware requires before it will write memory.
const POKE_TAG: [u8; 4] = *b"POKE";

/// Reply header: command echo, status, length or width, nonce, address.
const REPLY_HEADER_LEN: usize = 8;

/// Start of the peripheral register space.
const PERIPHERAL_BASE: u32 = 0x4000_0000;

/// Attempts at one request before giving up on stale replies.
const ATTEMPTS: usize = 3;

static NONCE: AtomicU8 = AtomicU8::new(0);

fn next_nonce() -> u8 {
    NONCE.fetch_add(1, Ordering::Relaxed)
}

/// Size of one bus access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Byte,
    Half,
    Word,
}

impl Width {
    pub fn bytes(self) -> usize {
        match self {
            Self::Byte => 1,
            Self::Half => 2,
            Self::Word => 4,
        }
    }

    pub fn from_bytes(n: u8) -> Option<Self> {
        match n {
            1 => Some(Self::Byte),
            2 => Some(Self::Half),
            4 => Some(Self::Word),
            _ => None,
        }
    }

    /// Registers are read whole; memory is fine a byte at a time.
    pub fn default_for(addr: u32) -> Self {
        if addr >= PERIPHERAL_BASE {
            Self::Word
        } else {
            Self::Byte
        }
    }
}

/// Why the firmware refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemStatus {
    /// Not an address this command may touch.
    BadRange,
    /// Length 0 or over 48, a bad width, or a length that is not a multiple of it.
    BadArgs,
    Unaligned,
    /// The poke tag did not arrive intact.
    NotArmed,
    /// Flash and the system control block cannot be written.
    ReadOnly,
    /// A status this driver does not know.
    Unknown(u8),
}

impl MemStatus {
    fn from_wire(code: u8) -> Option<Self> {
        match code {
            0 => None,
            1 => Some(Self::BadRange),
            2 => Some(Self::BadArgs),
            3 => Some(Self::Unaligned),
            4 => Some(Self::NotArmed),
            5 => Some(Self::ReadOnly),
            n => Some(Self::Unknown(n)),
        }
    }
}

impl fmt::Display for MemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRange => f.write_str(
                "address is outside flash, SRAM and the known register blocks, \
                 or the range crosses the end of one",
            ),
            Self::BadArgs => f.write_str("bad length or width"),
            Self::Unaligned => f.write_str("address is not aligned to the access width"),
            Self::NotArmed => f.write_str("the firmware did not see the poke tag"),
            Self::ReadOnly => f.write_str("flash and the system control block cannot be written"),
            Self::Unknown(n) => write!(f, "unknown status {n}"),
        }
    }
}

/// Why a reply cannot be used.
#[derive(Debug, PartialEq, Eq)]
enum ReplyError {
    /// Wrong command echo, or shorter than a header.
    Malformed,
    /// Nonce or address does not match the request: an earlier answer.
    Stale,
    /// The firmware refused.
    Refused(MemStatus),
}

/// A reply, checked against the request it answers.
struct Reply<'a> {
    /// Bytes returned (peek) or access width (poke).
    count: u8,
    /// Everything after the header.
    data: &'a [u8],
}

fn check_reply(resp: &[u8], cmd_byte: u8, addr: u32, nonce: u8) -> Result<Reply<'_>, ReplyError> {
    if resp.len() < REPLY_HEADER_LEN || resp[0] != cmd_byte {
        return Err(ReplyError::Malformed);
    }
    let echoed_addr = u32::from_le_bytes([resp[4], resp[5], resp[6], resp[7]]);
    if resp[3] != nonce || echoed_addr != addr {
        return Err(ReplyError::Stale);
    }
    if let Some(status) = MemStatus::from_wire(resp[1]) {
        return Err(ReplyError::Refused(status));
    }
    Ok(Reply {
        count: resp[2],
        data: &resp[REPLY_HEADER_LEN..],
    })
}

/// `[addr LE32, len, width, <checksum slot>, nonce]`
///
/// Payload byte 6 is where the transport writes its Bit7 checksum, over
/// whatever is there, so it never carries a field: a nonce kept in it comes
/// back as the checksum.
fn peek_request(addr: u32, len: u8, width: Width, nonce: u8) -> [u8; 8] {
    let a = addr.to_le_bytes();
    [a[0], a[1], a[2], a[3], len, width.bytes() as u8, 0, nonce]
}

/// `[addr LE32, width, nonce, <checksum slot>, value LE32, "POKE"]`
///
/// Byte 6 is the checksum slot again; the value starts after it so no byte of
/// it can be overwritten.
fn poke_request(addr: u32, width: Width, value: u32, nonce: u8) -> [u8; 15] {
    let a = addr.to_le_bytes();
    let v = value.to_le_bytes();
    let t = POKE_TAG;
    [
        a[0],
        a[1],
        a[2],
        a[3],
        width.bytes() as u8,
        nonce,
        0,
        v[0],
        v[1],
        v[2],
        v[3],
        t[0],
        t[1],
        t[2],
        t[3],
    ]
}

/// Split a read into requests the firmware accepts: `(address, length)` pairs.
fn plan_peek(addr: u32, len: usize, width: Width) -> Result<Vec<(u32, usize)>, KeyboardError> {
    let w = width.bytes();
    if len == 0 {
        return Err(KeyboardError::InvalidParameter(
            "length must be at least 1".into(),
        ));
    }
    if !len.is_multiple_of(w) {
        return Err(KeyboardError::InvalidParameter(format!(
            "length {len} is not a multiple of the {w}-byte access width"
        )));
    }
    if !(addr as usize).is_multiple_of(w) {
        return Err(KeyboardError::InvalidParameter(format!(
            "address {addr:#010x} is not aligned to the {w}-byte access width"
        )));
    }
    if u64::from(addr) + len as u64 > u64::from(u32::MAX) + 1 {
        return Err(KeyboardError::InvalidParameter(format!(
            "{len} bytes from {addr:#010x} runs past the end of the address space"
        )));
    }

    // 48 is a multiple of every width, so chunks stay aligned.
    let mut chunks = Vec::with_capacity(len.div_ceil(PEEK_MAX_LEN));
    let mut done = 0;
    while done < len {
        let n = (len - done).min(PEEK_MAX_LEN);
        chunks.push((addr + done as u32, n));
        done += n;
    }
    Ok(chunks)
}

fn refusal(what: &str, addr: u32, err: ReplyError) -> KeyboardError {
    match err {
        ReplyError::Refused(status) => KeyboardError::InvalidParameter(format!(
            "firmware refused {what} at {addr:#010x}: {status}"
        )),
        ReplyError::Stale => KeyboardError::UnexpectedResponse(format!(
            "{what} at {addr:#010x}: only stale replies came back"
        )),
        ReplyError::Malformed => {
            KeyboardError::UnexpectedResponse(format!("{what} at {addr:#010x}: malformed reply"))
        }
    }
}

impl KeyboardInterface {
    /// Whether this firmware answers peek and poke (patch capability bit 7).
    pub fn supports_peek_poke(&self) -> Result<bool, KeyboardError> {
        Ok(self
            .get_patch_info()?
            .is_some_and(|patch| patch.has_peek_poke()))
    }

    /// Read `len` bytes starting at `addr`, `width` bytes per access.
    ///
    /// The firmware only reads flash, SRAM, the system control block and the
    /// known register blocks. Reading a register can have side effects (a data
    /// register that clears a flag, for one).
    pub fn peek(&self, addr: u32, len: usize, width: Width) -> Result<Vec<u8>, KeyboardError> {
        let mut out = Vec::with_capacity(len);
        for (chunk_addr, chunk_len) in plan_peek(addr, len, width)? {
            out.extend(self.peek_chunk(chunk_addr, chunk_len, width)?);
        }
        Ok(out)
    }

    fn peek_chunk(&self, addr: u32, len: usize, width: Width) -> Result<Vec<u8>, KeyboardError> {
        let mut last = ReplyError::Stale;
        for _ in 0..ATTEMPTS {
            let nonce = next_nonce();
            let request = peek_request(addr, len as u8, width, nonce);
            let resp = self
                .transport
                .query_command(cmd::PEEK, &request, ChecksumType::Bit7)?;
            match check_reply(&resp, cmd::PEEK, addr, nonce) {
                Ok(reply) if usize::from(reply.count) == len && reply.data.len() >= len => {
                    return Ok(reply.data[..len].to_vec());
                }
                Ok(_) => last = ReplyError::Malformed,
                Err(ReplyError::Stale) => last = ReplyError::Stale,
                Err(e) => return Err(refusal("peek", addr, e)),
            }
        }
        Err(refusal("peek", addr, last))
    }

    /// Write one `width`-byte value at `addr`. SRAM and registers only.
    ///
    /// Returns the value read back from SRAM. For a register it is 0, because
    /// the firmware does not read registers back: some reads have side effects.
    pub fn poke(&self, addr: u32, width: Width, value: u32) -> Result<u32, KeyboardError> {
        let w = width.bytes();
        if !(addr as usize).is_multiple_of(w) {
            return Err(KeyboardError::InvalidParameter(format!(
                "address {addr:#010x} is not aligned to the {w}-byte access width"
            )));
        }
        if w < 4 && value >> (8 * w) != 0 {
            return Err(KeyboardError::InvalidParameter(format!(
                "value {value:#x} does not fit in {w} byte(s)"
            )));
        }

        let mut last = ReplyError::Stale;
        for _ in 0..ATTEMPTS {
            let nonce = next_nonce();
            let request = poke_request(addr, width, value, nonce);
            let resp = self
                .transport
                .query_command(cmd::POKE, &request, ChecksumType::Bit7)?;
            match check_reply(&resp, cmd::POKE, addr, nonce) {
                Ok(reply) if reply.data.len() >= 4 => {
                    return Ok(u32::from_le_bytes([
                        reply.data[0],
                        reply.data[1],
                        reply.data[2],
                        reply.data[3],
                    ]));
                }
                Ok(_) => last = ReplyError::Malformed,
                Err(ReplyError::Stale) => last = ReplyError::Stale,
                Err(e) => return Err(refusal("poke", addr, e)),
            }
        }
        Err(refusal("poke", addr, last))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Payload byte the transport overwrites with its Bit7 checksum.
    const CHECKSUM_SLOT: usize = 6;

    /// A reply as the firmware writes it: command echo, then the header, then data.
    fn reply(cmd_byte: u8, status: u8, count: u8, nonce: u8, addr: u32, data: &[u8]) -> Vec<u8> {
        let mut r = vec![cmd_byte, status, count, nonce];
        r.extend(addr.to_le_bytes());
        r.extend(data);
        r.resize(64, 0);
        r
    }

    // The vectors in the next two tests are mirrored in patch/test_memaccess.c,
    // so the firmware handlers are checked against the same bytes.
    #[test]
    fn peek_request_matches_the_firmware_layout() {
        // payload: address, length, width, checksum slot, nonce
        assert_eq!(
            peek_request(0x2000_0100, 16, Width::Byte, 0x5A),
            [0x00, 0x01, 0x00, 0x20, 16, 1, 0, 0x5A]
        );
        assert_eq!(
            peek_request(0x4002_0410, 4, Width::Word, 7),
            [0x10, 0x04, 0x02, 0x40, 4, 4, 0, 7]
        );
    }

    #[test]
    fn poke_request_matches_the_firmware_layout() {
        // payload: address, width, nonce, checksum slot, value, tag
        assert_eq!(
            poke_request(0x4002_0418, Width::Word, 0x0000_2000, 9),
            [
                0x18, 0x04, 0x02, 0x40, 4, 9, 0, 0x00, 0x20, 0x00, 0x00, b'P', b'O', b'K', b'E'
            ]
        );
    }

    /// The bug the first version shipped with: the transport's Bit7 checksum
    /// overwrote the nonce, and would have overwritten a poke value byte.
    #[test]
    fn the_transports_checksum_only_touches_the_reserved_byte() {
        use monsgeek_transport::protocol::{build_command, calculate_checksum};

        let mut requests: Vec<(u8, Vec<u8>)> = Vec::new();
        for nonce in [0u8, 1, 0x5A, 0xFF] {
            for (addr, len, width) in [
                (0x2000_0000, 48, Width::Byte),
                (0x4002_0410, 4, Width::Word),
                (0xE000_ED28, 4, Width::Word),
                (0x0800_5200, 16, Width::Half),
            ] {
                requests.push((cmd::PEEK, peek_request(addr, len, width, nonce).to_vec()));
            }
            // every byte of the value is nonzero, so a clobbered one shows
            for value in [0xDEAD_BEEF, 0x0102_0304, 0x0000_2000, 0xFFFF_FFFF] {
                requests.push((
                    cmd::POKE,
                    poke_request(0x4002_0418, Width::Word, value, nonce).to_vec(),
                ));
            }
        }

        for (cmd_byte, payload) in requests {
            assert_eq!(payload[CHECKSUM_SLOT], 0, "{cmd_byte:#x} {payload:02x?}");
            let frame = build_command(cmd_byte, &payload, ChecksumType::Bit7);
            // frame: report id, command, payload...
            assert_eq!(frame[1], cmd_byte);
            for (i, &byte) in payload.iter().enumerate() {
                if i != CHECKSUM_SLOT {
                    assert_eq!(frame[2 + i], byte, "payload byte {i} of {payload:02x?}");
                }
            }
            assert_eq!(
                frame[2 + CHECKSUM_SLOT],
                calculate_checksum(&frame[1..], ChecksumType::Bit7),
                "the checksum should be in the reserved byte"
            );
        }
    }

    #[test]
    fn a_good_reply_is_accepted() {
        let r = reply(cmd::PEEK, 0, 3, 0x11, 0x2000_0000, &[1, 2, 3]);
        let reply = check_reply(&r, cmd::PEEK, 0x2000_0000, 0x11).unwrap();
        assert_eq!(reply.count, 3);
        assert_eq!(&reply.data[..3], &[1, 2, 3]);
    }

    #[test]
    fn a_reply_to_another_request_is_stale() {
        let r = reply(cmd::PEEK, 0, 1, 0x11, 0x2000_0000, &[9]);
        // wrong nonce, same address: the case a repeated peek of one address hits
        assert!(matches!(
            check_reply(&r, cmd::PEEK, 0x2000_0000, 0x12),
            Err(ReplyError::Stale)
        ));
        // right nonce, wrong address
        assert!(matches!(
            check_reply(&r, cmd::PEEK, 0x2000_0004, 0x11),
            Err(ReplyError::Stale)
        ));
    }

    #[test]
    fn each_refusal_is_reported_by_status() {
        let cases = [
            (1, MemStatus::BadRange),
            (2, MemStatus::BadArgs),
            (3, MemStatus::Unaligned),
            (4, MemStatus::NotArmed),
            (5, MemStatus::ReadOnly),
            (77, MemStatus::Unknown(77)),
        ];
        for (code, want) in cases {
            let r = reply(cmd::POKE, code, 0, 5, 0x0800_0000, &[]);
            assert_eq!(
                check_reply(&r, cmd::POKE, 0x0800_0000, 5).err(),
                Some(ReplyError::Refused(want)),
                "status {code}"
            );
        }
    }

    #[test]
    fn a_stale_reply_is_not_taken_for_a_refusal() {
        // an old refusal must not fail a new request
        let r = reply(cmd::PEEK, 1, 0, 1, 0x5000_0000, &[]);
        assert!(matches!(
            check_reply(&r, cmd::PEEK, 0x2000_0000, 2),
            Err(ReplyError::Stale)
        ));
    }

    #[test]
    fn a_reply_to_a_different_command_is_malformed() {
        let r = reply(cmd::POKE, 0, 1, 1, 0x2000_0000, &[0]);
        assert_eq!(
            check_reply(&r, cmd::PEEK, 0x2000_0000, 1).err(),
            Some(ReplyError::Malformed)
        );
        assert_eq!(
            check_reply(&r[..7], cmd::POKE, 0x2000_0000, 1).err(),
            Some(ReplyError::Malformed)
        );
        assert_eq!(
            check_reply(&[], cmd::PEEK, 0, 0).err(),
            Some(ReplyError::Malformed)
        );
    }

    #[test]
    fn short_reads_are_one_request() {
        assert_eq!(
            plan_peek(0x2000_0000, 48, Width::Byte).unwrap(),
            [(0x2000_0000, 48)]
        );
        assert_eq!(
            plan_peek(0x2000_0000, 1, Width::Byte).unwrap(),
            [(0x2000_0000, 1)]
        );
    }

    #[test]
    fn long_reads_are_split_into_aligned_chunks() {
        let chunks = plan_peek(0x2000_0000, 100, Width::Word).unwrap();
        assert_eq!(
            chunks,
            [(0x2000_0000, 48), (0x2000_0030, 48), (0x2000_0060, 4)]
        );
        assert!(
            chunks
                .iter()
                .all(|&(a, n)| a % 4 == 0 && n % 4 == 0 && n <= PEEK_MAX_LEN)
        );
        assert_eq!(chunks.iter().map(|&(_, n)| n).sum::<usize>(), 100);
    }

    #[test]
    fn bad_reads_are_refused_before_any_traffic() {
        assert!(plan_peek(0x2000_0000, 0, Width::Byte).is_err());
        assert!(plan_peek(0x2000_0000, 6, Width::Word).is_err());
        assert!(plan_peek(0x2000_0002, 4, Width::Word).is_err());
        assert!(plan_peek(0xFFFF_FFFC, 8, Width::Word).is_err());
        // ending exactly at the top of the address space is legal to ask for
        assert!(plan_peek(0xFFFF_FFFC, 4, Width::Word).is_ok());
    }

    #[test]
    fn width_helpers() {
        assert_eq!(Width::from_bytes(3), None);
        assert_eq!(Width::from_bytes(0), None);
        for n in [1u8, 2, 4] {
            assert_eq!(Width::from_bytes(n).unwrap().bytes(), usize::from(n));
        }
        assert_eq!(Width::default_for(0x2000_0000), Width::Byte);
        assert_eq!(Width::default_for(0x0800_5000), Width::Byte);
        assert_eq!(Width::default_for(0x4002_0000), Width::Word);
        assert_eq!(Width::default_for(0xE000_ED28), Width::Word);
    }

    #[test]
    fn statuses_read_as_sentences() {
        assert!(MemStatus::BadRange.to_string().contains("outside flash"));
        assert!(
            MemStatus::ReadOnly
                .to_string()
                .contains("cannot be written")
        );
        assert_eq!(MemStatus::Unknown(9).to_string(), "unknown status 9");
    }
}
