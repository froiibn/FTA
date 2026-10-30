# iOS App Store 출시 준비

2026-10-30 기준 0.1.2 내부 빌드 9의 준비 기록이다. 실제 App Store 심사 제출·출시는 하지 않았다. 기존 데스크톱 0.1.2 배포본과 Android 작업은 별도다.

## 앱과 지원 범위

- Bundle ID/SKU: `app.fta.ios`. App Store Connect 앱 ID: `6828538463`.
- [버전 준비 화면](https://appstoreconnect.apple.com/apps/6828538463/distribution/ios/version/inflight).
- iOS 17 이상 arm64, iPhone·iPad. 실제 최신 iOS 시뮬레이터 검증과 최소 지원 OS 검증을 구분한다.
- UI 137개 키·카메라/로컬 네트워크 설명 2개를 한국어, 영어, 일본어, 중국어 간체/번체, 독일어, 프랑스어, 스페인어, 브라질 포르투갈어, 이탈리아어로 제공한다. 그 외 언어는 영어로 fallback한다.
- 개인정보 처리방침 전문은 한국어/영어다. 한국어 이외의 UI에서는 영문 전문을 제공하며 원본 라이선스 문구는 번역하지 않는다. 원어민 검토를 거쳤다고 주장하지 않는다.
- 수신 폴더의 기존 이름 `받은 파일`은 저장 경로/기록과의 호환성을 위해 유지한다.

## 재현 가능한 로컬 준비

프로젝트 루트에서 실행한다. Rust 도구가 PATH에 있어야 하며 Xcode 로컬 계정 설정과 개인 서명 설정은 저장소에 넣지 않는다.

```sh
python3 apps/ios/scripts/generate-localizations.py --check
python3 apps/ios/scripts/generate-notices.py --check
python3 apps/ios/scripts/generate-project.py
python3 apps/ios/scripts/validate-release.py output/FTA-iOS-0.1.2-build9-runtime-notices.xcarchive
xcrun swiftc -parse-as-library apps/ios/scripts/render-store-images.swift -o output/render-store-images
output/render-store-images
```

번역의 원본은 `../Localization/*.json`, 네이티브 결과는 `../FTA/Localization/*.lproj`다. 생성기는 포맷 인자·누락 키·외국어의 한국어 누출·UI 검사 사전의 일치를 확인한다. 최종 검증기는 실제 서명된 번들 안의 10개 언어 테이블·권한 설명·법적 문서·개인정보 매니페스트를 대조한다.

`PrivacyInfo.xcprivacy`에는 실제 사용하는 UserDefaults(CA92.1), 파일 메타데이터(C617.1/3B52.1)만 선언한다. iOS는 Apple 시스템 SQLite를 연결한다. Android·데스크톱은 bundled SQLite를 유지한다. 최종 archive는 statfs/fstatfs/statvfs/fstatvfs를 import하지 않으며 시스템 SQLite와 실제 연결됨을 검사한다. 존재하지 않는 디스크 공간 사용 목적을 선언하지 않는다.

`generate-notices.py`는 잠긴 iOS normal/build 의존성 그래프의 외부 패키지 169개와 Rust 표준 라이브러리 별도 항목의 원문 라이선스/저작권을 재현한다. Rust sysroot는 Cargo 그래프에 나오지 않으므로 `generate-runtime-notices.py`가 동일 툴체인의 공식 COPYRIGHT/라이선스와 iOS target rlib 목록에서 런타임 의존성 계열의 고지를 추출한다. 이 고지는 일부 빌드/디버그·플랫폼 변형을 포함할 수 있으며 모든 나열 버전이 앱에 링크됐다는 주장은 아니다. 툴체인 commit과 고지 해시가 다르면 빌드를 차단하고 재검토한다. 배포 바이너리와 빌드 도구의 구분을 고지한다. 누락된 asn1-rs-impl 원문 라이선스의 출처와 해시는 `../LegalSources/`에 보존한다.

## 스크린샷과 메타데이터

`metadata.json`은 10개 언어의 설명·부제·키워드·프로모션 문구·이미지 문구 원본이다. `FTAFlowTests/testStoreLocalizedScreenshots`는 실제 앱을 각 언어와 미지원 언어(hu)로 실행해 보내기·사진 소스 선택·기록 화면을 캡처한다. iPhone/iPad에서 모두 실행한다. 공개 이미지에 개인 LAN 기기 이름이 들어가지 않도록 검사용 시뮬레이터의 실제 수신 기능을 중지하며 온라인 기기나 완료 결과를 주입하지 않는다.

- 원본: `output/ios-store-assets/raw/{phone,ipad}/store-<locale>-{send,photos,history}.png`.
- 완성본: `output/ios-store-assets/promotional/{phone,ipad}/<locale>/`.
- iPhone 1320×2868, iPad 2064×2752, 로케일별 각 3개로 총 60개. 실제 화면 위에 제품 문구/여백/프레임만 합성한다. 앱 UI와 전송 상태는 다시 그리지 않는다.
- iPhone 대형 자산을 올리면 App Store가 동일 언어의 필수 중형 자산에 자동 적용하는 것을 확인했다.
- 사용자 승인 제목 `FTA - File Transfer Anything`(28자)을 10개 로케일 모두 저장하고 재접속 후 실제 값을 확인했다. 영어 이름 충돌은 해소됐다. 앱 안의 FTA 표시명·아이콘·bundle ID/SKU는 변경하지 않았다.
- 10개 로케일의 설명·부제·지원 링크·정책 URL과 iPhone/iPad 각 3개, 총 60개 이미지를 저장했다. 영어의 마지막 6개 업로드 후 처리 완료와 실제 파일명을 확인했고 기존 54개는 보존했다. 영어 설명·키워드·프로모션 문구는 재접속 후 원본과 일치함을 확인했다.
- 무료 가격/사용 가능 지역은 175개 국가·지역으로 준비했다. 앱이 출시된 상태는 아니다. 미검증 iOS-on-Mac/Vision Pro 호환 배포는 켜지 않는다.

## 검증 기록과 남은 항목

완료한 로컬 검사:

- iPhone·iPad: 10개 언어와 영어 fallback의 실제 네이티브 UI 검사/캡처.
- iPhone: 사진·파일 선택, 신뢰 설정 재시작, 메시지 입력, 시스템 외관, 내장 정책/라이선스의 UI 회귀 7개 통과.
- 실제 iOS 시뮬레이터의 SQLite 3.54.0: TLS 1 MiB 송수신·동명 파일 비덮어쓰기·DB 재열기/WAL·최종 저장 중단 복구·무결성 검사 통과.
- 최종 Release archive, Swift/Rust dSYM, 엄격한 로컬 서명/번들 검증 성공. 개발 서명 archive를 App Store 배포 export와 혼동하지 않는다.

Xcode 계정 자격증명 누락은 소유자의 재인증으로 해소됐다. 최신 `runtime-notices` archive를 Apple Distribution/App Store 프로파일로 export하고 `validate-release.py --require-distribution`으로 검증했다. 로컬 검증 IPA는 `output/ios-app-store-export-final9/FTA.ipa`이며 SHA-256은 `e92f0657b4fa8ae63f7fed71a78de990fb1c6910d5a3c430ddc141e0181aa32a`다. 동일 최종 archive의 App Store Connect 업로드와 서버 처리는 성공했고 0.1.2(9)가 TestFlight/버전의 빌드 추가 화면에 나타났다. 현재 상태는 **수출 규정 관련 문서 누락**이다. 버전 0.1.2에 빌드 9를 연결·저장한 뒤 재접속하여 유지됨을 확인했다. 테스트/심사 준비 완료를 의미하지 않는다. 사전 IPA를 업로드하지 않았다.

실제 제출 전 잔여 항목:

1. 업로드된 0.1.2(9)의 수출 규정 문서 차단 해소 후 연결된 빌드의 테스트 가능 상태 확인. 실제 문서 마법사는 OS 외부 표준 암호화 + 프랑스 배포에 **프랑스 암호화 신고 승인서 사본**을 요구한다. 서류 없이 저장하거나 면제·프랑스 제외를 추측하지 않았다.
2. 저작권 소유자 확인. 다른 앱의 저작권 문구를 이 앱 소유권으로 추정하지 않는다.
3. App Privacy의 ‘데이터 수집 없음’ 초안은 실제 코드와 Apple의 수집 정의에 기반해 저장했다. 최종 게시의 법적 준수·정보 갱신 동의는 소유자의 확인/승인이 필요하다.
4. 콘텐츠 권한 진술과 암호화 수출 규정. Rustls/ring의 표준 TLS 암호화가 OS 밖에서 구현되므로 `ITSAppUsesNonExemptEncryption=false`를 임의 추가하지 않는다. 면제 여부/프랑스 등 문서 요건은 소유자가 판정해야 한다.
5. 중국 본토 ICP 등록 적용 여부 등 국가별 추가 요구가 실제 배포에 해당하는지 소유자가 확인한다. 번호나 권리를 만들어 입력하지 않는다.
6. 최종 build 9의 실기 전송/권한·수명주기, 가장 오래된 지원 OS, Windows/Linux 상호 운용의 최종 재검증. 이전 build 8의 실기 사용 성공과 새 빌드의 실기 검증은 다르다.

연령 설문은 Apple의 정의에 따라 직접 메시지 있음, 공개 UGC/소셜 피드·내장 자유 웹브라우저·광고 없음으로 저장했다. 앱이 자체 제공하는 성인/의료/도박 등 콘텐츠는 없다. Apple 계산값은 일반 4+, 한국 전체, 브라질 A12, 베트남 00+다. 어린이 전용이나 임의 상향으로 지정하지 않았다.

## 공개 정책과 근거

- [한국어 정책](https://github.com/froiibn/FTA/blob/main/docs/privacy-policy.md), [English policy](https://github.com/froiibn/FTA/blob/main/docs/privacy-policy.en.md).
- GitHub 기본 브랜치에는 정책 문서만 각각 커밋 `3b092b675f59140609f697c96a124156d67375c3`, `052864748eb1649860ee50877d8ee7dec51b859d`로 게시했다. 익명 HTTP 접근과 전문 일치를 확인했다. iOS/Android/PC 개발 소스와 인증 자료는 게시하지 않았다. GitHub Actions를 추가/실행하지 않는다.
- [Apple App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/).
- [Required-reason API declarations](https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api).
- [App Privacy data collection definition](https://developer.apple.com/app-store/app-privacy-details/).
- [Age-rating definitions](https://developer.apple.com/help/app-store-connect/reference/app-information/age-ratings-values-and-definitions/).
- [Export compliance](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance/).

최종 추가 검사: `cargo fmt --all --check`, 기본 `cargo test --locked`(83 통과·기존 수동 1 제외), iOS bridge 검사(1 통과), 기본 및 iOS bridge `clippy --all-targets -- -D warnings`를 macOS에서 통과했다. iPhone 시뮬레이터를 실제 다크 외관·접근성 큰 텍스트로 설정해 10개 언어의 사진/파일 버튼 도달과 사진 소스 메뉴 조작도 추가로 통과했다(한 테스트가 10개 로케일을 순회). VoiceOver 사용성 전체를 검증했다거나 App Store 손쉬운 사용 인증을 받았다는 의미는 아니다.

Rust 표준 라이브러리 고지까지 포함한 최신 archive는 `output/FTA-iOS-0.1.2-build9-runtime-notices.xcarchive`다. 앞선 `global`/`system-sqlite` archive와 사전 IPA는 최종 업로드 대상이 아니다. Cargo 외부 의존성 169개에 더해 Rust 툴체인 런타임의 공식 저작권/라이선스 항목을 별도로 포함하며, 소스 구현이나 새 라이브러리는 추가하지 않는다. 배포 검증은 디버깅 권한뿐 아니라 실제 Apple Distribution 서명과 App Store 프로파일의 유형·만료·앱/팀 식별자 일치도 확인한다.
