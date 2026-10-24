# FTA 데스크톱

Windows·macOS의 공통 TypeScript 화면과 Tauri 2 Rust 호스트다. 파일 전송은 독립 `fta-engine` 프로세스가 담당한다. 브라우저 미리보기에는 가짜 기기나 전송 결과를 넣지 않는다.

## 개발

저장소 루트에서 `cargo build -p fta-engine`을 먼저 수행한다. 다음 명령은 이 디렉터리에서 실행한다.

```sh
npm ci
npm run build
npm run tauri dev
```

개발 서버만 실행하려면 `npm run dev`를 사용한다. 주소는 `http://127.0.0.1:1420`이며 브라우저에서는 전송 기능을 비활성화한다. Tauri 실행 래퍼는 `CARGO_HOME` 또는 현재 사용자의 rustup 설치 경로에서 Rust 도구를 찾아 하위 빌드 프로세스에도 같은 환경을 전달한다.

엔진 경로는 `FTA_ENGINE_BIN`, 앱 실행 파일 옆 `fta-engine`, 개발 빌드의 저장소 `target/debug/fta-engine` 순으로 찾는다. Windows는 `.exe`를 사용한다. 이미 해당 사용자 엔진이 실행 중이면 기존 IPC에 연결하며 독립 서비스는 앱 종료 때 중지하지 않는다.

## 배포

```sh
npm run bundle
```

이 명령은 현재 호스트용 실제 `fta-engine`·`fta-mcp`를 release 빌드한 뒤 Tauri sidecar로 포함한다. `npm run tauri build`만 실행하면 이 실행 파일들이 동봉되지 않으므로 배포에는 `bundle` 명령을 사용한다. macOS 앱 번들만 만들려면 `npm run bundle -- --bundles app`을 사용한다. 교차 빌드·서명·notarization·Windows 설치 후 알림 활성화는 대상 OS에서 별도로 검증해야 한다. 생성된 binaries와 dist는 소스 관리 대상이 아니다.

macOS 최소 배포 타깃은 UserNotifications의 Banner/List 표시를 기준으로 11.0이다. 이 값은 최소 OS의 실기 통과를 의미하지 않는다. MCP 클라이언트는 macOS의 `FTA.app/Contents/MacOS/fta-mcp` 또는 Windows 앱 옆의 `fta-mcp.exe`를 stdio 서버로 실행하도록 사용자가 등록한다. 데스크톱 앱은 이 서버를 자동 실행하거나 Codex·Claude 등의 전역 설정을 수정하지 않는다.

네이티브 아이콘 원본은 `src-tauri/icons/source.svg`이며 Tauri `icon` CLI로 PNG·ICO·ICNS를 생성한다. 네이비 컬러와 양방향 전송 표식을 툴바·앱 아이콘·favicon에서 함께 사용한다.

## 구현된 연결과 현재 한계

- 메인 화면은 기기별 상태 점을 표시하고 기록·설정은 툴바 아이콘으로 독립 OS 창을 연다. 보조 창을 닫아도 본체·엔진은 유지하고 알림 소비는 본체에서만 처리한다. My computer 영역과 페이지 탭은 사용하지 않는다.
- 기기 목록, 최초 등록·비교값 조회, 파일 선택·드롭, 텍스트·URL 전송, 기록·내용·복사·링크 열기, 수신 폴더·LAN 설정은 로컬 IPC로 실제 엔진과 연결한다.
- 별도 파일 놓기 영역 없이 온라인 등록 기기 아이콘에 드롭하면 해당 기기로 전송한다. 백엔드 단위를 정규화해 Mac의 논리 좌표는 유지하고 Windows 물리 좌표는 DPI로 변환한 뒤 실제 놓은 타일을 확인하며 기존 선택으로 대체하지 않는다. 파일 선택 버튼도 유지한다.
- 파일은 사용자 선택·드롭으로 허용한 경로만 전달한다. 파일 body는 WebView를 통과하지 않는다.
- 기본 창은 280×260 논리 픽셀이며 최소 240×230이다. 상단 접기 버튼을 누르면44×44 펼치기 버튼만 남고 다시 누르면 이전 크기로 복원한다. 고정 창, 트레이·메뉴 막대, 로그인 시 실행, 단일 인스턴스를 제공한다. 창 닫기는 수신을 유지하고 미전송 선택 목록만 비운다. macOS Dock 재열기는 본체를 펼쳐 표시하고 최소화를 해제한다. 완전한 앱 종료는 메뉴의 종료 또는 ⌘Q를 사용한다.
- 진행 표시는 엔진의 항목별 `bytes_transferred`를 BigInt로 합산한 바이트 진행률과 완료 파일 수다. 100% 송신과 최종 저장 완료를 구분한다.
- 알림은 영속 이벤트 커서를 사용한다. UI 리스너가 준비된 뒤 순서대로 처리하고 기록 반영·네이티브 알림 요청이 처리된 뒤 acknowledgement를 저장한다. 중단된 이벤트는 재시작 후 다시 처리하며, 중복 표시 가능성이 있는 at-least-once 처리다.
- 알림 비허용·아직 권한 미요청·개발 실행 파일의 네이티브 알림 불가 상태에서는 기록을 반영하고 이벤트를 acknowledge한다. 실제 요청 준비·프레임워크 등록 오류는 acknowledge하지 않아 재시도한다. OS가 집중 모드 등으로 실제 화면 표시를 억제하는 것은 앱이 보장할 수 없다.
- macOS는 `UNUserNotificationCenter` delegate를 앱 이벤트 루프 시작 전에 등록하고 알림의 영속 `userInfo`에 전송 UUID를 저장한다. 응답 시 이 UUID를 대기 큐에 두며 엔진 연결·기록 조회 성공 후에만 제거한다. 권한도 네이티브 API로 조회한다. [Apple delegate 문서](https://developer.apple.com/documentation/usernotifications/unusernotificationcenterdelegate)
- Windows는 AUMID·프로토콜 알림 어댑터와 `fta://inbox/<UUID>`를 사용한다. Tauri deep-link와 단일 인스턴스의 시작 인자를 같은 검증 큐로 연결한다. URI는 고정 scheme·host·단일 UUID만 허용하며 경로·셸 실행에 사용하지 않는다.
- 일반 종료와 서비스 재시작은 소유한 엔진에 `engine.shutdown`을 요청하고 제한 시간 동안 정상 종료를 기다린다. 응답 불가·시간 초과 때만 강제 종료하며 독립 서비스는 종료하지 않는다.
- 고정 여부를 보관하며 window-state 플러그인으로 위치·크기를 복원한다. 저장 위치와 겹치는 모니터가 없으면 OS의 기본 배치를 사용한다. 실제 다중 모니터 전환 동작은 추가 검증이 필요하다.

## 검증

저장소 루트에서 `cargo check -p fta-desktop`, `cargo clippy -p fta-desktop --all-targets --no-deps -- -D warnings`, `cargo test -p fta-desktop`를 수행한다. TypeScript는 이 디렉터리의 `npm run build`로 엄격 타입 검사와 프로덕션 번들을 검증한다. UI 디자인 기준은 [설계 문서](../../docs/ui-design.md)에 있다. 네이티브 알림·권한·드롭·자동 실행과 앱 종료 후 알림 클릭은 실제 서명·설치 형태에서 추가 검증이 필요하며, 컴파일이나 브라우저 렌더링만으로 실기 통과로 보고하지 않는다.

전체 화면과 네이티브 창은 고정 다크 테마다. 테마 전환 옵션은 제공하지 않는다. 기록은 600×520, 설정은 520×640 기본 크기의 별도 창이며 전송 버튼은 텍스트 입력 영역 안의 화살표로 표시한다.

모든 창은 OS 장식 대신 앱의 다크 타이틀바를 사용한다. 타이틀바의 빈 영역으로 창을 이동하고 닫기 버튼으로 해당 창을 닫는다. 본체 닫기는 트레이 수신을 유지한다. 접힌 상태에서도 수신·전송·초안을 유지하며, 펼쳐 기기 아이콘에 파일을 놓을 수 있다. 접힌 크기는 다음 실행의 본체 크기로 복원하지 않는다.
