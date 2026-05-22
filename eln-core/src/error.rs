use thiserror::Error;

/// 중앙화된 에러 코드 (fix-012)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfErrorCode {
    IoError,               // E4000
    NotAVault,             // E2001
    NotFound,              // E2002
    AlreadyExists,         // E3001
    AlreadyInitialized,    // E3002
    EditorNotSet,          // E4002
    ParseError,            // E4003
    SchemaVersionMismatch, // E5001
    InvalidInput,          // E1001
    Cycle,                 // E3003
}

impl ElfErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IoError => "E4000",
            Self::NotAVault => "E2001",
            Self::NotFound => "E2002",
            Self::AlreadyExists => "E3001",
            Self::AlreadyInitialized => "E3002",
            Self::EditorNotSet => "E4002",
            Self::ParseError => "E4003",
            Self::SchemaVersionMismatch => "E5001",
            Self::InvalidInput => "E1001",
            Self::Cycle => "E3003",
        }
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Self::IoError => "io_error",
            Self::NotAVault => "not_a_vault",
            Self::NotFound => "not_found",
            Self::AlreadyExists => "already_exists",
            Self::AlreadyInitialized => "already_initialized",
            Self::EditorNotSet => "editor_not_set",
            Self::ParseError => "parse_error",
            Self::SchemaVersionMismatch => "schema_version_mismatch",
            Self::InvalidInput => "invalid_input",
            Self::Cycle => "cycle",
        }
    }

    /// exit code 매핑
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidInput => 1,
            Self::NotAVault => 2,
            Self::NotFound => 2,
            Self::AlreadyExists => 3,
            Self::AlreadyInitialized => 3,
            Self::Cycle => 3,
            Self::IoError => 4,
            Self::EditorNotSet => 4,
            Self::ParseError => 4,
            Self::SchemaVersionMismatch => 5,
        }
    }
}

#[derive(Debug, Error)]
pub enum ElfError {
    #[error("현재 디렉터리 또는 상위에 vault가 없습니다 (.elendirna/config.toml 없음)")]
    NotAVault,

    #[error("이미 vault가 초기화되어 있습니다: {path}")]
    AlreadyInitialized { path: String },

    #[error("항목을 찾을 수 없습니다: {id}")]
    NotFound { id: String },

    #[error("이미 존재합니다: {id}")]
    AlreadyExists { id: String },

    #[error("$EDITOR가 설정되지 않았습니다")]
    EditorNotSet,

    #[error("파싱 오류: {message}")]
    ParseError { message: String },

    #[error("스키마 버전 불일치: vault={vault}, cli={cli}")]
    SchemaVersionMismatch { vault: u32, cli: u32 },

    #[error("입력값이 유효하지 않습니다: {message}")]
    InvalidInput { message: String },

    #[error("순환 참조가 감지됩니다: {chain}")]
    Cycle { chain: String },

    #[error("I/O 오류: {0}")]
    Io(#[from] std::io::Error),
}

impl ElfError {
    pub fn code(&self) -> ElfErrorCode {
        match self {
            Self::NotAVault => ElfErrorCode::NotAVault,
            Self::AlreadyInitialized { .. } => ElfErrorCode::AlreadyInitialized,
            Self::NotFound { .. } => ElfErrorCode::NotFound,
            Self::AlreadyExists { .. } => ElfErrorCode::AlreadyExists,
            Self::EditorNotSet => ElfErrorCode::EditorNotSet,
            Self::ParseError { .. } => ElfErrorCode::ParseError,
            Self::SchemaVersionMismatch { .. } => ElfErrorCode::SchemaVersionMismatch,
            Self::InvalidInput { .. } => ElfErrorCode::InvalidInput,
            Self::Cycle { .. } => ElfErrorCode::Cycle,
            Self::Io(_) => ElfErrorCode::IoError,
        }
    }

    pub fn exit_code(&self) -> i32 {
        self.code().exit_code()
    }

    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::NotAVault => Some("`elf init`으로 vault를 초기화하세요"),
            Self::NotFound { .. } => Some("`elf entry new`로 entry를 생성하세요"),
            Self::EditorNotSet => {
                Some("환경 변수 $EDITOR를 설정하거나 config.toml의 editor 필드를 지정하세요")
            }
            _ => None,
        }
    }

    /// stderr에 JSON 형식으로 에러 출력 (fix-015: --json 무관하게 항상 JSON)
    pub fn emit_json(&self) {
        let code = self.code();
        let obj = serde_json::json!({
            "error": code.slug(),
            "code":  code.as_str(),
            "message": self.to_string(),
            "hint": self.hint(),
            "fix": null,
        });
        eprintln!("{}", obj);
    }
}
