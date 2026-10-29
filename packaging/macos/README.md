# macOS 설치와 빌드

현재 릴리즈는 Apple Silicon(arm64)용이다. Intel Mac용 실행·빌드 검증은 수행하지 않았다.

## 0.1.2 배포

[0.1.2 릴리스](https://github.com/froiibn/FTA/releases/tag/v0.1.2)의 `FTA_0.1.2_aarch64.dmg`는 로컬 Mac에서 만든 Developer ID 서명·Apple 공증 배포본이다. 실제 공증 승인·티켓·Gatekeeper 검사 결과와 SHA-256은 릴리스의 `build-info-macos-0.1.2.json`과 `SHA256SUMS-macos-0.1.2.txt`에서 확인한다. 기존 0.1.1 배포본도 유지한다. 0.1.2의 소스·검증·Windows 후속 빌드는 [릴리스 기록](../../docs/releases/0.1.2.md)을 따른다.

## 0.1.1 설치

1. 공개된 [v0.1.1 릴리스](https://github.com/froiibn/FTA/releases/tag/v0.1.1)에서 [FTA_0.1.1_aarch64.dmg](https://github.com/froiibn/FTA/releases/download/v0.1.1/FTA_0.1.1_aarch64.dmg)와 [SHA256SUMS-macos-0.1.1.txt](https://github.com/froiibn/FTA/releases/download/v0.1.1/SHA256SUMS-macos-0.1.1.txt)를 내려받는다.
2. `shasum -a 256 FTA_0.1.1_aarch64.dmg` 결과를 체크섬 파일과 비교한다.
3. 전송 중이 아닌 때 기존 FTA를 메뉴의 종료 또는 ⌘Q로 종료한다. DMG를 열고 `FTA.app`을 Applications로 옮긴다.
4. Applications의 FTA를 실행한다. v0.1.1 Mac 배포본은 앱·동봉 실행 파일·DMG의 Developer ID 서명과 Apple 공증을 적용하고 DMG에 공증 티켓을 첨부한다. 첫 실행의 인터넷 다운로드 확인과 로컬 네트워크·알림 권한은 별개다. 이전 ad-hoc 배포본 대신 v0.1.1 DMG를 사용한다.
5. 기본 수신 주소는 loopback이다. 기기별 실제 집 LAN 주소·허용 대역·수신 폴더를 설정하고 서비스 재시작 후 기기를 등록한다. 기존 사용자 설정과 등록은 앱 교체로 초기화하지 않는다.

X는 창만 숨겨 수신을 유지한다. Dock 클릭이나 메뉴 막대에서 본체를 다시 연다. 완전한 앱 종료는 ⌘Q 또는 메뉴의 종료를 사용한다. 앱이 시작하지 않은 독립 엔진은 앱 종료 후에도 유지된다.

이전 v0.1.0 릴리스는 양쪽에서 확인하는 수동 등록 방식이다. v0.1.1은 최초 전송 시 수신자만 등록을 확인하며, 상대 기기에도 이 기능이 포함된 버전을 적용한다. [최초 전송 등록 안내](../../docs/first-send-registration.md)를 따른다.

## 재빌드

Rust·Node는 저장소의 툴체인과 데스크톱 문서를 따른다. 저장소 루트에서 실행한다.

```sh
npm --prefix apps/desktop ci
# APPLE_SIGNING_IDENTITY에는 이 Mac 키체인의 유효한 Developer ID Application 표시 이름을 설정한다.
: "${APPLE_SIGNING_IDENTITY:?Developer ID Application 서명 인증서를 지정하세요}"
CI=true npm --prefix apps/desktop run bundle -- --bundles app,dmg
codesign --verify --deep --strict target/release/bundle/macos/FTA.app
# 현재 제품 버전을 읽어 이전 배포본 대신 방금 빌드한 DMG를 검증한다.
fta_version=$(node -p "require('./apps/desktop/src-tauri/tauri.conf.json').version")
hdiutil verify "target/release/bundle/dmg/FTA_${fta_version}_aarch64.dmg"
```

`CI=true`는 로컬 패키징에서 Finder 꾸미기 자동화를 생략하며 GitHub Actions 실행을 뜻하지 않는다. `APPLE_SIGNING_IDENTITY=-`는 개발용 ad-hoc 서명으로만 사용할 수 있고 배포용 Developer ID 서명·공증을 대신하지 않는다. 공증 인증은 비밀번호를 저장소에 기록하지 않고 `notarytool store-credentials`로 로컬 키체인에 저장한다.

배포 전에는 설정한 키체인 프로필로 DMG를 제출하고 `Accepted`를 확인한 뒤 티켓을 첨부한다. 아래 `FTA_NOTARY_PROFILE`에는 해당 Mac에 저장한 프로필 이름을 지정한다. 심사 중이거나 실패한 산출물을 공증 완료로 게시하지 않는다.

```sh
: "${FTA_NOTARY_PROFILE:?공증용 로컬 키체인 프로필을 지정하세요}"
fta_dmg="target/release/bundle/dmg/FTA_${fta_version}_aarch64.dmg"
xcrun notarytool submit "$fta_dmg" --keychain-profile "$FTA_NOTARY_PROFILE" --wait
xcrun stapler staple "$fta_dmg"
xcrun stapler validate "$fta_dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$fta_dmg"
spctl --assess --type execute --verbose=2 target/release/bundle/macos/FTA.app
```

체크섬은 공증 티켓 첨부 후 계산한다. 서명·공증·Gatekeeper 검사 성공과 다른 Mac의 설치·실행·실기 전송 검증은 구분해 빌드 정보에 기록한다.

배포 파일은 `target/release/bundle/dmg/`에 생성된다. 번들에 엔진·MCP가 포함되어 있는지 확인하고 체크섬을 만든다. MCP는 사용자가 앱 내부 `Contents/MacOS/fta-mcp`를 stdio 서버로 등록하며 빌드가 클라이언트 설정을 바꾸지 않는다.
