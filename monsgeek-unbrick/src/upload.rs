//! Firmware dump uploader — native Windows only, no extra crates.
//!
//! Uses WinHTTP for the HTTPS POST and BCrypt for the SHA-256, both via
//! `windows-sys` FFI, so the tool keeps its tiny dependency footprint.
//!
//! All failures return `Err`; callers swallow them — uploading must NEVER block
//! keyboard recovery.

use anyhow::{bail, Result};
use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash,
    BCryptHashData, BCryptOpenAlgorithmProvider, BCRYPT_SHA256_ALGORITHM,
};

/// Default collection endpoint. Override at runtime with `MONSGEEK_UPLOAD_URL`
/// (e.g. `http://127.0.0.1:8787/api/firmware-upload` for local testing).
///
/// Host is fixed; the exact path depends on how the collector is fronted on
/// echtzeit.solutions (nginx vs. the existing Rust web service reverse proxy).
const UPLOAD_URL: &str = "https://echtzeit.solutions/api/firmware-upload";

// WinHTTP constants (stable values; declared locally to avoid import churn).
const WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;

fn resolve_url() -> String {
    std::env::var("MONSGEEK_UPLOAD_URL").unwrap_or_else(|_| UPLOAD_URL.to_string())
}

/// UTF-16, null-terminated, for Windows wide-string APIs.
fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// RAII guard that closes a WinHTTP `HINTERNET` on drop.
struct WinHttpHandle(*mut c_void);
impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// Split a URL into `(secure, host, port, path)`. Minimal, no url crate.
fn parse_url(url: &str) -> Result<(bool, String, u16, String)> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("missing scheme in URL: {url}"))?;
    let secure = match scheme {
        "https" => true,
        "http" => false,
        other => bail!("unsupported scheme: {other}"),
    };
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| anyhow::anyhow!("bad port"))?),
        None => (hostport, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        bail!("empty host in URL: {url}");
    }
    Ok((secure, host.to_string(), port, path.to_string()))
}

/// POST `body` to the given endpoint. Returns the HTTP status code.
fn https_post(
    secure: bool,
    host: &str,
    port: u16,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<u16> {
    let agent = to_utf16(concat!("monsgeek-unbrick/", env!("CARGO_PKG_VERSION")));
    let host_w = to_utf16(host);
    let path_w = to_utf16(path);
    let verb = to_utf16("POST");

    let header_block: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    let headers_w = to_utf16(&header_block);
    let header_chars = headers_w.len().saturating_sub(1) as u32; // exclude NUL

    unsafe {
        let session = WinHttpHandle(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ));
        if session.0.is_null() {
            bail!("WinHttpOpen failed: error {}", GetLastError());
        }

        // resolve / connect / send / receive timeouts (ms); generous for 256KB.
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 60_000);

        let conn = WinHttpHandle(WinHttpConnect(session.0, host_w.as_ptr(), port, 0));
        if conn.0.is_null() {
            bail!("WinHttpConnect failed: error {}", GetLastError());
        }

        let flags = if secure { WINHTTP_FLAG_SECURE } else { 0 };
        let req = WinHttpHandle(WinHttpOpenRequest(
            conn.0,
            verb.as_ptr(),
            path_w.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            flags,
        ));
        if req.0.is_null() {
            bail!("WinHttpOpenRequest failed: error {}", GetLastError());
        }

        let ok = WinHttpSendRequest(
            req.0,
            headers_w.as_ptr(),
            header_chars,
            body.as_ptr() as *const c_void,
            body.len() as u32,
            body.len() as u32,
            0,
        );
        if ok == 0 {
            bail!("WinHttpSendRequest failed: error {}", GetLastError());
        }

        if WinHttpReceiveResponse(req.0, ptr::null_mut()) == 0 {
            bail!("WinHttpReceiveResponse failed: error {}", GetLastError());
        }

        let mut code: u32 = 0;
        let mut len: u32 = 4;
        let ok = WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut code as *mut u32 as *mut c_void,
            &mut len,
            ptr::null_mut(),
        );
        if ok == 0 {
            bail!("WinHttpQueryHeaders failed: error {}", GetLastError());
        }
        Ok(code as u16)
    }
}

/// Lowercase-hex SHA-256 of `data`, via native BCrypt.
pub fn sha256_hex(data: &[u8]) -> Result<String> {
    unsafe {
        let mut alg: *mut c_void = ptr::null_mut();
        if BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0) != 0 {
            bail!("BCryptOpenAlgorithmProvider failed");
        }
        // Close the provider whatever happens next.
        let _alg_guard = AlgGuard(alg);

        let mut hash: *mut c_void = ptr::null_mut();
        // Null hash-object buffer => CNG allocates it internally.
        if BCryptCreateHash(alg, &mut hash, ptr::null_mut(), 0, ptr::null(), 0, 0) != 0 {
            bail!("BCryptCreateHash failed");
        }
        let _hash_guard = HashGuard(hash);

        if BCryptHashData(hash, data.as_ptr(), data.len() as u32, 0) != 0 {
            bail!("BCryptHashData failed");
        }

        let mut digest = [0u8; 32];
        if BCryptFinishHash(hash, digest.as_mut_ptr(), digest.len() as u32, 0) != 0 {
            bail!("BCryptFinishHash failed");
        }

        let mut out = String::with_capacity(64);
        for b in digest {
            out.push_str(&format!("{b:02x}"));
        }
        Ok(out)
    }
}

struct AlgGuard(*mut c_void);
impl Drop for AlgGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { BCryptCloseAlgorithmProvider(self.0, 0) };
        }
    }
}
struct HashGuard(*mut c_void);
impl Drop for HashGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { BCryptDestroyHash(self.0) };
        }
    }
}

/// Upload a full flash dump. Returns the HTTP status code on a completed
/// request. `chip_id` is the ASCII chip-ID string read from the device.
pub fn upload_dump(chip_id: &str, body: &[u8]) -> Result<u16> {
    let sha = sha256_hex(body).unwrap_or_else(|_| "unknown".to_string());
    let size_s = body.len().to_string();
    let (secure, host, port, path) = parse_url(&resolve_url())?;

    let headers = [
        ("Content-Type", "application/octet-stream"),
        ("X-Chip-Id", chip_id),
        ("X-Dump-Size", size_s.as_str()),
        ("X-Sha256", sha.as_str()),
    ];
    https_post(secure, &host, port, &path, &headers, body)
}

#[cfg(test)]
mod tests {
    use super::parse_url;

    #[test]
    fn parses_urls() {
        let (s, h, p, path) = parse_url("https://example.com/api/firmware-upload").unwrap();
        assert!(s && h == "example.com" && p == 443 && path == "/api/firmware-upload");

        let (s, h, p, path) = parse_url("http://127.0.0.1:8799/up").unwrap();
        assert!(!s && h == "127.0.0.1" && p == 8799 && path == "/up");

        let (_, h, p, path) = parse_url("https://host.tld").unwrap();
        assert!(h == "host.tld" && p == 443 && path == "/");

        assert!(parse_url("ftp://x/y").is_err());
        assert!(parse_url("no-scheme").is_err());
    }
}
