/// `elf guide --json` — AI-readable 커맨드 표면 출력
use clap::Args;
use crate::error::ElfError;

#[derive(Debug, Args)]
pub struct HelpArgs {
    /// JSON 형식으로 전체 커맨드 표면 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: HelpArgs) -> Result<(), ElfError> {
    let surface = command_surface();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&surface).unwrap());
    } else {
        // 사람 읽기용 요약
        println!("elf — Elendirna vault CLI (v{})", env!("CARGO_PKG_VERSION"));
        println!();
        println!("전역 플래그:");
        println!("  --vault <PATH>   vault 경로 직접 지정");
        println!("  --global         글로벌 vault (~/.elendirna/) 사용");
        println!("  --json           모든 출력을 JSON으로");
        println!();
        println!("커맨드:");
        for cmd in surface["commands"].as_array().unwrap_or(&vec![]) {
            let name = cmd["name"].as_str().unwrap_or("");
            let about = cmd["about"].as_str().unwrap_or("");
            println!("  {:<20} {}", name, about);
        }
        println!();
        println!("AI 워크플로 가이드:");
        println!("  세션 시작: sync log --tail 5 → query → bundle <id>");
        println!("  세션 종료: sync record --summary '...' --entries N####,...");
        println!("  컨텍스트 제어: bundle <id> --depth 0  (revisions만)");
        println!("                 bundle <id> --since N####@r####  (최근 delta만)");
    }
    Ok(())
}

fn command_surface() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "global_flags": [
            { "flag": "--vault <PATH>", "description": "vault 경로 직접 지정. 우선순위: --vault > --global > ELF_VAULT > cwd 탐색 > global 폴백. 첫 사용 시 vault_name을 글로벌 alias로 자동 등록." },
            { "flag": "--global",       "description": "글로벌 vault (~/.elendirna/) 강제 사용" },
            { "flag": "--json",         "description": "모든 출력을 JSON으로 (에러는 --json 무관 항상 JSON/stderr)" },
        ],
        "commands": [
            {
                "name": "entry new",
                "about": "새 entry 생성",
                "args": [
                    { "name": "title",      "required": true,  "description": "entry 제목" },
                    { "name": "--baseline", "required": false, "description": "baseline entry ID (예: N0001)" },
                    { "name": "--tag",      "required": false, "description": "태그 (여러 번 사용 가능)" },
                    { "name": "--dry-run",  "required": false, "description": "실제 생성 없이 계획만 출력" },
                ],
                "trigger": "새로운 아이디어, 결정, 기록을 남길 때"
            },
            {
                "name": "entry show",
                "about": "entry manifest + note body 조회",
                "args": [
                    { "name": "id", "required": true, "description": "entry ID (예: N0001)" },
                ],
                "trigger": "단일 entry 내용을 읽을 때"
            },
            {
                "name": "entry list",
                "about": "전체 entry 목록 조회 (tag/status/baseline 필터 지원)",
                "args": [
                    { "name": "--tag",      "required": false, "description": "태그 필터" },
                    { "name": "--status",   "required": false, "description": "draft / stable / archived" },
                    { "name": "--baseline", "required": false, "description": "baseline 필터" },
                ],
                "trigger": "작업 범위 파악, 태그별 탐색"
            },
            {
                "name": "entry status",
                "about": "entry status 변경 (draft / stable / archived)",
                "args": [
                    { "name": "id",     "required": true, "description": "entry ID" },
                    { "name": "status", "required": true, "description": "새 status: draft | stable | archived" },
                ],
                "trigger": "entry를 확정(stable) 또는 보관(archived)할 때"
            },
            {
                "name": "revision add",
                "about": "entry에 delta 추가 (생각의 변화 기록)",
                "args": [
                    { "name": "id",      "required": true,  "description": "entry ID" },
                    { "name": "--delta", "required": false, "description": "delta 내용 (생략 시 stdin)" },
                ],
                "trigger": "기존 entry의 내용이 바뀌었을 때. note.md를 직접 편집하지 말고 delta로 기록."
            },
            {
                "name": "bundle",
                "about": "entry + revision chain + linked entries 수집 (AI 컨텍스트 복원의 핵심)",
                "args": [
                    { "name": "id",       "required": true,  "description": "entry ID" },
                    { "name": "--depth",  "required": false, "description": "linked entry 탐색 깊이 (기본 1). 0=자신+revisions만, 1=직접 linked 전문, 2+=2홉 이상 manifest만" },
                    { "name": "--since",  "required": false, "description": "N####@r#### 또는 RFC 3339 — 이후 revision만 포함 (entry 본문은 항상 포함)" },
                ],
                "trigger": "세션 시작 시 컨텍스트 복원. 직접 파일을 읽지 말고 이 명령을 사용."
            },
            {
                "name": "query",
                "about": "sqlite 인덱스 기반 entry 검색",
                "args": [
                    { "name": "--tag",    "required": false, "description": "태그 필터" },
                    { "name": "--status", "required": false, "description": "status 필터" },
                    { "name": "--title",  "required": false, "description": "제목 키워드 검색" },
                ],
                "trigger": "특정 주제 entry를 찾을 때. 전체 목록보다 빠름."
            },
            {
                "name": "sync record",
                "about": "세션 요약을 sync.jsonl에 기록 (세션 종료 필수)",
                "args": [
                    { "name": "--summary", "required": true,  "description": "세션 요약 텍스트" },
                    { "name": "--entries", "required": false, "description": "작업한 entry ID 목록 (쉼표 구분)" },
                    { "name": "--agent",   "required": false, "description": "agent 이름 (기본: ELF_AGENT 환경변수 → human)" },
                ],
                "trigger": "세션 마칠 때 반드시 호출"
            },
            {
                "name": "sync log",
                "about": "sync.jsonl 최근 기록 조회",
                "args": [
                    { "name": "--tail",  "required": false, "description": "최근 N건 (기본 20)" },
                    { "name": "--agent", "required": false, "description": "특정 agent 필터" },
                ],
                "trigger": "세션 시작 시 이전 세션 활동 확인"
            },
            {
                "name": "validate",
                "about": "vault 무결성 검사 + index.sqlite 재생성",
                "args": [
                    { "name": "--fix", "required": false, "description": "자동 수정 가능한 항목 수정" },
                ],
                "trigger": "vault 상태 점검, index 재생성 필요 시"
            },
            {
                "name": "serve --mcp",
                "about": "MCP 서버 stdio transport로 구동",
                "args": [
                    { "name": "--vault", "required": false, "description": "vault 경로 지정" },
                ],
                "trigger": "Claude Desktop / MCP 클라이언트 연결용"
            },
            {
                "name": "serve",
                "about": "(--mcp 없이) MCP config snippet 출력",
                "trigger": "MCP 연결 설정 방법을 모를 때"
            },
            {
                "name": "link",
                "about": "두 entry 간 양방향 링크 생성",
                "args": [
                    { "name": "from", "required": true, "description": "출발 entry ID" },
                    { "name": "to",   "required": true, "description": "도착 entry ID" },
                ],
                "trigger": "두 entry가 관련될 때"
            },
        ],
        "workflow": {
            "session_start": [
                "sync log --tail 5  (이전 세션 활동 확인)",
                "query --tag <topic>  (작업 범위 파악)",
                "bundle <id>  (핵심 entry 컨텍스트 복원)",
            ],
            "session_end": [
                "sync record --summary '핵심 변화' --entries N####,...",
            ],
            "context_budget": [
                "bundle <id> --depth 0  (revisions만, 링크 없음)",
                "bundle <id> --since N####@r####  (최근 delta만)",
                "bundle <id> --depth 2  (2홉까지 manifest 메타데이터)",
            ]
        }
    })
}
