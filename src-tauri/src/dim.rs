use std::sync::{Arc, Mutex, OnceLock};

use tauri::{Runtime, WebviewWindow, WindowEvent};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{
            BLACK_BRUSH, GetMonitorInfoW, GetStockObject, HBRUSH, MONITOR_DEFAULTTONEAREST,
            MONITORINFO, MonitorFromWindow,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, IsWindow,
            LWA_ALPHA, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW,
            SetLayeredWindowAttributes, SetWindowPos, ShowWindow, WM_DISPLAYCHANGE, WNDCLASSW,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            WS_POPUP,
        },
    },
    core::{PCWSTR, w},
};

const DIM_CLASS: PCWSTR = w!("LinkmePopupDim");

unsafe extern "system" fn dim_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_DISPLAYCHANGE {
        // Hide stale bounds immediately. The next popup focus or move can recompute them.
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
    // Delegate painting and the standard window lifecycle to Windows.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn dim_class_instance() -> Option<HINSTANCE> {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    // The executable instance and the system's stock brush remain valid for the process lifetime.
    unsafe {
        let instance = HINSTANCE(GetModuleHandleW(PCWSTR::null()).ok()?.0);
        REGISTERED
            .get_or_init(|| {
                RegisterClassW(&WNDCLASSW {
                    hInstance: instance,
                    lpszClassName: DIM_CLASS,
                    lpfnWndProc: Some(dim_window_proc),
                    hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                    ..Default::default()
                }) != 0
            })
            .then_some(instance)
    }
}

#[derive(Default)]
struct Layer {
    raw: isize,
    closing: bool,
}

#[derive(Clone, Copy)]
enum Change {
    Refresh,
    Blur,
    Close,
    Destroy,
}

#[derive(Debug, PartialEq)]
enum Action {
    Refresh(isize),
    Hide(isize),
    Destroy(isize),
}

impl Layer {
    fn install(&mut self, raw: isize) -> bool {
        if self.closing || self.raw != 0 {
            return false;
        }
        self.raw = raw;
        true
    }

    fn change(&mut self, change: Change) -> Option<Action> {
        if matches!(change, Change::Destroy | Change::Close) {
            self.closing = true;
        }
        if self.raw == 0 {
            return None;
        }
        match change {
            Change::Destroy => Some(Action::Destroy(std::mem::take(&mut self.raw))),
            Change::Blur | Change::Close => Some(Action::Hide(self.raw)),
            Change::Refresh if !self.closing => Some(Action::Refresh(self.raw)),
            Change::Refresh => None,
        }
    }
}

fn passive_style() -> windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE {
    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST
}

pub fn attach<R: Runtime>(window: &WebviewWindow<R>, percent: u8) {
    let handle = Arc::new(Mutex::new(Layer::default()));
    let lifecycle_handle = handle.clone();
    let popup_window = window.clone();
    // Queue the listener before creating the independent HWND on the same dispatcher.
    window.on_window_event(move |event| {
        let change = match event {
            WindowEvent::Focused(false) => Change::Blur,
            WindowEvent::Focused(true)
            | WindowEvent::Moved(_)
            | WindowEvent::ScaleFactorChanged { .. } => Change::Refresh,
            WindowEvent::CloseRequested { .. } => Change::Close,
            WindowEvent::Destroyed => Change::Destroy,
            _ => return,
        };
        let action = lifecycle_handle
            .lock()
            .ok()
            .and_then(|mut layer| layer.change(change));
        unsafe {
            match action {
                Some(Action::Refresh(raw)) => {
                    if let Ok(popup) = popup_window.hwnd() {
                        show_behind(HWND(raw as _), popup);
                    }
                }
                Some(Action::Hide(raw)) => {
                    let _ = ShowWindow(HWND(raw as _), SW_HIDE);
                }
                Some(Action::Destroy(raw)) => {
                    let _ = DestroyWindow(HWND(raw as _));
                }
                None => {}
            }
        }
    });

    let popup_window = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(popup) = popup_window.hwnd() else {
            return;
        };
        // The layered surface has no input and is created and destroyed on the window thread.
        unsafe {
            if !IsWindow(Some(popup)).as_bool() || !crate::glass::windows_material::allowed() {
                return;
            }
            let Some(instance) = dim_class_instance() else {
                return;
            };
            let Ok(dim) = CreateWindowExW(
                passive_style(),
                DIM_CLASS,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            ) else {
                return;
            };
            let alpha = (u16::from(percent.min(crate::models::MAX_DIM_PERCENT)) * 255 / 100) as u8;
            if SetLayeredWindowAttributes(dim, COLORREF(0), alpha, LWA_ALPHA).is_err() {
                let _ = DestroyWindow(dim);
                return;
            }
            if !handle
                .lock()
                .is_ok_and(|mut layer| layer.install(dim.0 as isize))
            {
                let _ = DestroyWindow(dim);
                return;
            }
            show_behind(dim, popup);
        }
    });
}

fn show_behind(dim: HWND, popup: HWND) {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // Monitor and z-order are based only on the focused popup.
    unsafe {
        if GetForegroundWindow() != popup || !crate::glass::windows_material::allowed() {
            let _ = ShowWindow(dim, SW_HIDE);
            return;
        }
        let monitor = MonitorFromWindow(popup, MONITOR_DEFAULTTONEAREST);
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let bounds = info.rcMonitor;
            if SetWindowPos(
                dim,
                Some(popup),
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
            .is_ok()
            {
                return;
            }
        }
        let _ = ShowWindow(dim, SW_HIDE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dim_style_is_input_transparent_and_nonactivating_even_before_cleanup() {
        let flags = passive_style();
        for required in [
            WS_EX_LAYERED,
            WS_EX_TRANSPARENT,
            WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
        ] {
            assert!(flags.contains(required));
        }
    }

    #[test]
    fn dim_blur_refocus_close_destroy_and_late_creation() {
        let mut layer = Layer::default();
        assert!(layer.install(31));
        assert!(!layer.install(32));
        assert_eq!(layer.change(Change::Blur), Some(Action::Hide(31)));
        assert_eq!(layer.change(Change::Refresh), Some(Action::Refresh(31)));
        assert_eq!(layer.change(Change::Close), Some(Action::Hide(31)));
        assert_eq!(layer.change(Change::Refresh), None);
        assert_eq!(layer.change(Change::Destroy), Some(Action::Destroy(31)));
        assert_eq!(layer.change(Change::Destroy), None);
        assert!(!layer.install(33));
        let mut late = Layer::default();
        assert_eq!(late.change(Change::Close), None);
        assert!(!late.install(34));
    }
}
