# Proposal 002: Vault 자동 Init 이후 MCP 서버 설정 JSON 자동 제공 및 구성

## 개요
Proposal 001(`--auto-init` 플래그)을 통해 Vault 디렉터리를 자동으로 초기화할 수 있게 되더라도, AI 에이전트(Claude Desktop, Cursor, Roo-Code 등)가 Elendirna 서버와 통신하려면 결국 사용자가 각 에이전트의 MCP 설정 JSON(예: `claude_desktop_config.json`, `cline_mcp_settings.json` 등)에 Elendirna 서버 정보를 직접 추가해야 합니다.

이를 개선하기 위해, Vault 초기화 과정이나 특정 명령어 실행 시 **MCP 서버 구동에 필요한 JSON 설정을 자동으로 생성해 주거나, 클라이언트의 설정 파일에 직접 주입(Merge)해 주는 기능**을 제안합니다.

## 제안하는 변경 스펙

1. **설정 Snippet 자동 출력 및 파일 생성 (`elf init` / `elf serve --mcp --auto-init`)**
   Vault가 성공적으로 초기화되면, 터미널 표준 출력으로 즉시 복사하여 사용할 수 있는 JSON Snippet을 안내합니다.
   동시에 프로젝트 내부의 특정 위치(예: `.elendirna/mcp_server_snippet.json`)에 해당 JSON 조각을 파일로 저장해 두어 언제든 참조할 수 있도록 합니다.
   
   *생성 예시:*
   ```json
   {
     "mcpServers": {
       "elendirna": {
         "command": "elf",
         "args": ["serve", "--mcp"],
         "env": {}
       }
     }
   }
   ```

2. **자동 주입(Injection) 명령어 (선택 확장 기능)**
   클라이언트의 설정 파일 경로를 인자로 받아 JSON을 파싱하고 `mcpServers.elendirna` 블록을 안전하게 병합해 주는 유틸리티 명령어를 도입합니다.
   *예: `elf mcp-setup --target ~/.config/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`*

## 기대 효과

1. **Zero-Configuration에 가까운 온보딩**
   Vault 생성부터 에이전트 연동까지의 허들을 낮추어, 새로운 프로젝트에 도입할 때 초기 설정 시간을 극적으로 단축합니다.
2. **명확한 가이드라인 제공**
   명령어가 어떤 형태의 JSON을 작성해야 하는지 정확하게 제시하므로 오탈자나 인자(`args`) 설정 오류 등을 사전에 방지할 수 있습니다.
