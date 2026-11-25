#[cfg(windows)]
#[path = "restore.rs"]
mod restore;

use crate::{
    models::{
        CaptureMode, CaptureSettings, Category, Cursor, Link, LinkDraft, LinkPage, SaveResult,
    },
    validation::validate_url,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{path::Path, time::Duration};

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: &'static str,
}
impl AppError {
    pub fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
    pub fn database() -> Self {
        Self::new(
            "database",
            "Local database unavailable. Check disk space and access, then retry.",
        )
    }
    pub fn unexpected() -> Self {
        Self::new(
            "unexpected",
            "The result could not be confirmed. Reload links before retrying.",
        )
    }
    fn missing() -> Self {
        Self::new(
            "not_found",
            "This link no longer exists. Reload the library.",
        )
    }
    fn conflict() -> Self {
        Self::new(
            "conflict",
            "This link changed in another window. Keep your draft, then reload the saved version before editing again.",
        )
    }
}
impl From<rusqlite::Error> for AppError {
    fn from(_: rusqlite::Error) -> Self {
        Self::database()
    }
}
pub struct Database {
    connection: Connection,
}
const MAX_CATEGORIES: i64 = 1000;
const MAX_GROUPS: i64 = 2000;
const LINK_COLUMNS: &str = "l.id,l.url,l.title,l.subtitle,l.caption,l.category_id,c.name,l.revision,c.icon,c.color,l.group_id,g.name";
const LINK_RELATIONS: &str =
    "links l JOIN categories c ON c.id=l.category_id LEFT JOIN link_groups g ON g.id=l.group_id";

fn read_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<Link> {
    Ok(Link {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        subtitle: row.get(3)?,
        caption: row.get(4)?,
        category_id: row.get(5)?,
        category_name: row.get(6)?,
        revision: row.get(7)?,
        category_icon: row.get(8)?,
        category_color: row.get(9)?,
        group_id: row.get(10)?,
        group_name: row.get(11)?,
    })
}
fn find_link(connection: &Connection, id: i64) -> Result<Link, AppError> {
    connection.query_row(&format!("SELECT {LINK_COLUMNS} FROM links l JOIN categories c ON c.id=l.category_id LEFT JOIN link_groups g ON g.id=l.group_id WHERE l.id=?1"), [id], read_link).optional()?.ok_or_else(AppError::missing)
}
fn validate_category(name: &str, icon: &str) -> Result<(), AppError> {
    static ICONS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        serde_json::from_str(include_str!("../../src/category-icons.json"))
            .expect("valid category icon catalogue")
    });
    if name.trim().is_empty()
        || name.trim().chars().count() > 80
        || name.chars().any(char::is_control)
    {
        return Err(AppError::new(
            "invalid_category",
            "Use a category name of 1–80 characters without control characters.",
        ));
    }
    if !icon.is_empty() && !icons.iter().any(|item| item == icon) {
        return Err(AppError::new(
            "invalid_category",
            "Choose an icon from the catalogue.",
        ));
    }
    Ok(())
}
fn validate_metadata(draft: &LinkDraft) -> Result<(), AppError> {
    for value in [&draft.title, &draft.subtitle] {
        if value.chars().count() > 200 || value.chars().any(char::is_control) {
            return Err(AppError::new(
                "invalid_metadata",
                "Title and subtitle must be at most 200 characters without control characters.",
            ));
        }
    }
    if draft.caption.chars().count() > 1000
        || draft
            .caption
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(AppError::new(
            "invalid_metadata",
            "Caption must be at most 1,000 characters without unsupported control characters.",
        ));
    }
    if let Some(name) = &draft.new_category {
        validate_category(name, draft.new_category_icon.as_deref().unwrap_or(""))?;
    }
    Ok(())
}
impl Database {
    pub fn capture_settings(&self) -> Result<CaptureSettings, AppError> {
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='capture_settings')",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(CaptureSettings::default());
        }
        Self::read_capture_settings(&self.connection)
    }

    fn read_capture_settings(connection: &Connection) -> Result<CaptureSettings, AppError> {
        let search = if Self::has_capture_column(connection, "search_shortcut")? {
            "search_shortcut"
        } else {
            "'Ctrl+Shift+K'"
        };
        let dim = if Self::has_capture_column(connection, "dim_percent")? {
            "dim_percent".into()
        } else {
            crate::models::DEFAULT_DIM_PERCENT.to_string()
        };
        let transparency = if Self::has_capture_column(connection, "panel_transparency")? {
            "panel_transparency".into()
        } else {
            crate::models::DEFAULT_PANEL_TRANSPARENCY.to_string()
        };
        let highlight = if Self::has_capture_column(connection, "highlight")? {
            "highlight"
        } else {
            "7"
        };
        let sql = format!(
            "SELECT mode,shortcut,{search},{dim},{transparency},{highlight} FROM capture_settings WHERE id=1"
        );
        Ok(connection
            .query_row(&sql, [], |row| {
                Ok(CaptureSettings {
                    mode: if row.get::<_, String>(0)? == "quick" {
                        CaptureMode::Quick
                    } else {
                        CaptureMode::Prompt
                    },
                    shortcut: row.get(1)?,
                    search_shortcut: row.get(2)?,
                    dim_percent: row.get(3)?,
                    panel_transparency: row.get(4)?,
                    highlight: row.get(5)?,
                })
            })
            .optional()?
            .unwrap_or_default())
    }

    fn has_capture_column(connection: &Connection, name: &str) -> Result<bool, AppError> {
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('capture_settings') WHERE name=?1)",
            [name],
            |row| row.get(0),
        )?)
    }

    pub fn save_capture_settings(
        &mut self,
        settings: &CaptureSettings,
        expected: &CaptureSettings,
    ) -> Result<(), AppError> {
        if settings.dim_percent > crate::models::MAX_DIM_PERCENT {
            return Err(AppError::new(
                "invalid_setting",
                "Choose a background dimming value from 0% to 40%.",
            ));
        }
        if settings.panel_transparency > crate::models::MAX_PANEL_TRANSPARENCY {
            return Err(AppError::new(
                "invalid_setting",
                "Choose a panel transparency value from 0% to 25%.",
            ));
        }
        if settings.highlight > crate::models::MAX_CAPTURE_HIGHLIGHT {
            return Err(AppError::new(
                "invalid_setting",
                "Choose one of the Capture highlight colors.",
            ));
        }
        let mode = match settings.mode {
            CaptureMode::Prompt => "prompt",
            CaptureMode::Quick => "quick",
        };
        let tx = self.connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS capture_settings(
            id INTEGER PRIMARY KEY CHECK(id=1),
            mode TEXT NOT NULL CHECK(mode IN ('prompt','quick')),
            shortcut TEXT NOT NULL CHECK(length(shortcut) BETWEEN 1 AND 32));",
        )?;
        if Self::read_capture_settings(&tx)? != *expected {
            return Err(AppError::new(
                "conflict",
                "Capture settings changed in another Linkme window. Reopen Linkme before changing them.",
            ));
        }
        if !Self::has_capture_column(&tx, "search_shortcut")? {
            tx.execute_batch(
                "ALTER TABLE capture_settings ADD COLUMN search_shortcut TEXT NOT NULL
                DEFAULT 'Ctrl+Shift+K' CHECK(length(search_shortcut) BETWEEN 1 AND 32);",
            )?;
        }
        if !Self::has_capture_column(&tx, "dim_percent")? {
            tx.execute_batch(
                "ALTER TABLE capture_settings ADD COLUMN dim_percent INTEGER NOT NULL
                DEFAULT 40 CHECK(dim_percent BETWEEN 0 AND 40);",
            )?;
        }
        if !Self::has_capture_column(&tx, "panel_transparency")? {
            tx.execute_batch(
                "ALTER TABLE capture_settings ADD COLUMN panel_transparency INTEGER NOT NULL
                DEFAULT 25 CHECK(panel_transparency BETWEEN 0 AND 25);",
            )?;
        }
        if !Self::has_capture_column(&tx, "highlight")? {
            tx.execute_batch("ALTER TABLE capture_settings ADD COLUMN highlight INTEGER NOT NULL DEFAULT 7 CHECK(highlight BETWEEN 0 AND 7);")?;
        }
        tx.execute(
            "INSERT INTO capture_settings(id,mode,shortcut,search_shortcut,dim_percent,panel_transparency,highlight) VALUES(1,?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET mode=excluded.mode,shortcut=excluded.shortcut,
             search_shortcut=excluded.search_shortcut,dim_percent=excluded.dim_percent,
             panel_transparency=excluded.panel_transparency,highlight=excluded.highlight",
            params![mode, settings.shortcut, settings.search_shortcut, settings.dim_percent, settings.panel_transparency, settings.highlight],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self, AppError> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_millis(750))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 | 1 => {
                if version == 1 {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|_| AppError::database())?
                        .as_nanos();
                    let backup = path.with_file_name(format!(
                        "links-before-v2-{}-{stamp}.sqlite3",
                        std::process::id()
                    ));
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&backup)
                        .map_err(|_| AppError::database())?;
                    connection.backup(rusqlite::MAIN_DB, &backup, rusqlite::backup::NO_PROGRESS)?;
                }
                let tx = connection.transaction()?;
                if version == 0 {
                    tx.execute_batch("CREATE TABLE categories(id INTEGER PRIMARY KEY,name TEXT NOT NULL UNIQUE CHECK(length(trim(name)) BETWEEN 1 AND 80));INSERT INTO categories VALUES(1,'Inbox');")?;
                } else {
                    tx.execute_batch("ALTER TABLE links RENAME TO links_v1;")?;
                }
                tx.execute_batch("CREATE TABLE links (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    url TEXT NOT NULL UNIQUE CHECK(length(CAST(url AS BLOB)) BETWEEN 1 AND 8192),
                    category_id INTEGER NOT NULL REFERENCES categories(id),
                    created_at INTEGER NOT NULL DEFAULT(unixepoch()),updated_at INTEGER NOT NULL DEFAULT(unixepoch()),
                    title TEXT NOT NULL DEFAULT '' CHECK(length(title)<=200),
                    subtitle TEXT NOT NULL DEFAULT '' CHECK(length(subtitle)<=200),
                    caption TEXT NOT NULL DEFAULT '' CHECK(length(caption)<=1000),
                    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0));")?;
                if version == 1 {
                    tx.execute_batch("INSERT INTO links(id,url,category_id,created_at,updated_at) SELECT id,url,category_id,created_at,updated_at FROM links_v1;DROP TABLE links_v1;")?;
                }
                tx.execute_batch("CREATE UNIQUE INDEX categories_name_nocase ON categories(name COLLATE NOCASE);PRAGMA user_version=2;")?;
                tx.commit()?;
            }
            2..=6 => {}
            _ => return Err(AppError::database()),
        }
        if version < 3 {
            let tx = connection.transaction()?;
            tx.execute_batch("ALTER TABLE categories ADD COLUMN icon TEXT NOT NULL DEFAULT ''; PRAGMA user_version=3;")?;
            tx.commit()?;
        }
        if version < 4 {
            let tx = connection.transaction()?;
            tx.execute_batch("ALTER TABLE categories ADD COLUMN color INTEGER CHECK(color BETWEEN 0 AND 7); ALTER TABLE categories ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0 CHECK(pinned IN (0,1)); PRAGMA user_version=4;")?;
            tx.commit()?;
        }
        if version < 5 {
            let tx = connection.transaction()?;
            tx.execute_batch("CREATE TABLE link_groups(id INTEGER PRIMARY KEY,category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,name TEXT NOT NULL COLLATE NOCASE CHECK(length(trim(name)) BETWEEN 1 AND 80), UNIQUE(category_id,name));
                ALTER TABLE links ADD COLUMN group_id INTEGER REFERENCES link_groups(id);
                ALTER TABLE categories ADD COLUMN position INTEGER;
                PRAGMA user_version=5;")?;
            tx.commit()?;
        }
        if version < 6 {
            let tx = connection.transaction()?;
            tx.execute_batch("ALTER TABLE links ADD COLUMN sort_id INTEGER;
                CREATE INDEX links_sibling_order ON links(category_id,group_id,COALESCE(sort_id,id));
                PRAGMA user_version=6;")?;
            tx.commit()?;
        }
        // The index is additive and compatible with existing schema-v6 apps and backups.
        connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS links_browse_order ON links(COALESCE(sort_id,id));",
        )?;
        Ok(Self { connection })
    }
    #[cfg(windows)]
    pub fn backup_link_count(&self) -> Result<u32, AppError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM links", [], |row| row.get(0))?)
    }

    #[cfg(windows)]
    pub fn backup_bytes(&self, limit: u64) -> Result<Vec<u8>, AppError> {
        let pages: i64 = self
            .connection
            .pragma_query_value(None, "page_count", |row| row.get(0))?;
        let page_size: i64 = self
            .connection
            .pragma_query_value(None, "page_size", |row| row.get(0))?;
        let oversized = || {
            AppError::new(
                "native_action",
                "This library exceeds the 5 MB Google Drive backup limit.",
            )
        };
        if u64::try_from(pages.saturating_mul(page_size)).map_err(|_| oversized())? > limit {
            return Err(oversized());
        }
        let mut snapshot = Connection::open_in_memory()?;
        rusqlite::backup::Backup::new(&self.connection, &mut snapshot)?.run_to_completion(
            128,
            Duration::ZERO,
            None,
        )?;
        let bytes = snapshot.serialize(rusqlite::MAIN_DB)?;
        if bytes.len() as u64 > limit {
            return Err(oversized());
        }
        Ok(bytes.to_vec())
    }

    pub fn preview_preference(&self, enabled: Option<bool>) -> Result<bool, AppError> {
        if let Some(enabled) = enabled {
            self.connection.execute_batch("CREATE TABLE IF NOT EXISTS preview_preferences(
                id INTEGER PRIMARY KEY CHECK(id=1), enabled INTEGER NOT NULL CHECK(enabled IN (0,1)))")?;
            self.connection.execute(
                "INSERT INTO preview_preferences VALUES (1,?1)
                ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled",
                [enabled],
            )?;
            return Ok(enabled);
        }
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='preview_preferences' AND type='table')",
            [], |row| row.get(0))?;
        if !exists {
            return Ok(true);
        }
        Ok(self
            .connection
            .query_row(
                "SELECT enabled FROM preview_preferences WHERE id=1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(true))
    }

    pub fn update_preference(&self, enabled: Option<bool>) -> Result<bool, AppError> {
        if let Some(enabled) = enabled {
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS update_preferences(
                    id INTEGER PRIMARY KEY CHECK(id=1),
                    automatic INTEGER NOT NULL CHECK(automatic IN (0,1)))",
            )?;
            self.connection.execute(
                "INSERT INTO update_preferences VALUES (1,?1)
                 ON CONFLICT(id) DO UPDATE SET automatic=excluded.automatic",
                [enabled],
            )?;
            return Ok(enabled);
        }
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='update_preferences' AND type='table')",
            [], |row| row.get(0),
        )?;
        if !exists {
            return Ok(true);
        }
        Ok(self
            .connection
            .query_row(
                "SELECT automatic FROM update_preferences WHERE id=1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(true))
    }

    pub fn reset(&mut self, path: &Path) -> Result<(), AppError> {
        self.connection.pragma_update(None, "secure_delete", true)?;
        let tx = self.connection.transaction()?;
        tx.execute_batch(
            "DELETE FROM links;
            DELETE FROM categories;
            INSERT INTO categories(id,name) VALUES(1,'Inbox');
            DELETE FROM sqlite_sequence WHERE name='links';
            DROP TABLE IF EXISTS capture_settings; DROP TABLE IF EXISTS preview_preferences;
            DROP TABLE IF EXISTS update_preferences;",
        )?;
        let directory = path.parent().ok_or_else(AppError::database)?;
        for entry in std::fs::read_dir(directory).map_err(|_| AppError::database())? {
            let entry = entry.map_err(|_| AppError::database())?;
            let name = entry.file_name();
            let backup = name
                .to_str()
                .and_then(|s| s.strip_prefix("links-before-v2-"))
                .and_then(|s| s.strip_suffix(".sqlite3"))
                .and_then(|s| s.split_once('-'))
                .is_some_and(|(pid, stamp)| {
                    pid.parse::<u32>().is_ok() && stamp.parse::<u128>().is_ok()
                });
            if backup {
                std::fs::remove_file(entry.path()).map_err(|_| AppError::new("reset_backup", "Could not remove a migration backup. Close any app using it and try again."))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn save(&mut self, draft: &LinkDraft) -> Result<SaveResult, AppError> {
        let url = validate_url(&draft.url).map_err(|m| AppError::new("invalid_url", m))?;
        validate_metadata(draft)?;
        let tx = self.connection.transaction()?;
        if let Some(id) = draft.id {
            if draft.revision != Some(find_link(&tx, id)?.revision) {
                return Err(AppError::conflict());
            }
        } else if let Some(id) = tx
            .query_row("SELECT id FROM links WHERE url=?1", [&url], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            return Ok(SaveResult {
                link: find_link(&tx, id)?,
                created: false,
            });
        }
        let category_id = if let Some(name) = &draft.new_category {
            if tx.query_row("SELECT count(*) FROM categories", [], |r| {
                r.get::<_, i64>(0)
            })? >= MAX_CATEGORIES
            {
                return Err(AppError::new(
                    "invalid_category",
                    "The library supports up to 1,000 categories including Inbox.",
                ));
            }
            if tx
                .query_row(
                    "SELECT id FROM categories WHERE name=?1 COLLATE NOCASE",
                    [name.trim()],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .is_some()
            {
                return Err(AppError::new(
                    "invalid_category",
                    "A category with that name already exists. Choose it from the category list.",
                ));
            }
            tx.execute(
                "INSERT INTO categories(name,icon) VALUES(?1,?2)",
                params![
                    name.trim(),
                    draft.new_category_icon.as_deref().unwrap_or("")
                ],
            )?;
            tx.last_insert_rowid()
        } else {
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM categories WHERE id=?1)",
                [draft.category_id],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(AppError::new(
                    "invalid_category",
                    "Choose an existing category or create a new one.",
                ));
            }
            draft.category_id
        };
        if let Some(group) = draft.group_id
            && !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM link_groups WHERE id=?1 AND category_id=?2)",
                params![group, category_id],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Err(AppError::new(
                "invalid_category",
                "Choose a group in the selected category.",
            ));
        }
        let id = if let Some(id) = draft.id {
            if tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM links WHERE url=?1 AND id<>?2)",
                params![url, id],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(AppError::new(
                    "duplicate_url",
                    "Another saved link already uses this URL. Your original link has not changed.",
                ));
            }
            if tx.execute("UPDATE links SET url=?1,title=?2,subtitle=?3,caption=?4,category_id=?5,updated_at=unixepoch(),revision=revision+1 WHERE id=?6 AND revision=?7", params![url,draft.title.trim(),draft.subtitle.trim(),draft.caption,category_id,id,draft.revision])? != 1 { return Err(AppError::conflict()); }
            id
        } else {
            tx.execute(
                "INSERT INTO links(url,title,subtitle,caption,category_id) VALUES(?1,?2,?3,?4,?5)",
                params![
                    url,
                    draft.title.trim(),
                    draft.subtitle.trim(),
                    draft.caption,
                    category_id
                ],
            )?;
            tx.last_insert_rowid()
        };
        tx.execute(
            "UPDATE links SET group_id=?1 WHERE id=?2",
            params![draft.group_id, id],
        )?;
        let link = find_link(&tx, id)?;
        tx.commit()?;
        Ok(SaveResult {
            link,
            created: draft.id.is_none(),
        })
    }
    pub fn update_category(
        &mut self,
        id: i64,
        name: &str,
        icon: &str,
        color: Option<u8>,
        pinned: bool,
    ) -> Result<(), AppError> {
        if color.is_some_and(|c| c > 7) {
            return Err(AppError::new(
                "invalid_category",
                "Choose a color from the palette.",
            ));
        }
        validate_category(name, icon)?;
        let tx = self.connection.transaction()?;
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM categories WHERE id<>?1 AND name=?2 COLLATE NOCASE)",
            params![id, name.trim()],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "invalid_category",
                "A category with that name already exists.",
            ));
        }
        if tx.execute(
            "UPDATE categories SET name=?1,icon=?2,color=?4,pinned=?5 WHERE id=?3",
            params![name.trim(), icon, id, color, pinned],
        )? != 1
        {
            return Err(AppError::new(
                "not_found",
                "This category no longer exists.",
            ));
        }
        tx.commit()?;
        Ok(())
    }
    pub fn save_group(
        &mut self,
        category: i64,
        id: Option<i64>,
        name: &str,
    ) -> Result<(), AppError> {
        validate_category(name, "")?;
        let tx = self.connection.transaction()?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM link_groups WHERE category_id=?1 AND name=?2 AND id<>coalesce(?3,-1))",params![category,name.trim(),id],|r|r.get::<_,bool>(0))? {
            return Err(AppError::new("invalid_category","This category already has a group with that name."));
        }
        if let Some(id) = id {
            if tx.execute(
                "UPDATE link_groups SET name=?1 WHERE category_id=?2 AND id=?3",
                params![name.trim(), category, id],
            )? != 1
            {
                return Err(AppError::conflict());
            }
        } else {
            if tx.query_row("SELECT COUNT(*) FROM link_groups", [], |r| {
                r.get::<_, i64>(0)
            })? >= MAX_GROUPS
            {
                return Err(AppError::new(
                    "invalid_category",
                    "The library supports 2,000 groups.",
                ));
            }
            tx.execute(
                "INSERT INTO link_groups(category_id,name) VALUES(?1,?2)",
                params![category, name.trim()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rotate existing browse keys. Other branches retain their exact page positions.
    pub fn order_link(&mut self, id: i64, target: i64, before: bool) -> Result<(), AppError> {
        let tx = self.connection.transaction()?;
        let source = find_link(&tx, id)?;
        let destination = find_link(&tx, target)?;
        if source.category_id != destination.category_id || source.group_id != destination.group_id
        {
            return Err(AppError::conflict());
        }
        let mut siblings = {
            let mut stmt = tx.prepare(
                "SELECT id,COALESCE(sort_id,id) AS slot FROM links
                WHERE category_id=?1 AND group_id IS ?2 AND slot BETWEEN
                (SELECT MIN(COALESCE(sort_id,id)) FROM links WHERE id IN (?3,?4)) AND
                (SELECT MAX(COALESCE(sort_id,id)) FROM links WHERE id IN (?3,?4))
                ORDER BY slot DESC",
            )?;
            let rows = stmt.query_map(
                params![source.category_id, source.group_id, id, target],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let slots: Vec<_> = siblings.iter().map(|entry| entry.1).collect();
        let index = siblings
            .iter()
            .position(|entry| entry.0 == id)
            .ok_or_else(AppError::conflict)?;
        let item = siblings.remove(index);
        if id != target {
            let at = siblings
                .iter()
                .position(|entry| entry.0 == target)
                .ok_or_else(AppError::conflict)?;
            siblings.insert(at + usize::from(!before), item);
            let mut statement = tx.prepare("UPDATE links SET sort_id=?1 WHERE id=?2")?;
            for ((link, old), slot) in siblings.into_iter().zip(slots) {
                if slot != old {
                    statement.execute(params![slot, link])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn order_categories(&mut self, ids: Vec<i64>) -> Result<(), AppError> {
        let tx = self.connection.transaction()?;
        if ids.len() > 1000
            || tx.query_row("SELECT COUNT(*) FROM categories", [], |r| {
                r.get::<_, i64>(0)
            })? != ids.len() as i64
            || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
        {
            return Err(AppError::conflict());
        }
        for (position, id) in ids.iter().enumerate() {
            if tx.execute(
                "UPDATE categories SET position=?1 WHERE id=?2",
                params![position as i64, id],
            )? != 1
            {
                return Err(AppError::conflict());
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn categories(&self) -> Result<Vec<Category>, AppError> {
        let mut groups = std::collections::HashMap::<i64, Vec<crate::models::LinkGroup>>::new();
        let mut names = self
            .connection
            .prepare("SELECT category_id,id,name FROM link_groups ORDER BY category_id,name,id")?;
        let mut rows = names.query([])?;
        while let Some(row) = rows.next()? {
            groups
                .entry(row.get(0)?)
                .or_default()
                .push(crate::models::LinkGroup {
                    id: row.get(1)?,
                    name: row.get(2)?,
                });
        }
        let mut statement = self.connection.prepare(
            "SELECT id,name,icon,color,pinned FROM categories ORDER BY pinned DESC,position IS NULL,position,id<>1,name COLLATE NOCASE,id LIMIT 1000",
        )?;
        Ok(statement
            .query_map([], |r| {
                Ok(Category {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    icon: r.get(2)?,
                    color: r.get(3)?,
                    pinned: r.get(4)?,
                    groups: groups.remove(&r.get::<_, i64>(0)?).unwrap_or_default(),
                })
            })?
            .collect::<Result<_, _>>()?)
    }
    pub fn delete(&mut self, id: i64, revision: i64) -> Result<(), AppError> {
        let tx = self.connection.transaction()?;
        if find_link(&tx, id)?.revision != revision {
            return Err(AppError::conflict());
        }
        if tx.execute(
            "DELETE FROM links WHERE id=?1 AND revision=?2",
            params![id, revision],
        )? != 1
        {
            return Err(AppError::conflict());
        }
        tx.commit()?;
        Ok(())
    }
    pub fn validated_url(&self, id: i64) -> Result<String, AppError> {
        validate_url(&find_link(&self.connection, id)?.url)
            .map_err(|m| AppError::new("invalid_url", m))
    }
    pub fn list(&self, query: &str, before: Option<Cursor>) -> Result<LinkPage, AppError> {
        if query.chars().count() > 256
            || query.chars().any(char::is_control)
            || before.is_some_and(|c| !(0..=3).contains(&c.rank) || c.id <= 0)
        {
            return Err(AppError::new(
                "invalid_query",
                "Use a search of at most 256 characters without control characters.",
            ));
        }
        let query = query.trim();
        let rank = before.map(|cursor| cursor.rank);
        let id = before.map(|cursor| cursor.id);
        let browse_bindings = params![id];
        let search_bindings = params![query, rank, id];
        let mut sql;
        let bindings = if query.is_empty() {
            if rank.is_some_and(|rank| rank != 0) {
                return Ok(LinkPage {
                    links: Vec::new(),
                    next: None,
                });
            }
            sql = format!("SELECT {LINK_COLUMNS},0,COALESCE(l.sort_id,l.id) FROM {LINK_RELATIONS}");
            if id.is_some() {
                sql.push_str(" WHERE COALESCE(l.sort_id,l.id)<?1");
            }
            sql.push_str(" ORDER BY COALESCE(l.sort_id,l.id) DESC LIMIT 101");
            if id.is_some() {
                browse_bindings
            } else {
                params![]
            }
        } else {
            sql = format!(
                "WITH matches AS (
                    SELECT {LINK_COLUMNS}, CASE
                        WHEN ?1='' OR lower(l.title)=lower(?1) THEN 0
                        WHEN instr(lower(l.title),lower(?1))=1 THEN 1
                        WHEN instr(lower(l.title),lower(?1))>0 OR instr(lower(l.url),lower(?1))>0 THEN 2
                        ELSE 3 END AS rank,
                        CASE WHEN ?1='' THEN COALESCE(l.sort_id,l.id) ELSE l.id END AS page_id
                    FROM {LINK_RELATIONS}
                    WHERE ?1='' OR instr(lower(l.title),lower(?1))>0 OR instr(lower(l.url),lower(?1))>0
                        OR instr(lower(l.subtitle),lower(?1))>0 OR instr(lower(l.caption),lower(?1))>0
                        OR instr(lower(c.name),lower(?1))>0 OR instr(lower(g.name),lower(?1))>0
                )
                SELECT * FROM matches WHERE ?2 IS NULL OR rank>?2 OR (rank=?2 AND page_id<?3)
                ORDER BY rank,page_id DESC LIMIT 101"
            );
            search_bindings
        };
        let mut statement = self.connection.prepare_cached(&sql)?;
        let mut rows = statement
            .query_map(bindings, |r| {
                Ok((read_link(r)?, r.get::<_, i64>(12)?, r.get::<_, i64>(13)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let more = rows.len() > 100;
        rows.truncate(100);
        let next = if more {
            rows.last().map(|(_, rank, slot)| Cursor {
                id: *slot,
                rank: *rank,
            })
        } else {
            None
        };
        Ok(LinkPage {
            links: rows.into_iter().map(|(link, _, _)| link).collect(),
            next,
        })
    }
}
#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
