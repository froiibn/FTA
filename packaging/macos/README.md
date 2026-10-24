# macOS 개인용 설치 파일

현재 릴리즈는 Apple Silicon(arm64)용이다. Intel Mac용 실행·빌드 검증은 수행하지 않았다.

## 설치

1. GitHub Release에서 `FTA_0.1.0_aarch64.dmg`와 `SHA256SUMS-macos.txt`를 내려받는다. 비공개 저장소이므로 저장소 접근 권한으로 로그인한다.
2. `shasum -a 256 FTA_0.1.0_aarch64.dmg` 결과를 체크섬 파일과 비교한다.
3. 전송 중이 아닌 때 기존 FTA를 메뉴의 종료 또는 ⌘Q로 종료한다. DMG를 열고 `FTA.app`을 Applications로 옮긴다.
4. Applications의 FTA를 실행한다. 이 빌드는 번들·동봉 실행 파일에 ad-hoc 서명을 적용했으며 Apple Developer ID 배포 서명·공증을 제공하지 않는다. macOS가 첫 실행을 제한하면 운영체제의 개인정보 보호 및 보안 안내에 따라 직접 허용해야 한다. 보안 기능을 전체 해제하지 않는다.
5. 기본 수신 주소는 loopback이다. 기기별 실제 집 LAN 주소·허용 대역·수신 폴더를 설정하고 서비스 재시작 후 기기를 등록한다. 기존 사용자 설정과 등록은 앱 교체로 초기화하지 않는다.

X는 창만 숨겨 수신을 유지한다. Dock 클릭이나 메뉴 막대에서 본체를 다시 연다. 완전한 앱 종료는 ⌘Q 또는 메뉴의 종료를 사용한다. 앱이 시작하지 않은 독립 엔진은 앱 종료 후에도 유지된다.

## 재빌드

Rust·Node는 저장소의 툴체인과 데스크톱 문서를 따른다. 저장소 루트에서 실행한다.

```sh
npm --prefix apps/desktop ci
APPLE_SIGNING_IDENTITY=- CI=true npm --prefix apps/desktop run bundle -- --bundles app,dmg
codesign --verify --deep --strict target/release/bundle/macos/FTA.app
hdiutil verify target/release/bundle/dmg/FTA_0.1.0_aarch64.dmg
```

`CI=true`는 Finder 꾸미기 자동화를 생략해 무인 빌드에서 불필요한 UI 조작을 피한다. `APPLE_SIGNING_IDENTITY=-`는 키체인 승인 없이 실제 번들과 `fta-engine`·`fta-mcp`를 ad-hoc 서명한다. 서명 무결성 검증은 Apple 공증·다른 Mac에서의 최초 실행 허용을 의미하지 않는다.

배포 파일은 `target/release/bundle/dmg/`에 생성된다. 번들에 엔진·MCP가 포함되어 있는지 확인하고 체크섬을 만든다. MCP는 사용자가 앱 내부 `Contents/MacOS/fta-mcp`를 stdio 서버로 등록하며 빌드가 클라이언트 설정을 바꾸지 않는다.
