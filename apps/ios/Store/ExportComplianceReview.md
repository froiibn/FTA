# FTA 암호화 면제 재검토

2026-10-30. 결론은 **공식 확인 필요**다. 현재 Apple 화면의 문서 요구는 확인했지만 이를 FTA에 대한 정부의 개별 신고 의무 확정으로 해석하지 않는다. 확실한 면제 근거도 찾지 못했다. 미국 수출 분류, 프랑스 공급/수입 신고, Apple의 문서 제출은 서로 구분한다. 계정 답변·Info.plist·국가 설정은 변경하지 않았다.

## 실제 구현과 면제 요건

| 확인된 사실 | 근거와 의미 |
| --- | --- |
| 운영 통신은 HTTPS HTTP/1이며 별도 독자 암호 프로토콜이 아니다. | `crates/fta-core/src/network.rs:197`의 reqwest, `:497`의 TLS 수신, `:533`의 Hyper HTTP/1 서버, `:1264` 이후 HTTPS 전송. 파일은 HTTP 스트리밍 본문, 텍스트/URL은 JSON 본문이다. raw TLS duplex는 identity.rs의 테스트 경로다. |
| TLS 1.3은 Rustls/ring으로 양쪽에 구현된다. | `crates/fta-core/src/identity.rs:170`과 `:185`. Apple URLSession/Security의 TLS 구현만 사용하는 앱은 아니다. 인증·무결성뿐 아니라 일반 사용자 본문의 기밀성도 제공한다. |
| 기기 키와 자체 서명 인증서를 생성하고 상호 인증한다. | `identity.rs:39`, `:115`, `:132`. iOS 개인키는 Keychain에 보관하고 TLS용으로 읽는다. `crates/fta-platform/src/ios.rs:68`은 동기화 없는 ThisDeviceOnly 보호를 지정한다. Keychain 사용이 외부 TLS까지 OS 암호화로 바꾸지는 않는다. |
| 임의 사진·파일·텍스트·URL을 등록된 LAN 기기에 전송한다. | 은행/의료/DRM 전용, 관리·설정 데이터 전용, 인증만 하는 앱에 해당하지 않는다. HTTPS 서버 수신과 인증서 생성까지 포함하므로 ‘HTTPS 호출만’이라는 표현에 자동으로 대응시키지 않는다. |
| 공개 무료 배포를 준비하며 알고리즘/암호 강도를 바꾸는 UI는 없다. | 대중시장 요건 검토에 유리한 사실이다. 신뢰 기기/네트워크 설정은 암호 알고리즘 설정과 다르다. 이것만으로 미국 분류나 ANSSI Grand public 승인이 성립했다고 단정하지 않는다. |

## 프랑스 법령 대조

[LCEN 제30조](https://www.legifrance.gouv.fr/codes/section_lc/JORFTEXT000000801164/LEGISCTA000006136109/)의 개인 사용 자유와 인증/무결성 **전용** 수단의 자유는 다르다. TLS 본문 기밀성을 제공하는 FTA에 후자를 적용할 수 없다. 일반 공급/수입에는 예외가 없으면 신고 체계가 적용된다.

[Décret 2007-663 통합본](https://www.legifrance.gouv.fr/loda/id/JORFTEXT000000646995/2026-03-08)의 제1·3조와 Annex I/II를 대조했다. 확인된 통합본은 마지막 갱신 2020-01-01, 부록은 2007-05-04부터 유효하다. 2026-10-30 날짜 URL은 보안 차단으로 직접 열지 못했으며 오늘 접근한 ANSSI 안내와 함께 검토했다.

| 면제 후보 | FTA와의 대조 |
| --- | --- |
| Annex I A1–7: 스마트카드·방송·금융·통신망 무선채널·무선전화·복제방지·재생 제한 | 일반 파일 전송 앱의 면제 근거가 아니다. |
| C10: 열거된 IEEE 무선표준에 암호 기능이 한정된 장비 | FTA는 Wi-Fi 보호 외에 TLS를 구현한다. |
| D11: 정보시스템 관리/설정에 필요한 데이터만 암호화 | 사용자 일반 본문도 암호화하므로 불일치한다. |
| B8/E12: 개인이 운반하거나 개인 사용/개발용으로 수입·이전 | 해당 개인의 행위와 개발자의 일반 공급을 구분해야 한다. 공급 면제 목록이 아니다. |
| F13: 약한 키의 외부 수출 | 강한 TLS에 맞지 않으며 공급 면제도 아니다. |
| Annex II B3: 대중시장, 쉽게 변경 불가, 사용자 설치 | 공개 배포·고정 암호 기능과 맞는 부분은 있지만 **수출 체계**다. 공급/수입 신고 면제가 아니다. |

[ANSSI 현행 안내](https://cyber.gouv.fr/reglementation/reglementation-identite-confiance-numerique/controles-reglementaires-cryptographie/controle-moyen-de-cryptologie/controle-rglementaire-cryptographie-demarches/)는 Grand public의 수출허가 면제와 공급/수입 신고를 구분하며 이 분류를 신고 때 요청하고 ANSSI 검증을 받도록 설명한다. 부록에서 HTTPS/TLS 일반 면제나 오픈소스 의존성 사용만으로 성립하는 공급 면제는 찾지 못했다. 앱이 무료이거나 원래 개인용이라는 이유도 일반 공개 배포 면제를 확정하지 않는다.

## Apple의 현재 질문과 남은 해석

실제 빌드 9 ‘관리’의 첫 질문은 **‘앱에서 구현하는 암호화 알고리즘의 유형은 무엇입니까?’**다. 독자/OS 외 표준/둘 다/둘 다 아님을 선택하며, 이 앞에 HTTPS-only·면제 여부 질문은 나타나지 않았다. 답변 없이 닫았다. 이전 문서 마법사에서는 OS 외 표준 + 프랑스 배포에 신고 승인서 사본을 요구했다.

[Apple의 현재 설명](https://developer.apple.com/documentation/security/complying-with-encryption-export-regulations)은 OS 내장 암호화의 예로 URLSession HTTPS를 든다. 외부 Rustls HTTPS까지 무조건 면제한다고 명시하지 않는다. [현재 문서 표](https://developer.apple.com/help/app-store-connect/reference/app-information/export-compliance-documentation-for-encryption)는 OS 외 표준 암호화와 프랑스 배포에 문서를 요구한다. 따라서 옛 게시글의 HTTPS 질문을 현재 UI나 FTA의 면제 판정으로 대체할 수 없다. 다만 현재 표/질문만으로 법령상 개별 적용 여부를 확정하는 것도 피한다.

미국에서는 [BIS의 현행 5A002 범위](https://www.bis.gov/learn-support/encryption-controls/5a002-a.1-a.5)에 정보 송수신/통신이 포함된다. ‘암호화가 주목적이 아니다’만으로 완전 면제를 주장할 근거가 부족하다. [대중시장 분류](https://www.bis.gov/learn-support/encryption-controls/mass-market)와 [EAR 740.17](https://www.bis.gov/regulations/ear/740)의 허가 예외·분류·보고 요건은 각각 확인해야 하며 프랑스 신고를 면제하지 않는다. 폐기/변경 여부를 확인하지 않은 과거 Note 4 문구나 오픈소스 라이브러리의 지위를 완성 앱에 그대로 적용하지 않는다. 미국의 최종 ECCN/예외 분류도 이 문서에서 확정하지 않았다.

확인할 핵심은 Apple의 외부 TLS HTTPS·양방향 서버·자체 인증서에 대한 실제 면제 적용 여부와 ANSSI의 일반 공급 신고 대상/개별 면제 또는 기존 라이브러리 신고의 적용 가능성이다. [미발송 문의 초안](ExportComplianceQuestions.en.md)을 준비했다. 다른 앱의 설정과 비교 확인했다고 주장하지 않는다.

## 후속 소유자 결정과 적용 경로

소유자가 검토 결과를 읽은 뒤 문서 제출 면제로 처리하기로 명시 결정했다. 법적 개별 면제가 공식 확인된 것으로 바뀐 것은 아니다. [Apple Modify a build](https://developer.apple.com/documentation/appstoreconnectapi/patch-v1-builds-_id_)는 기존 빌드의 면제 설정 변경을 지원하며 `usesNonExemptEncryption=false`는 암호 미사용뿐 아니라 면제 암호 사용을 나타낸다. 현재 UI의 OS 외 표준 → 프랑스 예 경로에는 별도 면제 선언이 없다. 표준 API 개인키 저장 위치와 인증 환경에 사용 가능한 키가 없어 기존 빌드9에 아직 반영하지 않았다. 공식 API 인증을 준비하면 재빌드 없이 변경할 수 있고, 그 인증을 준비하지 않는 대안은 소유자의 면제 결정을 Info.plist에 반영한 새 빌드10이다. 경로 결정 전에 코드/빌드를 바꾸지 않았고 국가/알고리즘 사실도 유지했다.
