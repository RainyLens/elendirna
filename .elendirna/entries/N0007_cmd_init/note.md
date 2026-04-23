---
id: "N0007"
title: "cmd-init"
baseline: null
tags: []
---

# `elf init` 설계 문서

## 목적

새 Elendirna vault를 초기화한다. 표준 디렉터리 구조, 설정 파일, 에이전트용 `CLAUDE.md`, 사람용 `README.md`를 스캐폴드하여 이후 모든 커맨드가 동작할 수 있는 기반을 만든다.

---

## CLI 인터페이스

```
elf init [path]
```

> **바이너리:** `cargo install elendirna` 으로 설치되는 실행 파일은 `elf`. 이하 모든 문서에서 동일.

| 인자 | 타입 | 필수 | 설명 |
|------|------|------|------|
| `[path]` | string | ❌ | vault를 생성할 경로. 기본값: 현재 디렉터리 |

### 전역 플래그 (모든 커맨드 공통)

| 플래그 | 설명 |
|--------|------|
| `--json` | 구조화 JSON 출력 |
| `--dry-run` | 실제 변경 없이 결과 미리보기 |
| `--vault <path>` | 명시적 vault 경로 지정 |

### 예시

```sh
elf init                    # 현재 디렉터리를 vault로 초기화
elf init ~/notes/my-vault   # 지정 경로에 vault 생성
elf init --dry-run          # 생성될 파일 목록만 출력
```

---

## 동작 흐름

1. `[path]` 인자 확인. 미지정 시 현재 디렉터리 사용.
2. 이미 `.elendirna/` 가 존재하면 에러 (중복 초기화 방지).
3. 표준 vault 구조 생성:
   ```
   <path>/
   ├── .elendirna/
   │   ├── config.toml
   │   └── sync.jsonl        # 빈 파일로 생성 (append-only log)
   ├── entries/              # 빈 디렉터리
   ├── assets/
   │   ├── pdf/
   │   ├── img/
   │   └── web/
   ├── revisions/            # 빈 디렉터리
   ├── CLAUDE.md             # 에이전트용 3-line manifest
   └── README.md             # 사람용 vault 구조 설명
   ```
4. `.elendirna/config.toml` 기록:
   ```toml
   schema_version = 1
   vault_name = "<디렉터리 이름>"
   created = "<ISO-8601>"
   editor = "$EDITOR"
   ```
5. `CLAUDE.md` 자동 생성:
   ```markdown
   # Elendirna vault

   이 저장소는 `elf` CLI로만 수정합니다. 직접 파일 편집 금지.
   시작 시 `elf help --json`으로 명령 표면을 확인하고, 작업 종료 시 `elf sync record`로 기록하세요.
   스키마/규칙 위반은 `elf validate`가 보고합니다 — 에러의 `fix` 필드를 따르면 됩니다.
   아이디어 계보를 사람이 읽을 수 있게 합성할 때: `elf bundle <id>` 출력(raw delta chain)을 받아 시간 순으로 서술하세요. CLI는 압축된 체인만 냅니다 — unzip은 당신의 몫입니다.
   ```
6. `README.md` 자동 생성 (vault 구조 설명 템플릿).
7. `.gitignore`에 `index.sqlite` 추가.
8. 성공 메시지 출력:
   ```
   Initialized vault at <path>
   Run `elf help --json` to discover commands.
   ```

---

## 파일시스템 영향

| 경로 | 동작 |
|------|------|
| `.elendirna/config.toml` | 새로 생성 |
| `.elendirna/sync.jsonl` | 빈 파일로 생성 |
| `entries/` | 빈 디렉터리 생성 |
| `assets/pdf/`, `assets/img/`, `assets/web/` | 빈 디렉터리 생성 |
| `revisions/` | 빈 디렉터리 생성 |
| `CLAUDE.md` | 템플릿으로 생성 |
| `README.md` | 템플릿으로 생성 |
| `.gitignore` | `.elendirna/index.sqlite` 추가 |

---

## 에러 처리

| 상황 | 에러 코드 | 메시지 예시 |
|------|-----------|------------|
| `.elendirna/` 이미 존재 | E3001 (conflict) | `vault already initialized at <path>` |
| 쓰기 권한 없음 | E4001 (I/O) | `permission denied: <path>` |

JSON 에러 형식 (stderr):
```json
{
  "error": "already_initialized",
  "code": "E3001",
  "message": "vault already initialized at ./my-vault",
  "hint": "Use --vault to point to a different path",
  "fix": null
}
```

---

## 구현 노트

- `std::fs::create_dir_all`로 중첩 디렉터리 일괄 생성.
- `CLAUDE.md`와 `README.md`는 소스 코드 내 `const` 문자열 템플릿으로 관리 — 외부 파일 의존 없이 단일 바이너리 배포.
- `sync.jsonl`은 `init` 이벤트 자체를 첫 줄로 기록:
  ```jsonl
  {"ts": "...", "agent": "user", "action": "vault.init", "vault": "<name>"}
  ```
- `--dry-run` 시 생성될 파일 목록을 JSON 배열로 stdout 출력. 실제 변경 없음.
