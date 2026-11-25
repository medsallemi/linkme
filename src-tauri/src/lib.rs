#[cfg(windows)]
mod backup;
mod capture;
mod desktop;
#[cfg(windows)]
mod dim;
mod flight;
mod glass;
mod models;
mod native;
mod panel;
mod preview;
mod search;
mod startup;
mod storage;
mod updates;
mod validation;

use capture::{CaptureState, connect_capture, update_capture};
use models::{Category, Cursor, LinkDraft, LinkPage, SaveResult};
use panel::{dismiss_capture_panel, resize_capture_panel, save_panel_link, take_capture_panel};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use storage::{AppError, Database};
use tauri::{Manager, State};

struct Storage {
    path: Result<PathBuf, AppError>,
    database: Option<Database>,
}

type SharedStorage = Arc<Mutex<Storage>>;

async fn database_task<T: Send + 'static>(
    storage: SharedStorage,
    operation: impl FnOnce(&mut Database) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut storage = storage.lock().map_err(|_| AppError::unexpected())?;
        if storage.database.is_none() {
            let path = storage.path.as_ref().map_err(Clone::clone)?;
            let parent = path.parent().ok_or_else(AppError::database)?;
            std::fs::create_dir_all(parent).map_err(|_| AppError::database())?;
            storage.database = Some(Database::open(path)?);
        }
        operation(storage.database.as_mut().ok_or_else(AppError::unexpected)?)
    })
    .await
    .map_err(|_| AppError::unexpected())?
}

#[tauri::command]
async fn reset_linkme<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    storage: State<'_, SharedStorage>,
) -> Result<(), AppError> {
    window.clear_all_browsing_data().map_err(|_| {
        AppError::new(
            "reset_cache",
            "Could not clear Linkme's web cache. Try again.",
        )
    })?;
    let shared = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut storage = shared.lock().map_err(|_| AppError::unexpected())?;
        let path = storage.path.clone()?;
        if storage.database.is_none() {
            storage.database = Some(Database::open(&path)?);
        }
        storage
            .database
            .as_mut()
            .ok_or_else(AppError::unexpected)?
            .reset(&path)?;
        // Reject queued operations from old windows until the new process starts.
        storage.database = None;
        storage.path = Err(AppError::new("reset", "Linkme is restarting."));
        Ok::<(), AppError>(())
    })
    .await
    .map_err(|_| AppError::unexpected())??;
    app.request_restart();
    Ok(())
}

#[tauri::command]
async fn save_link(
    storage: State<'_, SharedStorage>,
    input: LinkDraft,
) -> Result<SaveResult, AppError> {
    database_task(storage.inner().clone(), move |database| {
        database.save(&input)
    })
    .await
}

#[tauri::command]
async fn list_links(
    storage: State<'_, SharedStorage>,
    query: String,
    before: Option<Cursor>,
) -> Result<LinkPage, AppError> {
    database_task(storage.inner().clone(), move |database| {
        database.list(&query, before)
    })
    .await
}

#[tauri::command]
async fn save_group(
    storage: State<'_, SharedStorage>,
    category: i64,
    id: Option<i64>,
    name: String,
) -> Result<(), AppError> {
    database_task(storage.inner().clone(), move |db| {
        db.save_group(category, id, &name)
    })
    .await
}
#[tauri::command]
async fn order_link(
    storage: State<'_, SharedStorage>,
    id: i64,
    target: i64,
    before: bool,
) -> Result<(), AppError> {
    database_task(storage.inner().clone(), move |db| {
        db.order_link(id, target, before)
    })
    .await
}
#[tauri::command]
async fn order_categories(
    storage: State<'_, SharedStorage>,
    ids: Vec<i64>,
) -> Result<(), AppError> {
    database_task(storage.inner().clone(), move |db| db.order_categories(ids)).await
}
#[tauri::command]
async fn update_category(
    storage: State<'_, SharedStorage>,
    id: i64,
    name: String,
    icon: String,
    color: Option<u8>,
    pinned: bool,
) -> Result<(), AppError> {
    database_task(storage.inner().clone(), move |db| {
        db.update_category(id, &name, &icon, color, pinned)
    })
    .await
}
#[tauri::command]
async fn list_categories(storage: State<'_, SharedStorage>) -> Result<Vec<Category>, AppError> {
    database_task(storage.inner().clone(), |database| database.categories()).await
}

#[tauri::command]
async fn delete_link(
    storage: State<'_, SharedStorage>,
    id: i64,
    revision: i64,
) -> Result<(), AppError> {
    database_task(storage.inner().clone(), move |database| {
        database.delete(id, revision)
    })
    .await
}

#[tauri::command]
async fn copy_link(storage: State<'_, SharedStorage>, id: i64) -> Result<(), AppError> {
    let url = database_task(storage.inner().clone(), move |database| {
        database.validated_url(id)
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || native::copy_url(&url))
        .await
        .map_err(|_| AppError::unexpected())?
}

#[tauri::command]
async fn share_link_email(storage: State<'_, SharedStorage>, id: i64) -> Result<(), AppError> {
    let url = database_task(storage.inner().clone(), move |database| {
        database.validated_url(id)
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || native::share_email(&url))
        .await
        .map_err(|_| AppError::unexpected())?
}

#[tauri::command]
async fn open_link(storage: State<'_, SharedStorage>, id: i64) -> Result<(), AppError> {
    let url = database_task(storage.inner().clone(), move |database| {
        database.validated_url(id)
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || native::open_url(&url))
        .await
        .map_err(|_| AppError::unexpected())?
}

#[tauri::command]
async fn open_drive_privacy_policy() -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        native::open_url("https://linkme.bacontrack.tn/privacy")
    })
    .await
    .map_err(|_| AppError::unexpected())?
}

fn allowed_navigation(url: &url::Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let endpoint = (url.scheme(), url.host_str(), url.port());
    matches!(
        endpoint,
        ("http", Some("tauri.localhost"), None) | ("tauri", Some("localhost"), None)
    ) || (cfg!(debug_assertions) && endpoint == ("http", Some("127.0.0.1"), Some(1420)))
}

pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(Arc::new(CaptureState::default()))
        .manage(desktop::Desktop::default())
        .manage(flight::Flights::default())
        .manage(search::SearchState::default())
        .manage(preview::PreviewState::default())
        .manage(preview::SiteIconState::default())
        .manage(Arc::new(updates::Updates::configured()))
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if desktop::opens_library(args) {
                desktop::restore_library(app);
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(capture::on_shortcut)
                .build(),
        )
        .on_window_event(|window, event| match (window.label(), event) {
            ("main", tauri::WindowEvent::CloseRequested { api, .. }) => {
                let desktop = window.state::<desktop::Desktop>();
                if desktop.has_tray() && !desktop.quitting() && window.hide().is_ok() {
                    window.state::<Arc<updates::Updates>>().end_opening();
                    api.prevent_close();
                } else if !desktop.quitting() {
                    window.app_handle().exit(0);
                }
            }
            ("capture", tauri::WindowEvent::Destroyed) => {
                let state = window.state::<Arc<CaptureState>>();
                if let Ok(mut panel) = state.panel.lock() {
                    *panel = None;
                }
                updates::desktop_ready(window.app_handle());
            }
            ("search", tauri::WindowEvent::Destroyed) => {
                let search = window.state::<search::SearchState>();
                if let Ok(mut restore) = search.0.lock() {
                    *restore = None;
                }
                updates::desktop_ready(window.app_handle());
            }
            _ => {}
        })
        .setup(|app| {
            let path = app
                .path()
                .app_local_data_dir()
                .map(|directory| directory.join("links.sqlite3"))
                .map_err(|_| AppError::database());
            app.manage(Arc::new(Mutex::new(Storage {
                path,
                database: None,
            })));
            let library =
                tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
                    .visible(false)
                    .focused(false)
                    .on_navigation(allowed_navigation)
                    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                    .build()?;
            #[cfg(windows)]
            updates::track_minimize(&library);
            if desktop::install_tray(app.handle()).is_err() {
                library.show()?;
            } else {
                #[cfg(all(windows, not(debug_assertions)))]
                let _ = startup::initialize_default();
            }
            #[cfg(windows)]
            {
                app.manage(Arc::new(backup::Service::default()));
                backup::start(app.handle());
            }
            if desktop::opens_library(
                std::env::args_os()
                    .skip(1)
                    .map(|arg| arg.to_string_lossy().into_owned()),
            ) {
                desktop::restore_library(app.handle());
            } else {
                desktop::show_library_if_requested(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            reset_linkme,
            #[cfg(windows)]
            backup::drive_backup,
            open_drive_privacy_policy,
            #[cfg(windows)]
            backup::list_drive_backups,
            #[cfg(windows)]
            backup::restore_drive_backup,
            startup::startup_preference,
            updates::update_preferences,
            updates::check_for_updates,
            updates::claim_update_notice,
            updates::open_update_release,
            preview::preview_preference,
            preview::preview_link,
            preview::site_icon,
            save_link,
            list_links,
            save_group,
            order_link,
            order_categories,
            update_category,
            list_categories,
            delete_link,
            copy_link,
            share_link_email,
            open_link,
            connect_capture,
            update_capture,
            desktop::popup_ready,
            take_capture_panel,
            dismiss_capture_panel,
            save_panel_link,
            resize_capture_panel,
            search::resize_search_panel,
            search::open_search_result
        ])
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_rejects_external_origins_and_lookalikes() {
        for value in [
            "https://example.com",
            "http://tauri.localhost.evil",
            "http://user@tauri.localhost",
            "blob:http://tauri.localhost/local-blob",
            "http://127.0.0.1:1421",
            "file:///C:/secret",
        ] {
            assert!(!allowed_navigation(&url::Url::parse(value).unwrap()));
        }
        assert!(allowed_navigation(
            &url::Url::parse("http://tauri.localhost/").unwrap()
        ));
        assert_eq!(
            allowed_navigation(&url::Url::parse("http://127.0.0.1:1420/").unwrap()),
            cfg!(debug_assertions)
        );
    }

    #[test]
    fn ipc_allows_only_local_main_window_and_registered_operations() {
        let directory = tempfile::tempdir().unwrap();
        let storage: SharedStorage = Arc::new(Mutex::new(Storage {
            path: Ok(directory.path().join("links.sqlite3")),
            database: None,
        }));
        let app = tauri::test::mock_builder()
            .manage(storage)
            .manage(Arc::new(CaptureState::default()))
            .manage(search::SearchState::default())
            .manage(preview::PreviewState::default())
            .manage(preview::SiteIconState::default())
            .manage(Arc::new(updates::Updates::default()))
            .invoke_handler(tauri::generate_handler![
                reset_linkme,
                #[cfg(windows)]
                backup::drive_backup,
                open_drive_privacy_policy,
                #[cfg(windows)]
                backup::list_drive_backups,
                #[cfg(windows)]
                backup::restore_drive_backup,
                startup::startup_preference,
                updates::update_preferences,
                updates::check_for_updates,
                updates::claim_update_notice,
                updates::open_update_release,
                preview::preview_preference,
                preview::preview_link,
                preview::site_icon,
                save_link,
                list_links,
                save_group,
                order_link,
                order_categories,
                update_category,
                list_categories,
                delete_link,
                copy_link,
                share_link_email,
                open_link,
                connect_capture,
                update_capture,
                desktop::popup_ready,
                take_capture_panel,
                dismiss_capture_panel,
                save_panel_link,
                resize_capture_panel,
                search::resize_search_panel,
                search::open_search_result
            ])
            .build(tauri::generate_context!())
            .unwrap();
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let other = tauri::WebviewWindowBuilder::new(&app, "other", Default::default())
            .build()
            .unwrap();
        let capture_window = tauri::WebviewWindowBuilder::new(&app, "capture", Default::default())
            .build()
            .unwrap();
        let search_window = tauri::WebviewWindowBuilder::new(&app, "search", Default::default())
            .build()
            .unwrap();
        let request = |command: &str, origin: &str| tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: origin.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(
                serde_json::json!({ "input": {"url": "https://example.com/ipc"}, "query":"", "before": null, "id":99999,"revision":0,
                    "settings":{"mode":"prompt","shortcut":"Ctrl+Shift+J"}, "events":"__CHANNEL__:10",
                    "shortcuts":["Ctrl+Shift+J"], "handler":"__CHANNEL__:10", "manual":true, "opening":0, "generation":1 }),
            ),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        };
        let result =
            tauri::test::get_ipc_response(&main, request("save_link", "http://tauri.localhost"))
                .unwrap()
                .deserialize::<serde_json::Value>()
                .unwrap();
        assert_eq!(result["created"], true);
        let installed = tauri::test::get_ipc_response(
            &main,
            request("check_for_updates", "http://tauri.localhost"),
        )
        .unwrap()
        .deserialize::<serde_json::Value>()
        .unwrap();
        assert_eq!(installed["outcome"], "unconfigured");
        assert_eq!(
            installed["status"]["installed"],
            app.package_info().version.to_string()
        );
        assert!(
            tauri::test::get_ipc_response(
                &main,
                request("update_preferences", "https://github.com")
            )
            .unwrap_err()
            .to_string()
            .contains("not allowed")
        );
        assert!(
            tauri::test::get_ipc_response(
                &search_window,
                request("list_links", "http://tauri.localhost")
            )
            .is_ok()
        );
        for command in [
            "startup_preference",
            "drive_backup",
            "open_drive_privacy_policy",
            "list_drive_backups",
            "restore_drive_backup",
            "update_preferences",
            "check_for_updates",
            "claim_update_notice",
            "open_update_release",
        ] {
            for popup in [&capture_window, &search_window] {
                let denied = tauri::test::get_ipc_response(
                    popup,
                    request(command, "http://tauri.localhost"),
                )
                .unwrap_err();
                assert!(denied.to_string().contains("not allowed"));
            }
        }
        for window in [&main, &search_window] {
            let missing = tauri::test::get_ipc_response(
                window,
                request("site_icon", "http://tauri.localhost"),
            )
            .unwrap_err();
            assert_eq!(missing["code"], "not_found");
        }
        for command in ["copy_link", "share_link_email"] {
            let error = tauri::test::get_ipc_response(
                &search_window,
                request(command, "http://tauri.localhost"),
            )
            .unwrap_err();
            assert_eq!(error["code"], "not_found");
            for (window, origin) in [
                (&capture_window, "http://tauri.localhost"),
                (&search_window, "https://example.com"),
            ] {
                let denied =
                    tauri::test::get_ipc_response(window, request(command, origin)).unwrap_err();
                assert!(denied.to_string().contains("not allowed"));
            }
        }
        let allowed_open = tauri::test::get_ipc_response(
            &search_window,
            request("open_search_result", "http://tauri.localhost"),
        )
        .unwrap_err();
        assert_eq!(allowed_open["code"], "unexpected"); // Missing live search activation, not denied by ACL.
        for popup in [&capture_window, &search_window] {
            tauri::test::get_ipc_response(popup, request("popup_ready", "http://tauri.localhost"))
                .unwrap();
        }
        for (window, command, origin) in [
            (
                &other,
                "open_drive_privacy_policy",
                "http://tauri.localhost",
            ),
            (&main, "open_drive_privacy_policy", "https://example.com"),
            (&main, "popup_ready", "http://tauri.localhost"),
            (&other, "popup_ready", "http://tauri.localhost"),
            (&capture_window, "popup_ready", "https://example.com"),
            (&search_window, "popup_ready", "https://example.com"),
            (&capture_window, "site_icon", "http://tauri.localhost"),
            (&search_window, "site_icon", "https://example.com"),
            (&main, "site_icon", "https://example.com"),
            (&search_window, "reset_linkme", "http://tauri.localhost"),
            (&search_window, "preview_link", "http://tauri.localhost"),
            (
                &capture_window,
                "preview_preference",
                "http://tauri.localhost",
            ),
            (&main, "preview_link", "https://example.com"),
            (&capture_window, "reset_linkme", "http://tauri.localhost"),
            (&main, "reset_linkme", "https://example.com"),
            (&main, "open_search_result", "http://tauri.localhost"),
            (&search_window, "open_search_result", "https://example.com"),
            (&search_window, "save_link", "http://tauri.localhost"),
            (&search_window, "update_category", "http://tauri.localhost"),
            (&search_window, "update_capture", "http://tauri.localhost"),
            (
                &search_window,
                "take_capture_panel",
                "http://tauri.localhost",
            ),
        ] {
            let error =
                tauri::test::get_ipc_response(window, request(command, origin)).unwrap_err();
            assert!(error.to_string().contains("not allowed"), "{error}");
        }
        assert!(
            tauri::test::get_ipc_response(&main, request("list_links", "http://tauri.localhost"))
                .is_ok()
        );
        assert!(
            tauri::test::get_ipc_response(&other, request("save_link", "http://tauri.localhost"))
                .is_err()
        );
        assert!(
            tauri::test::get_ipc_response(&main, request("list_links", "https://example.com"))
                .is_err()
        );
        for command in [
            "save_link",
            "list_links",
            "list_categories",
            "delete_link",
            "copy_link",
            "open_link",
        ] {
            assert!(
                tauri::test::get_ipc_response(&other, request(command, "http://tauri.localhost"))
                    .is_err()
            );
            assert!(
                tauri::test::get_ipc_response(&main, request(command, "https://example.com"))
                    .is_err()
            );
        }
        for command in ["connect_capture", "update_capture"] {
            for (window, origin) in [
                (&other, "http://tauri.localhost"),
                (&main, "https://example.com"),
            ] {
                let error =
                    tauri::test::get_ipc_response(window, request(command, origin)).unwrap_err();
                assert!(error.to_string().contains("not allowed"), "{error}");
            }
        }
        let error = tauri::test::get_ipc_response(
            &main,
            request("update_capture", "http://tauri.localhost"),
        )
        .unwrap_err();
        assert_eq!(error["code"], "unexpected"); // Allowed through ACL, but no live connection.
        for command in [
            "take_capture_panel",
            "resize_capture_panel",
            "save_panel_link",
            "dismiss_capture_panel",
        ] {
            let error =
                tauri::test::get_ipc_response(&main, request(command, "http://tauri.localhost"))
                    .unwrap_err();
            assert!(error.to_string().contains("not allowed"), "{error}");
        }
        for command in [
            "save_link",
            "copy_link",
            "open_link",
            "delete_link",
            "connect_capture",
            "update_capture",
        ] {
            let error = tauri::test::get_ipc_response(
                &capture_window,
                request(command, "http://tauri.localhost"),
            )
            .unwrap_err();
            assert!(error.to_string().contains("not allowed"), "{error}");
        }
        assert!(
            tauri::test::get_ipc_response(
                &capture_window,
                request("list_categories", "http://tauri.localhost")
            )
            .is_ok()
        );
        let error = tauri::test::get_ipc_response(
            &capture_window,
            request("take_capture_panel", "https://example.com"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not allowed"), "{error}");
        let error = tauri::test::get_ipc_response(
            &main,
            request("plugin:global-shortcut|register", "http://tauri.localhost"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not allowed"), "{error}");
        for command in ["copy_link", "open_link", "delete_link"] {
            let error =
                tauri::test::get_ipc_response(&main, request(command, "http://tauri.localhost"))
                    .unwrap_err();
            assert_eq!(error["code"], "not_found");
        }
        assert!(
            tauri::test::get_ipc_response(
                &main,
                request("plugin:fs|read_text_file", "http://tauri.localhost")
            )
            .is_err()
        );
    }
}
