use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use url::Url;

pub(super) use super::desktop_client::Client;
use super::{REAUTHORIZE, http};
use crate::storage::AppError;

const SCOPES: &str = "openid profile https://www.googleapis.com/auth/drive.file";
const MAX_JSON: usize = 256 * 1024;
const DRIVE_FILES: &str = "https://www.googleapis.com/drive/v3/files";

fn auth_error() -> AppError {
    AppError::new(
        "native_action",
        "Google sign-in did not complete. Try connecting again.",
    )
}
fn auth_config_error() -> AppError {
    AppError::new(
        "native_action",
        "Linkme needs the Desktop app OAuth file google-oauth.json in its local app data folder.",
    )
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    scope: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub(super) struct Profile {
    pub name: String,
    photo: Option<String>,
}
#[derive(Deserialize)]
struct UserInfo {
    sub: String,
    name: Option<String>,
    picture: Option<String>,
}

pub(super) struct Authorization {
    pub refresh: String,
    pub subject: String,
    pub profile: Profile,
    pub browser: BrowserReply,
}

pub(super) struct BrowserReply(Option<TcpStream>);

fn callback_response(mut stream: TcpStream, status: &str, content_type: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

impl BrowserReply {
    pub(super) fn connected(mut self) {
        if let Some(stream) = self.0.take() {
            callback_response(
                stream,
                "200 OK",
                "text/html",
                include_str!("connected.html"),
            );
        }
    }
}

impl Drop for BrowserReply {
    fn drop(&mut self) {
        if let Some(stream) = self.0.take() {
            callback_response(
                stream,
                "400 Bad Request",
                "text/plain",
                "Google sign-in did not complete. Return to Linkme and try again.",
            );
        }
    }
}

pub(super) fn random_id() -> Result<String, AppError> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| AppError::unexpected())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(super) fn client(dir: &Path) -> Result<Client, AppError> {
    let embedded = include_bytes!(concat!(env!("OUT_DIR"), "/desktop-oauth.json"));
    load_client(dir, embedded)
}

fn load_client(dir: &Path, embedded: &[u8]) -> Result<Client, AppError> {
    if embedded.is_empty() {
        return Client::from_export(&dir.join("google-oauth.json")).ok_or_else(auth_config_error);
    }
    let client: Client = serde_json::from_slice(embedded).map_err(|_| auth_config_error())?;
    client
        .valid()
        .then_some(client)
        .ok_or_else(auth_config_error)
}

#[derive(Debug, PartialEq, Eq)]
enum Callback {
    Code(String),
    Denied,
}

fn parse_callback(line: &str, state: &str) -> Option<Callback> {
    let mut words = line.split_ascii_whitespace();
    if words.next()? != "GET" {
        return None;
    }
    let target = words.next()?;
    if !target.starts_with("/oauth/callback?")
        || !matches!(words.next(), Some("HTTP/1.0" | "HTTP/1.1"))
        || words.next().is_some()
    {
        return None;
    }
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.fragment().is_some() {
        return None;
    }
    let mut returned_state = None;
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        let field = match key.as_ref() {
            "state" => &mut returned_state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if field.replace(value).is_some() {
            return None;
        }
    }
    if returned_state.as_deref() != Some(state) {
        return None;
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() => Some(Callback::Code(code.into_owned())),
        (None, Some(error)) if error == "access_denied" => Some(Callback::Denied),
        _ => None,
    }
}

fn wait_callback(listener: TcpListener, state: &str) -> Result<(String, BrowserReply), AppError> {
    listener.set_nonblocking(true).map_err(|_| auth_error())?;
    let deadline = Instant::now() + Duration::from_secs(150);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).map_err(|_| auth_error())?;
                stream
                    .set_read_timeout(Some(Duration::from_millis(500)))
                    .map_err(|_| auth_error())?;
                stream
                    .set_write_timeout(Some(Duration::from_millis(500)))
                    .map_err(|_| auth_error())?;
                let mut buffer = [0u8; 8192];
                let accepted = Instant::now();
                let mut used = 0;
                while used < buffer.len() && accepted.elapsed() < Duration::from_secs(1) {
                    let Ok(read) = stream.read(&mut buffer[used..]) else {
                        break;
                    };
                    if read == 0 {
                        break;
                    }
                    used += read;
                    if buffer[..used].ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let Some(line) = std::str::from_utf8(&buffer[..used])
                    .ok()
                    .and_then(|text| text.split_once('\n').map(|(line, _)| line))
                else {
                    continue;
                };
                let browser = BrowserReply(Some(stream));
                match parse_callback(line, state) {
                    Some(Callback::Code(code)) => return Ok((code, browser)),
                    Some(Callback::Denied) => return Err(auth_error()),
                    None => {}
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err(auth_error()),
        }
    }
    Err(auth_error())
}

fn address(raw: &str) -> Url {
    Url::parse(raw).expect("validated Google endpoint constant")
}

fn minimal_drive_scope(scope: &str) -> bool {
    let mut drive = false;
    for value in scope.split_ascii_whitespace() {
        match value {
            "https://www.googleapis.com/auth/drive.file" => drive = true,
            "openid" | "profile" | "https://www.googleapis.com/auth/userinfo.profile" => {}
            _ => return false,
        }
    }
    drive
}

fn token(client: &Client, params: &[(&str, &str)]) -> Result<Tokens, AppError> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &client.client_id)
        .append_pair("client_secret", &client.client_secret)
        .extend_pairs(params.iter().copied())
        .finish();
    let (status, bytes) = http::request(
        "POST",
        &address("https://oauth2.googleapis.com/token"),
        None,
        Some("application/x-www-form-urlencoded"),
        body.as_bytes(),
        MAX_JSON,
    )?;
    decode_token(status, &bytes)
}

fn decode_token(status: u32, bytes: &[u8]) -> Result<Tokens, AppError> {
    let rejected = status == 401
        || (status == 400
            && serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .is_some_and(|body| {
                    matches!(
                        body["error"].as_str(),
                        Some("invalid_grant" | "invalid_client" | "unauthorized_client")
                    )
                }));
    if rejected {
        return Err(AppError::new(
            REAUTHORIZE,
            "Google authorization expired or was rejected. Connect Google Drive again.",
        ));
    }
    let tokens: Tokens = decode(status, bytes)?;
    if tokens.access_token.is_empty() {
        return Err(auth_error());
    }
    Ok(tokens)
}

pub(super) fn connect(client: &Client) -> Result<Authorization, AppError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|_| auth_error())?;
    let port = listener.local_addr().map_err(|_| auth_error())?.port();
    let redirect = format!("http://127.0.0.1:{port}/oauth/callback");
    let verifier = random_id()?;
    let state = random_id()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut oauth = address("https://accounts.google.com/o/oauth2/v2/auth");
    oauth.query_pairs_mut().extend_pairs([
        ("client_id", client.client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
        ("state", &state),
    ]);
    crate::native::open_url(oauth.as_str())?;
    let (code, browser) = wait_callback(listener, &state)?;
    let tokens = token(
        client,
        &[
            ("code", &code),
            ("redirect_uri", &redirect),
            ("grant_type", "authorization_code"),
            ("code_verifier", &verifier),
        ],
    )?;
    if !tokens.scope.as_deref().is_some_and(minimal_drive_scope) {
        return Err(AppError::new(
            "native_action",
            "Allow Linkme to create its backup files in Google Drive, then connect again.",
        ));
    }
    let refresh = tokens.refresh_token.ok_or_else(auth_error)?;
    let (status, bytes) = http::request(
        "GET",
        &address("https://openidconnect.googleapis.com/v1/userinfo"),
        Some(&tokens.access_token),
        None,
        &[],
        MAX_JSON,
    )?;
    let user: UserInfo = decode(status, &bytes)?;
    let photo = user.picture.and_then(|picture| {
        let url = Url::parse(&picture).ok()?;
        if !url.host_str()?.ends_with(".googleusercontent.com") {
            return None;
        }
        let (status, bytes) = http::request("GET", &url, None, None, &[], 128 * 1024).ok()?;
        if status != 200 {
            return None;
        }
        crate::preview::preview_image_data(&bytes, "image/jpeg")
            .or_else(|| crate::preview::preview_image_data(&bytes, "image/png"))
    });
    let name: String = user
        .name
        .as_deref()
        .unwrap_or("Google account")
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    if user.sub.is_empty() || user.sub.len() > 255 {
        return Err(auth_error());
    }
    Ok(Authorization {
        refresh,
        subject: user.sub,
        profile: Profile { name, photo },
        browser,
    })
}

pub(super) fn refresh(client: &Client, refresh: &str) -> Result<String, AppError> {
    let response = token(
        client,
        &[("refresh_token", refresh), ("grant_type", "refresh_token")],
    )?;
    if response
        .scope
        .as_deref()
        .is_some_and(|scope| !minimal_drive_scope(scope))
    {
        return Err(auth_error());
    }
    Ok(response.access_token)
}

fn decode<T: DeserializeOwned>(status: u32, bytes: &[u8]) -> Result<T, AppError> {
    if !(200..300).contains(&status) {
        return Err(http::network_error());
    }
    serde_json::from_slice(bytes).map_err(|_| http::network_error())
}

#[derive(Deserialize)]
struct FileId {
    id: String,
    size: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedBackup {
    id: String,
    owned_by_me: bool,
    shared: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    created_time: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    app_properties: std::collections::HashMap<String, String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteBackups {
    files: Vec<ListedBackup>,
    next_page_token: Option<String>,
    #[serde(default)]
    incomplete_search: bool,
}

const BACKUP_TAG: &str = "appProperties has { key='linkmeBackup' and value='v1' }";
const FOLDER_TYPE: &str = "application/vnd.google-apps.folder";

fn find_files(token: &str, filter: &str) -> Result<RemoteBackups, AppError> {
    let mut list = address(DRIVE_FILES);
    let query = format!("trashed = false and {filter}");
    list.query_pairs_mut().extend_pairs([
        ("q", query.as_str()),
        ("orderBy", "createdTime desc"),
        ("pageSize", "100"),
        ("spaces", "drive"),
        (
            "fields",
            "nextPageToken,incompleteSearch,files(id,ownedByMe,shared,name,createdTime,size,appProperties)",
        ),
    ]);
    let (status, bytes) = http::request("GET", &list, Some(token), None, &[], MAX_JSON)?;
    decode(status, &bytes)
}

fn backup_folder(token: &str) -> Result<String, AppError> {
    let folders = find_files(
        token,
        &format!(
            "mimeType = '{FOLDER_TYPE}' and appProperties has {{ key='linkmeBackupFolder' and value='v1' }}"
        ),
    )?;
    if let Some(folder) = folders
        .files
        .into_iter()
        .find(|folder| folder.owned_by_me && !folder.shared)
    {
        return Ok(folder.id);
    }
    let body = serde_json::json!({
        "name":"Linkme Backups",
        "mimeType":FOLDER_TYPE,
        "description":"Private library snapshots from Linkme. Restore a snapshot from Linkme's Google Drive backup settings.",
        "appProperties":{"linkmeBackupFolder":"v1"}
    }).to_string();
    let (status, bytes) = http::request(
        "POST",
        &address("https://www.googleapis.com/drive/v3/files?fields=id"),
        Some(token),
        Some("application/json"),
        body.as_bytes(),
        MAX_JSON,
    )?;
    let folder: FileId = decode(status, &bytes)?;
    if folder.id.is_empty() {
        return Err(http::network_error());
    }
    Ok(folder.id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    id: String,
    name: String,
    created_time: String,
    size: usize,
    kind: Option<&'static str>,
    link_count: Option<u32>,
}

fn backup_entry(file: ListedBackup, max_size: usize) -> Option<BackupEntry> {
    let size = file.size?.parse::<usize>().ok()?;
    if size == 0 || size > max_size {
        return None;
    }
    let kind = match file
        .app_properties
        .get("linkmeBackupType")
        .map(String::as_str)
    {
        Some("Manual") => Some("Manual"),
        Some("Automatic") => Some("Automatic"),
        _ => None,
    };
    Some(BackupEntry {
        id: file.id,
        name: file.name,
        created_time: file.created_time,
        size,
        kind,
        link_count: file
            .app_properties
            .get("linkmeLinkCount")
            .and_then(|value| value.parse().ok()),
    })
}

pub(super) fn list_backups(token: &str, max_size: usize) -> Result<Vec<BackupEntry>, AppError> {
    Ok(find_files(token, BACKUP_TAG)?
        .files
        .into_iter()
        .filter(|file| file.owned_by_me)
        .filter_map(|file| backup_entry(file, max_size))
        .collect())
}

pub(super) fn download_backup(token: &str, id: &str, max_size: usize) -> Result<Vec<u8>, AppError> {
    if id.is_empty()
        || id.len() > 200
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::new(
            "native_action",
            "Choose a Linkme backup from the list.",
        ));
    }
    let entry = list_backups(token, max_size)?
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| {
            AppError::new(
                "native_action",
                "This Linkme backup is no longer available. Refresh the list and try again.",
            )
        })?;
    let mut url = address(DRIVE_FILES);
    url.path_segments_mut()
        .map_err(|_| AppError::unexpected())?
        .push(id);
    url.query_pairs_mut().append_pair("alt", "media");
    let (status, bytes) = http::request("GET", &url, Some(token), None, &[], max_size)?;
    if status != 200 || bytes.len() != entry.size {
        return Err(http::network_error());
    }
    Ok(bytes)
}

fn backup_name(now: u64, kind: &str, suffix: &str) -> Result<String, AppError> {
    let date =
        chrono::DateTime::from_timestamp(now.try_into().map_err(|_| AppError::unexpected())?, 0)
            .ok_or_else(AppError::unexpected)?
            .format("%Y-%m-%d_%H-%M-%SZ");
    Ok(format!("Linkme_Backup_{date}_{kind}_{suffix}.sqlite3"))
}

pub(super) fn upload(
    token: &str,
    installation: &str,
    now: u64,
    automatic: bool,
    link_count: u32,
    snapshot: &[u8],
) -> Result<String, AppError> {
    let folder = backup_folder(token)?;
    let boundary = random_id()?;
    let kind = if automatic { "Automatic" } else { "Manual" };
    let metadata = serde_json::json!({
        "name": backup_name(now, kind, &boundary[..8])?,
        "parents": [folder],
        "mimeType": "application/vnd.sqlite3",
        "description": "Private Linkme library snapshot. Contains saved URLs and notes.",
        "appProperties": {
            "linkmeBackup": "v1",
            "linkmeInstallation": installation,
            "linkmeBackupType": kind,
            "linkmeLinkCount": link_count.to_string(),
        },
    });
    let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=utf-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: application/vnd.sqlite3\r\n\r\n").into_bytes();
    body.extend_from_slice(snapshot);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let url = address(
        "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,size",
    );
    let (status, bytes) = http::request(
        "POST",
        &url,
        Some(token),
        Some(&format!("multipart/related; boundary={boundary}")),
        &body,
        MAX_JSON,
    )?;
    let result: FileId = decode(status, &bytes)?;
    if result
        .size
        .as_deref()
        .and_then(|size| size.parse::<usize>().ok())
        != Some(snapshot.len())
        || result.id.is_empty()
    {
        return Err(http::network_error());
    }
    Ok(result.id)
}

pub(super) fn prune(
    token: &str,
    installation: &str,
    uploaded: &str,
    keep: usize,
) -> Result<(), AppError> {
    if installation.len() != 43
        || !installation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(AppError::unexpected());
    }
    let result = find_files(
        token,
        &format!(
            "{BACKUP_TAG} and appProperties has {{ key='linkmeInstallation' and value='{installation}' }}"
        ),
    )?;
    let candidates = expired_backups(&result, uploaded, keep).ok_or_else(http::network_error)?;
    for file in candidates {
        let mut url = address(DRIVE_FILES);
        url.path_segments_mut()
            .map_err(|_| AppError::unexpected())?
            .push(&file.id);
        let (status, _) = http::request(
            "PATCH",
            &url,
            Some(token),
            Some("application/json"),
            br#"{"trashed":true}"#,
            MAX_JSON,
        )?;
        if status == 404 {
            continue;
        }
        if !(200..300).contains(&status) {
            return Err(http::network_error());
        }
    }
    Ok(())
}

// Drive returns newest first. Do not trim an incomplete listing or one
// that has not caught up with the just-confirmed upload.
fn expired_backups<'a>(
    result: &'a RemoteBackups,
    uploaded: &str,
    keep: usize,
) -> Option<Vec<&'a ListedBackup>> {
    let owned: Vec<_> = result
        .files
        .iter()
        .filter(|file| file.owned_by_me && !file.shared)
        .collect();
    if result.incomplete_search
        || result.next_page_token.is_some()
        || !owned.iter().take(keep).any(|file| file.id == uploaded)
    {
        return None;
    }
    Some(owned.into_iter().skip(keep).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_completion_opens_linkme_only_after_success() {
        for (query, committed) in [
            ("code=test&state=right", true),
            ("code=test&state=right", false),
            ("error=access_denied&state=right", false),
        ] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                if let Ok((code, reply)) = wait_callback(listener, "right") {
                    assert_eq!(code, "test");
                    if committed {
                        reply.connected();
                    }
                } else {
                    assert_eq!(query, "error=access_denied&state=right");
                }
            });
            let mut browser = TcpStream::connect(address).unwrap();
            browser
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                browser,
                "GET /oauth/callback?{query} HTTP/1.1\r\nHost: {address}\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            browser.read_to_string(&mut response).unwrap();
            server.join().unwrap();
            assert!(response.contains("Cache-Control: no-store"));
            assert!(response.contains("Referrer-Policy: no-referrer"));
            assert_eq!(response.starts_with("HTTP/1.1 200 OK"), committed);
            assert_eq!(response.contains("linkme-desktop://open"), committed);
            assert!(!response.contains(query));
        }
    }

    #[test]
    fn backup_names_and_history_preserve_legacy_entries() {
        assert_eq!(
            backup_name(0, "Manual", "abc12345").unwrap(),
            "Linkme_Backup_1970-01-01_00-00-00Z_Manual_abc12345.sqlite3"
        );
        let base = serde_json::json!({
            "id":"file", "ownedByMe":true, "shared":false,
            "name":"Linkme - earlier.sqlite3", "size":"128"
        });
        let legacy = backup_entry(serde_json::from_value(base.clone()).unwrap(), 256).unwrap();
        assert_eq!(legacy.name, "Linkme - earlier.sqlite3");
        assert_eq!((legacy.kind, legacy.link_count), (None, None));
        let mut tagged = base;
        tagged["appProperties"] = serde_json::json!({
            "linkmeBackupType":"Automatic", "linkmeLinkCount":"12"
        });
        let current = backup_entry(serde_json::from_value(tagged).unwrap(), 256).unwrap();
        assert_eq!(
            (current.kind, current.link_count),
            (Some("Automatic"), Some(12))
        );
    }
    #[test]
    fn retention_requires_confirmed_upload_and_skips_shared_or_foreign_files() {
        let mut files: RemoteBackups = serde_json::from_value(serde_json::json!({"files": [
            {"id":"new","ownedByMe":true,"shared":false},
            {"id":"shared","ownedByMe":true,"shared":true},
            {"id":"foreign","ownedByMe":false,"shared":false},
            {"id":"old","ownedByMe":true,"shared":false}
        ]}))
        .unwrap();
        let expired = expired_backups(&files, "new", 1).unwrap();
        assert_eq!(
            expired
                .iter()
                .map(|file| file.id.as_str())
                .collect::<Vec<_>>(),
            ["old"]
        );
        assert!(expired_backups(&files, "missing", 1).is_none());
        files.next_page_token = Some("more".into());
        assert!(expired_backups(&files, "new", 1).is_none());
        files.next_page_token = None;
        files.incomplete_search = true;
        assert!(expired_backups(&files, "new", 1).is_none());
    }

    #[test]
    fn rejects_mismatched_state_and_unexpected_callback_paths() {
        assert_eq!(
            parse_callback(
                "GET /oauth/callback?code=test&state=right HTTP/1.1",
                "right"
            ),
            Some(Callback::Code("test".into()))
        );
        assert!(
            parse_callback("GET /oauth/callback?code=test&state=bad HTTP/1.1", "right").is_none()
        );
        assert!(parse_callback("GET /anything?code=test&state=right HTTP/1.1", "right").is_none());
        assert_eq!(
            parse_callback(
                "GET /oauth/callback?state=right&error=access_denied HTTP/1.1",
                "right"
            ),
            Some(Callback::Denied)
        );
        for query in [
            "error=access_denied&state=right-extra",
            "state=right&code=one&code=two",
            "state=right&state=wrong&code=one",
            "state=right&code=one&error=access_denied",
        ] {
            let line = format!("GET /oauth/callback?{query} HTTP/1.1");
            assert!(parse_callback(&line, "right").is_none());
        }
    }

    #[test]
    fn oauth_requires_file_scope_and_rejects_excess_permissions() {
        assert!(minimal_drive_scope(SCOPES));
        assert!(minimal_drive_scope(
            "openid https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/drive.file"
        ));
        assert!(!minimal_drive_scope("openid profile"));
        assert!(!minimal_drive_scope(&format!(
            "{SCOPES} https://www.googleapis.com/auth/drive"
        )));
    }

    #[test]
    fn embedded_client_works_without_a_file_and_takes_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let embedded =
            br#"{"client_id":"production.apps.googleusercontent.com","client_secret":"fixture"}"#;
        assert!(load_client(dir.path(), b"").is_err());
        assert_eq!(
            load_client(dir.path(), embedded).unwrap().client_id,
            "production.apps.googleusercontent.com"
        );
        std::fs::write(dir.path().join("google-oauth.json"),
            br#"{"installed":{"client_id":"local.apps.googleusercontent.com","client_secret":"fixture"}}"#,
        ).unwrap();
        assert_eq!(
            load_client(dir.path(), embedded).unwrap().client_id,
            "production.apps.googleusercontent.com"
        );
        assert_eq!(
            load_client(dir.path(), b"").unwrap().client_id,
            "local.apps.googleusercontent.com"
        );
        assert!(load_client(dir.path(), b"malformed").is_err());
    }

    #[test]
    fn export_filter_rejects_other_credential_types_and_embeds_only_desktop_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("export.json");
        let client = serde_json::json!({
            "client_id": "desktop.apps.googleusercontent.com",
            "client_secret": "fixture",
            "token_uri": "https://example.com",
            "refresh_token": "must-not-be-embedded"
        });
        for invalid in [
            serde_json::json!({"type": "service_account", "private_key": "fixture"}),
            serde_json::json!({"web": client}),
            serde_json::json!({"installed": client, "web": client}),
            serde_json::json!({"installed": {"client_id": "bad.apps.googleusercontent.com", "client_secret": "bad\nvalue"}}),
        ] {
            std::fs::write(&file, invalid.to_string()).unwrap();
            assert!(Client::from_export(&file).is_none());
        }
        std::fs::write(&file, vec![b' '; 32_769]).unwrap();
        assert!(Client::from_export(&file).is_none());
        std::fs::write(&file, serde_json::json!({"installed": client}).to_string()).unwrap();
        let embedded = serde_json::to_value(Client::from_export(&file).unwrap()).unwrap();
        assert_eq!(
            embedded,
            serde_json::json!({
                "client_id": "desktop.apps.googleusercontent.com",
                "client_secret": "fixture"
            })
        );
    }

    #[test]
    fn revoked_and_unavailable_authorization_cannot_return_an_access_token() {
        let expired = decode_token(400, br#"{"error":"invalid_grant"}"#)
            .err()
            .unwrap();
        assert!(expired.message.contains("Connect Google Drive again"));
        assert_eq!(expired.code, REAUTHORIZE);
        let malformed = decode_token(400, br#"{"error":"invalid_request"}"#)
            .err()
            .unwrap();
        assert_eq!(malformed.code, http::network_error().code);
        let unavailable = decode_token(503, b"").err().unwrap();
        assert_eq!(unavailable.message, http::network_error().message);
        assert!(decode_token(200, br#"{"access_token":""}"#).is_err());
        let valid = decode_token(200, br#"{"access_token":"synthetic-ok"}"#).unwrap();
        assert_eq!(valid.access_token, "synthetic-ok");
    }
}
