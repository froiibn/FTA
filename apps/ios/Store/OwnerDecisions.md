# 소유자 확인이 필요한 제출 전 항목

실제 심사 제출은 승인되지 않았다. 다음은 코드 결함과 구분되는 계정·권리·규정 확인 항목이다.

1. **스토어 앱명**: 사용자가 `FTA - File Transfer Anything`(28자)을 정확한 제목으로 승인했다. 모든 스토어 로케일에 적용할 준비이며, 2026-10-30 재개 시 App Store Connect 세션이 로그인 화면으로 만료되어 실제 저장/가용성 확인은 재로그인 뒤 진행해야 한다. 앱 안의 FTA 이름·아이콘·bundle ID/SKU는 바꾸지 않는다.
2. **Xcode 계정**: 최종 distribution export가 `missing Xcode-Username / No Accounts`로 막혔다. Xcode → Settings → Accounts에서 기존 계정을 재인증한다. 암호/2FA를 채팅이나 저장소로 전달하지 않는다.
3. **저작권**: FTA의 실제 소유자 이름/브랜드를 확정한다. 이전 앱의 `SEIZIA` 문구만으로 FTA의 소유권을 단정하지 않는다.
4. **App Privacy 게시**: 개발자/제휴사 수집 없음은 코드와 [Apple 수집 정의](https://developer.apple.com/app-store/app-privacy-details/)를 대조한 저장 초안이다. 게시 확인은 정확성·법 준수·향후 갱신 동의까지 포함하므로 소유자 확인/승인이 남아 있다.
5. **콘텐츠 권한**: Apple은 ‘타사 콘텐츠에 접근하며 필요한 권한이 있음’과 ‘타사 콘텐츠를 포함/표시/접근하지 않음’ 중 하나를 요구한다. 앱은 사용자가 선택하는 파일·사진·텍스트를 처리하며 개발자가 콘텐츠를 제공하는 피드/서버는 없다. 권리 보유에 관한 진술을 임의 선택하지 않았다.
6. **암호화 수출 규정**: 실제 전송의 server/client는 `crates/fta-core/src/identity.rs`에서 TLS 1.3과 `rustls::crypto::ring::default_provider()`를 사용한다. Rustls 0.23/ring은 Apple OS 밖에서 구현되는 표준 암호화다. 프로젝트가 독자 암호 알고리즘을 만들지는 않는다. 이 기술적 사실과 법적 면제/신고/프랑스 문서 요건 판정은 다르다. `ITSAppUsesNonExemptEncryption=false`를 임의 지정하지 않았다. [Apple export-compliance 안내](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance/)를 기준으로 소유자가 해당 분류/서류를 확인해야 한다.
7. **중국 본토**: 175개 국가 선택 후 ICP 비안 번호 입력란이 나타난다. LAN 전용 앱의 적용 여부나 면제를 단정하지 않았고 등록 번호를 만들지 않았다. 법적 적용/허가 또는 제외 결정은 소유자가 한다.

심사 연락처는 동일 소유자가 기존 앱에 등록한 검증된 값을 App Store Connect 안에서만 재사용했다. 이 문서에는 개인 연락처, 계정 ID, 서명 키, 프로비저닝 파일이나 기기 식별자를 기록하지 않는다.
