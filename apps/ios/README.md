# FTA iOS 초안

SwiftUI 기본 컨트롤과 공통 Rust 엔진을 사용하는 별도 iOS 앱이다. 기존 Windows·macOS Tauri 앱과 Linux·MCP 실행 경로는 유지한다. iOS 17 이상·arm64를 이번 초안의 빌드 기준으로 선택했으며 전체 제품의 최소 OS 확정을 의미하지 않는다.

## 구성

- `FTA.xcodeproj`: Xcode에서 열 수 있는 공유 FTA scheme과 로컬 UI 검사.
- `FTA/`: SwiftUI 보내기·기록·설정 탭, 시스템 문서 선택기, Quick Look·공유, Keychain과 Bonjour 연결.
- `bridge/`: 공통 Rust 엔진을 앱 안에서 실행하는 `fta-ios` 정적 라이브러리. 제어 JSON만 C ABI를 통과한다.
- `scripts/build-rust.sh`: Xcode가 선택한 기기/시뮬레이터 타깃에 맞춰 Rust를 빌드한다.
- `scripts/generate-project.py`: 외부 생성기 없이 프로젝트를 재생성한다. Swift 파일 추가 뒤 실행한다.

컴퓨터 아이콘은 PC 버전 `apps/desktop/src/main.ts`의 두 path와 같은 좌표로 그린다. iOS·Android는 휴대폰 아이콘으로 표시한다. 미리보기에 가짜 기기를 넣지 않는다.

## 로컬 빌드

Xcode와 저장소 Rust 툴체인이 필요하다. Apple Silicon Mac에서 다음 타깃을 한 번 준비한다.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
open apps/ios/FTA.xcodeproj
```

Xcode의 FTA target에서 사용자 본인의 서명 팀을 선택하거나, 무시되는 `FTA/LocalSigning.xcconfig`에 본인의 `DEVELOPMENT_TEAM`을 지정하면 연결한 iPhone에서 실행할 수 있다. 저장소에는 팀·프로비저닝·사용자 기기 식별자를 기록하지 않는다. 기기용 unsigned 빌드 성공은 설치·실행 검증을 의미하지 않는다.

```sh
# Simulator Keychain also requires signing. Ad-hoc signing uses no developer account.
xcodebuild -project apps/ios/FTA.xcodeproj -scheme FTA \
  -sdk iphonesimulator -configuration Debug \
  -derivedDataPath output/ios-derived CODE_SIGN_IDENTITY=- build

# Use an available simulator name or ID from `xcrun simctl list devices available`.
xcodebuild -project apps/ios/FTA.xcodeproj -scheme FTA \
  -destination 'platform=iOS Simulator,name=iPhone 18 Pro' \
  -derivedDataPath output/ios-derived CODE_SIGN_IDENTITY=- test

cargo test -p fta-ios
cargo clippy -p fta-ios --all-targets -- -D warnings
cargo clippy -p fta-ios --target aarch64-apple-ios-sim -- -D warnings
```

빌드 단계에서 Rust 타깃을 자동 설치하지 않고 저장소 lockfile을 사용한다. GitHub Actions는 사용하지 않는다.

## 사용

1. 같은 LAN에서 상대 FTA를 실행한다.
2. 설정에서 사용할 Wi-Fi/유선 LAN 주소를 확인하고 집 네트워크 확인을 켠다. 실제 OS 인터페이스에서 구한 연결만 제공한다.
3. 로컬 네트워크 권한 요청이 나오면 앱 사용에 필요한 허용 여부를 선택한다. 권한 거절 시 iOS 설정에서 변경할 수 있다.
4. 보내기 탭에서 기기를 선택하고 파일 또는 텍스트·URL을 보낸다. 최초 전송은 수신자 한 번의 기기 등록 승인을 기다린다.
5. 파일은 `나의 iPhone → FTA → 받은 파일`에 저장된다. 기록에서 미리보기·공유·텍스트 복사·URL 열기를 사용할 수 있다.

앱을 백그라운드로 보내거나 화면을 잠그면 수신을 중지한다. 복귀 시 집 네트워크를 다시 확인한다. OS가 앱을 강제 중단한 경우 상대의 인증 probe와 연결 실패로 온라인이 해제된다. 상시 수신·원격 깨우기·오프라인 전달은 제공하지 않는다.

## 저장·복구·권한

- 수신 최종 파일만 Documents에 공개한다. 미완성 수신·DB·선택한 송신 사본은 비공개 Application Support에 둔다.
- 개인키는 동기화하지 않는 `WhenUnlockedThisDeviceOnly` Keychain에 저장한다. Rust TLS 사용 중에는 키가 앱 메모리에 존재한다. 비반출 Secure Enclave 서명 방식은 아니다.
- 새 설치·복원에서 키를 자동으로 기존 신원으로 신뢰하지 않는다. 기존 등록과 키가 맞지 않으면 전송을 차단한다.
- 선택한 파일을 security scope·파일 조정 하에 비공개 사본으로 준비한다. Rust는 이 Outgoing 디렉터리만 송신 허용 폴더로 인정한다.
- 원본 크기를 확정한 일반 파일만 전송한다. 클라우드 문서는 공급자의 로컬 준비가 필요할 수 있다.
- 최종 파일과 DB 기록이 저장된 후에만 완료한다. 부분 성공·실패·결과 불명을 구분하며 결과 확인은 payload 재송신을 하지 않는다.
- 파일 앱에서 이동·삭제한 수신 파일은 기록에 파일 없음으로 표시한다. 기록 삭제는 수신 파일을 삭제하지 않는다.
- 알림 제출 후 DB 소비자 커서를 확인한다. 알림 거절 시 표시를 생략하고 기록은 유지한다. OS의 실제 배너 표시는 보장하지 않는다.

외부 폴더 자동 수신, 별도 공유 확장, 백그라운드 전송 지속, Android 앱은 이번 초안에 포함하지 않는다. 일반 사진·영상 파일은 시스템 파일 선택기로 전송할 수 있다.

사진 선택은 시스템 갤러리, 사진 촬영은 카메라 권한을 확인한 시스템 카메라를 사용한다. 파일 선택은 파일 앱을 유지한다. 사진·파일 선택은 독립적이며 메시지 입력에서 URL 모드를 직접 선택하지 않는다. 단일 HTTP(S) URL만 자동 판별한다. 카메라가 없는 시뮬레이터에서는 촬영 불가를 안내한다.

메시지 판별 경계는 제품 소스를 직접 컴파일해 검사할 수 있다.

```sh
xcrun swiftc apps/ios/FTA/ContentRules.swift apps/ios/Tests/ContentRulesChecks.swift -o output/ios-content-checks
output/ios-content-checks
```

구현·검증 결과와 남은 실기기 항목은 [iOS 설계](../../docs/platforms/ios.md)를 따른다.

## 2026-10-30 모바일 수정

신뢰한 허용 대역·선택한 연결·송수신 사용은 재실행과 재접속 후 복원한다. 이전 엔진에 기록된 단일 승인 대역만 현재 연결과 정확히 일치할 때 이관하며 기본 대역 목록이나 새 연결을 임의 승인하지 않는다. 보내기 화면은 가로 기기 아이콘·흰색 사진/파일 버튼·통합 첨부·최대 네 줄 메시지와 원형 전송 아이콘으로 구성한다. 라이트/다크 시스템 외관과 네이티브 반투명 소재를 사용한다.

아이콘 재생성은 `xcrun swift apps/ios/scripts/render-icon.swift apps/desktop/src-tauri/icons/icon.png apps/ios/FTA/Assets.xcassets/AppIcon.appiconset/AppIcon.png apps/ios/FTA/Assets.xcassets/AppMark.imageset/AppMark.png`로 수행한다. 공통 데스크톱 원본의 투명 바깥 영역만 iOS 요구에 맞춰 흰색으로 채운다.

설정 저장 회귀는 `xcrun swiftc apps/ios/FTA/NetworkPreferences.swift apps/ios/Tests/NetworkPreferencesChecks.swift -o output/ios-network-checks` 후 해당 실행 파일로 검사한다. UI 검사는 재실행·백그라운드 복귀·송수신 중지 유지·갤러리 첨부·네 줄 메시지·시스템 외관을 실제 앱으로 검사한다. 검증 수치와 실기 설치 결과는 iOS 플랫폼 문서의 최신 검증 기록을 따른다.


## 2026-10-30 스크롤·아이콘 전송 상태

상단 브랜드와 본문은 함께 스크롤하고, 메시지 입력창은 후속 사용자 정정에 따라 기존 하단 위치를 유지한다. 사진 소스 popover는 사진 버튼 바로 아래에 연결하며 빈 영역 탭으로 키보드를 닫는다. 기기 아이콘을 소폭 확대하고 상태 점을 윤곽에 붙인다. 완료 문구 대신 실제 전송 진행률을 타일 테두리로 표시하고 최종 저장 응답 후 1.5초 체크를 표시한다. 결과 불명·부분 완료·실패는 성공 체크를 주지 않는다. 기록 화면은 기존 배경과 제목 방식을 유지하고 글자색만 흰색으로 변경한다.

전송 표시 경계 검사는 `xcrun swiftc apps/ios/FTA/TransferFeedback.swift apps/ios/Tests/TransferFeedbackChecks.swift -o output/ios-transfer-feedback-checks`로 빌드한 실행 파일을 사용한다. 저장 응답 전 성공 금지, 1.5초 경계, 이전 전송의 늦은 조회·타이머가 새 대상 표시를 지우지 않는 조건을 검사한다. iOS 0.1.2 빌드 4로 업데이트하며 PC 배포본과 자동 업데이트 기능은 변경하지 않는다.

0.1.2 빌드 5에서는 요청 범위를 벗어난 설정·기록 제목 변경, 기록 배경 변경, 빈 상태 높이 변경만 이전 상태로 복원했다. 사용자가 요청한 흰색 기록 글자와 메인 화면 수정은 유지한다. 관련 UI 검사 2개·서명 빌드·연결 iPhone 설치/실행을 확인했다.

0.1.2 빌드 6에서는 사용자 정정에 따라 원형 글래스 새로고침과 기존 하단 글래스 입력창을 복원했다. 공통 소재·색조·그림자 값은 원래 그대로 유지한다. 해당 UI 검사 3개·서명 빌드·iPhone 설치 버전을 확인했다. 기기 잠금으로 자동 실행은 거부되어 실기 실행 확인은 미완료다.

0.1.2 빌드 7은 사진/파일 전송의 `allowed directory contains a replaced link or inaccessible component` 오류를 수정한다. iOS에서는 시스템 부모 폴더 대신 Foundation이 제공한 앱 컨테이너 핸들을 기준으로 허용 Outgoing 폴더를 연다. 루트 밖 접근과 교체된 링크 차단은 유지한다. headless 83개·bridge 복합 검사 1개·시뮬레이터 UI 6개가 통과했고, 서명 빌드의 iPhone 설치·실행과 기존 기기 식별자 유지를 확인했다. 실기 사진의 상대 기기 최종 저장은 별도 재확인하지 않았다. UI 변경은 없다.

0.1.2 빌드 8은 요청한 전송 표시만 변경한다. 회전 점선을 제거하고 실제 바이트 진행률을 녹색 테두리로 표시하며, 최종 저장 완료 후 기기 아이콘이 두 번 깜빡인다. 기존 체크의 1.5초 유지·온라인 점·레이아웃·글래스는 유지한다. 상태 경계 검사·컴포넌트 시각 검토·시뮬레이터 UI 6개·서명 빌드와 iPhone 설치/실행을 확인했다. 실기 전송 중 효과의 직접 관찰은 미실시다.

## 2026-10-30 App Store 준비·다국어 (빌드 9)

설정 하단에서 오프라인 개인정보 처리방침·원문 의존성 고지·지원 링크를 제공하고 실제 required-reason API 매니페스트를 포함한다. iOS만 시스템 SQLite를 사용하며 실제 iOS TLS 파일 전송·중복 파일·WAL·최종 저장 복구/무결성을 확인했다. 데스크톱·Android의 bundled SQLite는 유지한다. 기존 글래스·새로고침·입력창·전송 표시 구조는 유지하면서 UI 137개 키와 권한 설명을 10개 언어로 리소스화했다. 미지원 언어는 영어, 정책 전문은 한국어/영어를 제공한다.

[출시 준비 상태·검사·스크린샷 제작·미완료 소유자 확인](Store/README.md)을 따른다. 최종 Apple Distribution/App Store export·검증 및 0.1.2(9) 업로드·서버 처리는 성공했다. 10개 스토어 제목은 `FTA - File Transfer Anything`으로 저장/재접속 확인했고 홍보 이미지 60개가 처리됐다. 현재 빌드는 수출 규정 문서 누락 상태이며 프랑스 포함 배포의 신고 승인서, 나머지 소유자 법적 확인 및 최종 실기/구형 OS 검증을 완료로 보고하지 않는다. 실제 심사 제출은 하지 않았다.
