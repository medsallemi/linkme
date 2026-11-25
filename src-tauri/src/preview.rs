use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(any(windows, test))]
use std::net::IpAddr;

use base64::{Engine, engine::general_purpose::STANDARD};
use html5ever::tokenizer::{
    BufferQueue, CharacterTokens, EndTag, StartTag, TagToken, Token, TokenSink, TokenSinkResult,
    Tokenizer, states::RawKind,
};
use serde::Serialize;
use tauri::State;
use url::{Host, Url};

use crate::{SharedStorage, database_task, storage::AppError};

#[derive(Default)]
pub struct PreviewState(AtomicBool);

#[derive(Default)]
pub struct SiteIconState(AtomicBool);

struct RequestGuard<'a>(&'a AtomicBool);
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Default, Serialize)]
pub struct Metadata {
    title: String,
    description: String,
    image: Option<String>,
}

fn unavailable() -> AppError {
    AppError::new("preview_unavailable", "Page preview unavailable.")
}

fn destination(input: &str) -> Option<Url> {
    let mut url = Url::parse(input).ok()?;
    if url.scheme() == "http" {
        url.set_scheme("https").ok()?;
    }
    let Host::Domain(domain) = url.host()? else {
        return None;
    };
    let domain = domain.trim_end_matches('.');
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !domain.contains('.')
        || !domain.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        || [
            ".localhost",
            ".local",
            ".internal",
            ".home",
            ".test",
            ".invalid",
            ".onion",
            ".arpa",
        ]
        .iter()
        .any(|suffix| domain.ends_with(suffix))
    {
        return None;
    }
    url.set_fragment(None);
    Some(url)
}

#[cfg(any(windows, test))]
fn public_ip(ip: IpAddr) -> bool {
    let IpAddr::V4(ip) = ip else { return false };
    !matches!(
        ip.octets(),
        [0 | 10 | 127 | 224..=255, ..]
            | [100, 64..=127, ..]
            | [169, 254, ..]
            | [172, 16..=31, ..]
            | [192, 0 | 168, ..]
            | [192, 88, 99, _]
            | [198, 18 | 19, ..]
            | [198, 51, 100, _]
            | [203, 0, 113, _]
    )
}

#[derive(Default)]
struct PageHead {
    metadata: Metadata,
    social_title: String,
    image_url: Option<String>,
    in_title: bool,
    done: bool,
}

fn clean_text(value: &str, limit: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

struct HeadSink(RefCell<PageHead>);

impl TokenSink for HeadSink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        use TokenSinkResult::{Continue, RawData};
        let mut head = self.0.borrow_mut();
        if head.done {
            return Continue;
        }
        match token {
            TagToken(tag) if tag.kind == StartTag => match tag.name.as_ref() {
                "script" => return RawData(RawKind::ScriptData),
                "style" => return RawData(RawKind::Rawtext),
                "body" => head.done = true,
                "title" => {
                    head.in_title = true;
                    return RawData(RawKind::Rcdata);
                }
                "meta" => {
                    let attr = |name: &str| {
                        tag.attrs
                            .iter()
                            .find(|attr| attr.name.local.as_ref() == name)
                            .map(|attr| attr.value.as_ref())
                    };
                    let kind = attr("property")
                        .or_else(|| attr("name"))
                        .unwrap_or_default();
                    if head.image_url.is_none()
                        && matches!(kind, "og:image" | "og:image:secure_url" | "twitter:image")
                    {
                        head.image_url = attr("content")
                            .map(str::trim)
                            .filter(|value| !value.is_empty() && value.len() <= 2048)
                            .map(str::to_owned);
                    }
                    if head.social_title.is_empty() && matches!(kind, "og:title" | "twitter:title")
                    {
                        head.social_title = clean_text(attr("content").unwrap_or_default(), 160);
                    }
                    if head.metadata.description.is_empty()
                        && (attr("name")
                            .is_some_and(|name| name.eq_ignore_ascii_case("description"))
                            || matches!(kind, "og:description" | "twitter:description"))
                    {
                        head.metadata.description =
                            clean_text(attr("content").unwrap_or_default(), 300);
                    }
                }
                _ => {}
            },
            TagToken(tag) if tag.kind == EndTag && tag.name.as_ref() == "head" => head.done = true,
            TagToken(tag) if tag.kind == EndTag && tag.name.as_ref() == "title" => {
                head.in_title = false
            }
            CharacterTokens(text) if head.in_title => head.metadata.title.push_str(&text),
            _ => {}
        }
        Continue
    }
}

fn read_head(bytes: &[u8]) -> (Metadata, Option<String>) {
    let input = BufferQueue::default();
    input.push_back(String::from_utf8_lossy(bytes).as_ref().into());
    let tokenizer = Tokenizer::new(
        HeadSink(RefCell::new(PageHead::default())),
        Default::default(),
    );
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let head = tokenizer.sink.0.into_inner();
    let mut result = head.metadata;
    if !head.social_title.is_empty() {
        result.title = head.social_title;
    }
    result.title = clean_text(&result.title, 160);
    (result, head.image_url)
}

// YouTube watch pages often return a JavaScript shell. Its public oEmbed endpoint
// returns the video title without loading the player or sending timestamp/tracking fields.
fn youtube_metadata_url(url: &Url) -> Option<Url> {
    let id = match url.host_str()? {
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => match url.path() {
            "/watch" => url
                .query_pairs()
                .find(|(key, _)| key == "v")?
                .1
                .into_owned(),
            path => path
                .strip_prefix("/shorts/")
                .or_else(|| path.strip_prefix("/live/"))
                .or_else(|| path.strip_prefix("/embed/"))?
                .to_owned(),
        },
        "youtu.be" => url.path().strip_prefix('/')?.to_owned(),
        _ => return None,
    };
    if id.len() != 11
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return None;
    }
    let mut endpoint = Url::parse("https://www.youtube.com/oembed").ok()?;
    endpoint
        .query_pairs_mut()
        .append_pair("format", "json")
        .append_pair("url", &format!("https://www.youtube.com/watch?v={id}"));
    Some(endpoint)
}

const IMAGE_LIMIT: usize = 512 * 1024;
const ICON_LIMIT: usize = 64 * 1024;
const ICON_TYPES: &str =
    "image/x-icon, image/vnd.microsoft.icon, image/png, image/jpeg, application/octet-stream";

fn raster_dimensions(bytes: &[u8], mime: &str) -> Option<(u32, u32)> {
    if mime == "image/png" && bytes.starts_with(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR") {
        return Some((
            u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?),
            u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?),
        ));
    }
    if mime != "image/jpeg" || !bytes.starts_with(b"\xff\xd8") {
        return None;
    }
    let mut segments = &bytes[2..];
    while let Some(rest) = segments.strip_prefix(&[0xff]) {
        let (&marker, data) = rest.split_first()?;
        if marker == 0xff {
            segments = rest;
            continue;
        }
        if matches!(marker, 0xda | 0xd9) {
            return None;
        }
        let length = u16::from_be_bytes(data.get(..2)?.try_into().ok()?) as usize;
        let segment = data.get(2..length)?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let height = u16::from_be_bytes(segment.get(1..3)?.try_into().ok()?);
            let width = u16::from_be_bytes(segment.get(3..5)?.try_into().ok()?);
            return Some((width.into(), height.into()));
        }
        segments = &data[length..];
    }
    None
}

fn complete_static_png(bytes: &[u8]) -> Option<()> {
    let mut chunks = bytes.strip_prefix(b"\x89PNG\r\n\x1a\n")?;
    let mut has_pixels = false;
    loop {
        let length = u32::from_be_bytes(chunks.get(..4)?.try_into().ok()?) as usize;
        let kind = chunks.get(4..8)?;
        chunks = chunks.get(12usize.checked_add(length)?..)?;
        match kind {
            b"acTL" | b"fcTL" | b"fdAT" | b"iCCP" | b"iTXt" | b"zTXt" => return None,
            b"IDAT" => has_pixels = true,
            b"IEND" => return (length == 0 && chunks.is_empty() && has_pixels).then_some(()),
            _ => {}
        }
    }
}

pub(crate) fn preview_image_data(bytes: &[u8], mime: &str) -> Option<String> {
    if bytes.len() > IMAGE_LIMIT {
        return None;
    }
    let complete = match mime {
        "image/jpeg" => bytes.ends_with(b"\xff\xd9"),
        "image/png" => complete_static_png(bytes).is_some(),
        _ => false,
    };
    if !complete {
        return None;
    }
    let (width, height) = raster_dimensions(bytes, mime)?;
    if width == 0 || height == 0 || width > 2048 || height > 2048 {
        return None;
    }
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn icon_destination(input: &str) -> Option<Url> {
    let mut url = destination(input)?;
    url.set_path("/favicon.ico");
    url.set_query(None);
    Some(url)
}

fn icon_data(bytes: &[u8], mime: &str) -> Option<String> {
    if bytes.len() > ICON_LIMIT {
        return None;
    }
    if matches!(mime, "image/png" | "image/jpeg") {
        let (width, height) = raster_dimensions(bytes, mime)?;
        return (width <= 256 && height <= 256)
            .then(|| preview_image_data(bytes, mime))
            .flatten();
    }
    if !matches!(
        mime,
        "image/x-icon" | "image/vnd.microsoft.icon" | "application/octet-stream"
    ) || !bytes.starts_with(&[0, 0, 1, 0])
    {
        return None;
    }
    let count = u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?) as usize;
    if !(1..=16).contains(&count) {
        return None;
    }
    let directory_end = 6 + count * 16;
    for entry in bytes.get(6..directory_end)?.as_chunks::<16>().0 {
        let dimension = |value: u8| if value == 0 { 256 } else { u32::from(value) };
        let (width, height) = (dimension(entry[0]), dimension(entry[1]));
        let size = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        if offset < directory_end {
            return None;
        }
        let image = bytes.get(offset..offset.checked_add(size)?)?;
        if let Some(dimensions) = raster_dimensions(image, "image/png") {
            if dimensions != (width, height) || complete_static_png(image).is_none() {
                return None;
            }
        } else {
            // Windows ICO bitmaps encode the XOR and AND masks together, at twice the height.
            let dib = u32::from_le_bytes(image.get(..4)?.try_into().ok()?) as usize;
            if !matches!(dib, 40 | 108 | 124)
                || image.len() <= dib
                || u32::from_le_bytes(image.get(4..8)?.try_into().ok()?) != width
                || u32::from_le_bytes(image.get(8..12)?.try_into().ok()?) != 2 * height
                || image.get(16..20)? != [0, 0, 0, 0]
            {
                return None;
            }
        }
    }
    Some(format!(
        "data:image/x-icon;base64,{}",
        STANDARD.encode(bytes)
    ))
}

fn preview_image(url: Url) -> Option<String> {
    let (_, bytes, mime) = fetch_body(url, "image/jpeg, image/png", IMAGE_LIMIT, 2).ok()?;
    preview_image_data(&bytes, &mime)
}

fn fetch_metadata(url: Url) -> Result<Metadata, AppError> {
    if let Some(endpoint) = youtube_metadata_url(&url) {
        let (endpoint, body, _) = fetch_body(endpoint, "application/json", 64 * 1024, 5)?;
        let json: serde_json::Value = serde_json::from_slice(&body).map_err(|_| unavailable())?;
        let image = json["thumbnail_url"]
            .as_str()
            .and_then(|value| endpoint.join(value).ok())
            .and_then(|url| destination(url.as_str()))
            .and_then(preview_image);
        return Ok(Metadata {
            title: clean_text(json["title"].as_str().unwrap_or_default(), 160),
            description: clean_text(json["author_name"].as_str().unwrap_or_default(), 300),
            image,
        });
    }
    let path = url.path().to_ascii_lowercase();
    if [".jpg", ".jpeg", ".png"]
        .iter()
        .any(|extension| path.ends_with(extension))
    {
        return Ok(Metadata {
            image: preview_image(url),
            ..Metadata::default()
        });
    }
    let (page, body, _) = fetch_body(url, "text/html", 64 * 1024, 5)?;
    let (mut metadata, image_url) = read_head(&body);
    metadata.image = image_url
        .and_then(|value| page.join(&value).ok())
        .and_then(|url| destination(url.as_str()))
        .and_then(preview_image);
    Ok(metadata)
}

#[tauri::command]
pub async fn preview_preference(
    storage: State<'_, SharedStorage>,
    enabled: Option<bool>,
) -> Result<bool, AppError> {
    database_task(storage.inner().clone(), move |db| {
        db.preview_preference(enabled)
    })
    .await
}

#[tauri::command]
pub async fn preview_link(
    storage: State<'_, SharedStorage>,
    preview: State<'_, PreviewState>,
    id: i64,
) -> Result<Metadata, AppError> {
    if preview.0.swap(true, Ordering::AcqRel) {
        return Err(AppError::new("preview_busy", "Another preview is loading."));
    }
    let _guard = RequestGuard(&preview.0);
    let url = database_task(storage.inner().clone(), move |db| {
        if !db.preview_preference(None)? {
            return Err(unavailable());
        }
        destination(&db.validated_url(id)?).ok_or_else(unavailable)
    })
    .await?;
    let metadata = tauri::async_runtime::spawn_blocking(move || fetch_metadata(url))
        .await
        .map_err(|_| unavailable())??;
    if metadata.title.is_empty() && metadata.description.is_empty() && metadata.image.is_none() {
        Err(unavailable())
    } else {
        Ok(metadata)
    }
}

#[tauri::command]
pub async fn site_icon(
    storage: State<'_, SharedStorage>,
    icons: State<'_, SiteIconState>,
    id: i64,
) -> Result<String, AppError> {
    if icons.0.swap(true, Ordering::AcqRel) {
        return Err(unavailable());
    }
    let _guard = RequestGuard(&icons.0);
    let url = database_task(storage.inner().clone(), move |db| {
        icon_destination(&db.validated_url(id)?).ok_or_else(unavailable)
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        let (_, bytes, mime) = fetch_body(url, ICON_TYPES, ICON_LIMIT, 2)?;
        icon_data(&bytes, &mime).ok_or_else(unavailable)
    })
    .await
    .map_err(|_| unavailable())?
}

#[cfg(windows)]
fn fetch_body(
    mut url: Url,
    mime: &str,
    limit: usize,
    seconds: u64,
) -> Result<(Url, Vec<u8>, String), AppError> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    for hop in 0..=3 {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let response = request_once(&url, mime, remaining, limit)?;
        if let Some(location) = redirect_location(&response) {
            if hop == 3 {
                break;
            }
            let next = url.join(location).map_err(|_| unavailable())?;
            url = destination(next.as_str()).ok_or_else(unavailable)?;
            continue;
        }
        return bounded_body(&response, mime, limit)
            .map(|(body, content_type)| (url, body.to_vec(), content_type.to_owned()))
            .ok_or_else(unavailable);
    }
    Err(unavailable())
}

#[cfg(windows)]
fn request_once(
    url: &Url,
    mime: &str,
    remaining: std::time::Duration,
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    use std::{
        io::{Read, Write},
        net::ToSocketAddrs,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };

    let deadline = std::time::Instant::now() + remaining;
    let host = url.host_str().ok_or_else(unavailable)?;
    let addresses = (host, 443)
        .to_socket_addrs()
        .map_err(|_| unavailable())?
        .map(|address| address.ip())
        .filter(|ip| public_ip(*ip))
        .take(8)
        .map(|ip| ip.to_string())
        .collect::<Vec<_>>();
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if addresses.is_empty() || remaining.is_zero() {
        return Err(unavailable());
    }
    let curl = crate::native::system_curl().ok_or_else(unavailable)?;
    // Supply the checked URL through stdin so it stays out of the process command line.
    let target = url.as_str();
    if target.contains(['"', '\\', '\r', '\n']) {
        return Err(unavailable());
    }
    let mut child = Command::new(curl)
        .args([
            "-q",
            "--config",
            "-",
            "--silent",
            "--fail",
            "--globoff",
            "--ipv4",
            "--noproxy",
            "*",
            "--proto",
            "=https",
            "--max-time",
            &format!("{:.3}", remaining.as_secs_f64().max(0.001)),
            "--header",
            &format!("Accept: {mime}"),
            "--header",
            "Accept-Encoding: identity",
            "--include",
            "--http1.1",
            "--resolve",
            &format!("{host}:443:{}", addresses.join(",")),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|_| unavailable())?;
    let wire_limit = limit + 8192;
    let result = (|| {
        let mut input = child.stdin.take().ok_or_else(unavailable)?;
        writeln!(input, "url = \"{target}\"").map_err(|_| unavailable())?;
        drop(input);
        let mut response = Vec::new();
        let mut output = child
            .stdout
            .take()
            .ok_or_else(unavailable)?
            .take((wire_limit + 1) as u64);
        output
            .read_to_end(&mut response)
            .map_err(|_| unavailable())?;
        if response.len() > wire_limit && mime != "text/html" {
            return Err(unavailable());
        }
        Ok(response)
    })();
    let stopped_after_prefix = result
        .as_ref()
        .is_ok_and(|response| response.len() > wire_limit && mime == "text/html");
    if result.is_err() || stopped_after_prefix {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|_| unavailable())?;
    if !status.success() && !stopped_after_prefix {
        return Err(unavailable());
    }
    result
}

#[cfg(not(windows))]
fn fetch_body(_: Url, _: &str, _: usize, _: u64) -> Result<(Url, Vec<u8>, String), AppError> {
    Err(unavailable())
}

#[cfg(any(windows, test))]
fn redirect_location(response: &[u8]) -> Option<&str> {
    let end = response.windows(4).position(|s| s == b"\r\n\r\n")?;
    if end > 8192 {
        return None;
    }
    let mut lines = std::str::from_utf8(&response[..end]).ok()?.lines();
    if !matches!(
        lines.next()?.split_whitespace().nth(1)?,
        "301" | "302" | "303" | "307" | "308"
    ) {
        return None;
    }
    lines
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("location"))
        .map(|(_, url)| url.trim())
}

#[cfg(any(windows, test))]
fn response_body<'a, 'b>(response: &'a [u8], expected: &'b str) -> Option<(&'a [u8], &'b str)> {
    let separator = response.windows(4).position(|part| part == b"\r\n\r\n")?;
    if separator > 8192 {
        return None;
    }
    let headers = std::str::from_utf8(&response[..separator]).ok()?;
    let mut lines = headers.lines();
    if lines.next()?.split_whitespace().nth(1)? != "200" {
        return None;
    }
    if lines
        .clone()
        .filter_map(|line| line.split_once(':'))
        .any(|(key, value)| {
            key.eq_ignore_ascii_case("content-encoding")
                && !value.trim().eq_ignore_ascii_case("identity")
        })
    {
        return None;
    }
    let content_type = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))?
        .1
        .split(';')
        .next()?
        .trim();
    let accepted = expected
        .split(',')
        .map(str::trim)
        .find(|mime| content_type.eq_ignore_ascii_case(mime))?;
    Some((&response[separator + 4..], accepted))
}

#[cfg(any(windows, test))]
fn bounded_body<'a, 'b>(
    response: &'a [u8],
    expected: &'b str,
    limit: usize,
) -> Option<(&'a [u8], &'b str)> {
    let (body, kind) = response_body(response, expected)?;
    if kind == "text/html" {
        Some((&body[..body.len().min(limit)], kind))
    } else {
        (body.len() <= limit).then_some((body, kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_restricts_network_destinations_and_preserves_local_links() {
        assert_eq!(
            destination("https://example.org/path#note")
                .unwrap()
                .as_str(),
            "https://example.org/path"
        );
        for url in [
            "https://user:password@example.org/path?q=1",
            "ftp://example.org",
            "https://127.0.0.1",
            "https://[::1]",
            "https://example.org:444",
            "https://user@example.org",
            "https://printer.local.",
        ] {
            assert!(destination(url).is_none(), "{url}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "192.168.1.1",
            "100.64.0.1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn previews_keep_public_queries_and_only_send_the_video_id_to_youtube() {
        assert_eq!(
            destination("http://example.org/path?q=1").unwrap().as_str(),
            "https://example.org/path?q=1"
        );
        assert_eq!(
            redirect_location(b"HTTP/1.1 307 Temporary Redirect\r\nLocation: /next?q=1\r\n\r\n"),
            Some("/next?q=1")
        );
        assert_eq!(read_head(br#"<meta property="og:title" content="Social title"><meta name="twitter:description" content="Summary">"#).0.title, "Social title");
        let ordinary = destination("https://example.org/find?q=blue&sort=new#private").unwrap();
        assert_eq!(
            ordinary.as_str(),
            "https://example.org/find?q=blue&sort=new"
        );
        assert!(youtube_metadata_url(&ordinary).is_none());
        for link in [
            "https://www.youtube.com/watch?v=abcdefghijk&t=273s",
            "https://youtu.be/abcdefghijk",
            "https://m.youtube.com/shorts/abcdefghijk",
        ] {
            let endpoint = youtube_metadata_url(&destination(link).unwrap()).unwrap();
            assert_eq!(endpoint.host_str(), Some("www.youtube.com"));
            assert_eq!(
                endpoint.query_pairs().last().unwrap().1,
                "https://www.youtube.com/watch?v=abcdefghijk"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "contacts example.org; run explicitly for a Windows network smoke test"]
    fn windows_public_preview_smoke() {
        let url = destination("https://example.org/").unwrap();
        let head = fetch_body(url, "text/html", 64 * 1024, 5).unwrap().1;
        assert_eq!(read_head(&head).0.title, "Example Domain");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "contacts YouTube for a public demonstration video"]
    fn windows_youtube_preview_smoke() {
        let url = destination("https://www.youtube.com/watch?v=jNQXAC9IVRw&t=1").unwrap();
        let metadata = fetch_metadata(url).unwrap();
        assert!(metadata.title.contains("Me at the zoo"));
        assert!(
            metadata
                .image
                .as_deref()
                .is_some_and(|image| image.starts_with("data:image/jpeg;base64,"))
        );
    }

    #[test]
    fn preview_images_are_raster_only_and_dimension_bounded() {
        let mut png = include_bytes!("../icons/32x32.png").to_vec();
        assert!(
            preview_image_data(&png, "image/png")
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(preview_image_data(&png, "image/svg+xml").is_none());
        let mut animated = png.clone();
        animated.splice(
            33..33,
            *b"\x00\x00\x00\x08acTL\x00\x00\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00",
        );
        assert!(preview_image_data(&animated, "image/png").is_none());
        let mut compressed_metadata = animated.clone();
        compressed_metadata[37..41].copy_from_slice(b"zTXt");
        assert!(preview_image_data(&compressed_metadata, "image/png").is_none());
        assert!(preview_image_data(&png[..png.len() - 1], "image/png").is_none());
        png[18] = 32;
        assert!(preview_image_data(&png, "image/png").is_none());
        let (_, image) = read_head(br#"<meta property="og:image" content="/hero.jpg">"#);
        assert_eq!(image.as_deref(), Some("/hero.jpg"));
    }

    #[test]
    fn only_successful_html_is_accepted() {
        assert!(
            response_body(
                b"HTTP/1.1 302 Found\r\nContent-Type: text/html\r\n\r\nbody",
                "text/html"
            )
            .is_none()
        );
        assert!(
            response_body(
                b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\nbody",
                "text/html"
            )
            .is_none()
        );
        assert_eq!(
            response_body(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=UTF-8\r\n\r\nbody",
                "text/html"
            ),
            Some((b"body".as_slice(), "text/html"))
        );
    }

    #[test]
    fn caps_network_payloads_and_reads_only_a_bounded_html_prefix() {
        let html = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\nabcdef";
        assert_eq!(bounded_body(html, "text/html", 3).unwrap().0, b"abc");
        let icon = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\nabcdef";
        assert!(bounded_body(icon, ICON_TYPES, 3).is_none());
        let encoded =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Encoding: gzip\r\n\r\nabcdef";
        assert!(bounded_body(encoded, "text/html", 3).is_none());
    }

    #[test]
    fn favicon_uses_only_the_saved_origin_and_raster_icon_sizes() {
        assert_eq!(
            icon_destination("http://example.org/path?q=private#local")
                .unwrap()
                .as_str(),
            "https://example.org/favicon.ico"
        );
        assert!(icon_destination("https://router.internal./a").is_none());
        let png = include_bytes!("../icons/32x32.png");
        let mut icon = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
        icon.extend_from_slice(&(png.len() as u32).to_le_bytes());
        icon.extend_from_slice(&22u32.to_le_bytes());
        icon.extend_from_slice(png);
        assert!(icon_data(&icon, "image/vnd.microsoft.icon").is_some());
        assert!(icon_data(&icon, "image/svg+xml").is_none());
        assert!(icon_data(&icon[..icon.len() - 1], "image/x-icon").is_none());
        icon[6] = 16;
        assert!(icon_data(&icon, "image/x-icon").is_none());
    }

    #[test]
    fn metadata_is_plain_text_and_script_contents_are_ignored() {
        let (parsed, _) = read_head(br#"<head><script>var a = '<meta name="description" content="wrong">';</script><title>A &amp; B</title><meta name="description" content=" Useful   text. "></head><title>ignored</title>"#);
        assert_eq!(parsed.title, "A & B");
        assert_eq!(parsed.description, "Useful text.");
    }
}
