# Codex·Claude용 FTA MCP

`fta-mcp`는 기존 FTA 엔진을 로컬 stdio MCP 도구로 제공한다. Rust로 구현했고 공식 Rust SDK `rmcp` 3.7을 사용한다. TypeScript 화면이나 WebView 없이 Windows·macOS·Linux에서 빌드할 수 있는 별도 실행 파일이다. 프로토콜의 초기화·버전 협상·요청 취소는 SDK에 맡긴다. [공식 Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)

## 실행 구조와 범위

`Codex / Claude → stdio MCP → fta-mcp → 현재 사용자 전용 IPC → fta-engine → 등록된 온라인 기기`

- MCP 프로세스가 엔진을 대신 실행하거나 네트워크 전송·DB 작업을 직접 수행하지 않는다. 먼저 기존 데스크톱 앱 또는 `fta-engine serve`를 실행하고 기기 등록과 집 네트워크 설정을 마친다.
- 송신 전에 기기 ID가 유일하고 등록·온라인 상태인지 확인하며, 엔진도 인증과 접근 범위를 다시 검사한다.
- 파일·텍스트·URL을 보내는 도구는 **상대 컴퓨터에 실제 내용을 즉시 전달**한다. 수신 승인 창을 새로 추가하지 않는다. MCP 클라이언트 자체의 도구 사용 승인 정책은 별개다.
- 외부 네트워크 접근·클라우드 중계·꺼진 기기 대기열·상대 폴더 탐색·MCP를 통한 기기 등록은 제공하지 않는다.

## 빌드와 등록

macOS 앱 번들에는 `Contents/MacOS/fta-mcp`, Windows 배포에는 앱 실행 파일 옆의 `fta-mcp.exe`를 포함한다. 설치된 실행 파일의 실제 경로를 `--binary`에 넣어 설정을 생성할 수 있다. 별도 설치·개발용 빌드는 다음 명령을 사용한다.

```sh
cargo build --release -p fta-mcp
```

macOS·Linux에서는 저장소 루트에서 바이너리와 공유 폴더의 절대 경로를 구해 등록할 수 있다. 아래는 실행 예시이며 작성 과정에서 사용자 설정을 변경하지 않았다.

```sh
fta_mcp_binary="$(pwd -P)/target/release/fta-mcp"
fta_share_dir="$(cd ./share && pwd -P)"

codex mcp add fta -- "$fta_mcp_binary" --allow-dir "$fta_share_dir"
claude mcp add --scope local --transport stdio fta -- "$fta_mcp_binary" --allow-dir "$fta_share_dir"
```

`share`는 사용자가 허용할 기존 폴더로 바꾼다. `CARGO_TARGET_DIR`나 설치 경로를 변경한 경우 해당 바이너리의 실제 경로를 사용한다. Codex의 stdio 등록은 `codex mcp add ... -- command`, 설정은 `[mcp_servers.fta]` 형식이다. [공식 Codex MCP 문서](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)

Windows PowerShell의 예:

```powershell
$ftaMcpBinary = (Resolve-Path .\target\release\fta-mcp.exe).Path
$ftaShareDir = (Resolve-Path .\share).Path
codex mcp add fta -- $ftaMcpBinary --allow-dir $ftaShareDir
claude mcp add --scope local --transport stdio fta -- $ftaMcpBinary --allow-dir $ftaShareDir
```

Claude Code는 `--` 뒤를 서버 실행 명령과 인자로 전달한다. `--scope local`은 현재 프로젝트에 대한 로컬 등록이다. Claude Desktop 등 `mcpServers` 형식 클라이언트용 JSON은 아래 생성기로 출력할 수 있으나 각 클라이언트의 적용 위치는 해당 제품 문서를 따른다. [공식 Claude Code MCP 문서](https://code.claude.com/docs/en/mcp)

## 현재 경로로 설정 예시 생성

```sh
python3 packaging/mcp/config-example.py --client codex --allow-dir ./share
python3 packaging/mcp/config-example.py --client claude --allow-dir ./share
```

생성기는 현재 프로젝트와 빌드 결과에서 실제 절대 경로를 계산해 stdout으로 출력한다. 개인 홈 경로를 가정하지 않고 글로벌 설정을 수정하지 않는다. 별도 설치 위치는 `--binary`로 지정한다. 생성된 예시를 검토하고 필요한 클라이언트에만 적용한다.

큰 영상 전송은 클라이언트의 도구 시간 제한보다 길 수 있다. Codex는 `tool_timeout_sec`를 지원하고 기본값은 60초이므로 예시 생성기는 3600초를 제시한다. 이 값은 환경에 맞게 정하며 FTA 엔진의 전송 시간 상한을 의미하지 않는다. [Codex 시간 제한 설정](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)

## 파일 접근 허용

- `--allow-dir DIRECTORY`를 반복하거나 `FTA_MCP_ALLOW_DIRS`를 OS 경로 목록 형식으로 전달한다. Unix 계열은 `:`, Windows는 `;` 구분이며 공백 포함 경로는 해당 셸에서 인용한다.
- 허용 폴더가 없으면 `fta_send_files`는 `FILE_ACCESS_DENIED`로 실패한다. 텍스트·URL과 조회 도구는 사용할 수 있다.
- 시작 시 허용 폴더는 기존 물리 디렉터리로 확인한다. 파일 경로는 절대 경로여야 하고 경로 구성 요소의 심볼릭 링크·Windows reparse point·상위 경로 이동을 거부한다. 정규화된 파일이 허용 디렉터리에 포함되는지 확인하며 일반 파일만 허용한다.
- 검증된 경로와 허용 root를 엔진에 전달해 송신 파일을 다시 검증하도록 한다. 같은 사용자 권한으로 허용 root 자체를 교체하는 공격까지 완전히 격리하는 보안 샌드박스로 간주하지 않는다.
- macOS의 `/tmp`, `/var` 등 심볼릭 링크 경로가 거부되면 물리 경로를 사용한다. 설정 생성기는 `resolve()`로 물리 경로를 출력한다.
- 엔진 경로를 분리해야 하는 테스트는 `--config-dir`, `--state-dir`, `--runtime-dir`로 지정한다. 기본값은 기존 `FTA_*` 설정과 OS 사용자 경로를 따른다.

## 도구 계약

| 도구 | 인자 | 동작 |
|---|---|---|
| `fta_status` | 없음 | 엔진과 온라인 기기 상태 조회 |
| `fta_list_devices` | 없음 | 발견·등록 기기와 온라인 상태 조회 |
| `fta_send_files` | `to`, `paths[]` | 허용 폴더의 일반 파일 1~100개 전송 |
| `fta_send_text` | `to`, `text` | 줄바꿈을 유지한 UTF-8 텍스트 전송 |
| `fta_send_url` | `to`, `url` | HTTP·HTTPS URL 전송; 자동 실행 없음 |
| `fta_list_inbox` | 선택적 `limit` | 송수신 기록 요약 1~1000건, 기본 20건 |
| `fta_get_transfer` | `transfer_id` | 본문 포함 상세 조회, 송신 결과 불명확 시 수신 측 상태 재확인 |
| `fta_cancel_transfer` | `transfer_id` | 진행 중 취소 요청; 완료 파일 보존 |

`to`에는 `fta_list_devices`에서 얻은 기기 ID 또는 유일한 ID 접두사를 넣는다. 텍스트·URL 한도는 UTF-8 1 MiB다. 도구별 입력 스키마 외 필드는 거부한다. 조회 도구는 읽기 중심, 전송은 외부 기기에 내용을 추가하는 비멱등 동작, 취소는 미완료 데이터를 버릴 수 있는 동작으로 annotations에 명시한다.

결과는 MCP `content` 텍스트와 `structuredContent`에 함께 제공한다.

```json
{"ok": true, "result": {"transfer_id": "...", "state": "completed", "error": null}}
```

실패는 `isError: true`와 `{ "ok": false, "error": { "code": "OFFLINE", "message": "..." } }`로 전달한다. 부분 성공·결과 불명확은 기록 전체를 `result`에 보존하면서 `ok: false`, `isError: true`를 표시한다. 전송 기록 안의 `error: null`이나 문자열을 별도의 JSON-RPC 오류로 오해하지 않는다.

취소 요청 후 확정되지 않았으면 `cancel_requested`를 유지하며 취소 완료로 표현하지 않는다. MCP 요청 취소 알림은 엔진 취소 요청으로 연결하고 취소된 JSON-RPC 요청의 응답은 SDK가 억제한다. 클라이언트가 연결을 종료하면 진행 중 전송에 취소를 요청하지만 이미 완료된 파일은 삭제하지 않는다.

## 검증과 운영 한계

stdio에는 한 줄당 최대 8 MiB의 입력 제한을 두고 동시에 실행할 수 있는 도구 수를 제한한다. stdout은 JSON-RPC 전용이며 메시지 본문을 진단 로그로 기록하지 않는다. 과대한 입력과 JSON 파싱 오류의 폐기·회복은 공식 SDK의 bounded transport 동작을 따른다. [MCP stdio 규약](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)

```sh
cargo test -p fta-mcp
cargo clippy -p fta-mcp --all-targets -- -D warnings
```

실제 실행 파일의 초기화·도구 목록·호출, private IPC 전달, 허용 경로·링크 거부, 오프라인 차단, 부분·불명확 결과, 취소와 stdio EOF 종료를 회귀 테스트한다. Codex·Claude 제품의 실제 등록과 UI 승인 흐름, 각 OS의 설치 배포는 해당 클라이언트·기기에서 별도 확인해야 한다.

## 최초 전송 시 등록 확인 변경 (2026-10-28)

기본 전송은 감지된 컴퓨터 선택 → 최초 수신자 확인 → 같은 전송 계속 → 이후 즉시 전송으로 변경한다. 일반 MCP 전송은 등록 기기 한정이며, Linux의 명시적인 요청·승인 CLI와 데스크톱 네이티브 수신 확인은 [최초 전송 등록 설계](first-send-registration.md)를 따른다. 이전 실기 검증 기록은 이 변경의 검증을 의미하지 않는다.
