use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State, ipc::Channel};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::{
    SharedStorage, database_task,
    desktop::{self, Desktop},
    models::{CaptureMode, CaptureSettings, LinkDraft, SaveResult},
    native, panel, search,
    storage::AppError,
    validation::validate_url,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConnection {
    settings: CaptureSettings,
    warning: Option<&'static str>,
    show_settings: bool,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CaptureEvent {
    Prompt { url: String },
    Saved { result: SaveResult },
    Error { error: AppError },
    Settings,
}

struct Connected {
    connection: CaptureConnection,
    persisted: CaptureSettings,
    shortcuts: [Option<Shortcut>; 2],
    events: Channel<CaptureEvent>,
}

#[derive(Default)]
pub struct CaptureState {
    busy: AtomicBool,
    settings_pending: AtomicBool,
    connected: Mutex<Option<Connected>>,
    pub panel: Mutex<Option<panel::Panel>>,
}

struct Busy(Arc<CaptureState>);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

impl CaptureState {
    fn claim(self: &Arc<Self>) -> Result<Busy, AppError> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| Busy(self.clone()))
            .map_err(|_| AppError::new("capture_busy", "Capture is busy. Try again in a moment."))
    }
}

fn parse_shortcut(input: &str) -> Result<(String, Shortcut), AppError> {
    let invalid = || {
        AppError::new(
            "invalid_shortcut",
            "Use Ctrl+Shift and one letter or digit, such as Ctrl+Shift+L.",
        )
    };
    if input.len() > 32 || !input.is_ascii() {
        return Err(invalid());
    }
    let (modifiers, key) = input.rsplit_once('+').ok_or_else(invalid)?;
    let key = key.trim();
    if key.len() != 1 || !key.as_bytes()[0].is_ascii_alphanumeric() {
        return Err(invalid());
    }
    let (first, second) = modifiers.split_once('+').ok_or_else(invalid)?;
    let first = first.trim().to_ascii_lowercase();
    let second = second.trim().to_ascii_lowercase();
    // AltGr can generate Ctrl+Alt on Windows while typing text.
    if !matches!(
        (first.as_str(), second.as_str()),
        ("ctrl" | "control", "shift") | ("shift", "ctrl" | "control")
    ) {
        return Err(invalid());
    }
    let normalized = format!("Ctrl+Shift+{}", key.to_ascii_uppercase());
    let shortcut = normalized.parse().map_err(|_| invalid())?;
    Ok((normalized, shortcut))
}

#[tauri::command]
pub async fn connect_capture<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<CaptureState>>,
    storage: State<'_, SharedStorage>,
    events: Channel<CaptureEvent>,
) -> Result<CaptureConnection, AppError> {
    let _busy = state.inner().claim()?;
    {
        let mut lock = state.connected.lock().map_err(|_| AppError::unexpected())?;
        if let Some(connected) = lock.as_mut() {
            connected.events = events;
            return Ok(connected.connection.clone());
        }
    }
    let (mut settings, mut warning) =
        match database_task(storage.inner().clone(), |db| db.capture_settings()).await {
            Ok(settings) => (settings, None),
            Err(_) => (
                CaptureSettings::default(),
                Some("Could not load capture settings. Using the Prompt default."),
            ),
        };
    let persisted = settings.clone();
    let defaults = CaptureSettings::default();
    let parsed = parse_shortcut(&settings.shortcut).or_else(|_| {
        warning = Some("Saved capture shortcut was invalid. Using the default.");
        parse_shortcut(&defaults.shortcut)
    })?;
    let search = parse_shortcut(&settings.search_shortcut).or_else(|_| {
        warning = Some("Saved search shortcut was invalid. Using the default.");
        parse_shortcut(&defaults.search_shortcut)
    })?;
    settings.shortcut = parsed.0;
    settings.search_shortcut = search.0;
    let manager = app.global_shortcut();
    let shortcuts = [
        manager.register(parsed.1).ok().map(|_| parsed.1),
        if parsed.1 != search.1 {
            manager.register(search.1).ok().map(|_| search.1)
        } else {
            None
        },
    ];
    if shortcuts[0].is_none() || shortcuts[1].is_none() {
        warning = Some(match shortcuts {
            [None, None] => {
                "Capture and search shortcuts are unavailable. Choose different shortcuts in Settings."
            }
            [None, _] => "The capture shortcut is unavailable. Choose a different one in Settings.",
            [_, None] => "The search shortcut is unavailable. Choose a different one in Settings.",
            _ => unreachable!(),
        });
    }
    let mut lock = state.connected.lock().map_err(|_| AppError::unexpected())?;
    let connection = CaptureConnection {
        settings,
        warning,
        show_settings: state.settings_pending.swap(false, Ordering::AcqRel),
    };
    *lock = Some(Connected {
        connection: CaptureConnection {
            show_settings: false,
            ..connection.clone()
        },
        persisted,
        shortcuts,
        events,
    });
    Ok(connection)
}

#[tauri::command]
pub async fn update_capture<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Arc<CaptureState>>,
    storage: State<'_, SharedStorage>,
    mut settings: CaptureSettings,
) -> Result<CaptureConnection, AppError> {
    let (name, capture) = parse_shortcut(&settings.shortcut)?;
    let (search_name, search) = parse_shortcut(&settings.search_shortcut)?;
    if capture == search {
        return Err(AppError::new(
            "shortcut_conflict",
            "Choose different shortcuts for capture and search.",
        ));
    }
    settings.shortcut = name;
    settings.search_shortcut = search_name;
    let _busy = state.inner().claim()?;
    let (old, expected) = {
        let lock = state.connected.lock().map_err(|_| AppError::unexpected())?;
        let connected = lock.as_ref().ok_or_else(AppError::unexpected)?;
        (connected.shortcuts, connected.persisted.clone())
    };
    let manager = app.global_shortcut();
    let next = [Some(capture), Some(search)];
    let claimed = reserve_new_keys(
        old,
        [capture, search],
        |key| manager.register(key).is_ok(),
        |key| manager.unregister(key).is_ok(),
    )?;
    let persisted = settings.clone();
    if let Err(error) = database_task(storage.inner().clone(), move |db| {
        db.save_capture_settings(&persisted, &expected)
    })
    .await
    {
        if !release_keys(claimed, |key| manager.unregister(key).is_ok()) {
            return Err(release_error());
        }
        return Err(error);
    }
    let released = release_keys(
        old.into_iter()
            .flatten()
            .filter(|key| !next.contains(&Some(*key))),
        |key| manager.unregister(key).is_ok(),
    );
    let warning =
        (!released).then_some("A previous shortcut could not be released. Quit and reopen Linkme.");
    let connection = CaptureConnection {
        settings,
        warning,
        show_settings: false,
    };
    let mut lock = state.connected.lock().map_err(|_| AppError::unexpected())?;
    let connected = lock.as_mut().ok_or_else(AppError::unexpected)?;
    connected.connection = connection.clone();
    connected.persisted = connection.settings.clone();
    connected.shortcuts = next;
    drop(lock);
    if let Ok(mut panel) = state.panel.lock()
        && let Some(panel) = panel.as_mut()
    {
        panel.settings = connection.settings.clone();
    }
    let appearance = crate::glass::appearance_script(&connection.settings);
    for label in ["capture", "search"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.eval(&appearance);
        }
    }
    for (label, window) in app.webview_windows() {
        if label.starts_with("flight-") {
            let _ = window.eval(&appearance);
        }
    }
    Ok(connection)
}

fn reserve_new_keys(
    old: [Option<Shortcut>; 2],
    next: [Shortcut; 2],
    mut register: impl FnMut(Shortcut) -> bool,
    release: impl FnMut(Shortcut) -> bool,
) -> Result<Vec<Shortcut>, AppError> {
    let mut claimed = Vec::new();
    for (index, key) in next.into_iter().enumerate() {
        if old.contains(&Some(key)) {
            continue;
        }
        if !register(key) {
            if !release_keys(claimed, release) {
                return Err(release_error());
            }
            let message = if index == 0 {
                "The new capture shortcut is unavailable. Previous bindings were kept."
            } else {
                "The new search shortcut is unavailable. Previous bindings were kept."
            };
            return Err(AppError::new("shortcut_conflict", message));
        }
        claimed.push(key);
    }
    Ok(claimed)
}

fn release_keys(
    keys: impl IntoIterator<Item = Shortcut>,
    mut release: impl FnMut(Shortcut) -> bool,
) -> bool {
    let mut success = true;
    for key in keys {
        success &= release(key);
    }
    success
}

fn release_error() -> AppError {
    AppError::new(
        "shortcut_conflict",
        "Could not save settings or release a candidate shortcut. Quit and reopen Linkme.",
    )
}

pub fn request_settings<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<Arc<CaptureState>>();
    if let Ok(lock) = state.connected.lock() {
        if let Some(connection) = lock.as_ref() {
            let _ = connection.events.send(CaptureEvent::Settings);
        } else {
            state.settings_pending.store(true, Ordering::Release);
        }
    }
}

async fn capture(
    storage: SharedStorage,
    mode: CaptureMode,
    clipboard: Result<String, AppError>,
) -> Result<CaptureEvent, AppError> {
    let url = validate_url(&clipboard?).map_err(|message| AppError::new("invalid_url", message))?;
    match mode {
        CaptureMode::Prompt => Ok(CaptureEvent::Prompt { url }),
        CaptureMode::Quick => {
            let draft = LinkDraft {
                url,
                ..LinkDraft::default()
            };
            let result = database_task(storage, move |db| db.save(&draft)).await?;
            Ok(CaptureEvent::Saved { result })
        }
    }
}

pub fn on_shortcut<R: Runtime>(app: &AppHandle<R>, key: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed || app.state::<Desktop>().quitting() {
        return;
    }
    let state = app.state::<Arc<CaptureState>>();
    let Ok(busy) = state.inner().claim() else {
        return;
    };
    let (settings, events, search_key) = {
        let Ok(lock) = state.connected.lock() else {
            return;
        };
        let Some(connected) = lock.as_ref() else {
            return;
        };
        let Some(index) = connected
            .shortcuts
            .iter()
            .position(|binding| binding.as_ref() == Some(key))
        else {
            return;
        };
        (
            connected.connection.settings.clone(),
            connected.events.clone(),
            index == 1,
        )
    };
    let foreground = native::foreground();
    if search_key {
        crate::flight::interrupt(app);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _busy = busy;
            if search::show(&app, foreground, &settings).is_err() {
                let _ = events.send(CaptureEvent::Error {
                    error: AppError::new(
                        "native_action",
                        "Could not open Linkme search. Try again or search in the library.",
                    ),
                });
                desktop::restore_library(&app);
            }
        });
        return;
    }
    if let Some(window) = app.get_webview_window("capture") {
        if let Ok(panel) = state.panel.lock()
            && let Some(panel) = panel.as_ref().filter(|panel| panel.prompt)
        {
            panel.focus.refocus(&window, foreground);
        }
        return;
    }
    let app = app.clone();
    let storage = app.state::<SharedStorage>().inner().clone();
    crate::flight::interrupt(&app);
    let epoch = crate::flight::epoch(&app);
    let sequence = native::clipboard_sequence();
    tauri::async_runtime::spawn(async move {
        let _busy = busy;
        let clipboard =
            tauri::async_runtime::spawn_blocking(move || native::read_capture_clipboard(sequence))
                .await
                .unwrap_or_else(|_| Err(AppError::unexpected()));
        let result = capture(storage, settings.mode, clipboard)
            .await
            .unwrap_or_else(|error| CaptureEvent::Error { error });
        if let CaptureEvent::Saved { result: saved } = &result {
            let _ = events.send(result.clone());
            if saved.created {
                let fallback = app.clone();
                let prefs = settings.clone();
                let message = result.clone();
                crate::flight::present(
                    &app,
                    saved,
                    epoch,
                    crate::flight::Origin::Quick(foreground),
                    &settings,
                    move |shown| {
                        if !shown && fallback.get_webview_window("capture").is_none() {
                            let _ = panel::present(&fallback, message, foreground, &prefs);
                        }
                    },
                );
                return;
            }
        }
        if panel::present(&app, result, foreground, &settings).is_err() {
            let _ = events.send(CaptureEvent::Error {
                error: AppError::new(
                    "native_action",
                    "Could not open the capture window. Reopen Linkme to retry.",
                ),
            });
        }
    });
}

pub fn notify_library<R: Runtime>(app: &AppHandle<R>, result: SaveResult) {
    notify_event(app, CaptureEvent::Saved { result });
}

pub fn notify_popup_failure<R: Runtime>(app: &AppHandle<R>, message: &'static str) {
    notify_event(
        app,
        CaptureEvent::Error {
            error: AppError::new("native_action", message),
        },
    );
}

fn notify_event<R: Runtime>(app: &AppHandle<R>, event: CaptureEvent) {
    let state = app.state::<Arc<CaptureState>>();
    if let Ok(connection) = state.connected.lock()
        && let Some(connection) = connection.as_ref()
    {
        let _ = connection.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use crate::{Storage, storage::Database};

    use super::*;

    #[test]
    fn failed_second_reservation_releases_the_candidate_and_keeps_both_old_keys() {
        let key = |letter: &str| parse_shortcut(&format!("Ctrl+Shift+{letter}")).unwrap().1;
        let old = [Some(key("L")), Some(key("K"))];
        let next = [key("J"), key("M")];
        let mut released = Vec::new();
        let error = reserve_new_keys(
            old,
            next,
            |key| key != next[1],
            |key| {
                released.push(key);
                true
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "shortcut_conflict");
        assert_eq!(released, [next[0]]);
        let swapped = [old[1].unwrap(), old[0].unwrap()];
        assert!(
            reserve_new_keys(
                old,
                swapped,
                |_| panic!("existing key re-registered"),
                |_| false
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn shortcut_requires_ctrl_and_a_second_modifier() {
        for input in [
            "L",
            "Ctrl+L",
            "Ctrl+Alt+L",
            "Ctrl+Alt+Shift+L",
            "Ctrl+Shift+Ctrl+L",
            "Ctrl+Shift+F4",
            "Super+Shift+L",
            "Ctrl+Shift+LL",
        ] {
            assert_eq!(
                parse_shortcut(input).unwrap_err().code,
                "invalid_shortcut",
                "{input}"
            );
        }
        assert_eq!(
            parse_shortcut(" shift + ctrl + 8 ").unwrap().0,
            "Ctrl+Shift+8"
        );
    }

    #[test]
    fn in_flight_capture_excludes_repeat_until_work_finishes() {
        let state = Arc::new(CaptureState::default());
        let permit = state.claim().unwrap();
        assert!(state.claim().is_err());
        drop(permit);
        assert!(state.claim().is_ok());
    }

    #[test]
    fn unavailable_clipboard_and_database_fail_without_preventing_retry() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("blocked");
        std::fs::write(&parent, b"not a directory").unwrap();
        let storage = Arc::new(Mutex::new(Storage {
            path: Ok(parent.join("links.sqlite3")),
            database: None,
        }));
        let url = || Ok("https://example.com/retry".into());
        tauri::async_runtime::block_on(async {
            assert_eq!(
                capture(
                    storage.clone(),
                    CaptureMode::Quick,
                    Err(AppError::new("clipboard", "busy"))
                )
                .await
                .err()
                .unwrap()
                .code,
                "clipboard"
            );
            assert_eq!(
                capture(storage.clone(), CaptureMode::Quick, url())
                    .await
                    .err()
                    .unwrap()
                    .code,
                "database"
            );
            std::fs::remove_file(&parent).unwrap();
            assert!(
                matches!(capture(storage, CaptureMode::Quick, url()).await.unwrap(),
                CaptureEvent::Saved { result } if result.created)
            );
        });
    }

    #[test]
    fn prompt_and_invalid_clipboard_never_write_quick_save_commits_and_deduplicates() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("links.sqlite3");
        let storage = Arc::new(Mutex::new(Storage {
            path: Ok(path.clone()),
            database: None,
        }));
        let url = "https://example.com/a?x=%2F#Exact";
        tauri::async_runtime::block_on(async {
            assert!(
                matches!(capture(storage.clone(), CaptureMode::Prompt, Ok(url.into())).await.unwrap(),
                CaptureEvent::Prompt { url: captured } if captured == url)
            );
            for input in [
                "",
                "javascript:alert(1)",
                "https://example.com\nhttps://example.com",
            ] {
                assert_eq!(
                    capture(storage.clone(), CaptureMode::Quick, Ok(input.into()))
                        .await
                        .err()
                        .unwrap()
                        .code,
                    "invalid_url"
                );
            }
            assert!(!path.exists());
            assert!(
                matches!(capture(storage.clone(), CaptureMode::Quick, Ok(url.into())).await.unwrap(),
                CaptureEvent::Saved { result } if result.created && result.link.category_id == 1 && result.link.url == url)
            );
            assert!(
                matches!(capture(storage.clone(), CaptureMode::Quick, Ok(url.into())).await.unwrap(),
                CaptureEvent::Saved { result } if !result.created)
            );
        });
        drop(storage);
        assert_eq!(
            Database::open(&path)
                .unwrap()
                .list("", None)
                .unwrap()
                .links
                .len(),
            1
        );
    }
}
