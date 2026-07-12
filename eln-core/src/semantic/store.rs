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

/// 검색 결과 1건 — entry 단위로 접힌 뒤 최고 점수 revision을 함께 보고한다.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub entry_id: String,
    pub rev_id: String,
    pub score: f32,
}

pub fn db_path(vault_root: &Path) -> PathBuf {
    vault::metadata_root(vault_root).join(DB_NAME)
}

pub fn exists(vault_root: &Path) -> bool {
    db_path(vault_root).exists()
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
    // v0.11 이전 레이아웃(entry당 1행, rev_id 없음)은 파생 캐시이므로 버리고 재구축한다
    // — 다음 `elf semantic reindex`가 전량 다시 채운다.
    if legacy_schema(conn)? {
        conn.execute("DROP TABLE embeddings", []).map_err(sql_error)?;
    }
    conn.execute(
        "CREATE TABLE IF NOT EXISTS embeddings (
            entry_id TEXT NOT NULL,
            rev_id TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            dim INTEGER NOT NULL,
            vec BLOB NOT NULL,
            updated TEXT,
            PRIMARY KEY (entry_id, rev_id)
        )",
        [],
    )
    .map_err(sql_error)?;
    Ok(())
}

fn legacy_schema(conn: &Connection) -> Result<bool, ElfError> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(embeddings)")
        .map_err(sql_error)?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    Ok(!columns.is_empty() && !columns.iter().any(|name| name == "rev_id"))
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

pub fn stored_metadata_conn(
    conn: &Connection,
    entry_id: &str,
    rev_id: &str,
) -> Result<Option<StoredMetadata>, ElfError> {
    conn.query_row(
        "SELECT content_hash, dim FROM embeddings WHERE entry_id = ?1 AND rev_id = ?2",
        params![entry_id, rev_id],
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
    rev_id: &str,
    content_hash: &str,
    vec: &[f32],
) -> Result<(), ElfError> {
    let conn = open(vault_root)?;
    upsert_conn(&conn, entry_id, rev_id, content_hash, vec)
}

pub fn upsert_conn(
    conn: &Connection,
    entry_id: &str,
    rev_id: &str,
    content_hash: &str,
    vec: &[f32],
) -> Result<(), ElfError> {
    let dim = i64::try_from(vec.len()).map_err(|_| ElfError::InvalidInput {
        message: "embedding dimension exceeds i64".to_string(),
    })?;
    let bytes = encode_f32_le(vec);
    let updated = Local::now().fixed_offset().to_rfc3339();
    conn.execute(
        "INSERT INTO embeddings(entry_id, rev_id, content_hash, dim, vec, updated)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(entry_id, rev_id) DO UPDATE SET
            content_hash = excluded.content_hash,
            dim = excluded.dim,
            vec = excluded.vec,
            updated = excluded.updated",
        params![entry_id, rev_id, content_hash, dim, bytes, updated],
    )
    .map_err(sql_error)?;
    Ok(())
}

/// vault에 더 이상 존재하지 않는 (entry_id, rev_id) 행 제거 — retract·rebase 잔재 정리.
pub fn prune_conn(
    conn: &Connection,
    valid: &std::collections::HashSet<(String, String)>,
) -> Result<usize, ElfError> {
    let mut stmt = conn
        .prepare("SELECT entry_id, rev_id FROM embeddings")
        .map_err(sql_error)?;
    let keys = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;

    let mut pruned = 0;
    for key in keys.iter().filter(|key| !valid.contains(key)) {
        conn.execute(
            "DELETE FROM embeddings WHERE entry_id = ?1 AND rev_id = ?2",
            params![key.0, key.1],
        )
        .map_err(sql_error)?;
        pruned += 1;
    }
    Ok(pruned)
}

pub fn search(vault_root: &Path, query_vec: &[f32], top_k: usize) -> Result<Vec<Hit>, ElfError> {
    let conn = open(vault_root)?;
    search_conn(&conn, query_vec, top_k)
}

/// entry의 모든 색인 벡터(base + revisions). centroid 질의용.
pub fn entry_vectors_read_only(
    vault_root: &Path,
    entry_id: &str,
) -> Result<Vec<Vec<f32>>, ElfError> {
    let conn = open_read_only(vault_root)?;
    let mut stmt = conn
        .prepare("SELECT dim, vec FROM embeddings WHERE entry_id = ?1")
        .map_err(sql_error)?;
    let rows = stmt
        .query_map(params![entry_id], |row| {
            Ok((row.get::<_, i64>(0)? as usize, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(sql_error)?;

    let mut vecs = Vec::new();
    for row in rows {
        let (dim, bytes) = row.map_err(sql_error)?;
        let vec = decode_f32_le(&bytes)?;
        if vec.len() != dim {
            return Err(ElfError::ParseError {
                message: format!(
                    "embedding dimension mismatch for {entry_id}: metadata={dim}, blob={}",
                    vec.len()
                ),
            });
        }
        vecs.push(vec);
    }
    Ok(vecs)
}

/// 벡터들의 평균 — entry의 "전체 주제" 질의 벡터. 비거나 차원이 섞이면 None.
pub fn centroid(vecs: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = vecs.first()?;
    let dim = first.len();
    if dim == 0 || vecs.iter().any(|vec| vec.len() != dim) {
        return None;
    }
    let mut mean = vec![0.0f32; dim];
    for vec in vecs {
        for (slot, value) in mean.iter_mut().zip(vec.iter()) {
            *slot += value;
        }
    }
    let count = vecs.len() as f32;
    for slot in mean.iter_mut() {
        *slot /= count;
    }
    Some(mean)
}

pub fn search_read_only(
    vault_root: &Path,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<Hit>, ElfError> {
    let conn = open_read_only(vault_root)?;
    search_conn(&conn, query_vec, top_k)
}

/// 전 행 brute-force cosine 후 entry 단위로 접는다(최고 점수 rev만 유지).
/// top_k는 접힌 뒤의 entry 수 기준.
pub fn search_conn(
    conn: &Connection,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<Hit>, ElfError> {
    if top_k == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = conn
        .prepare("SELECT entry_id, rev_id, dim, vec FROM embeddings")
        .map_err(sql_error)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as usize,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .map_err(sql_error)?;

    let mut best: std::collections::HashMap<String, Hit> = std::collections::HashMap::new();
    for row in rows {
        let (entry_id, rev_id, dim, bytes) = row.map_err(sql_error)?;
        if dim != query_vec.len() {
            continue;
        }
        let vec = decode_f32_le(&bytes)?;
        if vec.len() != query_vec.len() {
            continue;
        }
        let score = cosine(query_vec, &vec);
        match best.get(&entry_id) {
            Some(hit) if hit.score >= score => {}
            _ => {
                best.insert(
                    entry_id.clone(),
                    Hit {
                        entry_id,
                        rev_id,
                        score,
                    },
                );
            }
        }
    }

    let mut scored: Vec<Hit> = best.into_values().collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
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
        upsert_conn(&conn, "N0001", "r0000", "h1", &[1.0, 0.0]).unwrap();
        upsert_conn(&conn, "N0002", "r0000", "h2", &[0.7, 0.7]).unwrap();
        upsert_conn(&conn, "N0003", "r0000", "h3", &[0.0, 1.0]).unwrap();

        let hits = search_conn(&conn, &[1.0, 0.0], 2).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entry_id, "N0001");
        assert_eq!(hits[1].entry_id, "N0002");
    }

    #[test]
    fn search_folds_revisions_to_best_scoring_per_entry() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        upsert_conn(&conn, "N0001", "r0000", "base", &[0.1, 0.9]).unwrap();
        upsert_conn(&conn, "N0001", "r0003", "delta3", &[1.0, 0.0]).unwrap();
        upsert_conn(&conn, "N0002", "r0000", "other", &[0.7, 0.7]).unwrap();

        let hits = search_conn(&conn, &[1.0, 0.0], 5).unwrap();
        // N0001은 base가 아니라 r0003으로 1건만 잡힌다 — 국소 매치가 entry를 대표
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entry_id, "N0001");
        assert_eq!(hits[0].rev_id, "r0003");
        assert_eq!(hits[1].entry_id, "N0002");
    }

    #[test]
    fn legacy_single_vector_schema_is_dropped_and_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let path = db_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "CREATE TABLE embeddings (
                entry_id TEXT PRIMARY KEY,
                content_hash TEXT NOT NULL,
                dim INTEGER NOT NULL,
                vec BLOB NOT NULL,
                updated TEXT
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embeddings VALUES('N0001', 'h1', 2, x'0000803f00000000', NULL)",
            [],
        )
        .unwrap();
        drop(conn);

        let conn = open(dir.path()).unwrap();
        let hits = search_conn(&conn, &[1.0, 0.0], 5).unwrap();
        assert!(hits.is_empty());
        upsert_conn(&conn, "N0001", "r0000", "h1", &[1.0, 0.0]).unwrap();
        assert_eq!(search_conn(&conn, &[1.0, 0.0], 5).unwrap().len(), 1);
    }

    #[test]
    fn prune_removes_rows_missing_from_valid_set() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        upsert_conn(&conn, "N0001", "r0000", "h1", &[1.0, 0.0]).unwrap();
        upsert_conn(&conn, "N0001", "r0001", "h2", &[0.9, 0.1]).unwrap();
        upsert_conn(&conn, "N0002", "r0000", "h3", &[0.0, 1.0]).unwrap();

        let valid: std::collections::HashSet<(String, String)> =
            [("N0001".to_string(), "r0000".to_string())].into();
        let pruned = prune_conn(&conn, &valid).unwrap();

        assert_eq!(pruned, 2);
        let hits = search_conn(&conn, &[1.0, 0.0], 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].entry_id.as_str(), hits[0].rev_id.as_str()), ("N0001", "r0000"));
    }

    #[test]
    fn centroid_averages_vectors_and_rejects_mixed_dims() {
        assert_eq!(
            centroid(&[vec![1.0, 0.0], vec![0.0, 1.0]]),
            Some(vec![0.5, 0.5])
        );
        assert_eq!(centroid(&[]), None);
        assert_eq!(centroid(&[vec![1.0], vec![1.0, 0.0]]), None);
    }
}
