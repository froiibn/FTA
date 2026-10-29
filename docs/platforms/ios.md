# iOS 초안 설계·구현·검증

작성일: 2026-10-29. 상태: 네이티브 초안 구현, 로컬 컴파일·시뮬레이터·공통 Rust 검사 수행. 실제 iPhone LAN 검증 전이다.

## 결정과 경계

사용자가 iOS 우선, 별도 폴더, 기본 컨트롤·탭바, 네이티브 활용을 지시했다. 이에 `apps/ios`에 SwiftUI 앱을 추가했다. 이전의 모바일 Tauri 재사용 제안보다 이번 지시를 우선한다. Windows·macOS는 기존 TypeScript·Tauri 2, Linux·MCP는 기존 Rust·사용자 전용 IPC 구조를 유지한다. Android는 향후 목표이며 구현하지 않았다.

이번 빌드 기준은 iOS 17 이상, arm64 기기·Apple Silicon 시뮬레이터다. 전체 제품의 최소 지원 OS 확정이나 구형 OS 실기 검증을 의미하지 않는다.

## 실제 호출·발견 경로

`SwiftUI → 비동기 C ABI 제어 JSON → fta-ios 정적 라이브러리 → fta-core`다. 파일 바이트는 ABI/JSON으로 전달하지 않고 엔진에서 스트리밍한다. 브리지는 명령을 허용 목록으로 제한하고 선택 파일의 비공개 Outgoing 사본만 파일 송신 권한으로 설정한다. 네트워크 작업을 기다리는 동안 전역 호스트 잠금을 풀어 등록 승인·취소·진행률 조회가 병행된다.

NetServiceBrowser/NetService가 `_fta._tcp`와 `_fta-pair._tcp`를 검색·광고한다. 광고 포트는 실제 Rust 리스너에서 얻는다. 등록 리스너 포트가 바뀌면 광고도 갱신한다. 발견은 주소 힌트일 뿐이며 기존 TLS 1.3 상호 인증과 SPKI pin을 우회하지 않는다. iOS에서는 공통 엔진의 raw mDNS 실행 경로를 사용하지 않는다. [Apple Bonjour API](https://developer.apple.com/documentation/foundation/netservice), [서비스 선언](https://developer.apple.com/documentation/BundleResources/Information-Property-List/NSBonjourServices).

실제 OS 인터페이스에서 직접 연결된 private IPv4 LAN만 선택 후보로 제공한다. VPN·셀룰러를 후보로 제시하지 않는다. 사용자가 집 연결임을 확인한 뒤 시작하며 허용 대역을 넘어선 발견 주소는 Rust에서 거부한다. 네트워크 변경을 감지하면 중지한다. SSID 기반 신뢰 저장은 하지 않으며 백그라운드 복귀 후에도 다시 확인한다. IPv6 전용 LAN·Wi-Fi Direct·외부 중계는 이번 초안 범위가 아니다.

## 신뢰·온라인·수명

- 미등록 기기는 ‘발견됨 · 최초 등록 필요’다. 온라인으로 꾸미지 않는다.
- 최초 전송은 공통 `transfers.send_discovered`를 사용한다. 본문 없이 등록 의도를 보내고 수신자만 한 번 허용한다.
- 수신 등록 sheet는 종류·항목 수·기기 이름·만료를 표시한다. 파일명·메시지 본문은 승인 전에 보내지 않는다.
- 이후 등록된 기기는 인증 probe와 일반 전송 검증 후 별도 수신 승인 없이 받는다. 키 변경은 자동 신뢰하지 않는다.
- 엔진은 앱 전면 실행에 한해 LAN 리스너를 유지한다. 백그라운드·잠금에서 광고·소켓을 중지하고 복귀 후 집 연결을 재확인한다.
- 네트워크 중지와 저장소 수명을 분리해 중지 상태에서도 기록·본문·파일을 열 수 있다. 재시작은 이전 엔진을 소유한 네트워크 작업과 제어 요청이 끝난 뒤 진행한다.
- OS가 앱을 즉시 종료해 광고 해제를 수행하지 못해도 상대의 TLS probe·송신 직전 확인을 온라인 근거로 사용한다.

상시 LAN 서버나 백그라운드 URLSession 이식은 구현하지 않았다. [Apple 백그라운드 실행 제한](https://developer.apple.com/forums/thread/685525).

## 화면과 네이티브 기능

| 탭 | 구현 |
|---|---|
| 보내기 | 실제 기기·상태·선택, 사진/촬영·파일 선택, 단일 메시지 입력, 진행률·취소·결과 |
| 기록 | 전체/받음/보냄 필터, 상세·항목별 결과, 본문 확인·복사, HTTP(S) 사용자 열기, 받은 파일 Quick Look·공유, 결과 불명 재조회, 기록 삭제 |
| 설정 | 기기 이름 저장, 현재 LAN 선택·집 연결 확인·송수신 사용, 수신 경로 안내, 알림 권한·iOS 설정, 등록 해제 |

컴퓨터 아이콘은 PC 화면의 `M3 4h18v12H3z`, `M8 21h8m-4-5v5`와 같은 좌표를 Canvas로 그린다. 모바일은 휴대폰 SF Symbol이다. Dynamic Type·기본 시스템 색상·기본 탐색·sheet를 사용한다. 가짜 기기나 가짜 전송 결과는 없다. 상세 sheet와 등록 sheet의 표시 경로는 하나로 통합해 등록 요청을 우선한다.

## 파일·키·복구

- Documents/받은 파일: 최종 수신 파일. 파일 앱의 `나의 iPhone/FTA/받은 파일`에 공개한다. `UIFileSharingEnabled`와 `LSSupportsOpeningDocumentsInPlace`를 사용한다. [Apple 파일 공개 설정](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/LaunchServicesKeys.html).
- Application Support/FTA: DB·설정·비공개 수신 partial·선택 송신 사본. 자동 백업에서 제외한다. Foundation으로 경로를 얻으며 사용자/기기 경로를 하드코딩하지 않는다.
- Keychain: 동기화하지 않는 WhenUnlockedThisDeviceOnly 개인키. 키 접근 실패를 새 신원으로 조용히 대체하지 않는다. TLS 서명을 위해 개인키 DER는 Rust 메모리에서 사용하며 비반출 Secure Enclave 키라고 주장하지 않는다.
- 파일 선택: security-scoped 자원과 NSFileCoordinator로 일반 파일을 로컬 사본으로 준비한다. 공급자 오류·공간 부족·잘못된 파일은 실패한다. 공급자 문서는 준비에 인터넷이 필요할 수 있지만 기기 간 전송은 LAN으로 한정된다.
- 수신: 정확한 크기·해시 확인 후 최종 파일과 완료 DB 기록을 저장한다. 기존 파일을 덮어쓰지 않는다. 부분 성공·실패·결과 불명을 구분한다.
- 앱 설치 시 sandbox 위치가 바뀌면 앱 소유 수신 root·항목 경로·이벤트와 private partial 위치를 복구 전에 재배치한다. 다른 수신 root는 변경하지 않는다. 실제 파일 앱에서 사용자가 이동·삭제한 파일을 임의 복원하지 않는다.
- 기록 삭제는 수신 파일을 삭제하지 않는다. 앱 제거는 앱 소유 파일도 삭제할 수 있다.

공통 Windows·macOS·Linux의 partial 위치와 기존 저장 동작은 유지한다.

## 알림

`ios-notifications-v1` 소비자 커서로 영속 이벤트를 순서대로 읽는다. 수신 이벤트는 transfer ID를 포함한 로컬 알림으로 제출하고 처리 확인 후 커서를 갱신한다. 알림 요청 ID는 이벤트 sequence에서 파생해 중단 후 재처리 중복을 줄인다. 권한 미설정·거절은 표시 생략으로 명시적으로 처리하고 수신 기록은 유지한다. 알림 제출과 OS 배너 표시를 구분하며 정확히 한 번 표시를 보장하지 않는다. 알림 탭은 해당 상세 기록을 열고 URL·파일을 자동 실행하지 않는다.

## 로컬 검증 결과

사용자 소유 Mac, Xcode 27.1(27A9275), iOS SDK 27.1, iOS 27.0 iPhone 18 Pro 시뮬레이터, 저장소 Rust 1.99.1에서 검사했다.

| 검사 | 결과·범위 |
|---|---|
| `cargo fmt --all --check` | 통과 |
| `cargo test` | headless 79개 통과, 1개 기존 ignored. 모든 OS 검사로 해석하지 않음 |
| `cargo clippy --all-targets -- -D warnings` | Mac 기본 headless 타깃 통과 |
| `cargo test -p fta-ios` | 브리지 1개 통과: 실제 localhost TLS 파일 송수신·최종 파일 대조, 허용 폴더 밖 파일 차단, 중지 후 기록, 명령 제한·C 문자열 소유권 |
| `cargo clippy -p fta-ios --all-targets -- -D warnings` | Mac 브리지 통과 |
| iOS 기기·시뮬레이터 Rust Clippy | 각각 `--target aarch64-apple-ios` / `aarch64-apple-ios-sim` 통과 |
| Xcode iPhone용 Debug 빌드 | `-sdk iphoneos CODE_SIGNING_ALLOWED=NO` 통과. 서명·설치·실기 실행은 미검증 |
| Xcode 시뮬레이터 앱·UI 검사 | ad-hoc 서명으로 2개 통과. 실제 탭·빈 상태·비활성 전송·시스템 파일 선택기, 실제 Rust 리스너 시작·전면 수신·홈 이동 후 중지·복귀 확인 |
| Mac 데스크톱 회귀 | `cargo check -p fta-desktop`, 테스트 10개, Clippy 통과. `npm --prefix apps/desktop run build` 통과 |
| 수신 root 이동 회귀 | 공통 Rust 검사에서 소유 경로·이벤트 순번 보존 확인 |

UI 검사 attachment에서 보내기·기록·설정·파일 선택기·수신 활성/중지 화면을 시각 검토했다. 결과 bundle은 로컬 `output/ios-ui-validated.xcresult`에 있다. 시뮬레이터의 LAN 주소는 OS에서 얻은 실제 값이다.

초기 unsigned 시뮬레이터 앱의 Keychain 저장 실패는 앱 서명·Keychain entitlement로 해결했다. 알림 cursor 이름에 점을 넣어 공통 계약을 위반한 오류도 실제 수신 시작 검사에서 확인해 수정했다. 최종 UI 검사에서는 시작 오류·폴링 오류가 없음을 검사했다. Xcode의 SDK build-number 메타데이터 경고는 나타났지만 최종 검사와 빌드는 성공했다.

실제 iPhone 설치·서명·LAN 상호 전송, iPhone↔iPhone, Windows/Linux 실행, MCP→iPhone, 잠금·강제 종료·권한 철회·알림 실제 표시/클릭, 구형 iOS·다크 모드·대형 파일·공간 부족·클라우드 문서 공급자 실기는 미검증이다. Mac 브리지의 localhost 검사를 실제 iPhone 전송으로 보고하지 않는다.

## 후속 범위

외부 폴더로 자동 수신, 별도 공유 확장, background URLSession 전송 지속, Android는 이번 초안에 포함하지 않는다. 파일 앱의 일반 사진·영상 파일은 기존 파일 선택 흐름으로 보낼 수 있다. 실제 iPhone 검증 전 제품 전체 완료나 App Store 배포 준비 완료를 의미하지 않는다. 빌드 절차는 [iOS README](../../apps/ios/README.md)를 따른다. GitHub Actions·사용자 MCP 설정은 변경하지 않았다.

## 온라인 전송 대상 보완 (2026-10-29)

전송 목록은 등록된 온라인 기기와 실제 접속 가능한 미등록 최초 승인 후보만 표시한다. 설정의 등록 정보는 보존한다. NWPath가 Wi-Fi/유선 LAN을 잃으면 집 연결 확인·선택을 즉시 해제하고, 이전 연결에서 시작된 늦은 조회 응답이 목록을 되살리지 못하도록 세대를 확인한다. 원격 기기의 갑작스러운 Wi-Fi 단절은 다음 probe/접속 확인에서 제외한다. 실제 iPhone Wi-Fi 차단은 아직 실기 미검증이다.

## 사진·촬영·메시지 입력 추가 (2026-10-29)

사용자가 실제 모바일 LAN 연결과 메시지 전송 성공을 보고했다. 이는 사용자 확인 결과이며 에이전트의 파일·촬영·알림 실기 검증 완료를 의미하지 않는다.

사진 선택은 PHPicker의 이미지 선택만 허용하고 `loadFileRepresentation` 콜백 안에서 비공개 Outgoing 사본으로 복사한다. 임시 URL이 콜백 반환 후 유효하다고 가정하지 않는다. 촬영은 카메라 권한을 요청하고 시스템 전체 화면 카메라를 사용하며 사진 보관함에 자동 저장하지 않는다. 촬영 결과는 네이티브에서 JPEG로 준비한다. Photos 전체 읽기·쓰기 권한이나 마이크 권한을 요청하지 않는다. 준비한 사진은 일반 파일과 같은 Rust 크기·해시·최종 저장 검증을 사용한다.

사진과 파일 초안은 따로 유지하며 취소·실패 때 이전 선택을 보존한다. 파일 앱 입력은 유지한다. 메시지는 TextEditor 하나로 보내고 단일 HTTP(S) 주소만 URL로 자동 판별한다. 원문 공백·줄바꿈을 임의 제거하지 않는다. 대상은 흰색 컴퓨터/휴대폰 윤곽과 상태 점으로 표시하며 PC os 분기 아이콘도 추가했다. 서명 팀은 유지하고 연결된 iPhone에 앱 삭제 없이 빌드 2를 업데이트한다.

이번 변경의 로컬 UI 검사 3개, 메시지 판별 11개, Mac 데스크톱 테스트 10개·check·Clippy·TypeScript/Vite와 서명 기기 빌드가 통과했다. devicectl로 실제 iPhone의 `app.fta.ios` 0.1.0 빌드 2 설치·실행과 기존 기기 ID 유지까지 확인했다. 사진 준비는 시뮬레이터에서 실제 PHPicker 선택과 원본 해시 대조로 검증했다. 실제 카메라 촬영과 사진 송수신은 미검증이다. 이전 초안의 ‘기기 설치 미검증’ 기록은 그 시점의 결과이며 이번 업데이트 확인으로 설치·실행 범위가 추가됐다. PC 설치본은 교체하지 않았다.
