/// `.elendirna/index.sqlite` — 파생 캐시.
/// 항상 `elf validate`로 재생성 가능. vault 없이는 의미 없음.
use std::path::Path;
use rusqlite::{Connection, params};
use crate::error::ElfError;
use crate::vault::entry::Entry;
use crate::vault::revision::Revision;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS entries (
    id       TEXT PRIMARY KEY,
    title    TEXT NOT NULL,
    slug     TEXT NOT NULL,
    status   TEXT NOT NULL,
    created  TEXT NOT NULL,
    updated  TEXT NOT NULL,
    baseline TEXT
);

CREATE TABLE IF NOT EXISTS tags (
    entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    tag      TEXT NOT NULL,
    PRIMARY KEY (entry_id, tag)
);

CREATE TABLE IF NOT EXISTS links (
    from_id  TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    to_id    TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    PRIMARY KEY (from_id, to_id)
);

CREATE TABLE IF NOT EXISTS revisions (
    entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    rev_id   TEXT NOT NULL,
    baseline TEXT NOT NULL,
    created  TEXT NOT NULL,
    PRIMARY KEY (entry_id, rev_id)
);

PRAGMA foreign_keys = ON;
";

fn index_path(vault_root: &Path) -> std::path::PathBuf {
    vault_root.join(".elendirna").join("index.sqlite")
}

fn open(vault_root: &Path) -> Result<Connection, ElfError> {
    let path = index_path(vault_root);
    let conn = Connection::open(&path).map_err(|e| ElfError::Io(
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    ))?;
    conn.execute_batch(SCHEMA).map_err(|e| ElfError::Io(
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    ))?;
    Ok(conn)
}

/// vault의 모든 entry/revision을 index.sqlite에 재구성.
pub fn rebuild(vault_root: &Path) -> Result<usize, ElfError> {
    let conn = open(vault_root)?;

    conn.execute_batch("
        DELETE FROM revisions;
        DELETE FROM links;
        DELETE FROM tags;
        DELETE FROM entries;
    ").map_err(|e| ElfError::Io(
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    ))?;

    let entries = Entry::find_all(vault_root);
    let count = entries.len();

    for entry in &entries {
        let m = &entry.manifest;
        conn.execute(
            "INSERT OR REPLACE INTO entries (id, title, slug, status, created, updated, baseline)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                m.id,
                m.title,
                crate::vault::id::title_to_slug(&m.title),
                m.status.to_string(),
                m.created.to_rfc3339(),
                m.updated.to_rfc3339(),
                m.baseline,
            ],
        ).map_err(|e| ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        for tag in &m.tags {
            conn.execute(
                "INSERT OR IGNORE INTO tags (entry_id, tag) VALUES (?1, ?2)",
                params![m.id, tag],
            ).map_err(|e| ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }

        for link in &m.links {
            conn.execute(
                "INSERT OR IGNORE INTO links (from_id, to_id) VALUES (?1, ?2)",
                params![m.id, link],
            ).map_err(|e| ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        }

        if let Some(entry_id) = crate::vault::id::EntryId::from_str(&m.id) {
            for rev in Revision::list(vault_root, &entry_id) {
                conn.execute(
                    "INSERT OR IGNORE INTO revisions (entry_id, rev_id, baseline, created)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        m.id,
                        rev.rev_id.to_string(),
                        rev.baseline.to_string(),
                        rev.created.to_rfc3339(),
                    ],
                ).map_err(|e| ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            }
        }
    }

    Ok(count)
}

// ─── query ───────────────────────────────

pub struct QueryFilter {
    pub tag: Option<String>,
    pub status: Option<String>,
    pub baseline: Option<String>,
    pub title_contains: Option<String>,
}

pub struct QueryRow {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created: String,
    pub updated: String,
    pub baseline: Option<String>,
}

/// 필터 기반 entry 검색.
pub fn query(vault_root: &Path, filter: &QueryFilter) -> Result<Vec<QueryRow>, ElfError> {
    let conn = open(vault_root)?;

    let mut sql = String::from(
        "SELECT DISTINCT e.id, e.title, e.status, e.created, e.updated, e.baseline
         FROM entries e"
    );

    if filter.tag.is_some() {
        sql.push_str(" JOIN tags t ON e.id = t.entry_id");
    }

    let mut conditions: Vec<String> = vec![];
    if let Some(ref tag) = filter.tag {
        conditions.push(format!("t.tag = '{tag}'"));
    }
    if let Some(ref status) = filter.status {
        conditions.push(format!("e.status = '{status}'"));
    }
    if let Some(ref bl) = filter.baseline {
        conditions.push(format!("e.baseline LIKE '{bl}%'"));
    }
    if let Some(ref kw) = filter.title_contains {
        conditions.push(format!("e.title LIKE '%{kw}%'"));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY e.id");

    let mut stmt = conn.prepare(&sql).map_err(|e| ElfError::Io(
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    ))?;

    let rows = stmt.query_map([], |row| {
        Ok(QueryRow {
            id:       row.get(0)?,
            title:    row.get(1)?,
            status:   row.get(2)?,
            created:  row.get(3)?,
            updated:  row.get(4)?,
            baseline: row.get(5)?,
        })
    }).map_err(|e| ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| ElfError::Io(
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    ))
}
