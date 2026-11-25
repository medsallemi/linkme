mod desktop_client;
mod google;
mod http;

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Months};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};
use tokio::sync::Notify;

use crate::{
    SharedStorage,
    storage::{AppError, Database},
};

const SESSION: &str = "drive-backup.dpapi";
const SNAPSHOT_LIMIT: u64 = 5_000_000;
const RETRY: Duration = Duration::from_secs(60 * 60);
const REAUTHORIZE: &str = "google_reauthorization";
const CLIENT_CHANGED: &str = "Linkme's Google sign-in configuration changed. Reconnect the same Google account to resume backups.";

#[derive(Default)]
pub struct Service {
    transfer: Mutex<()>,
    next_run: Notify,
    warning: Mutex<Option<&'static str>>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    #[default]
    Off,
    Daily,
    Weekly,
    Monthly,
}
impl Frequency {
    fn next(self, last: u64) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::Daily => Some(last.saturating_add(86_400)),
            Self::Weekly => Some(last.saturating_add(604_800)),
            Self::Monthly => DateTime::from_timestamp(last.try_into().ok()?, 0)?
                .checked_add_months(Months::new(1))?
                .timestamp()
                .try_into()
                .ok(),
        }
    }
    fn keep(self) -> usize {
        match self {
            Self::Off | Self::Daily => 7,
            Self::Weekly => 4,
            Self::Monthly => 12,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Session {
    refresh: String,
    subject: String,
    #[serde(default)]
    client_id: Option<String>,
    profile: google::Profile,
    installation: String,
    frequency: Frequency,
    last_backup: Option<u64>,
}

impl Session {
    fn requires_reconnect(&self, client: &google::Client) -> bool {
        self.client_id
            .as_ref()
            .is_some_and(|previous| previous != &client.client_id)
    }

    fn token(&self, client: &google::Client) -> Result<String, AppError> {
        if self.requires_reconnect(client) {
            return Err(AppError::new(REAUTHORIZE, CLIENT_CHANGED));
        }
        google::refresh(client, &self.refresh)
    }

    fn connect(
        previous: Option<Self>,
        client: google::Client,
        refresh: String,
        subject: String,
        profile: google::Profile,
    ) -> Result<Self, AppError> {
        let client_id = Some(client.client_id);
        if let Some(current) = previous {
            if subject != current.subject {
                return Err(AppError::new(
                    "native_action",
                    "Choose the connected Google account, or disconnect it first to switch accounts.",
                ));
            }
            return Ok(Self {
                refresh,
                client_id,
                profile,
                ..current
            });
        }
        Ok(Self {
            refresh,
            subject,
            client_id,
            profile,
            installation: google::random_id()?,
            frequency: Frequency::Off,
            last_backup: None,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    configured: bool,
    requires_reconnect: bool,
    profile: Option<google::Profile>,
    frequency: Frequency,
    last_backup: Option<u64>,
    warning: Option<&'static str>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Status,
    Connect,
    Disconnect,
    Schedule { frequency: Frequency },
    Backup,
}

fn failure() -> AppError {
    AppError::new(
        "native_action",
        "Linkme could not read or protect the local Google Drive session.",
    )
}

fn protect(input: &mut [u8], encrypt: bool) -> Result<Vec<u8>, AppError> {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
            },
        },
        core::w,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input.len()).map_err(|_| failure())?,
        pbData: input.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input and output buffers are live and scoped to this synchronous DPAPI call.
    unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                w!("Linkme Drive Backup"),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|_| failure())?;
        if output.pbData.is_null() {
            return Err(failure());
        }
        let data = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let bytes = data.to_vec();
        data.fill(0);
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(bytes)
    }
}

fn read(dir: &Path) -> Result<Option<Session>, AppError> {
    let file = match fs::File::open(dir.join(SESSION)) {
        Ok(file) => file,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure()),
    };
    let mut bytes = Vec::new();
    file.take(256 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|_| failure())?;
    let mut plain = protect(&mut bytes, false)?;
    let session = serde_json::from_slice(&plain).map_err(|_| failure());
    plain.fill(0);
    session
}

fn write(dir: &Path, session: &Session) -> Result<(), AppError> {
    fs::create_dir_all(dir).map_err(|_| failure())?;
    let mut plain = serde_json::to_vec(session).map_err(|_| failure())?;
    let protected = protect(&mut plain, true);
    plain.fill(0);
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|_| failure())?;
    temp.write_all(&protected?).map_err(|_| failure())?;
    temp.as_file().sync_all().map_err(|_| failure())?;
    temp.persist(dir.join(SESSION)).map_err(|_| failure())?;
    Ok(())
}

fn remove_session(dir: &Path) -> Result<(), AppError> {
    match fs::remove_file(dir.join(SESSION)) {
        Ok(()) => Ok(()),
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(failure()),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn snapshot(storage: &SharedStorage, automatic: bool) -> Result<(Vec<u8>, u32), AppError> {
    let mut storage = storage.lock().map_err(|_| AppError::unexpected())?;
    if storage.database.is_none() {
        let db_path = storage.path.as_ref().map_err(Clone::clone)?;
        storage.database = Some(Database::open(db_path)?);
    }
    let database = storage.database.as_ref().ok_or_else(AppError::database)?;
    let count = database.backup_link_count()?;
    if automatic && count == 0 {
        return Err(AppError::new(
            "native_action",
            "Automatic backup waits until the library contains links.",
        ));
    }
    Ok((database.backup_bytes(SNAPSHOT_LIMIT)?, count))
}

fn backup(
    dir: &Path,
    storage: &SharedStorage,
    session: &mut Session,
    automatic: bool,
) -> Result<Option<&'static str>, AppError> {
    let time = now();
    let client = google::client(dir)?;
    let (bytes, link_count) = snapshot(storage, automatic)?;
    let token = session.token(&client)?;
    let uploaded = google::upload(
        &token,
        &session.installation,
        time,
        automatic,
        link_count,
        &bytes,
    )?;
    session.client_id = Some(client.client_id);
    session.last_backup = Some(time);
    write(dir, session)?;
    if google::prune(
        &token,
        &session.installation,
        &uploaded,
        session.frequency.keep(),
    )
    .is_err()
    {
        return Ok(Some(
            "Backup saved. Older versions could not be moved to Trash.",
        ));
    }
    Ok(None)
}

#[tauri::command]
pub async fn drive_backup<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Arc<Service>>,
    storage: State<'_, SharedStorage>,
    action: Action,
) -> Result<BackupStatus, AppError> {
    let dir = app.path().app_local_data_dir().map_err(|_| failure())?;
    let service = service.inner().clone();
    let storage = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let response = || {
            let session = read(&dir)?;
            let client = google::client(&dir).ok();
            let requires_reconnect = session
                .as_ref()
                .zip(client.as_ref())
                .is_some_and(|(session, client)| session.requires_reconnect(client));
            Ok(BackupStatus {
                configured: client.is_some(),
                requires_reconnect,
                profile: session.as_ref().map(|session| session.profile.clone()),
                frequency: session
                    .as_ref()
                    .map_or(Frequency::Off, |session| session.frequency),
                last_backup: session.and_then(|session| session.last_backup),
                warning: if requires_reconnect {
                    Some(CLIENT_CHANGED)
                } else {
                    *service.warning.lock().map_err(|_| AppError::unexpected())?
                },
            })
        };
        if matches!(action, Action::Status) {
            return response();
        }
        let _transaction = transfer(&service)?;
        let mut session = if matches!(action, Action::Disconnect) {
            None
        } else {
            read(&dir)?
        };
        let warning = match action {
            Action::Status => unreachable!(),
            Action::Connect => {
                let client = google::client(&dir)?;
                let google::Authorization {
                    refresh,
                    subject,
                    profile,
                    browser,
                } = google::connect(&client)?;
                let connected = Session::connect(session, client, refresh, subject, profile)?;
                write(&dir, &connected)?;
                browser.connected();
                None
            }
            Action::Disconnect => {
                remove_session(&dir)?;
                None
            }
            Action::Backup | Action::Schedule { .. } => {
                let session = session.as_mut().ok_or_else(|| {
                    AppError::new(
                        "native_action",
                        "Connect a Google account before setting up backup.",
                    )
                })?;
                if let Action::Schedule { frequency } = action {
                    session.frequency = frequency;
                    write(&dir, session)?;
                    None
                } else {
                    backup(&dir, &storage, session, false)?
                }
            }
        };
        *service.warning.lock().map_err(|_| AppError::unexpected())? = warning;
        service.next_run.notify_one();
        response()
    })
    .await
    .map_err(|_| AppError::unexpected())?
}

#[tauri::command]
pub async fn list_drive_backups<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Arc<Service>>,
) -> Result<Vec<google::BackupEntry>, AppError> {
    let dir = app.path().app_local_data_dir().map_err(|_| failure())?;
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _transaction = transfer(&service)?;
        let token = connected_token(&dir)?;
        google::list_backups(&token, SNAPSHOT_LIMIT as usize)
    })
    .await
    .map_err(|_| AppError::unexpected())?
}

#[tauri::command]
pub async fn restore_drive_backup<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Arc<Service>>,
    storage: State<'_, SharedStorage>,
    id: String,
) -> Result<(), AppError> {
    let dir = app.path().app_local_data_dir().map_err(|_| failure())?;
    let service = service.inner().clone();
    let storage = storage.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _transaction = transfer(&service)?;
        let token = connected_token(&dir)?;
        let bytes = google::download_backup(&token, &id, SNAPSHOT_LIMIT as usize)?;
        let mut storage = storage.lock().map_err(|_| AppError::unexpected())?;
        if storage.database.is_none() {
            let path = storage.path.as_ref().map_err(Clone::clone)?;
            storage.database = Some(Database::open(path)?);
        }
        storage
            .database
            .as_mut()
            .ok_or_else(AppError::database)?
            .restore_library(&bytes, &dir)?;
        Ok::<(), AppError>(())
    })
    .await
    .map_err(|_| AppError::unexpected())??;
    for label in ["capture", "search"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.destroy();
        }
    }
    crate::desktop::restore_library(&app);
    Ok(())
}

fn connected_token(dir: &Path) -> Result<String, AppError> {
    let session =
        read(dir)?.ok_or_else(|| AppError::new("native_action", "Connect Google Drive first."))?;
    session.token(&google::client(dir)?)
}

fn transfer(service: &Service) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
    service.transfer.try_lock().map_err(|_| {
        AppError::new(
            "native_action",
            "Linkme is already connecting or transferring a backup. Try again shortly.",
        )
    })
}

pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let Ok(dir) = app.path().app_local_data_dir() else {
        return;
    };
    let storage = app.state::<SharedStorage>().inner().clone();
    let service = app.state::<Arc<Service>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let work_dir: PathBuf = dir.clone();
            let storage = storage.clone();
            let worker = service.clone();
            let delay = tauri::async_runtime::spawn_blocking(move || {
                let Ok(_transfer) = worker.transfer.try_lock() else {
                    return Some(RETRY);
                };
                let Ok(Some(mut session)) = read(&work_dir) else {
                    return None;
                };
                let due = session.last_backup.map_or_else(
                    || session.frequency.next(0).map(|_| now()),
                    |last| session.frequency.next(last),
                )?;
                let wait = due.saturating_sub(now());
                if wait > 0 {
                    return Some(Duration::from_secs(wait));
                }
                let outcome = backup(&work_dir, &storage, &mut session, true);
                let delay = match &outcome {
                    Ok(_) => Some(Duration::from_secs(
                        session.frequency.next(now())?.saturating_sub(now()),
                    )),
                    Err(error) if error.code == REAUTHORIZE => None,
                    Err(_) => Some(RETRY),
                };
                if let Ok(mut warning) = worker.warning.lock() {
                    *warning = match outcome {
                        Ok(warning) => warning,
                        Err(error) => Some(error.message),
                    };
                }
                delay
            })
            .await
            .unwrap_or(Some(RETRY));
            if let Some(delay) = delay {
                let _ = tokio::time::timeout(delay, service.next_run.notified()).await;
            } else {
                service.next_run.notified().await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_uses_calendar_month_and_bounded_rolling_retention() {
        let jan = DateTime::parse_from_rfc3339("2026-01-31T12:00:00Z")
            .unwrap()
            .timestamp() as u64;
        let feb = DateTime::parse_from_rfc3339("2026-02-28T12:00:00Z")
            .unwrap()
            .timestamp() as u64;
        assert_eq!(Frequency::Daily.next(jan), Some(jan + 86400));
        assert_eq!(Frequency::Weekly.next(jan), Some(jan + 604800));
        assert_eq!(Frequency::Monthly.next(jan), Some(feb));
        assert_eq!(Frequency::Off.next(jan), None);
        assert_eq!(
            [
                Frequency::Daily.keep(),
                Frequency::Weekly.keep(),
                Frequency::Monthly.keep()
            ],
            [7, 4, 12]
        );
    }

    #[test]
    fn protects_session_bytes_for_the_current_windows_user() {
        let mut token = b"synthetic token".to_vec();
        let mut wrapped = protect(&mut token, true).unwrap();
        assert!(!wrapped.windows(token.len()).any(|part| part == token));
        assert_eq!(protect(&mut wrapped, false).unwrap(), token);
    }

    #[test]
    fn legacy_session_and_same_client_remain_compatible_but_a_new_client_requires_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let mut legacy: Session = serde_json::from_value(serde_json::json!({
            "refresh": "legacy-token",
            "subject": "synthetic-account",
            "profile": {"name": "Test Account"},
            "installation": "synthetic-installation",
            "frequency": "weekly",
            "last_backup": 123
        }))
        .unwrap();
        let client = |name: &str| google::Client {
            client_id: format!("{name}.apps.googleusercontent.com"),
            client_secret: "fixture".into(),
        };
        assert!(!legacy.requires_reconnect(&client("production")));
        legacy.client_id = Some(client("old").client_id);
        assert!(!legacy.requires_reconnect(&client("old")));
        write(dir.path(), &legacy).unwrap();
        let previous = read(dir.path()).unwrap().unwrap();
        assert_eq!(
            previous.token(&client("production")).unwrap_err().code,
            REAUTHORIZE
        );
        let profile = previous.profile.clone();
        assert!(
            Session::connect(
                Some(previous),
                client("production"),
                "other-token".into(),
                "another-account".into(),
                profile.clone()
            )
            .is_err()
        );
        let previous = read(dir.path()).unwrap().unwrap();
        assert_eq!(previous.refresh, "legacy-token");
        let reconnected = Session::connect(
            Some(previous),
            client("production"),
            "new-token".into(),
            "synthetic-account".into(),
            profile,
        )
        .unwrap();
        write(dir.path(), &reconnected).unwrap();
        let saved = read(dir.path()).unwrap().unwrap();
        assert!(!saved.requires_reconnect(&client("production")));
        assert_eq!(saved.refresh, "new-token");
        assert_eq!(saved.installation, "synthetic-installation");
        assert!(matches!(saved.frequency, Frequency::Weekly));
        assert_eq!(saved.last_backup, Some(123));
    }

    #[test]
    fn disconnect_serializes_with_transfers_and_removes_the_scheduled_session() {
        let dir = tempfile::tempdir().unwrap();
        let session = Session {
            refresh: "synthetic".into(),
            subject: "synthetic-account".into(),
            client_id: None,
            profile: serde_json::from_value(serde_json::json!({"name": "Test Account"})).unwrap(),
            installation: "synthetic-installation".into(),
            frequency: Frequency::Daily,
            last_backup: None,
        };
        write(dir.path(), &session).unwrap();
        let service = Service::default();
        {
            let _transfer = transfer(&service).unwrap();
            assert!(transfer(&service).is_err());
            assert!(read(dir.path()).unwrap().is_some());
        }
        let _disconnect = transfer(&service).unwrap();
        remove_session(dir.path()).unwrap();
        assert!(read(dir.path()).unwrap().is_none());
        assert!(
            connected_token(dir.path())
                .err()
                .unwrap()
                .message
                .contains("Connect Google Drive first")
        );
        remove_session(dir.path()).unwrap();
    }
}
