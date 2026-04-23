---
id: "N0002"
title: "Feature Proposal: Vault Export Migration Tool"
baseline: null
tags: []
---
# Feature Proposal: Vault Export Migration Tool

## 개요
Vault 디렉터리 간 데이터 이관(Migration)이나 백업/복원(Export/Import)을 원활하게 지원하기 위한 CLI 명령어(`elf export` / `elf import` 또는 `elf merge`)의 도입을 제안합니다.

## 배경 및 문제점 (왜 필요한가?)
Elendirna는 'Protocol-First' 설계 덕분에 `entries/`나 `revisions/` 폴더를 운영체제의 파일 복사 기능으로 단순히 옯긴 후 `elf validate`만 실행해도 `index.sqlite`가 자동 재생성되어 자가 복구(Auto-healing)가 완벽히 이루어집니다. 
하지만 이 과정에서 두 가지 운영상 한계가 발생합니다:

1. **AI 세션 로그(`sync.jsonl`)의 병합(Merge) 문제**
   디렉터리 강제 복사로 이관할 경우, 기존에 에이전트들이 쌓아둔 작업 맥락이 담긴 `sync.jsonl` 파일이 덮어쓰기 되거나 단절됩니다. AI가 장기 히스토리를 파악하는 핵심 핸드오프(Handoff) 로그가 소실되는 문제가 생깁니다.
2. **에이전트 이관 시나리오의 자동화**
   사용자가 단순히 CLI 상에서 "이 Vault의 데이터를 다른 프로젝트 Vault로 내보내 줘" 라고 AI에게 지시할 때, 표준 명령어 없이 복잡한 PowerShell 복사 등의 운영체제 스크립트로 처리하게 두는 것은 파일 오염 확률을 높이고 'CLI 강제성' 규칙에 어긋납니다.

## 기대 동작 (제안)
- `elf export --target [path]`
  단순 파일 복사를 넘어서, 메타데이터 무결성을 확인하고 전체 데이터를 패키징합니다.
- `elf merge [other-vault-path]`
  기존에 있는 두 개의 Vault 구조를 안전하게 합치며, 특히 `sync.jsonl`을 타임스탬프 기반으로 안전하게 병합(Append)하여 세션 단절을 방지합니다.
