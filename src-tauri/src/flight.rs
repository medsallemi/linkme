use crate::{
    allowed_navigation,
    desktop::Desktop,
    models::{CaptureSettings, SaveResult},
    native::Foreground,
};
use std::{
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindowBuilder,
};

const LOAD_LIMIT: Duration = Duration::from_millis(1800);
// Leave headroom after the 1840 ms animated scene for WebView event dispatch.
const VISIBLE_LIMIT: Duration = Duration::from_millis(2000);

#[derive(Clone, Copy)]
enum Signal {
    Loaded,
    Interrupt,
}

#[derive(Default)]
struct Slot {
    id: u64,
    sender: Option<Sender<Signal>>,
}

impl Slot {
    fn interrupt(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Signal::Interrupt);
        }
        self.id = self.id.wrapping_add(1);
    }
    fn begin(&mut self, expected: u64) -> Option<(u64, Sender<Signal>, Receiver<Signal>)> {
        if self.id != expected {
            return None;
        }
        self.interrupt();
        let (tx, rx) = mpsc::channel();
        self.sender = Some(tx.clone());
        Some((self.id, tx, rx))
    }
    fn finish(&mut self, id: u64) {
        if self.id == id {
            self.sender = None;
        }
    }
}

#[derive(Default)]
pub struct Flights {
    slot: Mutex<Slot>,
    // Only the presenter owns a WebView. A superseded worker never destroys its successor.
    presenter: Mutex<()>,
}

pub fn epoch<R: Runtime>(app: &AppHandle<R>) -> u64 {
    app.state::<Flights>()
        .slot
        .lock()
        .map_or(u64::MAX, |slot| slot.id)
}

pub fn interrupt<R: Runtime>(app: &AppHandle<R>) {
    if let Ok(mut slot) = app.state::<Flights>().slot.lock() {
        slot.interrupt();
    }
}

pub fn active<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.state::<Flights>()
        .slot
        .lock()
        .map_or(true, |slot| slot.sender.is_some())
}

fn finish<R: Runtime>(app: &AppHandle<R>, id: u64) {
    if let Ok(mut slot) = app.state::<Flights>().slot.lock() {
        slot.finish(id);
    }
    crate::updates::desktop_ready(app);
}

#[derive(Clone, Copy)]
pub enum Origin {
    Chip {
        center: (f64, f64),
        size: (f64, f64),
    }, // physical screen pixels
    Quick(Option<Foreground>),
}

#[derive(Debug)]
struct Route {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    start: [f64; 2],
    end_x: f64,
}

fn route(work: (i32, i32, u32, u32), scale: f64, chip: Option<(f64, f64)>) -> Option<Route> {
    let (left, top, width, height) = work;
    if width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let (left, top, w, h) = (
        f64::from(left),
        f64::from(top),
        f64::from(width),
        f64::from(height),
    );
    let target = left + w / 2.0;
    let (x, y) = chip.unwrap_or((target, top + h * 0.50));
    let (x, y) = (x.clamp(left, left + w), y.clamp(top, top + h));
    let x0 = (target.min(x) - 244.0 * scale).max(left).floor();
    let x1 = (target.max(x) + 244.0 * scale).min(left + w).ceil();
    let y1 = (y + 310.0 * scale)
        .max(top + 90.0 * scale)
        .min(top + h)
        .ceil();
    let size = PhysicalSize::new((x1 - x0) as u32, (y1 - top) as u32);
    Some(Route {
        position: PhysicalPosition::new(x0 as i32, top as i32),
        start: [
            (x - x0) / f64::from(size.width),
            (y - top) / f64::from(size.height),
        ],
        end_x: (target - x0) / f64::from(size.width),
        size,
    })
}

fn committed_labels(result: &SaveResult) -> Option<(String, String)> {
    if !result.created {
        return None;
    }
    let host = url::Url::parse(&result.link.url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_default();
    Some((host, result.link.category_name.clone()))
}

fn wait_for_expiry(signals: &Receiver<Signal>, duration: Duration) {
    let deadline = std::time::Instant::now() + duration;
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        if !matches!(signals.recv_timeout(remaining), Ok(Signal::Loaded)) {
            break;
        }
    }
}

fn show_passive<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_SHOWWINDOW, SetWindowPos, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
        };
        let hwnd = window.hwnd().ok()?;
        let required = (WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE).0;
        // Refuse to reveal any surface whose actual native input policy is incomplete.
        if unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & required != required {
            return None;
        }
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
        .ok()
    }
    #[cfg(not(windows))]
    {
        window.show().ok()
    }
}

// Only committed inserts are eligible. The callback reports presentation, never storage success.
pub fn present<R: Runtime>(
    app: &AppHandle<R>,
    result: &SaveResult,
    expected: u64,
    origin: Origin,
    settings: &CaptureSettings,
    ready: impl FnOnce(bool) + Send + 'static,
) {
    let Some((host, category)) = committed_labels(result) else {
        return;
    };
    let state = app.state::<Flights>();
    let Ok(mut slot) = state.slot.lock() else {
        ready(false);
        return;
    };
    let Some((id, loaded, signals)) = slot.begin(expected) else {
        return;
    };
    drop(slot);
    let app = app.clone();
    let settings = settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Flights>();
        let Ok(_owner) = state.presenter.lock() else {
            ready(false);
            return;
        };
        let current = || {
            state.slot.lock().is_ok_and(|slot| slot.id == id) && !app.state::<Desktop>().quitting()
        };
        if !current() {
            return;
        }
        let point = match origin {
            Origin::Chip { center, .. } => Some(center),
            Origin::Quick(foreground) => foreground.map(|f| f.point),
        };
        let monitor = point
            .and_then(|(x, y)| app.monitor_from_point(x, y).ok().flatten())
            .or_else(|| app.primary_monitor().ok().flatten());
        let route = monitor.and_then(|monitor| {
            let work = monitor.work_area();
            route(
                (
                    work.position.x,
                    work.position.y,
                    work.size.width,
                    work.size.height,
                ),
                monitor.scale_factor(),
                match origin {
                    Origin::Chip { center, .. } => Some(center),
                    Origin::Quick(_) => None,
                },
            )
        });
        let Some(route) = route else {
            finish(&app, id);
            if current() {
                ready(false);
            }
            return;
        };
        let window = WebviewWindowBuilder::new(
            &app,
            format!("flight-{id}"),
            WebviewUrl::App("flight.html".into()),
        )
        .title("Saved link")
        .visible(false)
        .focused(false)
        .focusable(false)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .drag_and_drop(false)
        .on_navigation(allowed_navigation)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .on_page_load(move |_, evt| {
            if evt.event() == tauri::webview::PageLoadEvent::Finished
                && evt.url().path().ends_with("/flight.html")
            {
                let _ = loaded.send(Signal::Loaded);
            }
        })
        .build();
        let Ok(window) = window else {
            finish(&app, id);
            if current() {
                ready(false);
            }
            return;
        };
        let shown = (|| -> Option<()> {
            window.set_ignore_cursor_events(true).ok()?;
            window.set_position(route.position).ok()?;
            window.set_size(route.size).ok()?;
            if !matches!(signals.recv_timeout(LOAD_LIMIT), Ok(Signal::Loaded)) || !current() {
                return None;
            }
            show_passive(&window)?;
            let data = serde_json::json!({
                "host": host,
                "category": category,
                "start": route.start,
                "endX": route.end_x,
                "highlight": settings.highlight,
                "transparency": settings.panel_transparency,
                "sourceSize": match origin {Origin::Chip {size,..} => Some([size.0,size.1]), Origin::Quick(_) => None},
                "glass": crate::glass::material_allowed()
            });
            window
                .eval(format!(
                    "window.linkmeScene={data};window.dispatchEvent(new Event('linkme-flight'))"
                ))
                .ok()?;
            Some(())
        })()
        .is_some();
        if current() {
            ready(shown);
        }
        if shown {
            // Native deadline also applies if WebView2 stalls, drops frames or never runs CSS.
            wait_for_expiry(&signals, VISIBLE_LIMIT);
        }
        let _ = window.destroy();
        finish(&app, id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_commit_results_drive_only_new_insert_flights() {
        use crate::{models::LinkDraft, storage::Database};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("links.sqlite3");
        let mut db = Database::open(&path).unwrap();
        let mut draft = LinkDraft {
            url: "https://example.com/private?secret=123".into(),
            new_category: Some("Reading".into()),
            ..Default::default()
        };
        let result = db.save(&draft).unwrap();
        assert_eq!(
            committed_labels(&result),
            Some(("example.com".into(), "Reading".into()))
        );
        let observer = Database::open(&path).unwrap();
        assert_eq!(observer.list("", None).unwrap().links.len(), 1);
        assert_eq!(committed_labels(&db.save(&draft).unwrap()), None);
        draft.url = "invalid".into();
        assert!(db.save(&draft).is_err());
        draft.new_category = None;
        for n in 0..6 {
            draft.url = format!("https://example.org/{n}");
            assert!(committed_labels(&db.save(&draft).unwrap()).is_some());
        }
        assert_eq!(observer.list("", None).unwrap().links.len(), 7);
    }

    #[test]
    fn native_deadline_survives_extra_load_events_and_wakes_on_interrupt() {
        let (tx, rx) = mpsc::channel();
        tx.send(Signal::Loaded).unwrap();
        wait_for_expiry(&rx, Duration::from_millis(2));
        tx.send(Signal::Interrupt).unwrap();
        wait_for_expiry(&rx, Duration::from_secs(60));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn latest_save_interrupts_even_before_load_and_stale_cleanup_cannot_cancel_it() {
        let mut slot = Slot::default();
        let (first, late, old) = slot.begin(slot.id).unwrap();
        let (second, _, new) = slot.begin(slot.id).unwrap();
        late.send(Signal::Loaded).unwrap();
        assert!(matches!(old.recv(), Ok(Signal::Interrupt)));
        assert!(slot.begin(first).is_none()); // A late commit from the preceding invocation.
        slot.finish(first);
        assert_eq!(slot.id, second);
        assert!(slot.sender.is_some());
        slot.interrupt();
        assert!(matches!(new.recv(), Ok(Signal::Interrupt)));
        slot.finish(second);
        assert!(slot.sender.is_none());
    }

    #[test]
    fn route_reconstructs_chip_and_targets_center_on_negative_mixed_dpi_monitor() {
        let r = route((-1920, -50, 1920, 1040), 1.5, Some((-875.0, 312.0))).unwrap();
        let x = |fraction| f64::from(r.position.x) + fraction * f64::from(r.size.width);
        assert_eq!(x(r.start[0]), -875.0);
        assert_eq!(x(r.end_x), -960.0);
        assert_eq!(
            f64::from(r.position.y) + r.start[1] * f64::from(r.size.height),
            312.0
        );
        assert!(r.size.width < 1920 && r.size.height < 1040);
        let tiny = route((10, 20, 80, 60), 2.0, None).unwrap();
        assert_eq!(tiny.size, PhysicalSize::new(80, 60));
        assert!(route((0, 0, 0, 0), 1.0, None).is_none());
    }
}
