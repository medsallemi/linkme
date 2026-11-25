#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    if tauri::webview_version().is_err() {
        start_error(
            "Linkme needs the Microsoft Edge WebView2 Runtime. Install or repair it from Microsoft, then reopen Linkme.",
        );
        return std::process::ExitCode::FAILURE;
    }
    if linkme_lib::run().is_err() {
        start_error(
            "Linkme could not start. Try reopening it. If the problem continues, repair Microsoft Edge WebView2.",
        );
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(windows)]
fn start_error(message: &str) {
    use windows::{
        Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
        core::{HSTRING, w},
    };
    let message = HSTRING::from(message);
    // The temporary UTF-16 message lives for the synchronous message box call.
    unsafe { MessageBoxW(None, &message, w!("Linkme"), MB_OK | MB_ICONERROR) };
}

#[cfg(not(windows))]
fn start_error(message: &str) {
    eprintln!("{message}");
}
