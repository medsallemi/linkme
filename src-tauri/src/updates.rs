use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow, async_runtime};

use crate::{SharedStorage, database_task, storage::AppError};

mod http;
#[cfg(test)]
mod tests;

const MAX_BODY: usize = 1024 * 1024;
const FRESH_FOR: Duration = Duration::from_secs(60 * 60);
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);
const MANUAL_COOLDOWN: Duration = Duration::from_secs(15);

struct Repository {
    slug: String,
}

impl Repository {
    fn parse(value: &str) -> Option<Self> {
        let (owner, repo) = value.split_once('/')?;
        let valid_owner = !owner.is_empty()
            && owner.len() <= 39
            && !owner.starts_with('-')
            && !owner.ends_with('-')
            && owner
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-');
        let valid_repo = !repo.is_empty()
            && repo.len() <= 100
            && !matches!(repo, "." | "..")
            && !repo.to_ascii_lowercase().ends_with(".git")
            && repo
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
        (valid_owner && valid_repo).then(|| Self { slug: value.into() })
    }

    fn list_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/releases?per_page=30",
            self.slug
        )
    }

    fn release_page(&self, tag: &str) -> String {
        format!("https://github.com/{}/releases/tag/{tag}", self.slug)
    }

    fn installer_url(&self, tag: &str, filename: &str) -> String {
        format!(
            "https://github.com/{}/releases/download/{tag}/{filename}",
            self.slug
        )
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Release {
    #[serde(skip)]
    tag: String,
    version: String,
    published_at: String,
    notes: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
    browser_download_url: String,
}

fn latest_compatible(
    repo: &Repository,
    installed: &Version,
    body: &[u8],
) -> Result<Option<Release>, ()> {
    if body.len() > MAX_BODY {
        return Err(());
    }
    let records: Vec<GitHubRelease> = serde_json::from_slice(body).map_err(|_| ())?;
    if records.len() > 30 {
        return Err(());
    }
    Ok(records
        .into_iter()
        .filter_map(|record| {
            let tag = record.tag_name;
            let version = Version::parse(tag.strip_prefix('v').unwrap_or(&tag)).ok()?;
            let published_at = record.published_at?;
            if record.draft
                || record.prerelease
                || !version.pre.is_empty()
                || !version.build.is_empty()
                || !version.cmp_precedence(installed).is_gt()
                || record.html_url != repo.release_page(&tag)
                || chrono::DateTime::parse_from_rfc3339(&published_at).is_err()
            {
                return None;
            }
            let filename = format!("Linkme_{version}_x64-setup.exe");
            let expected = repo.installer_url(&tag, &filename);
            record
                .assets
                .iter()
                .any(|asset| {
                    asset.name == filename
                        && asset.state == "uploaded"
                        && asset.size > 0
                        && asset.browser_download_url == expected
                })
                .then_some((version, tag, published_at, record.body))
        })
        .max_by(|a, b| a.0.cmp_precedence(&b.0))
        .map(|(version, tag, published_at, body)| Release {
            tag,
            version: version.to_string(),
            published_at,
            notes: summarize(body.as_deref().unwrap_or_default()),
        }))
}

fn summarize(body: &str) -> String {
    let mut chars = body.trim().chars().filter(|c| {
        (*c == '\n' || !c.is_control())
            && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    });
    let mut notes: String = chars.by_ref().take(900).collect();
    if chars.next().is_some() {
        notes.push('…');
    }
    notes
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum Outcome {
    Available,
    Current,
    Failed,
    Skipped,
    Unconfigured,
}

#[derive(Default)]
struct Cache {
    automatic: Option<bool>,
    release: Option<Release>,
    etag: Option<String>,
    checked_at: Option<u64>,
    attempted: Option<(Instant, Outcome)>,
    retry_until: Option<Instant>,
}

impl Cache {
    fn recent(&self, manual: bool, now: Instant) -> Option<Outcome> {
        if self.retry_until.is_some_and(|until| now < until) {
            return Some(Outcome::Failed);
        }
        let (time, outcome) = self.attempted?;
        let limit = if outcome == Outcome::Failed {
            RETRY_AFTER_FAILURE
        } else if manual {
            MANUAL_COOLDOWN
        } else {
            FRESH_FOR
        };
        (now.saturating_duration_since(time) < limit).then_some(outcome)
    }

    fn record(
        &mut self,
        repo: &Repository,
        version: &Version,
        response: Result<Vec<u8>, http::Failure>,
        now: Instant,
        unix: u64,
    ) -> Outcome {
        let outcome = self
            .apply(repo, version, response, now, unix)
            .unwrap_or(Outcome::Failed);
        self.attempted = Some((now, outcome));
        outcome
    }

    fn apply(
        &mut self,
        repo: &Repository,
        version: &Version,
        response: Result<Vec<u8>, http::Failure>,
        now: Instant,
        unix: u64,
    ) -> Option<Outcome> {
        let bytes = response.ok()?;
        let response = http::Response::parse(&bytes)?;
        match response.status {
            200 if response.is_json() => {
                self.release = latest_compatible(repo, version, response.body).ok()?;
                self.etag = response.etag().map(str::to_owned);
            }
            304 if self.etag.is_some() && self.checked_at.is_some() => {}
            403 | 429 => {
                self.retry_until = now.checked_add(response.retry_delay(unix));
                return None;
            }
            // All redirects are rejected, including repository renames.
            _ => return None,
        }
        self.checked_at = Some(unix);
        self.retry_until = None;
        Some(
            self.release
                .as_ref()
                .map_or(Outcome::Current, |_| Outcome::Available),
        )
    }
}

#[derive(Default)]
struct Opening {
    number: u32,
    minimized: bool,
    checked: bool,
    offered: bool,
}

impl Opening {
    fn end(&mut self) {
        self.number = self.number.wrapping_add(1);
        self.checked = false;
        self.offered = false;
    }

    #[cfg(any(windows, test))]
    fn minimize(&mut self, minimized: bool) -> bool {
        let ended = minimized && !self.minimized;
        if ended {
            self.end();
        }
        self.minimized = minimized;
        ended
    }

    fn claim(&mut self, number: u32) -> bool {
        if self.number != number || self.minimized || self.offered {
            return false;
        }
        self.offered = true;
        true
    }
}

#[derive(Default)]
pub struct Updates {
    repository: Option<Repository>,
    cache: Mutex<Cache>,
    opening: Mutex<Opening>,
    request: async_runtime::Mutex<()>,
}

impl Updates {
    pub fn configured() -> Self {
        Self {
            repository: option_env!("LINKME_RELEASE_REPOSITORY").and_then(Repository::parse),
            ..Self::default()
        }
    }

    pub fn end_opening(&self) {
        if let Ok(mut opening) = self.opening.lock() {
            opening.end();
        }
    }

    fn snapshot(&self, version: &Version) -> Result<UpdateStatus, AppError> {
        let cache = self.cache.lock().map_err(|_| AppError::unexpected())?;
        Ok(UpdateStatus {
            installed: version.to_string(),
            configured: self.repository.is_some(),
            automatic: cache.automatic.unwrap_or(true),
            last_checked: cache.checked_at,
            release: cache.release.clone(),
        })
    }

    async fn preference(
        self: &Arc<Self>,
        storage: SharedStorage,
        enabled: Option<bool>,
    ) -> Result<(), AppError> {
        if enabled.is_none()
            && self
                .cache
                .lock()
                .map_err(|_| AppError::unexpected())?
                .automatic
                .is_some()
        {
            return Ok(());
        }
        let state = self.clone();
        database_task(storage, move |db| {
            let value = db.update_preference(enabled)?;
            state
                .cache
                .lock()
                .map_err(|_| AppError::unexpected())?
                .automatic = Some(value);
            Ok(())
        })
        .await
    }

    async fn check(
        self: &Arc<Self>,
        version: &Version,
        opening: Option<u32>,
        fetch: impl FnOnce(String, Option<String>) -> Result<Vec<u8>, http::Failure> + Send + 'static,
    ) -> Result<Outcome, AppError> {
        let Some(repo) = &self.repository else {
            return Ok(Outcome::Unconfigured);
        };
        // Serialize checks, then re-read the cache so concurrent callers share the outcome.
        let _request = self.request.lock().await;
        let etag = {
            let cache = self.cache.lock().map_err(|_| AppError::unexpected())?;
            if let Some(number) = opening {
                let mut library = self.opening.lock().map_err(|_| AppError::unexpected())?;
                if library.number != number
                    || library.minimized
                    || library.offered
                    || cache.automatic != Some(true)
                {
                    return Ok(Outcome::Skipped);
                }
                if library.checked {
                    return Ok(cache
                        .attempted
                        .map_or(Outcome::Skipped, |(_, outcome)| outcome));
                }
                library.checked = true;
            }
            if let Some(outcome) = cache.recent(opening.is_none(), Instant::now()) {
                return Ok(outcome);
            }
            cache.etag.clone()
        };
        let url = repo.list_url();
        let response = async_runtime::spawn_blocking(move || fetch(url, etag))
            .await
            .map_err(|_| AppError::unexpected())?;
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut cache = self.cache.lock().map_err(|_| AppError::unexpected())?;
        Ok(cache.record(repo, version, response, Instant::now(), unix))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    installed: String,
    configured: bool,
    automatic: bool,
    last_checked: Option<u64>,
    release: Option<Release>,
}

#[derive(Serialize)]
pub struct UpdateCheck {
    outcome: Outcome,
    opening: Option<u32>,
    status: UpdateStatus,
}

fn is_open<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true)
}

#[tauri::command]
pub async fn update_preferences<R: Runtime>(
    app: AppHandle<R>,
    updates: State<'_, Arc<Updates>>,
    storage: State<'_, SharedStorage>,
    enabled: Option<bool>,
) -> Result<UpdateStatus, AppError> {
    updates
        .inner()
        .preference(storage.inner().clone(), enabled)
        .await?;
    updates.snapshot(&app.package_info().version)
}

#[tauri::command]
pub async fn check_for_updates<R: Runtime>(
    window: WebviewWindow<R>,
    updates: State<'_, Arc<Updates>>,
    storage: State<'_, SharedStorage>,
    manual: bool,
) -> Result<UpdateCheck, AppError> {
    let version = &window.app_handle().package_info().version;
    let opening = if !manual && is_open(&window) {
        Some(
            updates
                .opening
                .lock()
                .map_err(|_| AppError::unexpected())?
                .number,
        )
    } else {
        None
    };
    let outcome = if !manual && opening.is_none() {
        Outcome::Skipped
    } else {
        if !manual && updates.repository.is_some() {
            updates
                .inner()
                .preference(storage.inner().clone(), None)
                .await?;
        }
        updates.inner().check(version, opening, http::fetch).await?
    };
    Ok(UpdateCheck {
        outcome,
        opening,
        status: updates.snapshot(version)?,
    })
}

#[tauri::command]
pub async fn claim_update_notice<R: Runtime>(
    window: WebviewWindow<R>,
    updates: State<'_, Arc<Updates>>,
    opening: u32,
) -> Result<Option<UpdateStatus>, AppError> {
    let app = window.app_handle();
    let status = updates.snapshot(&app.package_info().version)?;
    if !status.configured
        || !status.automatic
        || status.release.is_none()
        || !is_open(&window)
        || !crate::native::is_foreground(&window)
        || ["capture", "search"].iter().any(|label| {
            app.get_webview_window(label)
                .is_some_and(|popup| popup.is_visible().unwrap_or(true))
        })
        || crate::flight::active(app)
    {
        return Ok(None);
    }
    let mut library = updates.opening.lock().map_err(|_| AppError::unexpected())?;
    Ok(library.claim(opening).then_some(status))
}

fn open_cached_release(
    updates: &Updates,
    launch: impl FnOnce(&str) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let destination = {
        let cache = updates.cache.lock().map_err(|_| AppError::unexpected())?;
        updates
            .repository
            .as_ref()
            .zip(cache.release.as_ref())
            .map(|(repo, release)| repo.release_page(&release.tag))
            .ok_or_else(|| {
                AppError::new(
                    "no_update",
                    "No update is available. Check for updates first.",
                )
            })?
    };
    launch(&destination)
}

#[tauri::command]
pub async fn open_update_release(updates: State<'_, Arc<Updates>>) -> Result<(), AppError> {
    let updates = updates.inner().clone();
    async_runtime::spawn_blocking(move || open_cached_release(&updates, crate::native::open_url))
        .await
        .map_err(|_| AppError::unexpected())?
}

// The Library retries a pending notice when desktop work finishes. This never shows a window.
pub fn desktop_ready<R: Runtime>(app: &AppHandle<R>) {
    if app
        .try_state::<Arc<Updates>>()
        .is_some_and(|updates| updates.repository.is_some())
        && let Some(window) = app.get_webview_window("main")
        && crate::native::is_foreground(&window)
    {
        let _ = window.eval("window.dispatchEvent(new Event('linkme-library-ready'))");
    }
}

#[cfg(windows)]
pub fn track_minimize<R: Runtime>(window: &WebviewWindow<R>) {
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{SIZE_MINIMIZED, WM_NCDESTROY, WM_SIZE},
        },
    };

    struct Watch<R: Runtime> {
        updates: Arc<Updates>,
        window: WebviewWindow<R>,
    }

    unsafe extern "system" fn resized<R: Runtime>(
        hwnd: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
        id: usize,
        data: usize,
    ) -> LRESULT {
        let ptr = data as *const Watch<R>;
        let mut minimized = None;
        if message == WM_SIZE
            && let Ok(mut opening) = unsafe { &*ptr }.updates.opening.lock()
            && opening.minimize(w.0 == SIZE_MINIMIZED as usize)
        {
            minimized = Some(unsafe { &*ptr }.window.clone());
        }
        if message == WM_NCDESTROY {
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(resized::<R>), id);
                drop(Box::from_raw(ptr.cast_mut()));
            }
        }
        let result = unsafe { DefSubclassProc(hwnd, message, w, l) };
        if let Some(window) = minimized {
            let _ = window.eval("window.dispatchEvent(new Event('linkme-library-minimized'))");
        }
        result
    }

    let state = window.state::<Arc<Updates>>();
    if state.repository.is_none() {
        return;
    }
    if let Ok(hwnd) = window.hwnd() {
        // Setup runs on the Library HWND's thread. The subclass owns this context until teardown.
        let ptr = Box::into_raw(Box::new(Watch {
            updates: state.inner().clone(),
            window: window.clone(),
        }));
        if !unsafe { SetWindowSubclass(hwnd, Some(resized::<R>), 1, ptr as usize) }.as_bool() {
            unsafe { drop(Box::from_raw(ptr)) };
        }
    }
}
