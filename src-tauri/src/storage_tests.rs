use crate::models::LinkDraft;
fn draft(url: &str) -> LinkDraft {
    LinkDraft {
        url: url.into(),
        category_id: 1,
        ..Default::default()
    }
}
fn edit(link: &Link) -> LinkDraft {
    LinkDraft {
        id: Some(link.id),
        revision: Some(link.revision),
        url: link.url.clone(),
        title: link.title.clone(),
        subtitle: link.subtitle.clone(),
        caption: link.caption.clone(),
        category_id: link.category_id,
        group_id: link.group_id,
        new_category: None,
        new_category_icon: None,
    }
}
impl Database {
    fn save_url(&mut self, url: &str) -> Result<(i64, bool), AppError> {
        self.save(&draft(url)).map(|r| (r.link.id, r.created))
    }
    fn list_ids(&self, before: Option<i64>) -> Result<Vec<(i64, String)>, AppError> {
        self.list("", before.map(|id| Cursor { rank: 0, id }))
            .map(|p| p.links.into_iter().map(|l| (l.id, l.url)).collect())
    }
}
use super::*;

#[test]
fn capture_settings_default_then_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("links.sqlite3");
    let mut db = Database::open(&path).unwrap();
    assert_eq!(db.capture_settings().unwrap(), CaptureSettings::default());
    let settings = CaptureSettings {
        mode: CaptureMode::Quick,
        shortcut: "Ctrl+Shift+J".into(),
        search_shortcut: "Ctrl+Shift+M".into(),
        dim_percent: 27,
        panel_transparency: 25,
        highlight: 7,
    };
    let mut other = Database::open(&path).unwrap();
    let original = other.capture_settings().unwrap();
    let invalid_highlight = CaptureSettings {
        highlight: 8,
        ..original.clone()
    };
    assert_eq!(
        db.save_capture_settings(&invalid_highlight, &original)
            .unwrap_err()
            .code,
        "invalid_setting"
    );
    assert_eq!(db.capture_settings().unwrap(), original);
    db.save_capture_settings(&settings, &original).unwrap();
    assert_eq!(
        other
            .save_capture_settings(&original, &original)
            .unwrap_err()
            .code,
        "conflict"
    );
    drop(other);
    drop(db);
    let db = Database::open(&path).unwrap();
    assert_eq!(db.capture_settings().unwrap(), settings);
    assert!(db.list("", None).unwrap().links.is_empty());
}

#[test]
fn old_capture_settings_keep_their_binding_and_gain_a_search_default() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    db.connection.execute_batch("CREATE TABLE capture_settings(id INTEGER PRIMARY KEY,mode TEXT NOT NULL,shortcut TEXT NOT NULL);
        INSERT INTO capture_settings VALUES(1,'quick','Ctrl+Shift+J');").unwrap();
    let expected = db.capture_settings().unwrap();
    assert_eq!(expected.dim_percent, 40);
    assert_eq!(expected.panel_transparency, 25);
    assert_eq!(expected.highlight, 7);
    assert_eq!(
        (
            expected.mode,
            expected.shortcut.as_str(),
            expected.search_shortcut.as_str()
        ),
        (CaptureMode::Quick, "Ctrl+Shift+J", "Ctrl+Shift+K")
    );
    let changed = CaptureSettings {
        search_shortcut: "Ctrl+Shift+R".into(),
        ..expected.clone()
    };
    db.save_capture_settings(&changed, &expected).unwrap();
    assert_eq!(db.capture_settings().unwrap(), changed);
}

#[test]
fn capture_dimming_rejects_out_of_range_values_without_changing_settings() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let original = db.capture_settings().unwrap();
    let bad = CaptureSettings {
        dim_percent: 41,
        ..original.clone()
    };
    assert_eq!(
        db.save_capture_settings(&bad, &original).unwrap_err().code,
        "invalid_setting"
    );
    assert_eq!(db.capture_settings().unwrap(), original);

    let off = CaptureSettings {
        dim_percent: 0,
        ..original.clone()
    };
    db.save_capture_settings(&off, &original).unwrap();
    assert_eq!(db.capture_settings().unwrap(), off);
    assert!(
        db.connection
            .execute("UPDATE capture_settings SET dim_percent=41", [])
            .is_err()
    );
}

#[test]
fn capture_panel_transparency_preserves_previous_dimming_and_has_its_own_bound() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    db.connection.execute_batch("CREATE TABLE capture_settings(id INTEGER PRIMARY KEY,mode TEXT NOT NULL,shortcut TEXT NOT NULL,search_shortcut TEXT NOT NULL,dim_percent INTEGER NOT NULL DEFAULT 20 CHECK(dim_percent BETWEEN 0 AND 40)); INSERT INTO capture_settings VALUES(1,'prompt','Ctrl+Shift+L','Ctrl+Shift+K',8);").unwrap();
    let original = db.capture_settings().unwrap();
    assert_eq!((original.dim_percent, original.panel_transparency), (8, 25));
    let bad = CaptureSettings {
        panel_transparency: 26,
        ..original.clone()
    };
    assert_eq!(
        db.save_capture_settings(&bad, &original).unwrap_err().code,
        "invalid_setting"
    );
    assert_eq!(db.capture_settings().unwrap(), original);

    let solid = CaptureSettings {
        panel_transparency: 0,
        ..original.clone()
    };
    db.save_capture_settings(&solid, &original).unwrap();
    assert_eq!(db.capture_settings().unwrap(), solid);
    assert!(
        db.connection
            .execute("UPDATE capture_settings SET panel_transparency=26", [])
            .is_err()
    );
}

#[test]
fn migrates_url_only_schema_without_changing_saved_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE categories(id INTEGER PRIMARY KEY,name TEXT NOT NULL UNIQUE); INSERT INTO categories VALUES(1,'Inbox'); CREATE TABLE links(id INTEGER PRIMARY KEY,url TEXT NOT NULL UNIQUE,category_id INTEGER NOT NULL REFERENCES categories(id),created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL); INSERT INTO links VALUES(41,'https://Example.com/?b=2&a=1#Keep',1,123,456); PRAGMA user_version=1;").unwrap();
    drop(connection);
    let db = Database::open(&path).unwrap();
    let row = db
        .connection
        .query_row(
            "SELECT id,url,title,subtitle,caption,revision,created_at,updated_at FROM links",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        (
            41,
            "https://Example.com/?b=2&a=1#Keep".into(),
            "".into(),
            "".into(),
            "".into(),
            0,
            123,
            456
        )
    );
    assert_eq!(
        db.connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        6
    );
}

#[test]
fn commits_and_survives_connection_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let url = "https://example.com/?a=1&b=2#Keep";
    let id;
    {
        let mut db = Database::open(&path).unwrap();
        (id, _) = db.save_url(url).unwrap();
    }
    assert_eq!(
        Database::open(&path).unwrap().list_ids(None).unwrap(),
        vec![(id, url.into())]
    );
}

#[test]
fn duplicates_return_existing_id_without_new_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(&dir.path().join("links.sqlite3")).unwrap();
    let (id, created) = db.save_url("https://example.com/?q=';DROP").unwrap();
    assert!(created);
    assert_eq!(
        db.save_url("https://example.com/?q=';DROP").unwrap(),
        (id, false)
    );
    assert_eq!(db.list_ids(None).unwrap().len(), 1);
}

#[test]
fn validates_before_writing_and_bounds_pages() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(&dir.path().join("links.sqlite3")).unwrap();
    assert!(db.save_url("javascript:alert(1)").is_err());
    for n in 0..105 {
        db.save_url(&format!("https://example.com/{n}")).unwrap();
    }
    let page = db.list_ids(None).unwrap();
    assert_eq!(page.len(), 100);
    assert!(page.windows(2).all(|rows| rows[0].0 > rows[1].0));
    let rest = db.list_ids(Some(page.last().unwrap().0)).unwrap();
    assert_eq!(rest.len(), 5);
    assert!(rest.iter().all(|row| row.0 < page.last().unwrap().0));
}

#[test]
fn cannot_open_invalid_path_or_silently_reset_corruption() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Database::open(dir.path()).is_err());
    let path = dir.path().join("broken.sqlite3");
    std::fs::write(&path, b"not a sqlite database").unwrap();
    assert!(Database::open(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"not a sqlite database");
}

#[test]
fn readonly_write_fails_without_false_success_or_data_loss() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(&dir.path().join("links.sqlite3")).unwrap();
    db.save_url("https://example.com/kept").unwrap();
    db.connection
        .pragma_update(None, "query_only", true)
        .unwrap();
    assert_eq!(
        db.save_url("https://example.com/rejected")
            .unwrap_err()
            .code,
        "database"
    );
    assert_eq!(db.list_ids(None).unwrap().len(), 1);
    db.connection
        .pragma_update(None, "query_only", false)
        .unwrap();
    assert!(db.save_url("https://example.com/retry").unwrap().1);
}

#[test]
fn busy_database_fails_then_recovers_after_lock_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let mut db = Database::open(&path).unwrap();
    let lock = Connection::open(&path).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(
        db.save_url("https://example.com/busy").unwrap_err().code,
        "database"
    );
    assert!(db.list_ids(None).unwrap().is_empty());
    lock.execute_batch("ROLLBACK").unwrap();
    assert!(db.save_url("https://example.com/busy").unwrap().1);
}

#[test]
fn preserves_future_schema_and_does_not_guess_a_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version = 99; CREATE TABLE future(value TEXT); INSERT INTO future VALUES('keep');").unwrap();
    drop(connection);
    assert!(Database::open(&path).is_err());
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT value FROM future", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
}

#[test]
fn exact_matching_preserves_distinct_queries_fragments_and_case() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Database::open(&dir.path().join("links.sqlite3")).unwrap();
    for input in [
        "https://example.com/?a=1&b=2",
        "https://example.com/?b=2&a=1",
        "https://example.com/#one",
        "https://example.com/#two",
        "https://Example.com/#two",
    ] {
        assert!(db.save_url(input).unwrap().1);
    }
    assert_eq!(db.list_ids(None).unwrap().len(), 5);
    assert!(!db.save_url("  https://example.com/#two  ").unwrap().1);
}

#[test]
fn metadata_category_move_and_restart_preserve_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let mut db = Database::open(&path).unwrap();
    let mut input = draft("https://example.com/metadata");
    input.title = "My title".into();
    input.subtitle = "Subtitle".into();
    input.caption = "First line\nSecond line".into();
    input.new_category = Some(" Reading ".into());
    let link = db.save(&input).unwrap().link;
    assert_eq!(link.category_name, "Reading");
    assert_eq!(db.categories().unwrap().len(), 2);
    let mut change = edit(&link);
    change.category_id = 1;
    change.subtitle = "Updated subtitle".into();
    let updated = db.save(&change).unwrap().link;
    assert_eq!(updated.title, link.title);
    assert_eq!(updated.caption, link.caption);
    assert_eq!(updated.category_name, "Inbox");
    assert_eq!(updated.revision, 1);
    drop(db);
    let db = Database::open(&path).unwrap();
    let got = &db.list("", None).unwrap().links[0];
    assert_eq!(got.subtitle, "Updated subtitle");
    assert_eq!(got.caption, "First line\nSecond line");
}
#[test]
fn duplicate_create_does_not_overwrite_or_create_an_unused_category() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let mut input = draft("https://example.com/dup");
    input.title = "Keep".into();
    let original = db.save(&input).unwrap().link;
    input.title = "Overwrite".into();
    input.new_category = Some("Unused".into());
    let duplicate = db.save(&input).unwrap();
    assert!(!duplicate.created);
    assert_eq!(duplicate.link.title, "Keep");
    assert_eq!(duplicate.link.id, original.id);
    assert_eq!(db.categories().unwrap().len(), 1);
}
#[test]
fn failed_edit_rolls_back_metadata_category_and_url() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let original = db.save(&draft("https://example.com/one")).unwrap().link;
    db.save(&draft("https://example.com/two")).unwrap();
    let mut change = edit(&original);
    change.url = "https://example.com/two".into();
    change.title = "Should not commit".into();
    change.new_category = Some("Should not exist".into());
    assert_eq!(db.save(&change).unwrap_err().code, "duplicate_url");
    assert_eq!(db.validated_url(original.id).unwrap(), original.url);
    assert_eq!(find_link(&db.connection, original.id).unwrap().title, "");
    assert_eq!(db.categories().unwrap().len(), 1);
    change.url = "javascript:bad".into();
    assert_eq!(db.save(&change).unwrap_err().code, "invalid_url");
}
#[test]
fn stale_edits_and_deletes_do_not_lose_a_newer_revision() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let original = db
        .save(&draft("https://example.com/revision"))
        .unwrap()
        .link;
    let mut first = edit(&original);
    first.title = "New revision".into();
    let current = db.save(&first).unwrap().link;
    assert_eq!(db.save(&edit(&original)).unwrap_err().code, "conflict");
    assert_eq!(
        db.delete(original.id, original.revision).unwrap_err().code,
        "conflict"
    );
    assert_eq!(
        find_link(&db.connection, original.id).unwrap().title,
        "New revision"
    );
    db.delete(current.id, current.revision).unwrap();
    assert!(db.list("", None).unwrap().links.is_empty());
    assert_eq!(
        db.delete(current.id, current.revision).unwrap_err().code,
        "not_found"
    );
    assert_eq!(db.validated_url(current.id).unwrap_err().code, "not_found");
    assert_eq!(db.save(&edit(&current)).unwrap_err().code, "not_found");
}
#[test]
fn category_metadata_limits_and_foreign_keys_are_enforced() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let mut input = draft("https://example.com/limits");
    for name in ["", "  ", "bad\nname"] {
        input.new_category = Some(name.into());
        assert_eq!(db.save(&input).unwrap_err().code, "invalid_category");
    }
    input.new_category = Some("é".repeat(81));
    assert!(db.save(&input).is_err());
    input.new_category = Some("inbox".into());
    assert_eq!(db.save(&input).unwrap_err().code, "invalid_category");
    input.new_category = None;
    input.category_id = 999;
    assert_eq!(db.save(&input).unwrap_err().code, "invalid_category");
    input.category_id = 1;
    input.title = "é".repeat(201);
    assert_eq!(db.save(&input).unwrap_err().code, "invalid_metadata");
    input.title = "é".repeat(200);
    input.caption = "x".repeat(1001);
    assert!(db.save(&input).is_err());
    input.caption = "allowed\nline\tindent".into();
    input.subtitle = "bad\0".into();
    assert!(db.save(&input).is_err());
    input.subtitle.clear();
    let link = db.save(&input).unwrap().link;
    assert!(
        db.connection
            .execute("UPDATE links SET category_id=999 WHERE id=?1", [link.id])
            .is_err()
    );
    assert!(
        db.connection
            .execute(
                "UPDATE links SET title=?1 WHERE id=?2",
                params!["x".repeat(201), link.id]
            )
            .is_err()
    );
    assert!(
        db.connection
            .execute("INSERT INTO categories(name) VALUES('INBOX')", [])
            .is_err()
    );
}
#[test]
fn copy_and_open_revalidate_stored_urls_and_missing_ids() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let link = db
        .save(&draft("https://example.com/?x=1#kept"))
        .unwrap()
        .link;
    assert_eq!(db.validated_url(link.id).unwrap(), link.url);
    for bad in [
        "javascript:alert(1)",
        "file:///C:/secret",
        "https://name:pass@example.com",
        "https://example.com/a\nb",
    ] {
        db.connection
            .execute("UPDATE links SET url=?1 WHERE id=?2", params![bad, link.id])
            .unwrap();
        assert_eq!(db.validated_url(link.id).unwrap_err().code, "invalid_url");
    }
    assert_eq!(db.validated_url(-1).unwrap_err().code, "not_found");
}
#[test]
fn search_is_literal_covers_all_fields_and_ranks_title_first() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    for (n, field) in ["title", "subtitle", "caption", "category", "url"]
        .iter()
        .enumerate()
    {
        let mut input = draft(&format!("https://example.com/{n}"));
        match *field {
            "title" => input.title = "Needle".into(),
            "subtitle" => input.subtitle = "a needle subtitle".into(),
            "caption" => input.caption = "notes NEEDLE".into(),
            "category" => input.new_category = Some("Needle category".into()),
            _ => input.url = "https://example.com/needle".into(),
        };
        db.save(&input).unwrap();
    }
    let results = db.list("NEEDLE", None).unwrap();
    assert_eq!(results.links.len(), 5);
    assert_eq!(results.links[0].title, "Needle");
    assert_eq!(results.links[1].url, "https://example.com/needle");
    let mut input = draft("https://example.com/literal");
    input.caption = "Literal %_\\' characters".into();
    let literal = db.save(&input).unwrap().link;
    assert_eq!(db.list("%_\\'", None).unwrap().links[0].id, literal.id);
    assert!(db.list("not present", None).unwrap().links.is_empty());
    assert!(db.list(&"x".repeat(257), None).is_err());
    assert!(db.list("bad\nquery", None).is_err());
    assert!(db.list("", Some(Cursor { rank: 99, id: 1 })).is_err());
}
#[test]
fn ranked_search_cursor_does_not_skip_or_repeat_results() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    for n in 0..110 {
        let mut input = draft(&format!("https://example.com/{n}"));
        input.title = if n % 3 == 0 {
            "token".into()
        } else if n % 3 == 1 {
            "token prefix".into()
        } else {
            "a token substring".into()
        };
        db.save(&input).unwrap();
    }
    let first = db.list("token", None).unwrap();
    assert_eq!(first.links.len(), 100);
    assert!(first.next.is_some());
    let second = db.list("token", first.next).unwrap();
    assert_eq!(second.links.len(), 10);
    assert!(second.next.is_none());
    let ids: std::collections::HashSet<_> = first
        .links
        .iter()
        .chain(&second.links)
        .map(|l| l.id)
        .collect();
    assert_eq!(ids.len(), 110);
}
#[test]
fn failed_migration_is_atomic_and_readonly_edits_deletes_preserve_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite3");
    let c = Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE categories(id INTEGER PRIMARY KEY,name TEXT);INSERT INTO categories VALUES(1,'Inbox'),(2,'INBOX');CREATE TABLE links(id INTEGER PRIMARY KEY,url TEXT,category_id INTEGER,created_at INTEGER,updated_at INTEGER);INSERT INTO links VALUES(1,'https://example.com/',1,1,1);PRAGMA user_version=1;").unwrap();
    drop(c);
    assert!(Database::open(&path).is_err());
    let c = Connection::open(path).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(c.prepare("SELECT title FROM links").is_err());
    drop(c);
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let link = db.save(&draft("https://example.com/kept")).unwrap().link;
    db.connection
        .pragma_update(None, "query_only", true)
        .unwrap();
    assert_eq!(
        db.delete(link.id, link.revision).unwrap_err().code,
        "database"
    );
    let mut change = edit(&link);
    change.title = "Not saved".into();
    assert_eq!(db.save(&change).unwrap_err().code, "database");
    assert_eq!(find_link(&db.connection, link.id).unwrap().title, "");
}
#[test]
fn category_count_is_bounded_without_hiding_existing_categories() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let tx = db.connection.transaction().unwrap();
    for n in 1..1000 {
        tx.execute(
            "INSERT INTO categories(name) VALUES(?1)",
            [format!("Category {n}")],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    assert_eq!(db.categories().unwrap().len(), 1000);
    let mut input = draft("https://example.com/limit");
    input.new_category = Some("Too many".into());
    assert_eq!(db.save(&input).unwrap_err().code, "invalid_category");
    assert!(db.list("", None).unwrap().links.is_empty());
}
#[path = "storage_bench.rs"]
mod performance;

#[test]
fn existing_schema_six_gets_an_index_and_keeps_its_browse_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.sqlite3");
    let mut db = Database::open(&path).unwrap();
    let (first, _) = db.save_url("https://example.org/first").unwrap();
    let (second, _) = db.save_url("https://example.org/second").unwrap();
    db.order_link(first, second, true).unwrap();
    db.connection
        .execute_batch("DROP INDEX links_browse_order;")
        .unwrap();
    drop(db);

    let reopened = Database::open(&path).unwrap();
    let plan = reopened
        .connection
        .query_row(
            "EXPLAIN QUERY PLAN SELECT id FROM links WHERE COALESCE(sort_id,id)<?1
            ORDER BY COALESCE(sort_id,id) DESC LIMIT 101",
            [second],
            |row| row.get::<_, String>(3),
        )
        .unwrap();
    assert!(plan.contains("links_browse_order"), "{plan}");
    assert_eq!(reopened.list("", None).unwrap().links[0].id, first);
    assert_eq!(
        reopened
            .list(
                "",
                Some(Cursor {
                    rank: 0,
                    id: second
                })
            )
            .unwrap()
            .links[0]
            .id,
        second
    );
    assert!(
        reopened
            .list(
                "",
                Some(Cursor {
                    rank: 1,
                    id: second
                })
            )
            .unwrap()
            .links
            .is_empty()
    );
    assert_eq!(
        reopened
            .connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
            .unwrap(),
        6
    );
}

#[test]
fn deleted_ids_are_never_reused_for_new_links() {
    let mut db = Database::open(Path::new(":memory:")).unwrap();
    let first = db.save(&draft("https://example.com/deleted")).unwrap().link;
    db.delete(first.id, first.revision).unwrap();
    let second = db.save(&draft("https://example.com/new")).unwrap().link;
    assert!(second.id > first.id);
    assert_eq!(db.save(&edit(&first)).unwrap_err().code, "not_found");
    assert_eq!(db.validated_url(second.id).unwrap(), second.url);
}

#[test]
fn category_art_and_rename_preserve_membership_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let mut db = Database::open(&path).unwrap();
    let mut input = draft("https://example.com/lesson");
    input.new_category = Some("Study".into());
    input.new_category_icon = Some("book".into());
    let link = db.save(&input).unwrap().link;
    let id = link.category_id;
    assert_eq!(
        db.update_category(id, "Inbox", "heart", None, false)
            .unwrap_err()
            .code,
        "invalid_category"
    );
    assert_eq!(
        db.update_category(id, "Courses", "unknown", None, false)
            .unwrap_err()
            .code,
        "invalid_category"
    );
    assert_eq!(db.categories().unwrap()[1].icon, "book");
    assert!(
        db.update_category(id, "Courses", "book", Some(8), true)
            .is_err()
    );
    db.update_category(id, "Courses", "🎓", Some(2), true)
        .unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    let first = &db.categories().unwrap()[0];
    assert_eq!(
        (first.id, first.icon.as_str(), first.color, first.pinned),
        (id, "🎓", Some(2), true)
    );
    let found = &db.list("Courses", None).unwrap().links[0];
    assert_eq!((found.id, found.category_id), (link.id, id));
}
#[test]
fn reset_reopens_as_fresh_and_rolls_back_if_backup_removal_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let backup = dir.path().join("links-before-v2-123-456.sqlite3");
    let unrelated = dir.path().join("personal.sqlite3");
    let mut db = Database::open(&path).unwrap();
    let mut link = draft("https://example.org/reset");
    link.new_category = Some("Reading".into());
    db.save(&link).unwrap();
    let defaults = db.capture_settings().unwrap();
    assert!(db.preview_preference(None).unwrap());
    db.preview_preference(Some(false)).unwrap();
    let mut prefs = defaults.clone();
    prefs.highlight = 1;
    db.save_capture_settings(&prefs, &defaults).unwrap();
    std::fs::create_dir(&backup).unwrap();
    assert!(db.reset(&path).is_err());
    assert_eq!(db.list("", None).unwrap().links.len(), 1);
    assert_eq!(db.capture_settings().unwrap(), prefs);
    std::fs::remove_dir(&backup).unwrap();
    std::fs::write(&backup, b"migration backup").unwrap();
    std::fs::write(&unrelated, b"keep").unwrap();
    db.reset(&path).unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    assert!(db.list("", None).unwrap().links.is_empty());
    let categories = db.categories().unwrap();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].name, "Inbox");
    assert_eq!(db.capture_settings().unwrap(), defaults);
    assert!(!backup.exists());
    assert!(unrelated.exists());
    assert!(db.preview_preference(None).unwrap());
}

#[test]
fn groups_preserve_links_and_reject_cross_category_assignments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let mut db = Database::open(&path).unwrap();
    db.save_group(1, None, "References").unwrap();
    let group = db.categories().unwrap()[0].groups[0].id;
    assert!(db.save_group(1, None, "references").is_err());
    let mut item = draft("https://example.com/grouped");
    item.group_id = Some(group);
    let saved = db.save(&item).unwrap().link;
    db.save_group(1, Some(group), "Papers").unwrap();
    assert_eq!(db.list("Papers", None).unwrap().links[0].id, saved.id);
    let mut bad = edit(&saved);
    bad.new_category = Some("Invalid move".into());
    assert!(db.save(&bad).is_err());
    assert_eq!(db.categories().unwrap().len(), 1);
    let mut second = draft("https://example.com/other");
    second.new_category = Some("Reading".into());
    let category = db.save(&second).unwrap().link.category_id;
    db.order_categories(vec![category, 1]).unwrap();
    assert!(db.order_categories(vec![1, 1]).is_err());
    drop(db);
    let db = Database::open(&path).unwrap();
    assert_eq!(db.categories().unwrap()[0].id, category);
    assert_eq!(
        db.list("Papers", None).unwrap().links[0].group_id,
        Some(group)
    );
}
#[test]
fn sibling_order_survives_reopen_and_paging_without_changing_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("order.sqlite3");
    let mut db = Database::open(&path).unwrap();
    db.save_group(1, None, "Reading").unwrap();
    let group = db.categories().unwrap()[0].groups[0].id;
    for i in 1..=103 {
        let mut d = draft(&format!("https://example.org/{i}"));
        if i != 2 {
            d.group_id = Some(group);
        }
        db.save(&d).unwrap();
    }
    assert!(db.order_link(1, 2, true).is_err());
    assert!(db.order_link(1, 999, true).is_err());
    db.order_link(1, 103, true).unwrap();
    db.order_link(103, 102, false).unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    let first = db.list("", None).unwrap();
    let second = db.list("", first.next).unwrap();
    let ids: Vec<_> = first
        .links
        .iter()
        .chain(&second.links)
        .map(|l| l.id)
        .collect();
    assert_eq!(&ids[..3], &[1, 102, 103]);
    assert_eq!(ids[101], 2);
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        103
    );
    assert_eq!(db.list("example", None).unwrap().links[0].id, 103);
}

#[cfg(windows)]
#[test]
fn drive_snapshot_contains_saved_library_without_touching_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.sqlite3");
    let mut db = Database::open(&source).unwrap();
    db.save_url("https://example.org/backup-test").unwrap();
    let bytes = db.backup_bytes(5 * 1024 * 1024).unwrap();
    assert!(db.backup_bytes(64).is_err());
    let snapshot = dir.path().join("snapshot.sqlite3");
    std::fs::write(&snapshot, bytes).unwrap();
    let copy = Database::open(&snapshot).unwrap();
    assert_eq!(db.list_ids(None).unwrap(), copy.list_ids(None).unwrap());
}

#[cfg(windows)]
#[test]
fn restores_library_and_keeps_device_preferences_and_recovery_copy() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = Database::open(&dir.path().join("source.sqlite3")).unwrap();
    source.save_group(1, None, "Reference").unwrap();
    let mut saved = draft("https://example.org/from-drive");
    saved.title = "Recovered link".into();
    saved.group_id = Some(source.categories().unwrap()[0].groups[0].id);
    source.save(&saved).unwrap();
    let bytes = source.backup_bytes(5_000_000).unwrap();

    let mut dest = Database::open(&dir.path().join("current.sqlite3")).unwrap();
    dest.save_url("https://example.org/keep-a-copy").unwrap();
    dest.preview_preference(Some(true)).unwrap();
    dest.restore_library(&bytes, dir.path()).unwrap();

    let restored = dest.list("", None).unwrap().links.pop().unwrap();
    assert_eq!(restored.title, "Recovered link");
    assert_eq!(restored.group_id, saved.group_id);
    assert!(dest.preview_preference(None).unwrap());
    let copy_path = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .find(|file| {
            file.file_name()
                .to_string_lossy()
                .starts_with("before-restore-")
        })
        .unwrap()
        .path();
    let safety_copy = Database::open(&copy_path).unwrap();
    assert_eq!(
        safety_copy.list_ids(None).unwrap()[0].1,
        "https://example.org/keep-a-copy"
    );
}

#[cfg(windows)]
#[test]
fn rejected_restore_leaves_current_library_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut incoming = Database::open(&dir.path().join("incoming.sqlite3")).unwrap();
    incoming.save_url("https://example.com/first").unwrap();
    incoming
        .connection
        .execute(
            "INSERT INTO links(url,category_id) VALUES('javascript:bad',1)",
            [],
        )
        .unwrap();
    let bytes = incoming.backup_bytes(5_000_000).unwrap();

    let mut dest = Database::open(&dir.path().join("current.sqlite3")).unwrap();
    dest.save_url("https://example.org/original").unwrap();
    assert_eq!(
        dest.restore_library(&bytes, dir.path()).unwrap_err().code,
        "invalid_backup"
    );
    assert!(dest.restore_library(b"not sqlite", dir.path()).is_err());
    assert_eq!(
        dest.list_ids(None).unwrap()[0].1,
        "https://example.org/original"
    );
}

#[test]
fn previews_default_on_and_an_explicit_opt_out_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("links.sqlite3");
    let db = Database::open(&path).unwrap();
    assert!(db.preview_preference(None).unwrap());
    assert!(!db.preview_preference(Some(false)).unwrap());
    drop(db);
    let reopened = Database::open(&path).unwrap();
    assert!(!reopened.preview_preference(None).unwrap());
}

#[cfg(windows)]
#[test]
fn restore_enforces_library_limits_before_creating_a_recovery_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut dest = Database::open(&dir.path().join("current.sqlite3")).unwrap();
    dest.save_url("https://example.invalid/keep").unwrap();
    for (name, seed, limit) in [
        (
            "categories.sqlite3",
            "INSERT INTO categories(id,name) SELECT id+1,'Collection '||id FROM items",
            MAX_CATEGORIES,
        ),
        (
            "groups.sqlite3",
            "INSERT INTO link_groups(category_id,name) SELECT 1,'Group '||id FROM items",
            MAX_GROUPS + 1,
        ),
    ] {
        let source = Database::open(&dir.path().join(name)).unwrap();
        source
            .connection
            .execute(
                &format!(
                    "WITH RECURSIVE items(id) AS (VALUES(1) UNION ALL
                SELECT id+1 FROM items WHERE id<?1) {seed}"
                ),
                [limit],
            )
            .unwrap();
        let bytes = source.backup_bytes(5_000_000).unwrap();
        assert_eq!(
            dest.restore_library(&bytes, dir.path()).unwrap_err().code,
            "invalid_backup"
        );
    }
    assert_eq!(
        dest.list_ids(None).unwrap()[0].1,
        "https://example.invalid/keep"
    );
    assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("before-restore-")
    }));
}

#[cfg(windows)]
#[test]
fn restore_rejects_unrepresentable_cursor_and_generated_backup_columns() {
    let dir = tempfile::tempdir().unwrap();
    let source = Database::open(&dir.path().join("source.sqlite3")).unwrap();
    source.connection.execute(
        "INSERT INTO links(url,category_id,sort_id) VALUES('https://example.invalid/import',1,?1)",
        [i64::MAX],
    ).unwrap();
    let mut dest = Database::open(&dir.path().join("current.sqlite3")).unwrap();
    dest.save_url("https://example.invalid/keep").unwrap();
    let bytes = source.backup_bytes(5_000_000).unwrap();
    assert_eq!(
        dest.restore_library(&bytes, dir.path()).unwrap_err().code,
        "invalid_backup"
    );
    source.connection.execute_batch(
        "UPDATE links SET sort_id=NULL; ALTER TABLE links ADD COLUMN derived TEXT AS (title) VIRTUAL;"
    ).unwrap();
    let bytes = source.backup_bytes(5_000_000).unwrap();
    assert_eq!(
        dest.restore_library(&bytes, dir.path()).unwrap_err().code,
        "invalid_backup"
    );
    assert_eq!(
        dest.list_ids(None).unwrap()[0].1,
        "https://example.invalid/keep"
    );
}
