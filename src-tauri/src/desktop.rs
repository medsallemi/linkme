use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use tauri::{
    AppHandle, Manager, Runtime, WebviewWindow, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    capture,
    native::{Foreground, is_foreground},
};

#[derive(Default)]
pub struct Desktop {
    tray: AtomicBool,
    quitting: AtomicBool,
    restore_pending: AtomicBool,
}

impl Desktop {
    pub fn has_tray(&self) -> bool {
        self.tray.load(Ordering::Acquire)
    }

    pub fn quitting(&self) -> bool {
        self.quitting.load(Ordering::Acquire)
    }
}

pub fn restore_library<R: Runtime>(app: &AppHandle<R>) {
    let desktop = app.state::<Desktop>();
    crate::flight::interrupt(app);
    if desktop.quitting() {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        for label in ["capture", "search"] {
            if let Some(popup) = app.get_webview_window(label) {
                let _ = popup.set_always_on_top(false);
            }
        }
        let opening =
            !window.is_visible().unwrap_or(true) || window.is_minimized().unwrap_or(false);
        let _ = window.show();
        let _ = window.unminimize();
        if opening {
            let _ = window.eval("window.dispatchEvent(new Event('linkme-library-open'))");
        }
        let _ = window.set_focus();
    } else {
        desktop.restore_pending.store(true, Ordering::Release);
    }
}

pub struct PopupFocus {
    origin: Mutex<Option<Foreground>>,
    pub restore: AtomicBool,
    pub generation: u32,
    startup: Mutex<PopupStartup>,
    #[cfg(windows)]
    startup_finished: tokio::sync::Notify,
}

#[derive(Clone, Copy)]
struct Presentation {
    focus: bool,
    dim_percent: u8,
    content_height: Option<u32>,
}

#[derive(Default)]
enum PopupStartup {
    #[default]
    Waiting,
    Ready(Option<u32>),
    Prepared(Presentation),
    Revealed,
    Canceled,
}

impl PopupStartup {
    fn prepare(&mut self, presentation: Presentation) -> Option<Presentation> {
        match self {
            Self::Waiting => {
                *self = Self::Prepared(presentation);
                None
            }
            Self::Ready(content_height) => {
                let content_height = *content_height;
                *self = Self::Revealed;
                Some(Presentation {
                    content_height,
                    ..presentation
                })
            }
            _ => None,
        }
    }

    fn ready(&mut self, content_height: Option<u32>) -> Option<Presentation> {
        match *self {
            Self::Waiting => {
                *self = Self::Ready(content_height);
                None
            }
            Self::Prepared(presentation) => {
                *self = Self::Revealed;
                Some(Presentation {
                    content_height,
                    ..presentation
                })
            }
            _ => None,
        }
    }

    fn cancel(&mut self) -> bool {
        if matches!(self, Self::Revealed | Self::Canceled) {
            return false;
        }
        *self = Self::Canceled;
        true
    }
}

impl PopupFocus {
    pub fn new(origin: Option<Foreground>) -> Self {
        static NEXT_GENERATION: AtomicU32 = AtomicU32::new(1);
        Self {
            origin: Mutex::new(origin),
            restore: AtomicBool::new(true),
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            startup: Mutex::default(),
            #[cfg(windows)]
            startup_finished: tokio::sync::Notify::new(),
        }
    }

    pub fn refocus<R: Runtime>(&self, window: &WebviewWindow<R>, origin: Option<Foreground>) {
        if !is_foreground(window)
            && origin.is_some()
            && let Ok(mut saved) = self.origin.lock()
        {
            *saved = origin;
        }
        if self
            .startup
            .lock()
            .is_ok_and(|state| matches!(*state, PopupStartup::Revealed))
        {
            let _ = window.set_always_on_top(true);
            let _ = window.set_focus();
        }
    }

    fn finish_startup(&self) {
        #[cfg(windows)]
        self.startup_finished.notify_one();
    }
}

// A popup may be activated again by clicking it, without invoking its shortcut.
#[cfg(windows)]
fn track_activation<R: Runtime>(
    window: &WebviewWindow<R>,
    focus: Arc<PopupFocus>,
) -> tauri::Result<()> {
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{WM_ACTIVATE, WM_NCDESTROY},
        },
    };
    unsafe extern "system" fn activation(
        hwnd: HWND,
        msg: u32,
        w: WPARAM,
        l: LPARAM,
        id: usize,
        data: usize,
    ) -> LRESULT {
        let ptr = data as *const PopupFocus;
        if msg == WM_ACTIVATE {
            let active = w.0 & 0xffff != 0;
            let other = HWND(l.0 as _);
            let previous = if active && other != hwnd {
                crate::native::foreground_window(other)
            } else {
                None
            };
            // Windows may omit the previous HWND. On a fresh shortcut keep its explicit
            // origin; deactivation has already cleared any older click-through origin.
            if let Ok(mut origin) = unsafe { &*ptr }.origin.lock() {
                record_activation(&mut origin, active, previous);
            }
        } else if msg == WM_NCDESTROY {
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(activation), id);
                drop(Arc::from_raw(ptr));
            }
        }
        unsafe { DefSubclassProc(hwnd, msg, w, l) }
    }
    let target = window.clone();
    window.run_on_main_thread(move || {
        if let Ok(hwnd) = target.hwnd() {
            let ptr = Arc::into_raw(focus);
            if !unsafe { SetWindowSubclass(hwnd, Some(activation), 1, ptr as usize) }.as_bool() {
                unsafe { drop(Arc::from_raw(ptr)) };
            }
        }
    })
}

#[cfg(any(windows, test))]
fn record_activation<T>(origin: &mut Option<T>, active: bool, previous: Option<T>) {
    if !active || previous.is_some() {
        *origin = previous;
    }
}

pub fn show_library_if_requested<R: Runtime>(app: &AppHandle<R>) {
    if app
        .state::<Desktop>()
        .restore_pending
        .swap(false, Ordering::AcqRel)
    {
        restore_library(app);
    }
}

pub fn opens_library<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut startup = false;
    for arg in args {
        match arg.as_ref() {
            "--open-library" => return true,
            "--windows-startup" => startup = true,
            _ => {}
        }
    }
    !startup
}

pub fn install_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open-library", "Open Library", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings, &quit])?;
    TrayIconBuilder::with_id("linkme")
        .icon(tauri::include_image!("./icons/tray-32.png"))
        .tooltip("Linkme")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open-library" => restore_library(app),
            "settings" => {
                restore_library(app);
                capture::request_settings(app);
            }
            "quit" => {
                app.state::<Desktop>()
                    .quitting
                    .store(true, Ordering::Release);
                crate::flight::interrupt(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                restore_library(tray.app_handle());
            }
        })
        .build(app)?;
    app.state::<Desktop>().tray.store(true, Ordering::Release);
    Ok(())
}

pub(crate) fn popup_frame(
    work: (i32, i32, u32, u32),
    logical_size: (f64, f64),
    scale: f64,
    logical_margin: (f64, f64),
) -> [i32; 4] {
    let (left, top, work_width, work_height) = work;
    let fit = |extent: u32, preferred: f64, margin: f64| {
        let available = extent
            .saturating_sub((margin * scale).round() as u32)
            .max(1);
        ((preferred * scale).round() as u32).clamp(1, available)
    };
    let width = fit(work_width, logical_size.0, logical_margin.0);
    let height = fit(work_height, logical_size.1, logical_margin.1);
    [
        left + work_width.saturating_sub(width) as i32 / 2,
        top + work_height.saturating_sub(height) as i32 / 2,
        width as i32,
        height as i32,
    ]
}

pub fn prepare_popup<R: Runtime>(
    window: &WebviewWindow<R>,
    origin: Option<Foreground>,
    size: (f64, f64),
    focus: bool,
    dim_percent: u8,
    return_focus: Arc<PopupFocus>,
) -> tauri::Result<()> {
    let app = window.app_handle();
    let monitor = origin
        .and_then(|o| app.monitor_from_point(o.point.0, o.point.1).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let work = monitor.work_area();
        let [x, y, width, height] = popup_frame(
            (
                work.position.x,
                work.position.y,
                work.size.width,
                work.size.height,
            ),
            size,
            monitor.scale_factor(),
            (
                16.0,
                if window.label() == "capture" {
                    0.0
                } else {
                    24.0
                },
            ),
        );
        crate::panel::apply_frame(window, x, y, width, height)
            .map_err(|_| tauri::Error::WindowNotFound)?;
    }
    if focus {
        #[cfg(windows)]
        track_activation(window, return_focus.clone())?;
    }
    let popup = window.clone();
    let context = return_focus.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { .. }
            if focus && context.restore.load(Ordering::Acquire) && is_foreground(&popup) =>
        {
            if let Some(origin) = context.origin.lock().ok().and_then(|o| *o) {
                origin.restore();
            }
        }
        WindowEvent::Destroyed => {
            if let Ok(mut state) = context.startup.lock() {
                state.cancel();
            }
            context.finish_startup();
        }
        _ => {}
    });
    let reveal = return_focus
        .startup
        .lock()
        .map_err(|_| tauri::Error::WindowNotFound)?
        .prepare(Presentation {
            focus,
            dim_percent,
            content_height: None,
        });
    if let Some(presentation) = reveal {
        return reveal_popup(window, &return_focus, presentation);
    }
    #[cfg(windows)]
    watch_popup_startup(window.clone(), return_focus);
    Ok(())
}

fn reveal_popup<R: Runtime>(
    window: &WebviewWindow<R>,
    context: &PopupFocus,
    presentation: Presentation,
) -> tauri::Result<()> {
    context.finish_startup();
    if let Some(height) = presentation.content_height {
        crate::panel::fit_first_frame(window, height);
    }
    #[cfg(windows)]
    if presentation.focus && presentation.dim_percent > 0 {
        crate::dim::attach(window, presentation.dim_percent);
    }
    window.show()?;
    if presentation.focus {
        let _ = window.set_focus();
    }
    #[cfg(not(windows))]
    let _ = presentation.dim_percent;
    Ok(())
}

#[cfg(windows)]
fn watch_popup_startup<R: Runtime>(window: WebviewWindow<R>, context: Arc<PopupFocus>) {
    tauri::async_runtime::spawn(async move {
        let ready = context.startup_finished.notified();
        if tokio::time::timeout(std::time::Duration::from_secs(5), ready)
            .await
            .is_err()
            && context.startup.lock().is_ok_and(|mut state| state.cancel())
        {
            fail_popup(&window);
        }
    });
}

fn fail_popup<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.destroy();
    let message = if window.label() == "search" {
        "Search could not open. Try the shortcut again or search in the Library."
    } else {
        "Capture could not open. Try the shortcut again."
    };
    capture::notify_popup_failure(window.app_handle(), message);
}

#[tauri::command]
pub async fn popup_ready<R: Runtime>(
    window: WebviewWindow<R>,
    generation: u32,
    content_height: Option<u32>,
) -> Result<(), crate::storage::AppError> {
    let app = window.app_handle();
    let context = match window.label() {
        "capture" => app
            .state::<Arc<capture::CaptureState>>()
            .panel
            .lock()
            .ok()
            .and_then(|panel| panel.as_ref().map(|panel| panel.focus.clone())),
        "search" => app
            .state::<crate::search::SearchState>()
            .0
            .lock()
            .ok()
            .and_then(|context| context.clone()),
        _ => None,
    };
    let Some(context) = context.filter(|context| context.generation == generation) else {
        return Ok(());
    };
    let reveal = context
        .startup
        .lock()
        .map_err(|_| crate::storage::AppError::unexpected())?
        .ready(content_height.filter(|_| window.label() == "capture"));
    if let Some(presentation) = reveal {
        let target = window.clone();
        window
            .run_on_main_thread(move || {
                if reveal_popup(&target, &context, presentation).is_err() {
                    fail_popup(&target);
                }
            })
            .map_err(|_| {
                fail_popup(&window);
                crate::storage::AppError::unexpected()
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_installer_and_secondary_launches_open_library_but_sign_in_does_not() {
        assert!(opens_library([] as [&str; 0]));
        assert!(opens_library(["--open-library"]));
        assert!(opens_library(["linkme.exe", "--open-library"]));
        assert!(!opens_library(["linkme.exe", "--windows-startup"]));
        assert!(opens_library(["--windows-startup", "--open-library"]));
    }

    #[test]
    fn popup_reveals_once_in_either_readiness_order() {
        let presentation = Presentation {
            focus: true,
            dim_percent: 36,
            content_height: None,
        };
        for frontend_first in [false, true] {
            let mut startup = PopupStartup::default();
            let reveal = if frontend_first {
                assert!(startup.ready(Some(412)).is_none());
                startup.prepare(presentation)
            } else {
                assert!(startup.prepare(presentation).is_none());
                startup.ready(Some(412))
            };
            let reveal = reveal.expect("both sides are ready");
            assert!(reveal.focus);
            assert_eq!(reveal.dim_percent, 36);
            assert_eq!(reveal.content_height, Some(412));
            assert!(startup.ready(None).is_none());
            assert!(startup.prepare(presentation).is_none());
            assert!(!startup.cancel());
        }
    }

    #[test]
    fn popup_cancellation_rejects_late_preparation_and_readiness() {
        let presentation = Presentation {
            focus: true,
            dim_percent: 36,
            content_height: None,
        };
        for mut startup in [
            PopupStartup::Waiting,
            PopupStartup::Ready(None),
            PopupStartup::Prepared(presentation),
        ] {
            assert!(startup.cancel());
            assert!(startup.prepare(presentation).is_none());
            assert!(startup.ready(None).is_none());
            assert!(!startup.cancel());
        }
    }

    #[test]
    fn latest_popup_activation_wins_and_unknown_reactivation_cannot_restore_a_stale_app() {
        let mut origin = Some("A");
        record_activation(&mut origin, true, None);
        assert_eq!(origin, Some("A"));
        record_activation(&mut origin, false, None);
        record_activation(&mut origin, true, Some("B"));
        assert_eq!(origin, Some("B"));
        record_activation(&mut origin, false, None);
        record_activation(&mut origin, true, None);
        assert_eq!(origin, None);
    }

    #[test]
    fn positioning_respects_negative_origins_and_small_work_areas() {
        assert_eq!(
            popup_frame((-1200, -100, 1200, 900), (480.0, 320.0), 1.25, (16.0, 24.0)),
            [-900, 150, 600, 400]
        );
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let [x, y, width, height] =
                popup_frame((-840, -500, 640, 420), (520.0, 370.0), scale, (16.0, 24.0));
            assert!(x >= -840 && y >= -500 && x + width <= -200 && y + height <= -80);
            assert!((2 * x + width + 1040).abs() <= 1);
            assert!((2 * y + height + 580).abs() <= 1);
        }
    }
}
