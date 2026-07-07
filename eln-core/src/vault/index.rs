use crate::error::ElfError;
use crate::vault::entry::Entry;
use crate::vault::revision::Revision;
use rusqlite::{Connection, params, params_from_iter};
use std::path::Path;

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

CREATE TABLE IF NOT EXISTS authored_edges (
    src         TEXT NOT NULL,
    dst         TEXT NOT NULL,
    rel         TEXT NOT NULL CHECK(rel IN ('baseline','manifest_link','revision_chain')),
    source_kind TEXT NOT NULL CHECK(source_kind IN ('manifest','revision')),
    source_ref  TEXT,
    created     TEXT,
    PRIMARY KEY (src, dst, rel, source_ref)
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
    crate::vault::metadata_root(vault_root).join("index.sqlite")
}

fn open(vault_root: &Path) -> Result<Connection, ElfError> {
    let path = index_path(vault_root);
    let conn =
        Connection::open(&path).map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;

    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA busy_timeout = 5000;
    ",
    )
    .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;

    conn.execute_batch(SCHEMA)
        .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
    Ok(conn)
}

pub fn rebuild(vault_root: &Path) -> Result<usize, ElfError> {
    let conn = open(vault_root)?;

    conn.execute_batch(
        "
        DELETE FROM revisions;
        DELETE FROM authored_edges;
        DELETE FROM links;
        DELETE FROM tags;
        DELETE FROM entries;
    ",
    )
    .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;

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
                m.baseline.as_deref(),
            ],
        )
        .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;

        for tag in &m.tags {
            conn.execute(
                "INSERT OR IGNORE INTO tags (entry_id, tag) VALUES (?1, ?2)",
                params![m.id, tag],
            )
            .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
        }

        if let Some(baseline) = &m.baseline {
            let dst = baseline.split('@').next().unwrap_or(baseline.as_str());
            conn.execute(
                "INSERT OR IGNORE INTO authored_edges
                 (src, dst, rel, source_kind, source_ref, created)
                 VALUES (?1, ?2, 'baseline', 'manifest', ?3, ?4)",
                params![m.id, dst, baseline, m.created.to_rfc3339()],
            )
            .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
        }

        for link in &m.links {
            conn.execute(
                "INSERT OR IGNORE INTO authored_edges
                 (src, dst, rel, source_kind, source_ref, created)
                 VALUES (?1, ?2, 'manifest_link', 'manifest', NULL, ?3)",
                params![m.id, link, m.created.to_rfc3339()],
            )
            .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
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
                )
                .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;

                let source_ref = format!("{}@{}", m.id, rev.rev_id);
                conn.execute(
                    "INSERT OR IGNORE INTO authored_edges
                     (src, dst, rel, source_kind, source_ref, created)
                     VALUES (?1, ?1, 'revision_chain', 'revision', ?2, ?3)",
                    params![m.id, source_ref, rev.created.to_rfc3339()],
                )
                .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
            }
        }
    }

    for entry in &entries {
        let m = &entry.manifest;
        for link in &m.links {
            conn.execute(
                "INSERT OR IGNORE INTO links (from_id, to_id) VALUES (?1, ?2)",
                params![m.id, link],
            )
            .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
        }
    }

    Ok(count)
}

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

/// index.sqlite stale check for entry manifests and revision files.
///
/// authored_edges includes revision_chain rows, so revision files participate in the same
/// mtime comparison as manifest.toml. Directory mtimes are intentionally ignored.
fn index_is_stale(vault_root: &Path) -> bool {
    let idx_mtime = match std::fs::metadata(index_path(vault_root)).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true,
    };

    let entries_dir = crate::vault::data_root(vault_root).join("entries");
    let Ok(rd) = std::fs::read_dir(&entries_dir) else {
        return false;
    };
    for entry in rd.flatten() {
        let manifest = entry.path().join("manifest.toml");
        if let Ok(m) = std::fs::metadata(&manifest).and_then(|md| md.modified())
            && m > idx_mtime
        {
            return true;
        }
    }

    let revisions_dir = crate::vault::data_root(vault_root).join("revisions");
    let Ok(rd) = std::fs::read_dir(&revisions_dir) else {
        return false;
    };
    for entry_dir in rd.flatten() {
        let Ok(file_type) = entry_dir.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Ok(revisions) = std::fs::read_dir(entry_dir.path()) else {
            continue;
        };
        for rev in revisions.flatten() {
            let path = rev.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            if let Ok(m) = std::fs::metadata(&path).and_then(|md| md.modified())
                && m > idx_mtime
            {
                return true;
            }
        }
    }
    false
}

pub fn query(vault_root: &Path, filter: &QueryFilter) -> Result<Vec<QueryRow>, ElfError> {
    if index_is_stale(vault_root) {
        let _ = rebuild(vault_root);
    }
    let conn = open(vault_root)?;

    let mut sql = String::from(
        "SELECT DISTINCT e.id, e.title, e.status, e.created, e.updated, e.baseline
         FROM entries e",
    );

    if filter.tag.is_some() {
        sql.push_str(" JOIN tags t ON e.id = t.entry_id");
    }

    let mut conditions: Vec<&str> = vec![];
    let mut values: Vec<String> = vec![];
    if let Some(ref tag) = filter.tag {
        conditions.push("t.tag = ?");
        values.push(tag.clone());
    }
    if let Some(ref status) = filter.status {
        conditions.push("e.status = ?");
        values.push(status.clone());
    }
    if let Some(ref bl) = filter.baseline {
        conditions.push("e.baseline LIKE ?");
        values.push(format!("{bl}%"));
    }
    if let Some(ref kw) = filter.title_contains {
        conditions.push("e.title LIKE ?");
        values.push(format!("%{kw}%"));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }
    sql.push_str(" ORDER BY CAST(substr(e.id, 2) AS INTEGER)");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;

    let rows = stmt
        .query_map(params_from_iter(values.iter()), |row| {
            Ok(QueryRow {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                created: row.get(3)?,
                updated: row.get(4)?,
                baseline: row.get(5)?,
            })
        })
        .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))
}
