use rusqlite::{Connection, Row, config::DbConfig, limits::Limit, params};

use super::{
    AppError, Database, MAX_CATEGORIES, MAX_GROUPS, validate_category, validate_metadata,
    validate_url,
};
use crate::models::LinkDraft;

const MAX_SAFE_JS_INTEGER: i64 = 9_007_199_254_740_991;

fn invalid() -> AppError {
    AppError::new(
        "invalid_backup",
        "This file is not a valid Linkme library backup.",
    )
}

fn read_source(data: &[u8]) -> Result<Connection, AppError> {
    if data.len() > 5_000_000 || !data.starts_with(b"SQLite format 3\0") {
        return Err(invalid());
    }
    let mut source = Connection::open_in_memory()?;
    source.pragma_update(None, "trusted_schema", false)?;
    source.pragma_update(None, "cell_size_check", true)?;
    source.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    source.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW, false)?;
    source.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false)?;
    // Limit SQL-generated values separately from the serialized file size.
    source.set_limit(Limit::SQLITE_LIMIT_LENGTH, 128 * 1024)?;
    source.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, 100)?;
    source
        .deserialize_read_exact(rusqlite::MAIN_DB, data, data.len(), true)
        .map_err(|_| invalid())?;
    let version: i64 = source
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| invalid())?;
    // All Drive snapshots shipped so far use library schema v6.
    if version != 6 {
        return Err(invalid());
    }
    for table in ["categories", "link_groups", "links"] {
        let ordinary: bool = source
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_list
                    WHERE schema='main' AND name=?1 AND type='table')
                    AND NOT EXISTS(SELECT 1 FROM pragma_table_xinfo(?1,'main')
                        WHERE hidden<>0)",
                [table],
                |row| row.get(0),
            )
            .map_err(|_| invalid())?;
        if !ordinary {
            return Err(invalid());
        }
    }
    let checked: String = source
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .map_err(|_| invalid())?;
    if checked != "ok" {
        return Err(invalid());
    }
    let within_limits: bool = source
        .query_row(
            "SELECT (SELECT COUNT(*) FROM categories)<=?1
                AND (SELECT COUNT(*) FROM link_groups)<=?2",
            params![MAX_CATEGORIES, MAX_GROUPS],
            |row| row.get(0),
        )
        .map_err(|_| invalid())?;
    if !within_limits {
        return Err(invalid());
    }
    Ok(source)
}

fn positive(row: &Row<'_>, index: usize) -> Result<i64, AppError> {
    let value: i64 = row.get(index).map_err(|_| invalid())?;
    if !(1..=MAX_SAFE_JS_INTEGER).contains(&value) {
        return Err(invalid());
    }
    Ok(value)
}

fn copy_rows(
    source: &Connection,
    select: &str,
    mut insert: impl FnMut(&Row<'_>) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let mut select = source.prepare(select).map_err(|_| invalid())?;
    let mut rows = select.query([]).map_err(|_| invalid())?;
    while let Some(row) = rows.next().map_err(|_| invalid())? {
        insert(row)?;
    }
    Ok(())
}

impl Database {
    pub fn restore_library(
        &mut self,
        bytes: &[u8],
        directory: &std::path::Path,
    ) -> Result<(), AppError> {
        let source = read_source(bytes)?;
        // Preserve the existing library before the write transaction. The recovery copy stays local.
        let safety = tempfile::Builder::new()
            .prefix("before-restore-")
            .suffix(".sqlite3")
            .tempfile_in(directory)
            .map_err(|_| AppError::database())?
            .into_temp_path();
        self.connection
            .backup(rusqlite::MAIN_DB, &safety, rusqlite::backup::NO_PROGRESS)?;
        let tx = self.connection.transaction()?;
        tx.execute_batch("DELETE FROM links; DELETE FROM link_groups; DELETE FROM categories;")?;
        {
            let mut insert = tx.prepare("INSERT INTO categories(id,name,icon,color,pinned,position) VALUES(?1,?2,?3,?4,?5,?6)")?;
            copy_rows(
                &source,
                "SELECT id,name,icon,color,pinned,position FROM categories",
                |row| {
                    let name: String = row.get(1).map_err(|_| invalid())?;
                    let icon: String = row.get(2).map_err(|_| invalid())?;
                    validate_category(&name, &icon).map_err(|_| invalid())?;
                    insert.execute(params![
                        positive(row, 0)?,
                        name,
                        icon,
                        row.get::<_, Option<u8>>(3).map_err(|_| invalid())?,
                        row.get::<_, i64>(4).map_err(|_| invalid())?,
                        row.get::<_, Option<i64>>(5).map_err(|_| invalid())?,
                    ])?;
                    Ok(())
                },
            )?;
        }
        {
            let mut insert =
                tx.prepare("INSERT INTO link_groups(id,category_id,name) VALUES(?1,?2,?3)")?;
            copy_rows(
                &source,
                "SELECT id,category_id,name FROM link_groups",
                |row| {
                    let name: String = row.get(2).map_err(|_| invalid())?;
                    validate_category(&name, "").map_err(|_| invalid())?;
                    insert.execute(params![positive(row, 0)?, positive(row, 1)?, name])?;
                    Ok(())
                },
            )?;
        }
        {
            let mut insert = tx.prepare("INSERT INTO links(id,url,category_id,created_at,updated_at,title,subtitle,caption,revision,group_id,sort_id)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)")?;
            copy_rows(
                &source,
                "SELECT id,url,category_id,created_at,updated_at,title,subtitle,caption,revision,group_id,sort_id FROM links",
                |row| {
                    let url: String = row.get(1).map_err(|_| invalid())?;
                    let checked = validate_url(&url).map_err(|_| invalid())?;
                    if checked != url {
                        return Err(invalid());
                    }
                    let draft = LinkDraft {
                        title: row.get(5).map_err(|_| invalid())?,
                        subtitle: row.get(6).map_err(|_| invalid())?,
                        caption: row.get(7).map_err(|_| invalid())?,
                        ..Default::default()
                    };
                    validate_metadata(&draft).map_err(|_| invalid())?;
                    let revision: i64 = row.get(8).map_err(|_| invalid())?;
                    let sort_id: Option<i64> = row.get(10).map_err(|_| invalid())?;
                    if !(0..=MAX_SAFE_JS_INTEGER).contains(&revision)
                        || sort_id.is_some_and(|key| !(1..=MAX_SAFE_JS_INTEGER).contains(&key))
                    {
                        return Err(invalid());
                    }
                    insert.execute(params![
                        positive(row, 0)?,
                        url,
                        positive(row, 2)?,
                        row.get::<_, i64>(3).map_err(|_| invalid())?,
                        row.get::<_, i64>(4).map_err(|_| invalid())?,
                        draft.title,
                        draft.subtitle,
                        draft.caption,
                        revision,
                        row.get::<_, Option<i64>>(9).map_err(|_| invalid())?,
                        sort_id,
                    ])?;
                    Ok(())
                },
            )?;
        }
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM categories WHERE id=1)
              AND NOT EXISTS(SELECT 1 FROM links l JOIN link_groups g ON l.group_id=g.id WHERE l.category_id<>g.category_id)
              AND NOT EXISTS(SELECT 1 FROM links GROUP BY COALESCE(sort_id,id) HAVING COUNT(*)>1 OR COALESCE(sort_id,id)<=0)",
            [], |row| row.get(0),
        )?;
        if !valid {
            return Err(invalid());
        }
        safety.keep().map_err(|_| AppError::database())?;
        tx.commit()?;
        Ok(())
    }
}
