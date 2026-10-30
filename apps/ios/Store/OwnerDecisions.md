# 소유자 확인이 필요한 제출 전 항목

실제 심사 제출은 승인되지 않았다. 다음은 코드 결함과 구분되는 계정·권리·규정 확인 항목이다.

1. **완료 — 스토어 앱명**: 사용자 승인 제목 `FTA - File Transfer Anything`(28자)을 10개 로케일에 저장하고 재접속 후 실제 값을 확인했다. 영어 이름 충돌은 해소됐다. 앱 안의 FTA 이름·아이콘·bundle ID/SKU는 바꾸지 않았다.
2. **완료 — Xcode 계정/빌드 업로드**: 소유자 재인증 후 최종 archive의 Apple Distribution/App Store export와 엄격한 로컬 검증, 0.1.2(9) 업로드와 버전 빌드 연결·재접속 검증이 성공했다. Apple 서버 처리는 끝났으나 현재 상태는 아래 6번의 `수출 규정 관련 문서 누락`이다. 테스트 가능·심사 준비 완료를 의미하지 않는다.
3. **저작권**: FTA의 실제 소유자 이름/브랜드를 확정한다. 이전 앱의 `SEIZIA` 문구만으로 FTA의 소유권을 단정하지 않는다.
4. **App Privacy 게시**: 개발자/제휴사 수집 없음은 코드와 [Apple 수집 정의](https://developer.apple.com/app-store/app-privacy-details/)를 대조한 저장 초안이다. 게시 확인은 정확성·법 준수·향후 갱신 동의까지 포함하므로 소유자 확인/승인이 남아 있다.
5. **콘텐츠 권한**: Apple은 ‘타사 콘텐츠에 접근하며 필요한 권한이 있음’과 ‘타사 콘텐츠를 포함/표시/접근하지 않음’ 중 하나를 요구한다. 앱은 사용자가 선택하는 파일·사진·텍스트를 처리하며 개발자가 콘텐츠를 제공하는 피드/서버는 없다. 권리 보유에 관한 진술을 임의 선택하지 않았다.
6. **암호화 수출 규정 — 공식 면제 적용 확인 필요/현재 빌드 문서 누락**: 실제 전송의 server/client는 `crates/fta-core/src/identity.rs`에서 TLS 1.3과 `rustls::crypto::ring::default_provider()`를 사용한다. Rustls 0.23/ring은 Apple OS 밖에서 구현되는 표준 암호화다. 프로젝트가 독자 암호 알고리즘을 만들지는 않는다. App Store Connect의 문서 마법사에서 이 기술적 분류와 프랑스 배포 ‘예’를 선택한 초안은 **‘프랑스 암호화 신고 승인서’ 사본** 첨부를 요구했다. 파일 없이 저장하지 않았고 임의 면제 진술·프랑스 제외·`ITSAppUsesNonExemptEncryption=false`를 적용하지 않았다. [Apple의 문서 요구 표](https://developer.apple.com/help/app-store-connect/reference/app-information/export-compliance-documentation-for-encryption)는 OS 외부 표준 알고리즘과 프랑스 배포에 대한 이 요구를 확인한다. 이 UI 요구를 FTA의 법적 신고 의무 확정으로 해석하지 않는다. 운영 경로는 HTTPS이며 OS 외 TLS·서버 수신·인증서 생성에 대한 면제 적용은 공식 확인이 필요하다. [법령/실제 구현 대조](ExportComplianceReview.md)와 [미발송 문의 초안](ExportComplianceQuestions.en.md)을 따른다. 면제가 확정되지 않은 상태에서 신고 답변·국가·Info.plist를 변경하지 않는다.
7. **중국 본토**: 175개 국가 선택 후 ICP 비안 번호 입력란이 나타난다. LAN 전용 앱의 적용 여부나 면제를 단정하지 않았고 등록 번호를 만들지 않았다. 법적 적용/허가 또는 제외 결정은 소유자가 한다.

심사 연락처는 동일 소유자가 기존 앱에 등록한 검증된 값을 App Store Connect 안에서만 재사용했다. 이 문서에는 개인 연락처, 계정 ID, 서명 키, 프로비저닝 파일이나 기기 식별자를 기록하지 않는다.
