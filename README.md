# FTA · File Transfer Anything

같은 로컬 네트워크에 있는 내 컴퓨터 사이에서 **파일·텍스트·URL을 보내는 전송 앱**이다. Windows·macOS에서는 작은 데스크톱 창으로, 화면 없는 Linux 서버에서는 CLI와 백그라운드 엔진으로 사용한다. 클라우드 중계 없이 기기 사이에서 직접 전송하며, Codex·Claude 같은 MCP 클라이언트에서도 등록된 기기로 보낼 수 있다.

FTA는 **File Transfer Anything**의 약자다.

## 주요 기능

- **파일 드래그 전송**: 받는 컴퓨터의 아이콘에 파일을 끌어놓거나 파일 선택 버튼으로 보낸다.
- **텍스트·URL 전송**: 짧은 메모와 링크를 보내고 수신 기록에서 내용을 확인·복사하거나 링크를 연다.
- **처음 한 번 기기 등록**: 감지된 컴퓨터에 처음 보내면 상대가 등록을 허용한다. 이후 등록된 온라인 기기는 매번 승인하지 않고 수신한다.
- **지정 폴더에 자동 저장**: 수신 파일을 설정한 폴더에 저장하며 같은 이름의 기존 파일을 덮어쓰지 않는다.
- **백그라운드 수신**: Windows 트레이와 macOS 메뉴 막대에서 앱을 유지하고 수신 알림을 받는다.
- **Linux·MCP 지원**: GUI 없이 파일·텍스트·URL을 송수신하고, MCP에서는 허용한 폴더의 파일을 등록된 온라인 기기로 보낸다.

현재 연결 가능한 기기에 전송한다. 외부 네트워크 접속, 상대 폴더 탐색, 오프라인 기기로의 나중 전달은 지원 범위에 포함하지 않는다. 수신 파일과 URL은 자동 실행하지 않는다.

## 다운로드와 플랫폼

Windows x64 설치 파일은 [Releases](https://github.com/froiibn/FTA/releases)에서 다운로드한다. 현재 Windows 배포본은 [v0.1.1](https://github.com/froiibn/FTA/releases/tag/v0.1.1)이며 SHA-256과 빌드 정보를 함께 제공한다. 설치 파일을 사용할 때는 Rust·Node.js 같은 개발 도구가 필요 없다.

| 플랫폼 | 사용 형태 | 설치·빌드 안내 |
|---|---|---|
| Windows | 데스크톱 앱·트레이, Windows 전용 어두운 스타일 | [x64 설치·빌드](packaging/windows/README.md) |
| macOS | 데스크톱 앱·메뉴 막대, Mac 전용 스타일 | [앱·DMG 설치·빌드](packaging/macos/README.md) |
| Linux | CLI·백그라운드 엔진, systemd 사용자 서비스 | [서버 설치·서비스](packaging/linux/README.md) |

Windows 설치 기본 위치는 OS의 Program Files 아래 `FTA`이며 설치 마법사에서 다른 폴더를 선택할 수 있다. 설정·등록 정보는 사용자별 데이터 폴더에, 수신 파일은 사용자가 지정한 수신 폴더에 저장한다.

## 처음 사용하기

1. 보내는 기기와 받는 기기에 앱 또는 엔진을 실행한다. 최초 전송 등록을 사용하려면 양쪽 모두 이 기능이 포함된 버전을 사용한다.
2. 설정에서 기기 이름·수신 폴더·LAN 연결을 확인한다. Windows는 신뢰하는 연결을 개인 네트워크로 설정하고 FTA의 수신을 허용한다.
3. 연결 설정을 저장한 뒤 **서비스 재시작**으로 적용한다. 같은 네트워크 기기 자동 발견도 켠다.
4. 감지된 컴퓨터를 선택해 파일·텍스트·URL을 보낸다. 처음 한 번 상대의 등록 허용을 받으면 이후에는 바로 전송한다.

등록 승인 전에는 파일 본문·텍스트·URL을 상대에게 보내지 않는다. 등록된 기기도 실제 온라인 상태와 인증을 확인한 뒤 전송한다. Linux에서 받는 경우 CLI로 등록 요청을 확인하고 허용한다. 자세한 동작과 CLI 명령은 [최초 전송 등록 안내](docs/first-send-registration.md)를 참고한다.

현재 구현과 OS별 실기 검증·남은 항목은 [구현 현황](docs/implementation-status.md)과 [검증 계획](docs/verification-plan.md)에 정리한다. Windows v0.1.1은 로컬 컴파일·패키징과 배포 파일 해시를 확인한 빌드이며, 해당 설치본의 설치·실행·실제 전송 검증과는 구분한다.

## 확정 기술 스택

- 공통 전송 엔진·Linux CLI·서비스: Rust
- Windows·macOS 공통 화면: TypeScript
- 데스크톱 앱: Tauri 2 (Rust 호스트와 TypeScript 화면)
- Linux 서버: Tauri·WebView 없이 Rust 실행 파일만 배포
- Codex·Claude 연동: 공식 rmcp SDK를 사용하는 Rust stdio MCP 서버

개발 작업 기준은 [AGENTS.md](AGENTS.md)에 정리한다.

## 문서

1. [요구사항 정의서](docs/requirements.md): 사용자가 확정한 목적과 기능, 제외 범위
2. [요구사항 분석서](docs/analysis.md): 사용 흐름, 제약, 검증 기준과 미결정 사항
3. [초기 설계서](docs/design.md): 요구사항을 구현하기 위한 구조와 동작 제안
4. [공통 상세 설계](docs/detailed-design.md): 구성, 확정 기술 스택, 기기 등록, 데이터·저장 정책
5. [전송 프로토콜](docs/protocol.md): API, 상태, 무결성, 중복·취소·오류 처리
6. [화면·CLI 설계](docs/interaction-design.md): 드래그 전송, 수신 화면, 명령어 계약
7. OS별 설계: [Windows](docs/platforms/windows.md), [macOS](docs/platforms/macos.md), [Linux](docs/platforms/linux.md)
8. [검증 계획](docs/verification-plan.md): 실제 4대 전송 행렬과 실패·복구 시험
9. [UI 디자인](docs/ui-design.md): 데스크톱 유틸리티 화면과 시각 규칙
10. [MCP 연동](docs/mcp.md): Codex·Claude 도구와 등록 방법

## 소스에서 실행

Rust 1.99.1(저장소 툴체인 파일)과 데스크톱 개발용 Node.js 24를 사용한다. Rust 도구가 PATH에 없다면 설치 경로를 PATH에 추가한다.

```sh
cargo build -p fta-engine -p fta-cli
cargo run -p fta-engine -- serve
```

다른 터미널에서 `cargo run -p fta-cli -- status`로 상태를 확인한다. 데스크톱은 엔진을 빌드한 다음 다음 명령으로 실행한다.

```sh
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri dev
```

엔진 기본 설정은 loopback 전용이다. 처음 LAN에서 사용할 때는 앱의 네트워크 설정을 사용하거나 해당 기기의 LAN 수신 주소·허용 네트워크 CIDR·자동 발견을 설정하고 엔진을 재시작한다. 데스크톱의 기본 등록 흐름은 위의 최초 전송 승인 방식이다. 수동 주소·비교값 확인은 고급 CLI 흐름으로 유지한다.

- [데스크톱 개발·배포](apps/desktop/README.md)
- [Windows x64 빌드·설치·검증](packaging/windows/README.md)
- [macOS 설치·빌드](packaging/macos/README.md)
- [Linux 서버 설치·서비스](packaging/linux/README.md)
- [검증 명령과 구현 한계](docs/implementation-status.md)

## Codex·Claude에서 전송

```sh
cargo build --release -p fta-mcp
python3 packaging/mcp/config-example.py --client codex --allow-dir /absolute/path/to/share
```

설정 생성기는 현재 설치 경로에 맞는 등록 예시를 출력한다. 엔진을 먼저 실행하고 기기를 등록해야 하며, 허용 폴더를 지정하지 않으면 MCP 파일 전송은 비활성화된다. 텍스트·URL은 별도 파일 권한 없이 전송할 수 있다. [연동 문서](docs/mcp.md)의 Codex·Claude 등록 명령을 사용한다.
