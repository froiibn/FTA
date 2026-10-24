# macOS 상세 설계안

작성일: 2026-10-25. 대상은 맥 2대다. macOS 버전과 Apple Silicon/Intel 여부는 미확인이므로 배포 타깃은 실제 기기 확인 후 확정한다.

## 1. 실행 구조

TypeScript 공통 화면, Tauri 2 Rust 호스트, 독립 Rust 엔진을 앱 번들에 포함한다. Tauri 메뉴 막대 앱이 사용자 세션에서 엔진을 시작·감시한다. TypeScript는 Tauri 명령을 호출하고 Rust 호스트가 Unix domain socket으로 엔진과 통신한다. 화면 렌더링은 macOS의 WKWebView를 사용한다. 다른 사용자의 엔진이나 중복 인스턴스를 재사용하지 않는다. 첫 버전은 별도 시스템 LaunchDaemon 없이 로그인 세션 안에서 동작한다.

창 닫기는 수신을 유지하고 메뉴의 종료는 엔진을 종료한다. 로그인 시 실행은 사용자 선택으로 등록하며 지원 버전에 맞춰 ServiceManagement 어댑터를 구현한다. macOS 13 이상에서 SMAppService가 로그인 항목·에이전트 관리를 제공하므로 우선 후보로 삼는다. [Apple: SMAppService](https://developer.apple.com/documentation/ServiceManagement/SMAppService)

로그아웃·종료·절전 중 수신은 보장하지 않는다. 화면 잠금 상태라도 앱이 실행되고 절전하지 않으면 수신한다. Mac을 계속 깨워 두는 정책은 기본으로 넣지 않는다.

## 2. 화면과 OS 연결

- 메뉴 막대 아이콘에서 전송 패널·기록·설정을 연다.
- Tauri의 고정 창과 파일 드롭 이벤트로 Finder의 일반 파일을 받는다. 패널 동작 보완이 필요할 때만 Rust macOS 어댑터를 추가한다.
- 메뉴 막대 팝오버는 닫힐 수 있으므로 지속적으로 보이는 드롭 영역은 별도 패널로 제공한다.
- Tauri 알림 플러그인과 필요한 macOS 어댑터를 통해 UserNotifications 로컬 알림을 사용하고 클릭 시 transfer_id로 내용 화면을 연다. 최초 설정에서 알림 권한을 요청한다. [Apple: UserNotifications](https://developer.apple.com/documentation/usernotifications)
- 사용자 동작으로 Tauri 플러그인·Rust 어댑터를 통해 클립보드 복사와 브라우저·Finder 열기를 수행한다. OS 내부 연동은 NSPasteboard·NSWorkspace 등을 사용할 수 있으나 별도 Swift UI는 만들지 않는다.
- Dock 노출 여부는 메뉴 막대 중심 기본값과 기록 창의 접근성을 함께 검증해 결정한다.

## 3. 권한과 배포

서명된 앱 번들을 통한 직접 배포를 초기 기준으로 제안한다. App Store 배포와 App Sandbox는 초기 범위에서 보류하며, 추후 도입 시 사용자 선택 폴더 접근과 엔진 권한 전달 구조를 다시 검토한다. 전체 디스크 접근 권한을 기본 요구로 삼지 않는다.

로컬 네트워크 개인정보 보호가 적용되는 macOS 환경에서는 용도 설명과 필요한 서비스 선언을 준비하고, 앱 번들 및 서명 정체성을 유지한 상태로 권한 요청·거부·재허용을 검증한다. 별도 엔진 프로세스의 권한 귀속은 실제 배포 번들로 확인해야 하며 개발 터미널 실행 결과만으로 판단하지 않는다. [Apple: TN3179](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy), [용도 설명 키](https://developer.apple.com/documentation/BundleResources/Information-Property-List/NSLocalNetworkUsageDescription)

## 4. 저장과 키

| 항목 | 기준 경로·방식 |
|---|---|
| 기본 수신 폴더 | OS에서 얻은 Downloads 아래 `FTA`; 최초 설정에서 쓰기 검증 |
| DB·설정 | `~/Library/Application Support/FTA` |
| 로그 | `~/Library/Logs/FTA` |
| IPC | 사용자 전용 런타임 디렉터리의 socket; 디렉터리 0700 |
| 인증키 | Keychain 보호 후보; 엔진과 UI 접근 경계를 구현 검증 |

Keychain 키를 엔진에 전달할 경우 로컬 인증 경로 외에 노출하지 않고 로그·명령행·환경 변수에 넣지 않는다. 재부팅 후 키 접근이나 사용자 세션이 준비되지 않으면 수신 불가 상태를 표시한다.

APFS의 대소문자 설정, Unicode 파일명, 외장 드라이브 제거, 다운로드 폴더 접근 거부를 검증한다. 수신 파일과 폴더는 Finder에서 열 수 있어야 하며 실행 파일이나 URL을 자동 실행하지 않는다.

## 5. 배포·검증

Tauri 번들러로 생성한 앱 번들의 엔진을 포함한 서명과 notarization을 배포 검증 항목으로 둔다. 로그인 항목 등록 경로가 앱 이동·업데이트 뒤에도 유효한지 확인한다. 지원 아키텍처는 맥 두 대의 CPU 확인 후 universal 또는 개별 빌드로 결정한다.

필수 검증: Finder 드롭, 전체 화면·다중 모니터의 고정 패널, 메뉴 막대 유지, 알림 거부와 재허용, 앱 재실행을 포함한 알림 클릭, 키 접근 실패, 맥 간 양방향 전송, Windows·Linux 연동, 절전 복귀·로그아웃·네트워크 전환.
