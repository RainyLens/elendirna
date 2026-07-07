use crate::error::ElfError;
use crate::vault;
use chrono::Local;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::{Path, PathBuf};

const DB_NAME: &str = "semantic.sqlite";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMetadata {
    pub content_hash: String,
    pub dim: usize,
}

pub fn db_path(vault_root: &Path) -> PathBuf {
    vault::metadata_root(vault_root).join(DB_NAME)
}

pub fn open(vault_root: &Path) -> Result<Connection, ElfError> {
    let path = db_path(vault_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path).map_err(sql_error)?;
    ensure_schema(&conn)?;
    Ok(conn)
}

fn ensure_schema(conn: &Connection) -> Result<(), ElfError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS embeddings (
            entry_id TEXT PRIMARY KEY,
            content_hash TEXT NOT NULL,
            dim INTEGER NOT NULL,
            vec BLOB NOT NULL,
            updated TEXT
        )",
        [],
    )
    .map_err(sql_error)?;
    Ok(())
}

fn open_read_only(vault_root: &Path) -> Result<Connection, ElfError> {
    let path = db_path(vault_root);
    Connection::open_with_flags(
        sqlite_immutable_uri(&path),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(sql_error)
}

fn sqlite_immutable_uri(path: &Path) -> String {
    let path = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/");
    let path = path.strip_prefix("//?/").unwrap_or(&path);
    let mut uri = String::from("file:");
    for byte in path.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'.' | b'_' | b'-' => {
                uri.push(*byte as char);
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    uri
}

pub fn stored_metadata(
    vault_root: &Path,
    entry_id: &str,
) -> Result<Option<StoredMetadata>, ElfError> {
    let conn = open(vault_root)?;
    stored_metadata_conn(&conn, entry_id)
}

fn stored_metadata_conn(
    conn: &Connection,
    entry_id: &str,
) -> Result<Option<StoredMetadata>, ElfError> {
    conn.query_row(
        "SELECT content_hash, dim FROM embeddings WHERE entry_id = ?1",
        params![entry_id],
        |row| {
            Ok(StoredMetadata {
                content_hash: row.get(0)?,
                dim: row.get::<_, i64>(1)? as usize,
            })
        },
    )
    .optional()
    .map_err(sql_error)
}

pub fn upsert(
    vault_root: &Path,
    entry_id: &str,
    content_hash: &str,
    vec: &[f32],
) -> Result<(), ElfError> {
    let conn = open(vault_root)?;
    upsert_conn(&conn, entry_id, content_hash, vec)
}

pub fn upsert_conn(
    conn: &Connection,
    entry_id: &str,
    content_hash: &str,
    vec: &[f32],
) -> Result<(), ElfError> {
    let dim = i64::try_from(vec.len()).map_err(|_| ElfError::InvalidInput {
        message: "embedding dimension exceeds i64".to_string(),
    })?;
    let bytes = encode_f32_le(vec);
    let updated = Local::now().fixed_offset().to_rfc3339();
    conn.execute(
        "INSERT INTO embeddings(entry_id, content_hash, dim, vec, updated)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(entry_id) DO UPDATE SET
            content_hash = excluded.content_hash,
            dim = excluded.dim,
            vec = excluded.vec,
            updated = excluded.updated",
        params![entry_id, content_hash, dim, bytes, updated],
    )
    .map_err(sql_error)?;
    Ok(())
}

pub fn search(
    vault_root: &Path,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<(String, f32)>, ElfError> {
    let conn = open(vault_root)?;
    search_conn(&conn, query_vec, top_k)
}

pub fn entry_vector_read_only(
    vault_root: &Path,
    entry_id: &str,
) -> Result<Option<Vec<f32>>, ElfError> {
    let conn = open_read_only(vault_root)?;
    conn.query_row(
        "SELECT dim, vec FROM embeddings WHERE entry_id = ?1",
        params![entry_id],
        |row| Ok((row.get::<_, i64>(0)? as usize, row.get::<_, Vec<u8>>(1)?)),
    )
    .optional()
    .map_err(sql_error)?
    .map(|(dim, bytes)| {
        let vec = decode_f32_le(&bytes)?;
        if vec.len() != dim {
            return Err(ElfError::ParseError {
                message: format!(
                    "embedding dimension mismatch for {entry_id}: metadata={dim}, blob={}",
                    vec.len()
                ),
            });
        }
        Ok(vec)
    })
    .transpose()
}

pub fn search_read_only(
    vault_root: &Path,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<(String, f32)>, ElfError> {
    let conn = open_read_only(vault_root)?;
    search_conn(&conn, query_vec, top_k)
}

pub fn search_conn(
    conn: &Connection,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<(String, f32)>, ElfError> {
    if top_k == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare("SELECT entry_id, dim, vec FROM embeddings")
        .map_err(sql_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as usize,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(sql_error)?;

    let mut scored = Vec::new();
    for row in rows {
        let (entry_id, dim, bytes) = row.map_err(sql_error)?;
        if dim != query_vec.len() {
            continue;
        }
        let vec = decode_f32_le(&bytes)?;
        if vec.len() != query_vec.len() {
            continue;
        }
        scored.push((entry_id, cosine(query_vec, &vec)));
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    Ok(scored)
}

pub fn encode_f32_le(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for value in vec {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub fn decode_f32_le(bytes: &[u8]) -> Result<Vec<f32>, ElfError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(ElfError::ParseError {
            message: format!(
                "embedding blob length is not divisible by 4: {}",
                bytes.len()
            ),
        });
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

fn sql_error(err: rusqlite::Error) -> ElfError {
    ElfError::InvalidInput {
        message: format!("semantic.sqlite error: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_little_endian_round_trip() {
        let input = vec![0.0, 1.5, -2.25, std::f32::consts::PI];
        let bytes = encode_f32_le(&input);
        assert_eq!(decode_f32_le(&bytes).unwrap(), input);
    }

    #[test]
    fn cosine_scores_expected_vectors() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 0.0001);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 0.0001);
        assert!(cosine(&[1.0, 0.0], &[-1.0, 0.0]) < -0.9999);
    }

    #[test]
    fn search_orders_by_cosine_with_fixture_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        upsert_conn(&conn, "N0001", "h1", &[1.0, 0.0]).unwrap();
        upsert_conn(&conn, "N0002", "h2", &[0.7, 0.7]).unwrap();
        upsert_conn(&conn, "N0003", "h3", &[0.0, 1.0]).unwrap();

        let hits = search_conn(&conn, &[1.0, 0.0], 2).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "N0001");
        assert_eq!(hits[1].0, "N0002");
    }
}
