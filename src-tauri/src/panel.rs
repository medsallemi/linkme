use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use tauri::{AppHandle, Manager, Runtime, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{
    SharedStorage, allowed_navigation,
    capture::{CaptureEvent, CaptureState, notify_library},
    database_task, desktop, glass,
    models::{CaptureSettings, LinkDraft, SaveResult},
    native::Foreground,
    storage::AppError,
};

pub struct Panel {
    resize_generation: Arc<AtomicU64>,
    pub prompt: bool,
    pub focus: Arc<desktop::PopupFocus>,
    initial: Option<CaptureEvent>,
    pub settings: CaptureSettings,
}

const CAPTURE_WIDTH: f64 = 440.0;

pub fn present<R: Runtime>(
    app: &AppHandle<R>,
    event: CaptureEvent,
    foreground: Option<Foreground>,
    settings: &CaptureSettings,
) -> tauri::Result<()> {
    let prompt = matches!(event, CaptureEvent::Prompt { .. });
    let duplicate = matches!(&event, CaptureEvent::Saved { result } if !result.created);
    let focusable = prompt || duplicate || matches!(&event, CaptureEvent::Error { .. });
    let size = (
        CAPTURE_WIDTH,
        if prompt {
            358.0
        } else if duplicate {
            235.0
        } else {
            164.0
        },
    );
    let focus = Arc::new(desktop::PopupFocus::new(foreground));
    let state = app.state::<Arc<CaptureState>>();
    let Ok(mut panel) = state.panel.lock() else {
        return Err(tauri::Error::WindowNotFound);
    };
    if panel.is_some() {
        return Err(tauri::Error::WindowNotFound);
    }
    *panel = Some(Panel {
        resize_generation: Arc::default(),
        prompt,
        focus: focus.clone(),
        initial: Some(event),
        settings: settings.clone(),
    });
    drop(panel);

    let builder = WebviewWindowBuilder::new(app, "capture", WebviewUrl::default())
        .title(if prompt {
            "Capture link"
        } else {
            "Link capture"
        })
        .decorations(false)
        .inner_size(size.0, size.1)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .visible(false)
        .focused(false)
        .focusable(focusable)
        .skip_taskbar(true)
        .always_on_top(true)
        .drag_and_drop(false)
        .on_navigation(allowed_navigation)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny);
    let window = glass::popup(builder, app, settings, focus.generation).build();

    let window = match window {
        Ok(window) => window,
        Err(error) => {
            if let Ok(mut panel) = state.panel.lock() {
                *panel = None;
            }
            return Err(error);
        }
    };
    let dim_percent = if prompt { settings.dim_percent } else { 0 };
    if let Err(error) =
        desktop::prepare_popup(&window, foreground, size, focusable, dim_percent, focus)
    {
        let _ = window.destroy();
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub fn take_capture_panel(state: State<'_, Arc<CaptureState>>) -> Result<CaptureEvent, AppError> {
    state
        .panel
        .lock()
        .map_err(|_| AppError::unexpected())?
        .as_mut()
        .and_then(|panel| panel.initial.take())
        .ok_or_else(AppError::unexpected)
}

#[tauri::command]
pub async fn dismiss_capture_panel<R: Runtime>(window: WebviewWindow<R>) -> Result<(), AppError> {
    window.close().map_err(|_| AppError::unexpected())
}

fn require_capture_title(title: &str) -> Result<(), AppError> {
    if title.trim().is_empty() {
        return Err(AppError::new(
            "invalid_metadata",
            "Enter a title to save this link.",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn save_panel_link<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<CaptureState>>,
    storage: State<'_, SharedStorage>,
    window: WebviewWindow<R>,
    input: LinkDraft,
    anchor: Option<[f64; 2]>,
    presentation: tauri::ipc::Channel<bool>,
) -> Result<SaveResult, AppError> {
    let settings = state
        .panel
        .lock()
        .map_err(|_| AppError::unexpected())?
        .as_ref()
        .filter(|panel| panel.prompt && input.id.is_none() && input.revision.is_none())
        .map(|panel| panel.settings.clone())
        .ok_or_else(|| AppError::new("native_action", "Open a new capture to save a link."))?;
    require_capture_title(&input.title)?;
    let epoch = crate::flight::epoch(&app);
    let result = database_task(storage.inner().clone(), move |db| db.save(&input)).await?;
    notify_library(&app, result.clone());
    if !result.created {
        return Ok(result);
    }
    // Accept only a point inside this WebView, never a desktop position from IPC.
    let origin = window
        .inner_position()
        .ok()
        .zip(window.inner_size().ok())
        .zip(window.scale_factor().ok())
        .map(|((pos, size), scale)| {
            let [x, y] = anchor.unwrap_or([0.5 * f64::from(size.width) / scale, 32.0]);
            crate::flight::Origin::Chip {
                size: (
                    f64::from(size.width) / scale,
                    f64::from(size.height) / scale,
                ),
                center: (
                    f64::from(pos.x) + (x * scale).clamp(0.0, f64::from(size.width)),
                    f64::from(pos.y) + (y * scale).clamp(0.0, f64::from(size.height)),
                ),
            }
        })
        .unwrap_or_else(|| crate::flight::Origin::Quick(crate::native::foreground()));
    crate::flight::present(&app, &result, epoch, origin, &settings, move |shown| {
        let _ = presentation.send(shown);
    });
    Ok(result)
}

fn panel_height(expanded: bool, content_height: Option<u32>, available: f64) -> f64 {
    let baseline: f64 = if expanded { 582.0 } else { 358.0 };
    let requested =
        content_height.map_or(baseline, |height| f64::from(height).clamp(358.0, 1800.0));
    requested.min((available - 24.0).max(1.0))
}

pub(crate) fn fit_first_frame<R: Runtime>(window: &WebviewWindow<R>, height: u32) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let preferred = panel_height(false, Some(height), f64::from(work.size.height) / scale);
    let [x, y, width, height] = desktop::popup_frame(
        (
            work.position.x,
            work.position.y,
            work.size.width,
            work.size.height,
        ),
        (CAPTURE_WIDTH, preferred),
        scale,
        (16.0, 0.0),
    );
    if window
        .outer_position()
        .ok()
        .zip(window.outer_size().ok())
        .is_some_and(|(pos, size)| {
            [pos.x, pos.y, size.width as i32, size.height as i32] == [x, y, width, height]
        })
    {
        return;
    }
    let _ = apply_frame(window, x, y, width, height);
}

// Interpolate the actual frame so its opposite edges reach the monitor center together.
fn resize_frame(start: [i32; 4], end: [i32; 4], elapsed: f64) -> [i32; 4] {
    let t = elapsed.clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - t).powi(3);
    std::array::from_fn(|i| {
        (f64::from(start[i]) + (f64::from(end[i]) - f64::from(start[i])) * eased).round() as i32
    })
}

#[tauri::command]
pub async fn resize_capture_panel<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, Arc<CaptureState>>,
    expanded: bool,
    content_height: Option<u32>,
    animate: bool,
) -> Result<(), AppError> {
    let (owner, generation) = {
        let panel = state.panel.lock().map_err(|_| AppError::unexpected())?;
        let token = &panel
            .as_ref()
            .filter(|p| p.prompt)
            .ok_or_else(AppError::unexpected)?
            .resize_generation;
        (
            Arc::downgrade(token),
            token.fetch_add(1, Ordering::Relaxed) + 1,
        )
    };
    let monitor = window
        .current_monitor()
        .map_err(|_| AppError::unexpected())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(AppError::unexpected)?;
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let height = panel_height(
        expanded,
        content_height,
        f64::from(work.size.height) / scale,
    );
    let end = desktop::popup_frame(
        (
            work.position.x,
            work.position.y,
            work.size.width,
            work.size.height,
        ),
        (CAPTURE_WIDTH, height),
        scale,
        (16.0, 0.0),
    );
    let pos = window
        .outer_position()
        .map_err(|_| AppError::unexpected())?;
    let size = window.outer_size().map_err(|_| AppError::unexpected())?;
    let start = [pos.x, pos.y, size.width as i32, size.height as i32];
    if start == end {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let begin = std::time::Instant::now();
        loop {
            let t = if animate {
                begin.elapsed().as_secs_f64() / 0.23
            } else {
                1.0
            };
            let [x, y, width, height] = resize_frame(start, end, t);
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let target = window.clone();
            let owner = owner.clone();
            window
                .run_on_main_thread(move || {
                    let current = owner
                        .upgrade()
                        .is_some_and(|g| g.load(Ordering::Relaxed) == generation);
                    let result = if current {
                        apply_frame(&target, x, y, width, height).map(|()| true)
                    } else {
                        Ok(false)
                    };
                    let _ = sender.send(result);
                })
                .map_err(|_| AppError::unexpected())?;
            if !receiver.recv().map_err(|_| AppError::unexpected())?? || t >= 1.0 {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    })
    .await
    .map_err(|_| AppError::unexpected())?
}

pub(crate) fn apply_frame<R: Runtime>(
    window: &WebviewWindow<R>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};
        let hwnd = window.hwnd().map_err(|_| AppError::unexpected())?;
        unsafe {
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        }
        .map_err(|_| AppError::unexpected())
    }
    #[cfg(not(windows))]
    {
        window
            .set_size(tauri::PhysicalSize::new(width as u32, height as u32))
            .and_then(|()| window.set_position(tauri::PhysicalPosition::new(x, y)))
            .map_err(|_| AppError::unexpected())
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    #[test]
    fn prompt_title_must_contain_non_whitespace() {
        assert!(require_capture_title("").is_err());
        assert!(require_capture_title(" \t\u{2003}").is_err());
        assert!(require_capture_title(" A saved title ").is_ok());
    }

    #[test]
    fn expansion_and_collapse_keep_the_center_on_negative_origin_monitors() {
        let small = [-800, -29, 440, 358];
        let large = [-760, -141, 360, 582];
        for (from, to) in [(small, large), (large, small)] {
            for step in 0..=12 {
                let [x, y, w, h] = resize_frame(from, to, f64::from(step) / 12.);
                assert!((2 * x + w + 1160).abs() <= 1);
                assert!((2 * y + h - 300).abs() <= 1);
            }
            assert_eq!(resize_frame(from, to, 1.), to);
        }
    }
    #[test]
    fn details_expand_and_fit_short_scaled_work_areas() {
        assert_eq!(panel_height(false, None, 800.0), 358.0);
        assert_eq!(panel_height(true, None, 800.0), 582.0);
        assert_eq!(panel_height(true, None, 400.0), 376.0);
        assert_eq!(panel_height(true, Some(780), 800.0), 776.0);
        assert_eq!(panel_height(false, Some(428), 800.0), 428.0);
        assert_eq!(panel_height(true, Some(u32::MAX), 4000.0), 1800.0);
    }
}
