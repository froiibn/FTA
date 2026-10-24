# 로컬 기기 간 전송 앱

집 안의 같은 네트워크에 있는 내 컴퓨터 사이에서 파일, 텍스트, URL을 간편하게 전달하는 앱이다. 프로젝트 폴더명은 FTA이며 제품 이름은 미정이다.

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

1차 구현을 진행했으며 현재 동작·검증·남은 항목은 [구현 현황](docs/implementation-status.md)에 정리한다. 실제 4대의 교차 OS 전송과 설치·서명 검증은 아직 남아 있다.

## 실행

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

처음에는 loopback 전용이다. 설정 화면에서 해당 기기의 LAN IP:포트와 집 네트워크 CIDR을 함께 저장하고 엔진을 재시작한다. 자동 발견도 설정에서 선택한다. 최초 등록은 받는 기기에서 등록 모드를 열고 표시되는 임시 주소를 보내는 기기에 입력한다. 양쪽의 비교값이 일치하면 양쪽에서 확정한다.

- [데스크톱 개발·배포](apps/desktop/README.md)
- [Windows x64 빌드·설치·검증](packaging/windows/README.md)
- [Linux 서버 설치·서비스](packaging/linux/README.md)
- [검증 명령과 구현 한계](docs/implementation-status.md)

## Codex·Claude에서 전송

```sh
cargo build --release -p fta-mcp
python3 packaging/mcp/config-example.py --client codex --allow-dir /absolute/path/to/share
```

설정 생성기는 현재 설치 경로에 맞는 등록 예시를 출력한다. 엔진을 먼저 실행하고 기기를 등록해야 하며, 허용 폴더를 지정하지 않으면 MCP 파일 전송은 비활성화된다. 텍스트·URL은 별도 파일 권한 없이 전송할 수 있다. [연동 문서](docs/mcp.md)의 Codex·Claude 등록 명령을 사용한다.
