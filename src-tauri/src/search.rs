use std::sync::{Arc, Mutex, atomic::Ordering};

use tauri::{AppHandle, Manager, Runtime, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{
    SharedStorage, allowed_navigation, desktop, glass, native::Foreground, open_link,
    storage::AppError,
};

#[derive(Default)]
pub struct SearchState(pub Mutex<Option<Arc<desktop::PopupFocus>>>);

const SEARCH_WIDTH: f64 = 520.0;

pub fn show<R: Runtime>(
    app: &AppHandle<R>,
    origin: Option<Foreground>,
    settings: &crate::models::CaptureSettings,
) -> tauri::Result<()> {
    if let Some(existing) = app.get_webview_window("search") {
        if let Ok(context) = app.state::<SearchState>().0.lock()
            && let Some(context) = context.as_ref()
        {
            context.refocus(&existing, origin);
        }
        return Ok(());
    }
    let size = (SEARCH_WIDTH, 370.0);
    let builder = WebviewWindowBuilder::new(app, "search", WebviewUrl::default())
        .title("Search saved links")
        .decorations(false)
        .inner_size(size.0, size.1)
        .visible(false)
        .focused(false)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .drag_and_drop(false)
        .on_navigation(allowed_navigation)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny);
    let restore = Arc::new(desktop::PopupFocus::new(origin));
    let state = app.state::<SearchState>();
    *state.0.lock().map_err(|_| tauri::Error::WindowNotFound)? = Some(restore.clone());
    let window = match glass::popup(builder, app, settings, restore.generation).build() {
        Ok(window) => window,
        Err(error) => {
            if let Ok(mut stored) = state.0.lock() {
                *stored = None;
            }
            return Err(error);
        }
    };
    if let Err(error) =
        desktop::prepare_popup(&window, origin, size, true, settings.dim_percent, restore)
    {
        let _ = window.destroy();
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub fn resize_search_panel<R: Runtime>(
    window: WebviewWindow<R>,
    results: u32,
) -> Result<(), AppError> {
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let monitor = target
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| target.primary_monitor().ok().flatten());
            let (Some(monitor), Ok(size), Ok(pos)) =
                (monitor, target.outer_size(), target.outer_position())
            else {
                return;
            };
            let work = monitor.work_area();
            let preferred = 230.0 + 56.0 * f64::from(results.min(5));
            let [x, y, width, height] = desktop::popup_frame(
                (
                    work.position.x,
                    work.position.y,
                    work.size.width,
                    work.size.height,
                ),
                (SEARCH_WIDTH, preferred),
                monitor.scale_factor(),
                (16.0, 24.0),
            );
            if [pos.x, pos.y, size.width as i32, size.height as i32] == [x, y, width, height] {
                return;
            }
            let _ = crate::panel::apply_frame(&target, x, y, width, height);
        })
        .map_err(|_| AppError::unexpected())
}

#[tauri::command]
pub async fn open_search_result<R: Runtime>(
    window: WebviewWindow<R>,
    search: State<'_, SearchState>,
    storage: State<'_, SharedStorage>,
    id: i64,
) -> Result<(), AppError> {
    let restore = search
        .0
        .lock()
        .map_err(|_| AppError::unexpected())?
        .clone()
        .ok_or_else(AppError::unexpected)?;
    restore
        .restore
        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
        .map_err(|_| AppError::new("capture_busy", "An item is already opening."))?;
    if let Err(error) = open_link(storage, id).await {
        restore.restore.store(true, Ordering::Release);
        return Err(error);
    }
    window
        .close()
        .map_err(|_| AppError::new("native_action", "Link opened. Use Escape to close search."))
}
