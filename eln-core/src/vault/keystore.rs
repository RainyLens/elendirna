//! API key keystore (N0115 S3a — MCP 외부 노출 인증).
//!
//! 키 레코드 / 해시 검증 / in-memory registry. keystore 파일은 vault **바깥** global
//! config dir(`~/.elendirna/keys.toml`)에 저장한다 — vault-local `.elendirna/`는
//! git-tracked라 비밀이 커밋에 유출되므로 절대 금지([[N0115]] r0002 Axis A).
//!
//! raw 키는 발급 시 1회만 노출하고, 저장은 SHA-256 hash만. 검증은 입력 키 hash 대조.
//! 머신 토큰은 고엔트로피(256-bit)라 argon2 없이 SHA-256으로 충분([[N0115]] r0002).

use std::collections::HashMap;
use std::path::PathBuf;

use eln_plugin_sdk::{Identity, Permissions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ElfError;

/// raw 키 prefix — Bearer 토큰 식별 + 사람이 "이건 elf 키" 인지하게.
const KEY_PREFIX: &str = "eln_";
/// keystore 파일 schema 버전.
const KEYSTORE_SCHEMA_VERSION: u32 = 1;

/// `~/.elendirna/keys.toml` 경로. global config dir(vault 바깥) 관례를 재사용한다.
/// vault-local `.elendirna/`는 절대 사용 금지(비밀 유출).
pub fn keystore_path() -> Option<PathBuf> {
    crate::vault::config::VaultConfig::global_config_path().map(|p| p.with_file_name("keys.toml"))
}

/// 바이트 슬라이스를 소문자 hex 문자열로 (의존성 없이 hand-roll).
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// raw 키 문자열(`eln_...` 전체)의 SHA-256 hex. 저장·검증 양쪽이 동일 함수를 쓴다.
pub fn hash_key(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    to_hex(&hasher.finalize())
}

/// 새 raw 키 생성 — `eln_` + 32 random bytes의 hex(64자). CSPRNG(`rand::fill` = ThreadRng).
pub fn generate_raw_key() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    format!("{KEY_PREFIX}{}", to_hex(&bytes))
}

/// 단일 API key 레코드. `permissions`는 SDK `Permissions`(외부 crate bitflags, Serialize
/// 미보장)를 `u32` bits로 직렬화 — 로드 시 `from_bits_truncate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub key_hash: String,
    /// 빈 문자열 = `Identity::Human`, 그 외 = `Identity::Agent { name }`.
    #[serde(default)]
    pub identity: String,
    pub permission_bits: u32,
    #[serde(default)]
    pub label: String,
    pub created: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

impl ApiKeyRecord {
    /// revoked 되지 않았으면 활성.
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }

    /// 저장된 bits → SDK `Permissions` (정의되지 않은 비트는 truncate).
    pub fn permissions(&self) -> Permissions {
        Permissions::from_bits_truncate(self.permission_bits)
    }

    /// identity 문자열 → SDK `Identity`. 빈 문자열은 Human.
    pub fn identity(&self) -> Identity {
        if self.identity.trim().is_empty() {
            Identity::Human
        } else {
            Identity::Agent {
                name: self.identity.clone(),
            }
        }
    }
}

/// keys.toml 파일 표면 — `[[keys]]` array-of-tables.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeyStoreFile {
    pub schema_version: u32,
    #[serde(default)]
    pub keys: Vec<ApiKeyRecord>,
}

impl Default for KeyStoreFile {
    fn default() -> Self {
        Self {
            schema_version: KEYSTORE_SCHEMA_VERSION,
            keys: Vec::new(),
        }
    }
}

/// keys.toml 로드. 파일 부재 시 빈 store(미초기화)로 취급.
pub fn load_file() -> Result<KeyStoreFile, ElfError> {
    let Some(path) = keystore_path() else {
        return Ok(KeyStoreFile::default());
    };
    if !path.exists() {
        return Ok(KeyStoreFile::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    toml::from_str(&raw).map_err(|e| ElfError::ParseError {
        message: format!("keys.toml 파싱 실패: {e}"),
    })
}

/// keys.toml 원자적 저장.
pub fn write_file(store: &KeyStoreFile) -> Result<(), ElfError> {
    let path = keystore_path().ok_or_else(|| ElfError::InvalidInput {
        message: "홈 디렉터리를 결정할 수 없어 keys.toml 경로를 만들 수 없습니다".to_string(),
    })?;
    let content = toml::to_string_pretty(store).map_err(|e| ElfError::ParseError {
        message: format!("keys.toml 직렬화 실패: {e}"),
    })?;
    crate::vault::util::atomic_write(&path, content.as_bytes())
}

/// 새 키 발급 — raw 키를 생성/저장(hash만)하고 (raw, record)를 반환한다.
/// raw는 호출자가 1회만 사용자에게 노출한다.
pub fn add_key(
    label: &str,
    identity: Option<&str>,
    permissions: Permissions,
) -> Result<(String, ApiKeyRecord), ElfError> {
    let raw = generate_raw_key();
    let key_hash = hash_key(&raw);
    // id = hash 앞 12 hex — list/revoke에서 키를 식별(raw 비노출).
    let id = format!("k_{}", &key_hash[..12]);
    let record = ApiKeyRecord {
        id,
        key_hash,
        identity: identity.unwrap_or("").to_string(),
        permission_bits: permissions.bits(),
        label: label.to_string(),
        created: chrono::Local::now().to_rfc3339(),
        revoked_at: None,
    };
    let mut store = load_file()?;
    store.keys.push(record.clone());
    write_file(&store)?;
    Ok((raw, record))
}

/// 활성 키를 revoke(`revoked_at` 스탬프). 해당 id가 없거나 이미 revoked면 false.
pub fn revoke_key(id: &str) -> Result<bool, ElfError> {
    let mut store = load_file()?;
    let mut changed = false;
    for rec in &mut store.keys {
        if rec.id == id && rec.revoked_at.is_none() {
            rec.revoked_at = Some(chrono::Local::now().to_rfc3339());
            changed = true;
        }
    }
    if changed {
        write_file(&store)?;
    }
    Ok(changed)
}

/// 인증된 API key의 in-memory registry. 서버 생성 시 1회 로드되어 `Arc`로 공유한다.
/// per-call 디스크 I/O 금지 — revocation 반영은 재시작 단위(v1, [[N0115]] r0002).
#[derive(Debug, Default)]
pub struct KeyRegistry {
    /// key_hash(SHA-256 hex) → record. revoked 레코드는 로드 시 제외.
    by_hash: HashMap<String, ApiKeyRecord>,
}

impl KeyRegistry {
    /// 빈 registry — stdio(인증 무관) 및 auth 미초기화 HTTP transport용.
    pub fn empty() -> Self {
        Self::default()
    }

    /// 레코드 목록으로 registry 구성 (revoked 제외). 테스트가 디스크/`USERPROFILE` 없이 주입.
    pub fn from_records<I: IntoIterator<Item = ApiKeyRecord>>(records: I) -> Self {
        let by_hash = records
            .into_iter()
            .filter(|r| r.is_active())
            .map(|r| (r.key_hash.clone(), r))
            .collect();
        Self { by_hash }
    }

    /// keys.toml에서 활성 키만 로드. **파일 부재**는 미초기화(빈 registry)로 정상 처리하나
    /// (`load_file`이 부재를 `Ok(default)`로 줌), 파일이 **존재하나 읽기/파싱 실패**면 `Err`를
    /// 전파한다(fail-closed). 손상/권한 오류를 빈 registry로 삼키면 보호되던 배포가 무인증
    /// anonymous READ로 강등되므로([[N0115]] findings P2#3) — "부재=익명 허용"과
    /// "존재하나 깨짐=기동 거부"를 가른다.
    pub fn load_from_disk() -> Result<Self, ElfError> {
        let store = load_file()?;
        Ok(Self::from_records(store.keys))
    }

    /// 활성 키가 하나라도 있으면 true = "auth 초기화됨" 상태.
    /// serve가 이 값으로 `/mcp` Bearer 강제 여부를 판정한다([[N0115]] 게이팅 모델).
    pub fn is_initialized(&self) -> bool {
        !self.by_hash.is_empty()
    }

    /// raw 키(Bearer 토큰 전체)를 hash 대조해 활성 레코드를 찾는다.
    pub fn lookup(&self, raw: &str) -> Option<&ApiKeyRecord> {
        self.by_hash.get(&hash_key(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_64_hex() {
        let raw = "eln_deadbeef";
        let h = hash_key(raw);
        assert_eq!(h.len(), 64);
        assert_eq!(h, hash_key(raw));
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generated_keys_are_prefixed_and_unique() {
        let a = generate_raw_key();
        let b = generate_raw_key();
        assert!(a.starts_with("eln_"));
        assert_eq!(a.len(), 4 + 64);
        assert_ne!(a, b);
    }

    #[test]
    fn registry_lookup_matches_raw_and_excludes_revoked() {
        let raw = generate_raw_key();
        let active = ApiKeyRecord {
            id: "k_active".into(),
            key_hash: hash_key(&raw),
            identity: "claude".into(),
            permission_bits: Permissions::WRITE.union(Permissions::READ).bits(),
            label: "t".into(),
            created: "2026-01-01T00:00:00+09:00".into(),
            revoked_at: None,
        };
        let revoked_raw = generate_raw_key();
        let revoked = ApiKeyRecord {
            id: "k_revoked".into(),
            key_hash: hash_key(&revoked_raw),
            identity: String::new(),
            permission_bits: Permissions::READ.bits(),
            label: "t".into(),
            created: "2026-01-01T00:00:00+09:00".into(),
            revoked_at: Some("2026-01-02T00:00:00+09:00".into()),
        };
        let reg = KeyRegistry::from_records(vec![active, revoked]);
        assert!(reg.is_initialized());
        let found = reg.lookup(&raw).expect("active key found");
        assert!(found.permissions().contains(Permissions::WRITE));
        assert_eq!(found.identity(), Identity::Agent { name: "claude".into() });
        assert!(reg.lookup(&revoked_raw).is_none(), "revoked excluded");
        assert!(reg.lookup("eln_nope").is_none());
    }

    #[test]
    fn empty_registry_not_initialized() {
        assert!(!KeyRegistry::empty().is_initialized());
    }

    #[test]
    fn record_identity_empty_is_human() {
        let rec = ApiKeyRecord {
            id: "k".into(),
            key_hash: "h".into(),
            identity: String::new(),
            permission_bits: Permissions::READ.bits(),
            label: String::new(),
            created: "t".into(),
            revoked_at: None,
        };
        assert_eq!(rec.identity(), Identity::Human);
    }
}
