---
id: "N0015"
title: "001-mcp-auto-init"
baseline: null
tags: []
---

# Proposal 001: MCP 서버 구동 시 `--auto-init` 플래그 제공

## 개요
현재 `elf serve --mcp` 명령은 탐색 과정에서 Vault(`config.toml` 및 연관 디렉터리 구조)를 찾지 못할 경우, `NotAVault` 에러를 반환하며 서버 구동에 실패합니다. 

단일 에이전트(AI)가 여러 저장소에서 작업할 때는 Vault 환경을 매번 수동으로 구성하기 번거로울 수 있습니다. 이를 해결하기 위해 명령어 인자로 `--auto-init` 플래그를 추가로 제공하여, **Vault가 없을 경우 에러로 중단되지 않고 즉시 CWD(현재 작업 디렉터리)에 기본 구조를 초기화한 후 서버를 구동**하도록 지원하는 방안을 제안합니다.

## 제안하는 변경 스펙

1. **`ServeArgs` 구조체 확장 (`src/cli/serve.rs`)**
   ```rust
   pub struct ServeArgs {
       // 기존 필드들...
       
       /// vault가 없을 경우 현재 디렉터리에 자동으로 생성
       #[arg(long)]
       pub auto_init: bool,
   }
   ```

2. **루트 탐색 Fallback 로직 수정 (`src/cli/serve.rs 의 run()`)**
   - 현재 작동하는 `vault::find_vault_root(&cwd)` 로직이 `NotAVault` 에러로 진입했을 때 대응합니다.
   - 만약 `--auto-init` 플래그가 주어졌다면, 내부적으로 `cli::init::run(...)` 을 호출하여 CWD에 Vault를 즉시 스캐폴딩(Scaffolding)합니다.
   - Vault 생성이 완료되면 생성된 CWD 경로를 root로 취급하여 `mcp::run_stdio` 로직을 정상 진행합니다.

## 기대 효과

1. **자동화된 온보딩 프로세스**
   AI 에이전트 측 설정(`settings.local.json` 등)에서 `"args": ["serve", "--mcp", "--auto-init"]`를 추가해 둔다면, 어떤 프로젝트 저장소에서든 Agent가 처음 켜졌을 때 즉각적으로 지식 베이스를 쓸 수 있게 됩니다.
   
2. **안전성 유지**
   의도치 않은 경로에 불필요한 Vault 폴더 구조가 생성되는 것을 막기 위해, 기본 동작(에러 발생)은 그대로 유지하고 명시적으로 옵션을 요구하는 방식이므로 하위 호환성을 해치지 않습니다.
