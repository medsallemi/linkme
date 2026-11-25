use crate::storage::AppError;

#[cfg(windows)]
pub fn system_curl() -> Option<std::path::PathBuf> {
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut directory = [0u16; 260];
    // SAFETY: the supplied buffer is writable for the length passed by the binding.
    let count = unsafe { GetSystemDirectoryW(Some(&mut directory)) } as usize;
    (count != 0 && count < directory.len()).then(|| {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};
        std::path::PathBuf::from(OsString::from_wide(&directory[..count])).join("curl.exe")
    })
}

#[derive(Clone, Copy)]
pub struct Foreground {
    pub point: (f64, f64),
    #[cfg(windows)]
    handle: isize,
    #[cfg(windows)]
    process: u32,
}

pub fn foreground() -> Option<Foreground> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        foreground_window(unsafe { GetForegroundWindow() })
    }
    #[cfg(not(windows))]
    None
}

#[cfg(windows)]
pub(crate) fn foreground_window(handle: windows::Win32::Foundation::HWND) -> Option<Foreground> {
    use windows::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{GetWindowRect, GetWindowThreadProcessId},
    };
    let mut bounds = RECT::default();
    let mut process = 0;
    unsafe { GetWindowRect(handle, &mut bounds) }.ok()?;
    unsafe { GetWindowThreadProcessId(handle, Some(&mut process)) };
    (process != 0).then_some(Foreground {
        handle: handle.0 as isize,
        process,
        point: (
            (f64::from(bounds.left) + f64::from(bounds.right)) / 2.,
            (f64::from(bounds.top) + f64::from(bounds.bottom)) / 2.,
        ),
    })
}

pub fn is_foreground<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        window
            .hwnd()
            .is_ok_and(|handle| handle.0 == unsafe { GetForegroundWindow() }.0)
    }
    #[cfg(not(windows))]
    window.is_focused().unwrap_or(false)
}

impl Foreground {
    pub fn restore(self) {
        #[cfg(windows)]
        {
            use windows::Win32::{
                Foundation::HWND,
                UI::WindowsAndMessaging::{
                    GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
                },
            };
            let handle = HWND(self.handle as _);
            let mut process = 0;
            // Windows can deny activation. Only try the original live, visible process.
            unsafe {
                GetWindowThreadProcessId(handle, Some(&mut process));
                if process != 0
                    && process == self.process
                    && IsWindowVisible(handle).as_bool()
                    && !IsIconic(handle).as_bool()
                {
                    let _ = SetForegroundWindow(handle);
                }
            }
        }
    }
}

pub fn clipboard_sequence() -> u32 {
    #[cfg(windows)]
    return clipboard_win::seq_num().map_or(0, std::num::NonZeroU32::get);
    #[cfg(not(windows))]
    0
}

pub fn read_capture_clipboard(sequence: u32) -> Result<String, AppError> {
    #[cfg(windows)]
    {
        use clipboard_win::{Clipboard, Getter, formats, is_format_avail, size};
        let unavailable = || {
            AppError::new(
                "clipboard",
                "Clipboard text is unavailable. Copy a link, then try again.",
            )
        };
        let _clipboard = Clipboard::new().map_err(|_| unavailable())?;
        if clipboard_sequence() != sequence {
            return Err(AppError::new(
                "clipboard",
                "The clipboard changed during capture. Try the shortcut again.",
            ));
        }
        if !is_format_avail(formats::CF_UNICODETEXT) {
            return Err(AppError::new(
                "clipboard",
                "The clipboard contains no text. Copy a link first.",
            ));
        }
        // Bound the native allocation before converting untrusted UTF-16 to UTF-8.
        if size(formats::CF_UNICODETEXT).ok_or_else(unavailable)?.get() > 32 * 1024 {
            return Err(AppError::new(
                "invalid_url",
                "Keep the URL within 8,192 UTF-8 bytes.",
            ));
        }
        let mut value = Vec::new();
        formats::RawData(formats::CF_UNICODETEXT)
            .read_clipboard(&mut value)
            .map_err(|_| unavailable())?;
        decode_clipboard_text(&value).ok_or_else(unavailable)
    }
    #[cfg(not(windows))]
    Err(AppError::new(
        "clipboard",
        "Clipboard capture is currently supported on Windows only.",
    ))
}

#[cfg(any(windows, test))]
fn decode_clipboard_text(bytes: &[u8]) -> Option<String> {
    let mut text = Vec::new();
    for unit in bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(u16::from_le_bytes)
    {
        if unit == 0 {
            return String::from_utf16(&text).ok();
        }
        text.push(unit);
    }
    None
}

pub fn copy_url(url: &str) -> Result<(), AppError> {
    #[cfg(windows)]
    return clipboard_win::set_clipboard_string(url).map_err(|_| {
        AppError::new(
            "native_action",
            "Could not copy the URL. The clipboard may be busy; try again.",
        )
    });
    #[cfg(not(windows))]
    Err(AppError::new(
        "native_action",
        "Copy is currently supported on Windows only.",
    ))
}

fn share_address(url: &str) -> String {
    let body: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
    format!("mailto:?body={}", body.replace('+', "%20"))
}
pub fn share_email(url: &str) -> Result<(), AppError> {
    launch_uri(&share_address(url)).map_err(|_| {
        AppError::new(
            "native_action",
            "Could not open your email app. Use Copy link instead.",
        )
    })
}
pub fn open_url(url: &str) -> Result<(), AppError> {
    launch_uri(url)
}
fn launch_uri(url: &str) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        use windows::{
            Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
            core::{HSTRING, PCWSTR, w},
        };
        let target = HSTRING::from(url);
        // Callers supply a validated HTTP(S) URL or an internally constructed mailto URI.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as usize > 32 {
            return Ok(());
        }
        Err(AppError::new(
            "native_action",
            "Could not open the URL. Check your default browser and try again.",
        ))
    }
    #[cfg(not(windows))]
    Err(AppError::new(
        "native_action",
        "Opening URLs is currently supported on Windows only.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_draft_has_only_one_encoded_body() {
        let saved = "https://example.org/?q=a+b&bcc=test@example.org#part";
        let target = url::Url::parse(&share_address(saved)).unwrap();
        assert_eq!(target.scheme(), "mailto");
        assert_eq!(
            target.query_pairs().collect::<Vec<_>>(),
            vec![("body".into(), saved.into())]
        );
    }
    #[test]
    fn clipboard_unicode_requires_termination_and_rejects_invalid_surrogates() {
        assert_eq!(
            decode_clipboard_text(&[65, 0, 0, 0, 255, 255]).as_deref(),
            Some("A")
        );
        assert_eq!(decode_clipboard_text(&[65, 0]), None);
        assert_eq!(decode_clipboard_text(&[0, 216, 0, 0]), None);
    }
}
