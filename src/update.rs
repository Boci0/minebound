//! Zero-dependency in-game updater backed by GitHub Releases and WinHTTP.
//! Checks `Boci0/minebound` latest release off the game thread, streams `Minebound-setup.exe`
//! to `%TEMP%` with live progress, verifies its PE signature, and launches Inno Setup on request.

use std::path::PathBuf;
use std::sync::Mutex;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const API_HOST: &str = "api.github.com";
const API_PATH: &str = "/repos/Boci0/minebound/releases/latest";
const ALLOWED_PREFIX: &str = "https://github.com/Boci0/minebound/releases/download/";
const ASSET_NAME: &str = "Minebound-setup.exe";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, url: String, size: u64 },
    Downloading { version: String, done: u64, total: u64 },
    Ready { version: String, path: PathBuf },
    Failed(String),
}

static STATE: Mutex<Status> = Mutex::new(Status::Idle);

pub fn status() -> Status {
    STATE.lock().map(|s| s.clone()).unwrap_or(Status::Idle)
}

fn set_status(next: Status) {
    if let Ok(mut s) = STATE.lock() {
        *s = next;
    }
}

/// Parses `"v1.7.0"` or `"1.7.0"` into `(major, minor, patch)`.
pub fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let clean = s.trim().trim_start_matches(['v', 'V']);
    let core = clean.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(remote: &str, current: &str) -> bool {
    matches!((parse_semver(remote), parse_semver(current)), (Some(r), Some(c)) if r > c)
}

/// Starts a background check against GitHub Releases. No-op while checking or downloading.
pub fn check() {
    {
        let Ok(mut s) = STATE.lock() else { return };
        if matches!(*s, Status::Checking | Status::Downloading { .. }) {
            return;
        }
        *s = Status::Checking;
    }
    std::thread::spawn(|| {
        let next = match fetch_latest() {
            Ok(Some((version, url, size))) if is_newer(&version, VERSION) => Status::Available { version, url, size },
            Ok(_) => Status::UpToDate,
            Err(e) => Status::Failed(e),
        };
        set_status(next);
    });
}

/// Starts downloading the available installer into `%TEMP%` on a background thread.
pub fn download() {
    let (version, url, size) = {
        let Ok(mut s) = STATE.lock() else { return };
        let Status::Available { version, url, size } = s.clone() else { return };
        *s = Status::Downloading { version: version.clone(), done: 0, total: size };
        (version, url, size)
    };
    std::thread::spawn(move || {
        match download_installer(&version, &url, size) {
            Ok(path) => set_status(Status::Ready { version, path }),
            Err(e) => set_status(Status::Failed(e)),
        }
    });
}

/// Launches the downloaded Inno Setup installer. Returns true when launched so the game can exit.
pub fn launch_installer() -> bool {
    let Status::Ready { path, .. } = status() else { return false };
    if !path.exists() {
        set_status(Status::Failed("installer missing".into()));
        return false;
    }
    match std::process::Command::new(&path).arg("/CLOSEAPPLICATIONS").spawn() {
        Ok(_) => true,
        Err(_) => {
            set_status(Status::Failed("could not start setup".into()));
            false
        }
    }
}

/// Extracts `(version_no_v, download_url, size)` from a GitHub release JSON payload.
fn parse_release_json(body: &str) -> Result<Option<(String, String, u64)>, String> {
    let tag = json_str(body, "tag_name").ok_or_else(|| "bad release response".to_string())?;
    let version = tag.trim().trim_start_matches(['v', 'V']).to_string();
    if parse_semver(&version).is_none() {
        return Err("invalid version tag".into());
    }
    // Locate the Minebound-setup.exe asset URL and optional byte size.
    let mut rest = body;
    let mut found_url = None;
    let mut found_size = 0u64;
    while let Some(idx) = rest.find("\"browser_download_url\"") {
        let slice = &rest[idx..];
        if let Some(url) = json_str(slice, "browser_download_url") {
            if url.starts_with(ALLOWED_PREFIX) && url.ends_with(ASSET_NAME) {
                found_url = Some(url);
                // Bound size lookup to this asset's enclosing JSON object (skipping nested blocks like uploader).
                let bytes = rest.as_bytes();
                let mut depth = 0i32;
                let mut obj_start = None;
                for i in (0..idx).rev() {
                    match bytes[i] {
                        b'}' => depth += 1,
                        b'{' if depth == 0 => {
                            obj_start = Some(i);
                            break;
                        }
                        b'{' => depth -= 1,
                        _ => {}
                    }
                }
                let start = obj_start.unwrap_or_else(|| idx.saturating_sub(600));
                found_size = json_u64_last(&rest[start..idx], "size")
                    .or_else(|| json_u64_last(&rest[idx.saturating_sub(600)..idx], "size"))
                    .unwrap_or(0);
                break;
            }
        }
        rest = &rest[idx + 22..];
    }
    Ok(found_url.map(|url| (version, url, found_size)))
}

fn json_str(src: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let pos = src.find(&pat)?;
    let after = src[pos + pat.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let after = after.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = after.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            _ => out.push(c),
        }
    }
    None
}

fn json_u64_last(src: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\"");
    let pos = src.rfind(&pat)?;
    let after = src[pos + pat.len()..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn split_https(url: &str) -> Result<(&str, &str), String> {
    let rest = url.strip_prefix("https://").ok_or_else(|| "https required".to_string())?;
    let (host, _) = rest.split_once('/').ok_or_else(|| "malformed url".to_string())?;
    if host.is_empty() {
        return Err("empty host".into());
    }
    Ok((host, &rest[host.len()..]))
}

fn fetch_latest() -> Result<Option<(String, String, u64)>, String> {
    let mut buf = Vec::with_capacity(8192);
    http::get(API_HOST, API_PATH, "Accept: application/vnd.github+json\r\n", |chunk, _| {
        if buf.len() + chunk.len() > 256 * 1024 {
            return Err("response too large".into());
        }
        buf.extend_from_slice(chunk);
        Ok(())
    })?;
    let body = String::from_utf8(buf).map_err(|_| "invalid utf-8".to_string())?;
    parse_release_json(&body)
}

fn download_installer(version: &str, url: &str, known_size: u64) -> Result<PathBuf, String> {
    if !url.starts_with(ALLOWED_PREFIX) || !url.ends_with(ASSET_NAME) {
        return Err("untrusted download url".into());
    }
    let (host, path) = split_https(url)?;
    let safe_ver: String = version.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.').collect();
    let dir = std::env::temp_dir();
    let tmp = dir.join(format!("Minebound-setup-{safe_ver}.tmp"));
    let dest = dir.join(format!("Minebound-setup-{safe_ver}.exe"));
    let raw_file = std::fs::File::create(&tmp).map_err(|_| "cannot write temp file".to_string())?;
    let mut file = std::io::BufWriter::with_capacity(256 * 1024, raw_file);
    let mut done = 0u64;
    let mut header = [0u8; 2];
    let mut head_n = 0usize;
    let mut last_report = 0u64;
    let res = http::get(host, path, "", |chunk, content_len| {
        use std::io::Write;
        for &b in chunk.iter().take(2 - head_n) {
            header[head_n] = b;
            head_n += 1;
        }
        file.write_all(chunk).map_err(|_| "disk write failed".to_string())?;
        done += chunk.len() as u64;
        let total = if content_len > 0 { content_len } else { known_size };
        if done.saturating_sub(last_report) >= 128 * 1024 || (total > 0 && done >= total) {
            last_report = done;
            set_status(Status::Downloading { version: version.to_string(), done, total });
        }
        Ok(())
    });
    let flush_res = {
        use std::io::Write;
        file.flush().map_err(|_| "disk write failed".to_string())
    };
    drop(file);
    if let Err(e) = res.and(flush_res) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if head_n < 2 || &header != b"MZ" || done < 1024 {
        let _ = std::fs::remove_file(&tmp);
        return Err("invalid installer file".into());
    }
    let _ = std::fs::remove_file(&dest);
    std::fs::rename(&tmp, &dest).map_err(|_| "cannot finalize installer".to_string())?;
    Ok(dest)
}

#[cfg(windows)]
mod http {
    use core::ffi::c_void;

    const WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
    const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
    const WINHTTP_ADDREQ_FLAG_ADD: u32 = 0x2000_0000;
    const WINHTTP_ADDREQ_FLAG_REPLACE: u32 = 0x8000_0000;
    const WINHTTP_QUERY_CONTENT_LENGTH: u32 = 5;
    const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
    const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;

    #[link(name = "winhttp")]
    extern "system" {
        fn WinHttpOpen(agent: *const u16, access: u32, proxy: *const u16, bypass: *const u16, flags: u32) -> *mut c_void;
        fn WinHttpSetTimeouts(h: *mut c_void, resolve: i32, connect: i32, send: i32, receive: i32) -> i32;
        fn WinHttpConnect(session: *mut c_void, server: *const u16, port: u16, reserved: u32) -> *mut c_void;
        fn WinHttpOpenRequest(
            connect: *mut c_void,
            verb: *const u16,
            object: *const u16,
            version: *const u16,
            referrer: *const u16,
            accept: *const *const u16,
            flags: u32,
        ) -> *mut c_void;
        fn WinHttpAddRequestHeaders(req: *mut c_void, headers: *const u16, len: u32, modifiers: u32) -> i32;
        fn WinHttpSendRequest(req: *mut c_void, headers: *const u16, hlen: u32, opt: *mut c_void, olen: u32, total: u32, ctx: usize) -> i32;
        fn WinHttpReceiveResponse(req: *mut c_void, reserved: *mut c_void) -> i32;
        fn WinHttpQueryHeaders(req: *mut c_void, level: u32, name: *const u16, buf: *mut c_void, len: *mut u32, index: *mut u32) -> i32;
        fn WinHttpReadData(req: *mut c_void, buf: *mut c_void, to_read: u32, read: *mut u32) -> i32;
        fn WinHttpCloseHandle(h: *mut c_void) -> i32;
    }

    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0); }
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(core::iter::once(0)).collect()
    }

    pub fn get(host: &str, path: &str, extra_headers: &str, mut on_chunk: impl FnMut(&[u8], u64) -> Result<(), String>) -> Result<(), String> {
        unsafe {
            let agent = wide(&format!("Minebound/{}", super::VERSION));
            let session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, core::ptr::null(), core::ptr::null(), 0));
            if session.0.is_null() {
                return Err("network unavailable".into());
            }
            WinHttpSetTimeouts(session.0, 8_000, 10_000, 15_000, 30_000);
            let h_w = wide(host);
            let conn = Handle(WinHttpConnect(session.0, h_w.as_ptr(), 443, 0));
            if conn.0.is_null() {
                return Err("connection failed".into());
            }
            let verb = wide("GET");
            let p_w = wide(path);
            let req = Handle(WinHttpOpenRequest(conn.0, verb.as_ptr(), p_w.as_ptr(), core::ptr::null(), core::ptr::null(), core::ptr::null(), WINHTTP_FLAG_SECURE));
            if req.0.is_null() {
                return Err("request failed".into());
            }
            if !extra_headers.is_empty() {
                let hdr = wide(extra_headers);
                WinHttpAddRequestHeaders(req.0, hdr.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE);
            }
            if WinHttpSendRequest(req.0, core::ptr::null(), 0, core::ptr::null_mut(), 0, 0, 0) == 0 {
                return Err("could not reach GitHub".into());
            }
            if WinHttpReceiveResponse(req.0, core::ptr::null_mut()) == 0 {
                return Err("no response".into());
            }
            let (mut code, mut code_sz) = (0u32, 4u32);
            if WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, core::ptr::null(), (&mut code as *mut u32).cast(), &mut code_sz, core::ptr::null_mut()) == 0 || code != 200 {
                return Err(format!("HTTP {code}"));
            }
            let (mut clen, mut clen_sz) = (0u32, 4u32);
            let total = if WinHttpQueryHeaders(req.0, WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER, core::ptr::null(), (&mut clen as *mut u32).cast(), &mut clen_sz, core::ptr::null_mut()) != 0 {
                clen as u64
            } else {
                0
            };
            let mut buf = [0u8; 64 * 1024];
            loop {
                let mut read = 0u32;
                if WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read) == 0 {
                    return Err("download interrupted".into());
                }
                if read == 0 {
                    break;
                }
                on_chunk(&buf[..read as usize], total)?;
            }
            Ok(())
        }
    }
}

#[cfg(not(windows))]
mod http {
    pub fn get(_host: &str, _path: &str, _extra: &str, _on_chunk: impl FnMut(&[u8], u64) -> Result<(), String>) -> Result<(), String> {
        Err("updates supported on Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_v_prefix_and_ordering() {
        assert_eq!(parse_semver("v1.7.0"), Some((1, 7, 0)));
        assert_eq!(parse_semver("2.0.1-rc1"), Some((2, 0, 1)));
        assert!(is_newer("v1.7.1", "1.7.0"));
        assert!(is_newer("v1.8.0", "1.7.9"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("v1.7.0", "1.7.0"));
        assert!(!is_newer("v1.6.9", "1.7.0"));
    }

    #[test]
    fn release_json_parser_extracts_version_url_and_size() {
        let sample = r#"{
            "tag_name": "v1.7.1",
            "assets": [
                {
                    "name": "Minebound-setup.exe",
                    "size": 49161551,
                    "browser_download_url": "https://github.com/Boci0/minebound/releases/download/v1.7.1/Minebound-setup.exe"
                }
            ]
        }"#;
        let parsed = parse_release_json(sample).unwrap().unwrap();
        assert_eq!(parsed.0, "1.7.1");
        assert_eq!(parsed.1, "https://github.com/Boci0/minebound/releases/download/v1.7.1/Minebound-setup.exe");
        assert_eq!(parsed.2, 49_161_551);
        // Reject untrusted download hosts
        let bad = r#"{"tag_name":"v9.9.9","assets":[{"size":10,"browser_download_url":"https://evil.example/Minebound-setup.exe"}]}"#;
        assert_eq!(parse_release_json(bad).unwrap(), None);
        // Find size even when >600 bytes separate "size" and "browser_download_url"
        let pad = "a".repeat(800);
        let long_asset = format!(
            r#"{{"tag_name":"v1.7.1","assets":[{{"name":"other.zip","size":123,"browser_download_url":"https://github.com/Boci0/minebound/releases/download/v1.7.1/other.zip"}},{{"name":"Minebound-setup.exe","uploader":{{"login":"bot"}},"size":49228731,"digest":"{pad}","browser_download_url":"https://github.com/Boci0/minebound/releases/download/v1.7.1/Minebound-setup.exe"}}]}}"#
        );
        let parsed = parse_release_json(&long_asset).unwrap().unwrap();
        assert_eq!(parsed.2, 49_228_731);
    }
}
