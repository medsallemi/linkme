use super::*;

fn source() -> Repository {
    Repository::parse("fixtures/linkme-test").unwrap()
}

fn release(tag: &str, version: &str) -> serde_json::Value {
    serde_json::json!({
        "tag_name": tag,
        "html_url": format!("https://github.com/fixtures/linkme-test/releases/tag/{tag}"),
        "published_at": "2026-11-20T12:00:00Z",
        "draft": false, "prerelease": false,
        "body": "Faster search.",
        "assets": [{
            "name": format!("Linkme_{version}_x64-setup.exe"), "state": "uploaded", "size": 1800,
            "browser_download_url": format!("https://github.com/fixtures/linkme-test/releases/download/{tag}/Linkme_{version}_x64-setup.exe")
        }]
    })
}

fn select(records: &[serde_json::Value], installed: &str) -> Option<Release> {
    latest_compatible(
        &source(),
        &Version::parse(installed).unwrap(),
        &serde_json::to_vec(records).unwrap(),
    )
    .unwrap()
}

#[test]
fn selects_newest_compatible_release_by_semver_precedence() {
    let records = [release("v0.10.0", "0.10.0"), release("v0.9.1", "0.9.1")];
    assert_eq!(select(&records, "0.9.0").unwrap().version, "0.10.0");
    assert!(select(&records, "0.10.0").is_none());
    assert!(select(&records, "1.0.0").is_none());
    assert!(select(&records, "0.10.0+local").is_none());
}

#[test]
fn skips_unpublished_incompatible_and_unverified_releases() {
    let mut draft = release("v2.0.0", "2.0.0");
    draft["draft"] = true.into();
    let mut pre = release("v2.0.1", "2.0.1");
    pre["prerelease"] = true.into();
    let mut unpublished = release("v2.0.2", "2.0.2");
    unpublished["published_at"] = serde_json::Value::Null;
    let mut empty = release("v2.0.3", "2.0.3");
    empty["assets"] = serde_json::json!([]);
    let mut arm = release("v2.0.4", "2.0.4");
    arm["assets"][0]["name"] = "Linkme_2.0.4_arm64-setup.exe".into();
    let mut alien = release("v2.0.5", "2.0.5");
    alien["html_url"] = "https://github.com/other/linkme-test/releases/tag/v2.0.5".into();
    let mut wrong_asset = release("v2.0.6", "2.0.6");
    wrong_asset["assets"][0]["browser_download_url"] =
        "https://github.com/other/installer.exe".into();
    let records = [
        draft,
        pre,
        unpublished,
        empty,
        arm,
        alien,
        wrong_asset,
        release("v3.0.0-rc.1", "3.0.0-rc.1"),
        release("v1.1.0", "1.1.0"),
    ];
    assert_eq!(select(&records, "1.0.0").unwrap().version, "1.1.0");
}

#[test]
fn rejects_malformed_oversized_and_unsafe_source_values() {
    let installed = Version::parse("1.0.0").unwrap();
    for payload in [
        b"{broken".to_vec(),
        b"{}".to_vec(),
        vec![b' '; MAX_BODY + 1],
    ] {
        assert!(latest_compatible(&source(), &installed, &payload).is_err());
    }
    for repository in [
        "",
        "https://github.com/owner/repo",
        "owner/../repo",
        "owner/.",
        "owner/repo?extra=1",
        "owner/repo%2fextra",
        "owner/repo#frag",
        "owner/repo.git",
        "bad:owner/repo",
    ] {
        assert!(Repository::parse(repository).is_none(), "{repository}");
    }
    let mut notes = release("v1.1.0", "1.1.0");
    notes["body"] = format!("<img src=x onerror=alert(1)>\u{202e}\n{}", "z".repeat(1500)).into();
    let notes = select(&[notes], "1.0.0").unwrap().notes;
    assert!(notes.starts_with("<img"));
    assert!(!notes.contains('\u{202e}'));
    assert_eq!(notes.chars().count(), 901);
}

#[test]
fn checks_once_per_opening_and_claims_once_even_after_remount() {
    let mut opening = Opening::default();
    let initial = opening.number;
    assert!(opening.claim(initial));
    assert!(!opening.claim(initial));
    assert!(opening.minimize(true));
    assert!(!opening.minimize(true));
    assert!(!opening.claim(initial + 1));
    assert!(!opening.minimize(false));
    assert_eq!(opening.number, initial + 1);
    assert!(!opening.claim(initial));
    assert!(opening.claim(initial + 1));
    opening.end();
    assert!(opening.claim(initial + 2));
}

fn wire(status: u16, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut data =
        format!("HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\n{headers}\r\n")
            .into_bytes();
    data.extend_from_slice(body);
    data
}

#[test]
fn revalidates_etags_and_respects_rate_limits_and_transport_failures() {
    let mut cache = Cache::default();
    let repo = source();
    let version = Version::parse("1.0.0").unwrap();
    let now = Instant::now();
    let payload = serde_json::to_vec(&[release("v1.1.0", "1.1.0")]).unwrap();
    assert_eq!(
        cache.record(
            &repo,
            &version,
            Ok(wire(200, "ETag: W/\"release\"\r\n", &payload)),
            now,
            1000
        ),
        Outcome::Available,
    );
    assert_eq!(cache.etag.as_deref(), Some("W/\"release\""));
    assert_eq!(
        cache.recent(false, now + Duration::from_secs(60)),
        Some(Outcome::Available)
    );
    assert_eq!(cache.recent(true, now + Duration::from_secs(60)), None);
    assert_eq!(
        cache.record(&repo, &version, Ok(wire(304, "", &[])), now, 1100),
        Outcome::Available,
    );
    assert_eq!(cache.checked_at, Some(1100));
    assert_eq!(
        cache.record(
            &repo,
            &version,
            Ok(wire(
                429,
                "Retry-After: 120\r\nX-RateLimit-Remaining: 0\r\nX-RateLimit-Reset: 1340\r\n",
                &[]
            )),
            now,
            1100
        ),
        Outcome::Failed,
    );
    assert_eq!(
        cache.recent(true, now + Duration::from_secs(239)),
        Some(Outcome::Failed)
    );
    assert_eq!(cache.recent(true, now + Duration::from_secs(240)), None);
    for error in [http::Failure::Unavailable, http::Failure::Timeout] {
        assert_eq!(
            cache.record(&repo, &version, Err(error), now, 2000),
            Outcome::Failed
        );
        assert_eq!(cache.checked_at, Some(1100));
    }
}

#[test]
fn does_not_accept_redirects_forged_etags_or_unbounded_wire_data() {
    assert!(http::Response::parse(&wire(200, "", &vec![b' '; MAX_BODY + 1])).is_none());
    let large_header = format!("X-Big: {}\r\n", "a".repeat(17 * 1024));
    assert!(http::Response::parse(&wire(200, &large_header, b"[]")).is_none());
    let forged = wire(200, "ETag: \"one\"\r\nETag: \"two\"\r\n", b"[]");
    assert!(http::Response::parse(&forged).unwrap().etag().is_none());
    let injected = wire(200, "ETag: \"bad value\"\r\n", b"[]");
    assert!(http::Response::parse(&injected).unwrap().etag().is_none());
    let mut cache = Cache::default();
    let repo = source();
    let version = Version::parse("0.9.0").unwrap();
    for response in [
        wire(301, "Location: https://github.com/other/repo\r\n", b""),
        wire(304, "", b""),
        wire(200, "", b"not json"),
    ] {
        assert_eq!(
            cache.record(&repo, &version, Ok(response), Instant::now(), 100),
            Outcome::Failed
        );
        assert!(cache.checked_at.is_none());
    }
}

#[test]
fn unconfigured_and_disabled_automatic_checks_do_not_use_the_transport() {
    async_runtime::block_on(async {
        let version = Version::parse("1.0.0").unwrap();
        let state = Arc::new(Updates::default());
        let unexpected = |_, _| panic!("contacted the network");
        assert_eq!(
            state.check(&version, None, unexpected).await.unwrap(),
            Outcome::Unconfigured
        );
        let state = Arc::new(Updates {
            repository: Some(source()),
            ..Updates::default()
        });
        state.cache.lock().unwrap().automatic = Some(false);
        assert_eq!(
            state.check(&version, Some(0), unexpected).await.unwrap(),
            Outcome::Skipped
        );
        assert_eq!(
            state
                .check(&version, None, |url, etag| {
                    assert_eq!(
                        url,
                        "https://api.github.com/repos/fixtures/linkme-test/releases?per_page=30"
                    );
                    assert!(etag.is_none());
                    Ok(wire(200, "", b"[]"))
                })
                .await
                .unwrap(),
            Outcome::Current
        );
    });
}

#[test]
fn simultaneous_checks_share_the_single_transport_request_and_open_a_canonical_page() {
    async_runtime::block_on(async {
        let state = Arc::new(Updates {
            repository: Some(source()),
            ..Updates::default()
        });
        state.cache.lock().unwrap().automatic = Some(true);
        let version = Version::parse("0.9.0").unwrap();
        let (started, receive) = async_runtime::channel(1);
        let (complete, waiting) = std::sync::mpsc::channel();
        let worker = state.clone();
        let installed = version.clone();
        let request = async_runtime::spawn(async move {
            worker
                .check(&installed, Some(0), move |_, _| {
                    started.blocking_send(()).unwrap();
                    waiting.recv().unwrap();
                    let response = serde_json::to_vec(&[release("v0.10.0", "0.10.0")]).unwrap();
                    Ok(wire(200, "ETag: \"1\"\r\n", &response))
                })
                .await
                .unwrap()
        });
        let mut receive = receive;
        receive.recv().await.unwrap();
        let manual = state.clone();
        let installed = version.clone();
        let duplicate = async_runtime::spawn(async move {
            manual
                .check(&installed, None, |_, _| panic!("duplicate request"))
                .await
                .unwrap()
        });
        complete.send(()).unwrap();
        assert_eq!(request.await.unwrap(), Outcome::Available);
        assert_eq!(duplicate.await.unwrap(), Outcome::Available);
        assert_eq!(
            state
                .check(&version, Some(0), |_, _| panic!("rechecked same opening"))
                .await
                .unwrap(),
            Outcome::Available
        );
        open_cached_release(&state, |url| {
            assert_eq!(
                url,
                "https://github.com/fixtures/linkme-test/releases/tag/v0.10.0"
            );
            Ok(())
        })
        .unwrap();
        assert!(
            open_cached_release(&Updates::default(), |_| panic!("opened missing release")).is_err()
        );
    });
}

#[test]
fn automatic_preference_persists_and_resets_with_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let db = crate::storage::Database::open(&path).unwrap();
    assert!(db.update_preference(None).unwrap());
    assert!(!db.update_preference(Some(false)).unwrap());
    drop(db);
    let mut db = crate::storage::Database::open(&path).unwrap();
    assert!(!db.update_preference(None).unwrap());
    db.reset(&path).unwrap();
    assert!(db.update_preference(None).unwrap());
}
