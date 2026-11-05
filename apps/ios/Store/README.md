# iOS App Store 출시 준비

최신 체크포인트(2026-11-06): **0.1.4 (11) 로컬 배포 빌드 검증 완료 / 스토어 초안 제출 준비 중**. 10개 언어 설명과 iPhone·iPad·Duo 90장 최신 이미지를 저장하고 재조회 검증했다. 빌드 업로드는 Xcode 계정 접근 오류와 후속 Mac 잠금 때문에 미완료이며 심사 제출하지 않았다. 자세한 검증·해시·남은 단계는 [모바일 0.1.4 배포 기록](../../../docs/releases/mobile-0.1.4.md)을 따른다.

아래는 0.1.2 (10)의 당시 준비 기록이다. 해당 버전은 이후 소유자가 제출하여 승인·공개 확인했다.

2026-11-01 기준 0.1.2 내부 빌드 10의 준비 기록이다. 빌드 9는 업로드/버전 연결 이력으로 보존한다. 실제 App Store 심사 제출·출시는 하지 않았다. 기존 데스크톱 0.1.2 배포본과 Android 작업은 별도다.

최신 상태: 빌드 10의 Apple Distribution/App Store export·엄격한 검증·업로드 접수까지 완료했다. 실제 배포 번들의 `ITSAppUsesNonExemptEncryption=false`는 소유자의 문서면제 결정이며 암호 미사용/정부의 개별 면제 확인을 뜻하지 않는다. CLI는 `No Accounts`와 새 인증서/기존 프로파일 불일치로 실패했으나 GUI 계정·팀 권한과 CLI의 UID/HOME/Xcode/키체인 문맥은 정상 일치했다. 따라서 단순 사용자 미로그인으로 단정하지 않고 Organizer 경로를 사용했다. GUI 자동 서명이 FTA 프로파일을 준비했고 실제 서명이 소유자의 새 Distribution 신원 및 프로파일 인증서와 일치함을 검증했다. 인증서 삭제/폐기/재발급과 개인키 추출은 하지 않았다.

Organizer는 **App upload complete: FTA 0.1.2 (10) uploaded**를 표시했다. 로컬 검증 IPA는 `output/ios-app-store-export-final10-organizer/FTA.ipa`, SHA-256은 `623b78320f96c94500a8811f5968a425ff4ae81b35cc3c1f8111cff130ba362e`다. 실제 App Store 프로파일 유형·만료·앱/팀 식별자·서명 인증서 일치, `get-task-allow=false`, 버전 0.1.2(10), 면제값 false, 10개 언어·권한 설명·법률 고지·PrivacyManifest를 확인했다. 증빙 요약은 `output/ios-app-store-build10-validation.json`에 있으며 비밀 서명 식별자는 포함하지 않는다.

2026-11-01 재로그인 후 Apple 서버 처리와 빌드 10의 문서 누락 해소를 확인했다. TestFlight 표시는 ‘제출 준비 완료’지만 테스터 그룹/설치가 없어 실제 TestFlight 테스트를 했다는 뜻은 아니다. 버전 0.1.2의 연결을 9에서 10으로 변경·저장·재접속 검증했다. 최종 필수정보 검증을 거쳐 **제출 초안 1개 / 심사 준비됨(Ready for Review)** 이며 항목은 `iOS 0.1.2 (10)` 하나다. 별도 최종 버튼 **‘심사를 위해 제출’은 누르지 않았다**. [Apple 공식 절차](https://developer.apple.com/help/app-store-connect/manage-submissions-to-app-review/submit-an-app)에서 Add for Review는 초안 준비이고 Submit for Review가 실제 제출이다. 재접속 후 상태·빌드·초안 유지와 수동 출시를 확인했다. 재업로드/재빌드는 하지 않았다.

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
python3 apps/ios/scripts/validate-release.py output/FTA-iOS-0.1.2-build10-owner-exemption.xcarchive
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

2026-10-31 후속 [Duo·iPad 확인](DuoScreenshots.md): 실제 Duo 시뮬레이터의 10개 언어 원본으로 Duo 슬롯에 각 3장, 30장을 추가 등록하고 재접속 저장까지 확인했다. 기존 iPhone/iPad 60장은 유지해 총 90장이다. iPad는 10개 언어의 필수 33cm 슬롯에 이미 각 3장이 있었고 서버 원본 30장 모두 2064×2752다. 선택 12.9 슬롯의 ‘기존 자산 사용/직접 업로드 0개’와 구분한다. Duo 외부·내부 UI 테스트와 부분 펼침 회귀는 통과했지만 내부 XCTest 검은 캡처는 사용하지 않았다. 앱 기능/화면/빌드 인증 대기 상태는 변경하지 않았다.

완료한 로컬 검사:

- iPhone·iPad: 10개 언어와 영어 fallback의 실제 네이티브 UI 검사/캡처.
- iPhone: 사진·파일 선택, 신뢰 설정 재시작, 메시지 입력, 시스템 외관, 내장 정책/라이선스의 UI 회귀 7개 통과.
- 실제 iOS 시뮬레이터의 SQLite 3.54.0: TLS 1 MiB 송수신·동명 파일 비덮어쓰기·DB 재열기/WAL·최종 저장 중단 복구·무결성 검사 통과.
- 최종 Release archive, Swift/Rust dSYM, 엄격한 로컬 서명/번들 검증 성공. 개발 서명 archive를 App Store 배포 export와 혼동하지 않는다.

Xcode 계정 자격증명 누락은 소유자의 재인증으로 해소됐다. 최신 `runtime-notices` archive를 Apple Distribution/App Store 프로파일로 export하고 `validate-release.py --require-distribution`으로 검증했다. 로컬 검증 IPA는 `output/ios-app-store-export-final9/FTA.ipa`이며 SHA-256은 `e92f0657b4fa8ae63f7fed71a78de990fb1c6910d5a3c430ddc141e0181aa32a`다. 동일 최종 archive의 App Store Connect 업로드와 서버 처리는 성공했고 0.1.2(9)가 TestFlight/버전의 빌드 추가 화면에 나타났다. 빌드 9 확인 당시 상태는 **수출 규정 관련 문서 누락**이었다. 당시 빌드 9 연결/재접속 이력을 보존하며 현재 제출 초안에는 빌드 10만 연결돼 있다. 테스트/심사 준비 완료를 의미하지 않는다. 사전 IPA를 업로드하지 않았다.

2026-11-01 현재 전체 점검 및 소유자 승인 반영:

- 전10개 언어의 앱명/부제/설명/키워드/프로모션은 `metadata.json`과 일치하며 지원 URL과 정책 URL도 전수 확인했다. 각 언어 iPhone 중형(대형 자산 상속), iPad 33cm, Duo에 각 3장씩 90장이 표시되고 처리 중 자산은 없다. 크기/원본 캡처 검사는 앞선 기록과 구분하며 이번에는 기존 자산을 재업로드하지 않았다.
- 소유자 명시 승인에 따라 저작권 `2026 SEIZIA`, 콘텐츠 권한 ‘아니요, 이 앱은 타사 콘텐츠를 포함, 표시 또는 이용하지 않습니다’를 저장·재접속 확인했다. ‘데이터 수집되지 않음’은 정확성/법 준수/향후 갱신 동의까지 승인받아 실제 게시했고 재접속 후 게시함 표시를 확인했다.
- 무료/유료 계약과 기존 DSA 규정 준수 상태는 활성화됨이며 앱은 거래자로 선언돼 있다. 연락처·은행·세금의 비공개 값은 문서에 기록하지 않는다. 연락처 필수 4항목은 채워졌고 심사 로그인 불필요, 메모는 `ReviewNotes.en.txt`와 일치한다.
- 실제 가격은 전175개 지역 모두 0.00이며 상태도 전부 ‘앱 출시 후 사용 가능’이다. 중국 본토도 같은 상태이고 ICP 누락 차단은 표시되지 않는다. 사전 주문 없음, 공개 배포, Mac/Vision Pro 배포 비활성, 수동 출시를 유지한다.
- 중국 ICP는 빈칸이나 [Apple 현재 설명](https://developer.apple.com/help/app-store-connect/reference/app-information/app-information)은 일부 앱/해당되는 경우라고 구분한다. 사용자 선택 자료의 LAN 전송 앱을 뉴스/종교/출판/게임 허가 대상이나 ICP 필수 대상으로 단정하지 않는다. 개별 법 적용을 기관이 확정한 것은 아니지만 현재 서버의 필수정보 누락 차단으로 보고하지 않는다. 암호화도 소유자 면제 선언 반영과 정부의 법적 판정을 구분한다.

현재 확인된 필수 스토어 항목 누락은 없으며 최종 사용자 동작은 제출 초안의 **‘심사를 위해 제출’** 이다. 서버가 초안을 허용한 사실은 Apple 심사 승인이나 규정에 대한 개별 법적 판단을 보장하지 않는다. 실기 build 10의 사진/파일·권한·네트워크 재접속/수명주기, 최소 iOS 17, Windows/Linux 최종 상호 운용은 별도의 미검증 범위다. 이전 build 8 실기 성공·시뮬레이터 검증을 이를 대신한 것으로 보고하지 않는다.

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

Rust 표준 라이브러리 고지와 소유자 면제 선언을 포함한 최신 archive는 `output/FTA-iOS-0.1.2-build10-owner-exemption.xcarchive`다. 빌드 10의 실제 번들 버전/면제값/기기 식별 설정과 약 49 MB dSYM을 확인했다. 빌드 9 `runtime-notices` archive의 성공 업로드 이력은 유지한다. 앞선 `global`/`system-sqlite` archive와 사전 IPA는 최종 업로드 대상이 아니다. Cargo 외부 의존성 169개에 더해 Rust 툴체인 런타임의 공식 저작권/라이선스 항목을 별도로 포함하며, 소스 구현이나 새 라이브러리는 추가하지 않는다. 배포 검증은 디버깅 권한뿐 아니라 실제 Apple Distribution 서명과 App Store 프로파일의 유형·만료·앱/팀 식별자 일치도 확인한다.
