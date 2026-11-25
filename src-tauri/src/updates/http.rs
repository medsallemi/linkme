use std::time::Duration;

use super::{MAX_BODY, RETRY_AFTER_FAILURE};

const MAX_HEADERS: usize = 16 * 1024;
#[cfg(windows)]
const MAX_WIRE: usize = MAX_HEADERS + MAX_BODY + 4;

pub(super) enum Failure {
    Unavailable,
    #[cfg(any(windows, test))]
    Timeout,
}

pub(super) struct Response<'a> {
    pub status: u16,
    pub body: &'a [u8],
    headers: &'a str,
}

impl<'a> Response<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let separator = bytes[..bytes.len().min(MAX_HEADERS + 4)]
            .windows(4)
            .position(|part| part == b"\r\n\r\n")?;
        let (headers, body) = (&bytes[..separator], &bytes[separator + 4..]);
        if body.len() > MAX_BODY {
            return None;
        }
        let headers = std::str::from_utf8(headers).ok()?;
        let (line, headers) = headers.split_once("\r\n").unwrap_or((headers, ""));
        let mut parts = line.split_ascii_whitespace();
        if !matches!(parts.next()?, "HTTP/1.0" | "HTTP/1.1") {
            return None;
        }
        Some(Self {
            status: parts.next()?.parse().ok()?,
            body,
            headers,
        })
    }

    fn header(&self, name: &str) -> Option<&'a str> {
        let mut values = self
            .headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(key, _)| key.eq_ignore_ascii_case(name));
        let (_, value) = values.next()?;
        values.next().is_none().then(|| value.trim())
    }

    pub fn is_json(&self) -> bool {
        self.header("content-type")
            .and_then(|value| value.split(';').next())
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
    }

    pub fn etag(&self) -> Option<&'a str> {
        let value = self.header("etag")?;
        let quoted = value.strip_prefix("W/").unwrap_or(value);
        let interior = quoted.strip_prefix('"')?.strip_suffix('"')?;
        (value.len() <= 256
            && interior
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b'"'))
        .then_some(value)
    }

    pub fn retry_delay(&self, unix: u64) -> Duration {
        let seconds = |value: &str| value.parse::<u64>().ok();
        let retry = self.header("retry-after").and_then(seconds).unwrap_or(0);
        let reset = if self.header("x-ratelimit-remaining") == Some("0") {
            self.header("x-ratelimit-reset")
                .and_then(seconds)
                .unwrap_or(0)
                .saturating_sub(unix)
        } else {
            0
        };
        Duration::from_secs(
            retry
                .max(reset)
                .clamp(RETRY_AFTER_FAILURE.as_secs(), 24 * 60 * 60),
        )
    }
}

#[cfg(windows)]
pub(super) fn fetch(url: String, etag: Option<String>) -> Result<Vec<u8>, Failure> {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    let executable = crate::native::system_curl().ok_or(Failure::Unavailable)?;
    // Repository constructs the URL. We do not load curl configuration or follow redirects.
    let mut command = Command::new(executable);
    command.args([
        "-q",
        "--silent",
        "--globoff",
        "--noproxy",
        "*",
        "--proto",
        "=https",
        "--http1.1",
        "--connect-timeout",
        "4",
        "--max-time",
        "8",
        "--include",
        "--header",
        "Accept: application/vnd.github+json",
        "--header",
        "Accept-Encoding: identity",
        "--header",
        "X-GitHub-Api-Version: 2022-11-28",
        "--user-agent",
        "Linkme",
    ]);
    if let Some(etag) = etag {
        command
            .arg("--header")
            .arg(format!("If-None-Match: {etag}"));
    }
    let mut child = command
        .arg("--url")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|_| Failure::Unavailable)?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or(Failure::Unavailable)
        .and_then(|stdout| {
            stdout
                .take((MAX_WIRE + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| Failure::Unavailable)
        });
    if read.is_err() || bytes.len() > MAX_WIRE {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|_| Failure::Unavailable)?;
    if status.code() == Some(28) {
        return Err(Failure::Timeout);
    }
    if !status.success() || bytes.len() > MAX_WIRE {
        return Err(Failure::Unavailable);
    }
    read?;
    Ok(bytes)
}

#[cfg(not(windows))]
pub(super) fn fetch(_: String, _: Option<String>) -> Result<Vec<u8>, Failure> {
    Err(Failure::Unavailable)
}
