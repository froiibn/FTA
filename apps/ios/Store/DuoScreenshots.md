# Duo·iPad 스크린샷 확인 기록

2026-10-31. 기존 iPhone/iPad 자산을 보존하고 Duo 10개 언어에 각 3장, 총 30장을 추가했다. App Store Connect에서 처리 완료·파일명과 재접속 후 저장 유지를 전부 확인했다. 전체는 iPhone 30 + iPad 30 + Duo 30 = 90장이다. 심사 제출/출시는 하지 않았다.

## 공식 규격과 실제 환경

- 설치된 Xcode 27.1(27A9275), iOS 27.1 시뮬레이터(24A94425)의 실제 iPhone Duo를 사용했다. 새 SDK·런타임 설치는 하지 않았다.
- [Apple 스크린샷 규격](https://developer.apple.com/help/app-store-connect/reference/app-information/screenshot-specifications)은 외부 1398×2034/2034×1398, 내부 2007×2853/2853×2007을 허용한다. 현행 ASC는 이 네 크기를 하나의 Duo 슬롯에서 받으며 외부/내부별 별도 필수 슬롯은 없다. 2027년 4월부터 관련 SDK 앱에 Duo 스크린샷이 필수라는 안내와 현재 저장 가능 여부는 구분한다.
- [Apple Duo 준비 안내](https://developer.apple.com/iphone-duo/prepare/)에 따라 Device Hub의 Closed/Open/Partially Open 제어로 실제 기기 자세를 바꿨다. 앱은 iOS 27.1 SDK로 빌드했으며 세로 탭/비대칭 안전 영역은 OS의 실제 표시다.

## 실행·제작·등록

1. Closed에서 기존 `testStoreLocalizedScreenshots`를 실행했다. 10개 지원 언어와 hu의 영어 fallback에서 실제 보내기·사진 선택·기록 원본 33장을 확보했다. 송수신을 실제 중지해 개인 LAN 기기 이름이 노출되지 않게 했고 가짜 온라인 기기/전송 결과는 넣지 않았다.
2. Open에서도 동일 테스트가 통과했다. 다만 XCTest 첨부는 검은 2006×2852 버퍼여서 홍보에 사용하지 않았다. 실제 Device Hub 화면과 내부 디스플레이 지정 `simctl io ... screenshot --display=3`의 정상 2853×2007 화면으로 렌더링을 확인했다. 이는 전체 내부 홍보 30장 캡처 완료를 뜻하지 않는다. [Apple 알려진 캡처 문제](https://developer.apple.com/documentation/xcode-release-notes/xcode-27_2-release-notes)도 참고하되 해당 원인이 개별 확정됐다고 단정하지 않는다.
3. Partially Open에서 사진 소스/성장 입력창과 하단 입력 위치 회귀 2개를 통과했다. 실기 Duo, 모든 Split View·접근성 조합의 검증을 완료한 것은 아니다. 닫힌 화면에서 긴 독일어 빈 상태 제목은 네이티브 말줄임표로 표시되지만 설명·버튼·입력은 접근 가능하다. 임의 UI 수정은 하지 않았다.
4. 업로드는 정상 검증된 외부 원본만 사용했다. 기존 브랜드 아이콘·남색 배경·서체·문구와 균일 비율의 실제 화면 합성을 유지했다. 1398×2034 불투명 RGB의 각 언어 3장을 Duo 슬롯에 등록하고 10개 언어 모두 처리 완료·재접속 저장을 확인했다. 화면·상태를 다시 그리거나 iPhone/iPad 원본을 Duo처럼 늘리지 않았다.

검사/원본/완성본 경로:

- `output/ios-store-duo-closed.xcresult`, `output/ios-store-duo-open.xcresult`, `output/ios-duo-partial-controls.xcresult`.
- `output/ios-store-assets/raw/duo-outer/store-<locale>-<scene>.png`.
- `output/ios-store-assets/promotional/duo-outer/<locale>/{1-send,2-photos,3-history}.png`.
- `output/ios-store-assets/duo-open-xctest-invalid/`는 제외된 검은 첨부이며 제작 입력이 아니다.
- `output/ios-store-assets/duo-ipad-store-audit.json`에 언어별 서버 파일명·해상도·상속/처리 확인을 보존했다.

재제작은 프로젝트 루트에서 `output/render-store-images duo-outer`로 실행한다. 합성기는 Duo 실제 원본 크기를 확인하며 기존 phone/ipad 자산을 덮어쓰지 않는다. 앱 코드·버전·암호화·국가 설정은 변경하지 않았다.

## iPad가 없어 보이는 경우

한국어/영어/일본어/중국어 간체·번체/독일어/프랑스어/스페인어/브라질 포르투갈어/이탈리아어 모두 **iPad 33.0cm 디스플레이 필수 슬롯**에 직접 등록된 3장이 있다. 서버 미리보기의 30장 모두 2064×2752로 확인했다. 누락이 없어 재업로드하지 않았다.

미디어 관리의 선택 슬롯인 iPad Pro(2세대)12.9는 전 언어에서 이 자산을 상속한다. 여기에 ‘기존 자산 사용’과 함께 **직접 업로드 0개**가 표시될 수 있다. 필수 33cm 슬롯의 3장 누락과 혼동하지 않는다. Apple Watch 타깃이 없어 해당 슬롯에는 등록하지 않았다.
