# 구현 현황과 검증 기록

작성일: 2026-10-25. 설계에 따라 실행 가능한 기반과 주요 송수신 경로를 구현했다. 이 문서는 구현된 동작과 아직 남은 설계 항목을 구분한다. 전체 제품의 출시 완료를 의미하지 않는다.

## 작업 분담과 재검토

- Windows 서브에이전트: OS 경로·DPAPI·사용자 전용 named pipe·자동 실행·디렉터리 ACL·CI. 추가 검토 후 인증서 검증·저장·복구를 보완했다.
- macOS 서브에이전트: TypeScript 공통 화면·Tauri 호스트·Keychain·메뉴 막대·플러그인·배포 준비.
- Linux 서브에이전트: CLI·XDG 경로·systemd 서비스·종료와 제어 계약.
- 부모 에이전트: 공통 설정·IPC·TLS·기기 등록·HTTP 전송·통합, Rust 스킬에 따른 코드 검토와 테스트, Playwright 화면 확인.

검토 결과를 다시 개발에 반영했다. 긴 전송의 IPC 대기 제한, 설정 누적 저장 유실, 완료·취소 경쟁, 등록 TLS의 클라이언트 신원 확인, 전송 중 수신 폴더 변경, 중복 commit, 저장과 DB 사이의 복구 문제를 수정하고 회귀 테스트를 추가했다. 또한 CLI가 정상 전송의 `error: null`을 실패로 오해하는 문제를 찾아 IPC 오류와 전송 결과 메타데이터를 분리했다.

## 현재 구현

| 범위 | 현재 동작 |
|---|---|
| 공통 엔진 | Rust 독립 프로세스, 사용자 전용 로컬 IPC, 상태 디렉터리 단일 인스턴스 |
| 네트워크 | 명시적으로 허용한 대역·바인딩 주소, TLS 1.3 상호 인증, HTTP 스트리밍 |
| 최초 등록 | 받는 기기의 임시 등록 주소에 연결, 양쪽 인증서·nonce 비교, 양쪽 확인 후 등록 |
| 발견 | 설정한 주소의 인터페이스에서 mDNS 발견, 새 주소는 기존 인증서로 검증 후 갱신 |
| 파일 | 일반 파일·여러 파일·빈 파일, 스트리밍 SHA-256, 임시 저장, 덮어쓰기 없는 최종 저장 |
| 텍스트·URL | UTF-8 최대 1 MiB, 로컬 기록·내용 보기·복사·HTTP/HTTPS 열기 |
| 기록 | SQLite, 수신 폴더 고정, 충돌 처리, 재시작 복구, 기간·건수·본문 용량 기준 정리 |
| 상태 | 완료·부분 성공·실패·취소·결과 불명확, 불명확 결과의 읽기 전용 원격 재조회 |
| 데스크톱 | 트레이·메뉴 막대, 드롭·파일 선택, 고정 창·위치 복원, 설정, 수신 기록, 알림 연결 |
| Linux | GUI 없는 CLI·엔진, JSON·종료코드, systemd 사용자 서비스 설치·제어 |
| MCP | Rust stdio 서버의 8개 도구, 동일 IPC, 허용 폴더 제한, 취소·결과 조회 |
| 알림·진행률 | DB 이벤트 순번·소비자 커서, 실제 바이트 진행률, 네이티브 알림 활성화 대기 큐 |
| 자격 증명 | Windows DPAPI, macOS Keychain, Linux 사용자 전용 권한 파일 |

기본 설정은 loopback에서만 수신한다. 최초 LAN 사용 전에 수신 주소·허용 대역을 설정하고 엔진을 재시작한다. 로그인 시 실행은 사용자가 선택한다. 수신 승인과 최초 기기 등록 확인은 별개다.

## 수행한 검증

초기 전체 로컬 검사: `cargo test --workspace` 73개 통과. 자식 프로세스 전용 fixture 1개는 직접 실행 대상에서 제외되며 상위 lifecycle 테스트가 실제로 호출한다. `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, TypeScript 프로덕션 빌드도 통과했다. 엔진·CLI·macOS 데스크톱 개발 실행 파일의 네이티브 빌드를 확인했다. 이후 UI·플랫폼 변경의 검사는 아래 기록과 UI 검증 문서를 따른다.

- `rust-skills` 기준으로 오류 전파, 비동기 잠금, 스트리밍·메모리, 신뢰 경계와 상태 전이를 검토했다.
- macOS에서 공통 Rust·CLI·플랫폼 테스트, 실제 localhost TLS 기기 등록과 파일·텍스트·URL 송수신을 실행했다.
- 큰 escaped 메시지·0바이트 파일·한글 이름·충돌·중복 commit·잘못된 해시·미등록/등록 해제 기기 차단·수신 폴더 변경을 검증했다.
- 별도 자식 프로세스에서 실제 IPC 준비·중복 엔진 거부·SIGTERM 정상 종료·socket 정리·재시작을 검증했다. 테스트 신원은 메모리에서 생성하여 개인 Keychain을 변경하지 않는다.
- Tauri Rust 호스트의 타입 검사·테스트와 TypeScript 엄격 타입 검사·번들을 검증했다.
- Playwright에서 실제 브라우저 화면을 열어 기본 화면·설정·브라우저 전송 비활성화와 작은 창 레이아웃을 확인했다. 이는 네이티브 WebView·OS 알림 검증을 대체하지 않는다.
- Windows 플랫폼 코드는 GNU 타깃 크로스 검사·Clippy로 검토했다. 공통 코어 Windows 크로스 빌드는 로컬 MinGW C 컴파일러 부재로 실행하지 못했다.

명령별 최종 결과는 작업 보고를 따른다. 기존 GitHub `Check` 워크플로가 소스·태그 push와 PR에서 자동 실행된 사실을 확인했고, 이후 실행은 Actions 예산 제한으로 시작되지 못했다. 사용자 지정에 따라 해당 워크플로를 비활성화하고 삭제했다. 앞으로 빌드·검증·패키징은 사용자 소유의 해당 OS 컴퓨터에서 수행하며 GitHub는 소스 관리와 완성된 설치 산출물 배포에 사용한다. [운영 원칙과 Windows 권한 검사 보완](verification-plan.md)을 참고한다.

## 추가 구현과 재개 확인

2026-10-25 권한 변경으로 중단된 작업을 재확인했다. 소스와 기존 실행 프로세스가 보존되어 있었으며, 재개 후 현재 소스의 검사를 다시 수행한다.

- `frontend-design` 계획·검토·구현·시각 검증에 따라 네이비 툴바와 기기 목록 중심으로 화면을 개편했다. 작은 창·넓은 창을 확인했으며 가짜 온라인 기기는 사용하지 않았다.
- 실제 바이트 진행률, 영속 이벤트·소비자 커서와 재시작 재처리, macOS UserNotifications 및 Windows 프로토콜 알림의 상세 화면 활성화 경로를 구현했다. OS의 실제 화면 표시나 정확히 한 번 표시를 보장한다는 의미는 아니다.
- 등록 전용 mDNS 주소 광고, 동일 SPKI의 인증서 갱신 허용과 다른 키 차단, 등록 해제 시 기존 연결 취소를 구현했다.
- 선택한 인터페이스 변화·확인 실패 시 네트워크를 닫고 설정 재확인·재시작을 요구한다. 다른 네트워크로 자동 전환하지 않으며 SSID·게이트웨이를 통한 집 네트워크 식별은 제공하지 않는다.
- 하드 링크 불가 시 플랫폼의 원자적 no-replace rename을 사용한다. 종료 상태의 잔여 임시 파일도 재시작 시 정리하고 완성된 사용자 파일은 보존한다.
- MCP 허용 폴더는 엔진의 파일 열기까지 검증하고 디렉터리 핸들로 경로를 제한한다. 같은 OS 계정의 악성 프로세스를 격리하는 샌드박스는 아니다. 실제 stdio→IPC→TLS 파일·텍스트·URL 송수신을 테스트했다.
- macOS arm64 앱에 엔진과 MCP 실행 파일을 동봉했다. 실제 앱 엔진의 IPC 상태와 동봉 MCP의 초기화·도구 목록·상태 조회를 확인했다. 사용자 Codex·Claude 설정은 변경하지 않았다.

## 남은 실기 검증과 배포 한계

- Windows 11 / AMD Ryzen 5 5600X(x86-64)는 사용자에게 확인했다. Windows 에이전트가 설치·재설치·기본 실행을 확인했고, 이후 공동 LAN 시험에서 방화벽 설정과 양방향 파일 전송을 확인했다. 네이티브 알림 표시·앱 종료 후 알림 클릭·드롭·자동 실행은 아직 미검증이다.
- 현재 Mac 검증 환경은 macOS 26.6.2 arm64다. 다른 Mac의 OS·CPU, Linux 서버의 배포판·CPU는 아직 확인되지 않았다.
- 실제 4대 간 12방향 전송, 실제 LAN의 multicast 발견, Linux systemd·SSH 종료·부팅 후 수신을 추가 검증해야 한다.
- 재개 후 macOS 네이티브 화면 조회·새로고침·정상 종료와 IPC를 확인했다. 시작 직후 일시적인 IPC 연결 실패 뒤 복구되어도 오류 문구가 남는 문제를 수정했다. 알림 권한 승인·콜드 스타트 알림 클릭은 아직 검증하지 않았으며 브라우저 시각 검증과 구분한다.
- 최소 macOS 배포 타깃 11.0은 API 기준이며 최소 OS 실기 통과를 뜻하지 않는다. Windows 제거·macOS Developer ID 배포 서명·notarization과 다중 모니터 복원은 추가 검증이 필요하다. 개인용 Mac DMG는 ad-hoc 서명과 번들 무결성을 확인했다.
- 원격 CI는 실행하지 않았다. Rust 툴체인은 로컬 검증한 1.99.1로 고정한다.

## Windows 테스트용 준비

Windows PC에서 직접 실행하는 x64 MSVC 빌드 스크립트와 현재 사용자용 NSIS 설정을 추가했다. 전체 검사 후 설치 파일·CLI·엔진·MCP·해시·빌드 정보를 수집한다. [Windows 절차](../packaging/windows/README.md)를 따른다. Mac에서 Windows 빌드를 실행한 것은 아니다. Windows 측 빌드·설치·기본 실행은 Windows 에이전트의 별도 검증 결과와 구분하며 실제 LAN 공동 시험은 아래 기록을 따른다.

## Windows 네이티브 검증

2026-10-25 Windows 11 Pro x64(빌드 26300)에서 Rust 1.99.1 MSVC, Node.js 24.14.1, Visual Studio Build Tools 2022와 Windows SDK로 검사했다. 기존 GNU Rust 기본 설정은 유지하고 빌드 프로세스의 `RUSTUP_TOOLCHAIN`만 MSVC로 지정했다.

- headless 테스트 63개와 데스크톱 테스트 4개가 통과했다. 실제 localhost TLS 파일·텍스트·URL 송수신, 충돌·무결성·복구, DPAPI, 사용자 전용 named pipe, 알림 바로가기 등록·재등록을 포함한다.
- `cargo fmt --all --check`, headless·데스크톱 Clippy(`--all-targets -- -D warnings`), `cargo check -p fta-desktop`, TypeScript/Vite 빌드가 통과했다.
- 첫 네이티브 테스트에서 알림 바로가기 대상의 `\\?\` 경로를 Windows Shell이 거부하는 문제와 borrowed HSTRING을 PROPVARIANT가 해제하는 힙 손상을 확인해 수정했다. Shell에 전달하는 알림·자동 실행 경로를 안전하게 정규화하고, borrowed variant의 소유권 해제를 막았다. 기존 등록·재등록 테스트와 프로토콜 명령 경로 검증으로 재확인했다.
- `packaging/windows/build.ps1`이 정상 종료했고 `FTA_0.1.0_x64-setup.exe`를 생성했다. 엔진·MCP를 동봉한 현재 사용자용 NSIS이며, CLI·엔진·MCP·해시·빌드 로그·환경 정보를 `output/windows-x64/20261024T184504Z/`에 수집했다. 배포용 CLI·엔진·MCP의 `--help` 실행도 확인했다. 설치 프로그램 생성과 설치 후 동작 검증은 구분한다.
- 당시 Windows 수정 직후에는 macOS·Linux 검사를 다시 실행하지 않았다. macOS 최종 릴리즈 검사는 별도 기록을 따른다. 설치·실행과 Windows↔Mac 실제 LAN 파일 검증 결과는 아래에 기록한다. 나머지 기기 조합과 네이티브 알림 표시·클릭·로그인 시 자동 실행은 추가 검증이 필요하다.

## Windows 설치와 실행 확인

2026-10-25 위 Windows PC에서 NSIS 현재 사용자 설치가 종료 코드 0으로 완료됐다. `%LOCALAPPDATA%\FTA`에 데스크톱·엔진·MCP와 제거 프로그램이 설치되고 Windows 제거 목록에 FTA 0.1.0이 등록됐다.

- 설치 앱의 실제 WebView 화면, 설정 화면의 기기 이름·수신 폴더, 동봉 엔진의 자동 시작을 확인했다. 배포 CLI의 `status --json`과 `config show --json`이 성공했다. 기본 수신 폴더는 Known Folder Downloads 아래 `FTA`이며, 재설치 후 기존 기기 ID와 설정이 유지됐다.
- 창의 X 버튼으로 숨긴 뒤에도 같은 엔진의 IPC 응답이 유지됐다. 재실행하면 기존 창이 열리고 데스크톱·엔진은 각각 하나만 유지됐다. 앱의 종료 버튼으로 소유 엔진까지 종료되고 재시작되는 것을 확인했다.
- 첫 설치 실행에서 NSIS가 등록한 프로토콜 실행 경로와 런타임의 경로 별칭이 달라 자기 등록을 다른 앱으로 오인하는 오류를 확인했다. 소유자 표식이 없는 기존 명령은 고정된 `"실행 파일" "%1"` 형식을 검사한 뒤 파일 ID로 같은 실행 파일인지 확인한다. 명시적 타 앱 소유자나 추가 인자가 있는 명령은 계속 거부한다.
- 회귀 테스트를 추가한 수정본에서 headless 64개·데스크톱 4개 테스트, 포맷·Clippy·데스크톱 검사·TypeScript 빌드를 통과했다. 새 산출물은 `output/windows-x64/20261024T185852Z/`다. 수정본을 재설치한 뒤 시작 stderr가 비어 있고, FTA 프로토콜 소유자 표식과 AUMID 바로가기가 생성되는 것을 확인했다. 엔진·MCP 설치 파일 해시도 산출물과 일치했다.
- 설치·실행 확인 시점에는 앱과 엔진을 loopback 수신·기기 미등록 상태로 남겼다. 이후 LAN 설정·Mac 등록과 전송 검증은 다음 절차에서 수행했다. 알림 표시·클릭, 로그인 시 자동 실행, Explorer 드롭, 제거는 설치·실행 확인에 포함하지 않았다.

## 검증 명령

저장소 루트:

```sh
cargo fmt --all --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo check -p fta-desktop
cargo test -p fta-desktop
cargo clippy -p fta-desktop --all-targets -- -D warnings
npm --prefix apps/desktop ci
npm --prefix apps/desktop run build
```

`cargo test` 기본 멤버는 headless 구성으로 Tauri·WebView 빌드를 요구하지 않는다. 데스크톱 검사는 Windows·macOS에서 별도로 실행한다. Linux 설치·명령은 [서버 문서](../packaging/linux/README.md)를 따른다.

## 실제 Mac ↔ Windows LAN 시험 (2026-10-25)

실제 기기의 양방향 파일 전송과 해시 대조, 양쪽 수신의 기존 파일 보존을 확인했다. 시험 환경·전송 ID·검증 범위는 [실기 결과](lan-test-2026-10-25.md)를 따른다. 이는 전체 4대 12방향·GUI 드롭·OS 알림 검증을 대체하지 않는다.

## iOS 네이티브 초안 (2026-10-29)

사용자 지시로 별도 `apps/ios`의 SwiftUI 기본 컨트롤·탭바 앱과 공통 Rust 정적 라이브러리 브리지를 추가했다. 파일·텍스트·URL 송수신, 최초 수신자 등록 승인, 실제 기기 목록·PC와 동일한 컴퓨터 아이콘·휴대폰 아이콘, 파일 앱 수신 폴더, 기록·Quick Look·공유·복사·사용자 링크 열기·취소·등록 해제·알림 커서를 연결했다. 앱 전면 수신을 기준으로 하며 백그라운드·잠금 시 중지한다. 기존 데스크톱 소스와 Linux·MCP 경로는 유지한다.

Mac headless 79개(기존 ignored 1개 별도), 데스크톱 10개, 브리지 실제 localhost TLS 검사 1개, 시뮬레이터 UI 검사 2개가 통과했다. fmt·headless/desktop/bridge Clippy·iOS 두 타깃 Clippy·TypeScript/Vite·시뮬레이터 빌드·기기용 unsigned Debug 빌드를 확인했다. 실제 iPhone 설치·LAN 전송·OS 알림 및 Windows/Linux 재실행은 미검증이다. [설계·정확한 검증 범위](platforms/ios.md), [로컬 빌드](../apps/ios/README.md). GitHub Actions는 사용하지 않았다.

## 온라인 기기 목록 정리 (2026-10-29)

Windows·macOS 전송·드롭 목록과 iOS 보내기 목록에서 오프라인 등록을 제외했다. 설정의 등록 해제 정보는 보존한다. 같은 ID의 주소 변경은 기존 pin 인증 후 갱신하고, 이름만 같은 다른 ID를 신뢰하거나 병합하지 않는다. 미등록 발견 후보도 TCP 접속 가능성을 확인해 갑작스러운 Wi-Fi 단절로 mDNS goodbye가 유실된 항목을 다음 조회에서 제외한다. 이 확인은 자동 등록·본문 전송을 하지 않는다. iOS는 NWPath의 Wi-Fi/유선 LAN 상실 시 확인·선택을 즉시 해제하고 이전 연결의 늦은 조회 응답을 폐기한다.

Mac에서 headless 81개 통과(기존 ignored 1개), 데스크톱 10개, 브리지 1개와 fmt·headless/desktop/bridge Clippy·iOS 두 타깃 Clippy·TypeScript/Vite를 확인했다. 주소 변경·동일 이름의 이전 등록·goodbye 없는 리스너 단절·신뢰 저장 미변경 회귀를 포함한다. iOS 시뮬레이터 UI 2개가 통과했고 중지 후 빈 전송 목록을 시각 검토했다(`output/ios-online-only.xcresult`). 사용자 상태 확인 요청 후 실행 중인 빌드·테스트가 없고 마지막 UI 검사가 통과했음을 확인한 뒤 마지막 네트워크 중지 경계를 포함해 Rust·Mac 검사를 다시 실행했다. iPhone용 unsigned 빌드도 통과했다. 실제 iPhone Wi-Fi 차단·원격 모바일 단절과 Windows/Linux 재실행은 아직 미검증이며 GitHub Actions는 사용하지 않았다.

## 모바일 미디어·기기 아이콘 보완과 iPhone 업데이트 (2026-10-29)

사용자 확인으로 실제 모바일 LAN 연결과 메시지 전송이 성공했다. PC 목록은 여전히 컴퓨터 아이콘으로 고정되어 있음을 코드에서 확인해 ios/android를 휴대폰 윤곽으로 분기했다. TypeScript/Vite 빌드와 Mac 데스크톱 check·테스트 10개·Clippy가 통과했다. PC 설치본은 이번 작업에서 교체하지 않았다.

iOS의 텍스트/URL Picker를 제거하고 단일 TextEditor와 HTTP(S) 자동 판별로 바꿨다. 사진은 갤러리 PHPicker와 권한 확인 후 시스템 카메라, 파일은 기존 파일 앱으로 나눴다. 사진·파일 초안을 분리하고 실제 사진 파일을 비공개 Outgoing으로 준비해 기존 Rust 스트리밍을 사용한다. 대상은 흰색 윤곽·짙은 중성색 받침·PC와 같은 초록/주황 상태 점을 사용한다. 새 외부 라이브러리는 추가하지 않았다.

메시지 판별 11개 경계가 통과했다. 시뮬레이터 UI 3개가 통과했으며 최종 결과는 `output/ios-media-final.xcresult`다. 갤러리를 실제 로딩하고 사진을 선택·추가하여 사진 보내기 준비 완료와 파일 초안 미변경을 검사했다. 자체 앱 아이콘 PNG를 시뮬레이터 사진 fixture로 추가해 준비 파일의 SHA-256이 원본과 같음을 확인했다. 실제 LAN 발견 기기·사진/파일 분리·단일 입력 화면을 시각 검토했다. iPhone 카메라 촬영·실제 사진 송신은 아직 미검증이다.

사용자가 Xcode에서 설정한 서명 팀과 bundle ID를 유지한 서명 빌드가 성공했다. 연결된 실제 iPhone에 기존 앱을 삭제하지 않고 0.1.0 빌드 2를 설치했다. devicectl에서 설치 버전과 새 앱 프로세스 실행을 확인했고, 업데이트 전후 기기 ID가 동일함을 확인했다. 카메라·사진 권한을 사용자를 대신해 승인하거나 사진을 임의 촬영하지 않았다. GitHub Actions·릴리스 배포는 수행하지 않았다.
