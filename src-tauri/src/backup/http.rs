use std::{
    ffi::c_void,
    ptr,
    time::{Duration, Instant},
};

use url::Url;
use windows::{
    Win32::Networking::WinHttp::*,
    core::{HSTRING, PCWSTR, w},
};

use crate::storage::AppError;

pub(crate) fn network_error() -> AppError {
    AppError::new(
        "native_action",
        "Google Drive is unavailable. Check your connection and try again.",
    )
}

struct Handle(*mut c_void);
impl Handle {
    fn new(raw: *mut c_void) -> Result<Self, AppError> {
        if raw.is_null() {
            Err(network_error())
        } else {
            Ok(Self(raw))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns a valid WinHTTP handle.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

pub(super) fn request(
    method: &str,
    url: &Url,
    token: Option<&str>,
    content_type: Option<&str>,
    body: &[u8],
    limit: usize,
) -> Result<(u32, Vec<u8>), AppError> {
    let host = url.host_str().ok_or_else(network_error)?;
    let allowed = matches!(
        host,
        "oauth2.googleapis.com" | "openidconnect.googleapis.com" | "www.googleapis.com"
    ) || host.ends_with(".googleusercontent.com");
    if !allowed
        || url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || token.is_some_and(|value| value.bytes().any(|byte| !byte.is_ascii_graphic()))
        || (host.ends_with(".googleusercontent.com") && (method != "GET" || token.is_some()))
    {
        return Err(network_error());
    }
    let host = HSTRING::from(host);
    let verb = HSTRING::from(method);
    let path = HSTRING::from(match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    });
    let mut headers = String::new();
    if let Some(token) = token {
        headers.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(content_type) = content_type {
        headers.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    let headers: Vec<_> = headers.encode_utf16().collect();
    let count = u32::try_from(body.len()).map_err(|_| network_error())?;
    let began = Instant::now();
    // SAFETY: parameters and buffers live until each synchronous WinHTTP call returns.
    unsafe {
        let session = Handle::new(WinHttpOpen(
            w!("Linkme"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 5_000, 5_000, 15_000, 10_000).map_err(|_| network_error())?;
        let client = Handle::new(WinHttpConnect(session.0, &host, 443, 0))?;
        let request = Handle::new(WinHttpOpenRequest(
            client.0,
            &verb,
            &path,
            PCWSTR::null(),
            PCWSTR::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes()),
        )
        .map_err(|_| network_error())?;
        WinHttpSendRequest(
            request.0,
            (!headers.is_empty()).then_some(&headers),
            (!body.is_empty()).then_some(body.as_ptr().cast()),
            count,
            count,
            0,
        )
        .map_err(|_| network_error())?;
        WinHttpReceiveResponse(request.0, ptr::null_mut()).map_err(|_| network_error())?;
        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut size,
            ptr::null_mut(),
        )
        .map_err(|_| network_error())?;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let mut read = 0;
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            )
            .map_err(|_| network_error())?;
            if read == 0 {
                return Ok((status, bytes));
            }
            if bytes.len() + read as usize > limit || began.elapsed() > Duration::from_secs(40) {
                return Err(network_error());
            }
            bytes.extend_from_slice(&buffer[..read as usize]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unrelated_domains_before_network_access() {
        let url = Url::parse("https://example.com/test").unwrap();
        assert!(request("GET", &url, Some("secret"), None, &[], 1024).is_err());
        let cdn = Url::parse("https://lh3.googleusercontent.com/test").unwrap();
        assert!(request("GET", &cdn, Some("secret"), None, &[], 1024).is_err());
    }

    #[test]
    #[ignore = "Contacts Google's public HTTPS API without a token"]
    fn windows_google_endpoint_smoke() {
        let endpoint = Url::parse("https://openidconnect.googleapis.com/v1/userinfo").unwrap();
        let (status, _) = request("GET", &endpoint, None, None, &[], 32768).unwrap();
        assert_eq!(status, 401);
    }
}
