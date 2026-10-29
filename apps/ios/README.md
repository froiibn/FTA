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
