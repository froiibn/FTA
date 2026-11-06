# PC 자동 업데이트와 로컬 배포

## 제품 동작

Windows·macOS의 0.1.5부터 Tauri 공식 Rust updater 2.14.1을 사용한다. 시작 30초 뒤와 6시간 간격으로 확인한다. 새 버전의 네이티브 확인창은 프로세스당 한 번 표시한다. ‘나중에’ 후에는 설정의 확인 아이콘으로 다시 시작한다. 확인 후 다운로드하며 검증·네트워크·설치 실패는 성공으로 표시하지 않는다. 개발 빌드는 주기 확인을 중지하고 명시적인 설정 확인만 허용한다.

기존 설정의 앱 동작 행·시스템 글꼴·색과 확인 아이콘 하나만 사용한다. 메인 화면·토스트 구조를 변경하지 않는다. 접근성 이름·툴팁을 유지한다. Mac/Windows 스타일의 520×640·380×400 네 경우를 캡처·검토하고 온라인 기기·전송 결과를 주입하지 않는다.

0.1.4 이하 버전에는 updater가 없으므로 0.1.5를 설치 파일로 한 번 교체해야 한다. 이후 더 높은 버전부터 새 경로를 사용한다.

## 신뢰와 실행 경계

- HTTPS GitHub Releases의 `latest-{{target}}-{{arch}}.json`을 사용한다. 타깃별 버전으로 Mac만 먼저 배포됐을 때 Windows에 다른 플랫폼이나 옛 파일을 새 버전으로 광고하지 않는다.
- 다운로드 URL은 `github.com/froiibn/FTA/releases/download/v<version>/<asset>`로 제한한다. WebView는 URL·키·설치 경로를 지정할 수 없고 updater·네트워크·파일·셸 권한을 받지 않는다.
- 공개 키만 앱 설정에 포함한다. 개인키는 저장소 외부 OS 사용자 전용 폴더에 보관한다. 기존 앱의 신뢰를 유지하려면 같은 개인키를 안전하게 유지해야 한다. Windows PC에 개인키를 복사하지 않고 검증한 NSIS를 게시 Mac에서 서명할 수 있다.
- `requireSignedVersion=true`와 CLI의 버전 결합 서명으로 JSON 버전만 바꾼 옛 패키지도 거부한다. 기본 비교는 현재보다 높은 버전만 허용한다.
- Windows는 기존 perMachine NSIS·passive 설치와 OS UAC를 사용한다. quiet·승격 우회·방화벽 완화를 추가하지 않는다.

## 전송 보존

호스트의 lifecycle guard로 시작·재시작·종료·설치를 직렬화한다. 다운로드/검증 후 앱이 시작한 엔진만 설치용 종료를 요청한다. 별도 CLI/MCP 서비스는 자동 종료하지 않고 별도 엔진 종료 후 재실행을 안내한다.

새 로컬 IPC `engine.update_shutdown`은 네트워크 진입 gate의 배타적 잠금을 즉시 얻어야 한다. 실제 송신은 최종 수신 응답까지, 수신 요청은 저장/응답 생성까지 read guard를 유지한다. 잠금 실패 또는 DB 전체의 비최종 전송은 BUSY이며 shutdown을 시작하지 않는다. 화면의 기록 페이지 제한과 무관하다. 성공 시 shutdown token을 설정하고 새 진입은 STOPPING으로 거부한다. 일반 종료·LAN 프로토콜·등록 규칙은 유지한다.

호스트는 엔진 프로세스 종료를 최대 10초 확인하며 강제 kill로 업데이트하지 않는다. Mac은 설치 후 재시작하고 Windows는 공식 updater가 설치 프로그램 실행 후 종료·재시작한다. 실패가 반환되면 수신 엔진 복구를 시도한다. 설정·등록·수신 파일은 설치 번들 밖에 유지하고 MCP 클라이언트 설정은 변경하지 않는다.

## GitHub Actions 없는 배포

`packaging/macos/build-release.py`는 깨끗한 커밋에서 로컬 필수 검사, 앱/DMG 빌드·서명·공증 승인·앱/DMG 티켓·Gatekeeper·이미지 검증 후 최종 `.app.tar.gz`를 만든다. `packaging/publish-release.py`는 실제 바이트의 빌드 정보·해시·버전·번들 구성 확인, 버전 결합 서명과 변조 거부 검사를 거쳐 타깃별 JSON·SHA-256·빌드 정보를 준비한다. `--publish`에서만 게시한다.

첫 공개는 draft에 모든 자산을 올리고 재다운로드 해시를 대조한 뒤 latest로 공개한다. Windows를 나중에 같은 버전에 추가할 때는 공개 Mac 자산을 덮어쓰지 않고 새 설치 파일·서명·증빙을 먼저 올려 대조한 뒤 Windows feed를 마지막에 게시한다. 새 버전에서 다른 OS의 feed가 있으면 기존 버전 결합 서명을 유지해 복사한다. Actions 워크플로·계정 예산·보호 설정은 변경하지 않는다.

Windows는 사용자 Windows PC에서 `packaging/windows/build.ps1`로 빌드·검사한다. 서명은 게시 컴퓨터에서 처리할 수 있어 개인키 없이 NSIS를 패키징한다. 다른 OS의 실행·설치/UAC 검증을 Mac에서 통과로 보고하지 않는다.

공식 API/형식: [Tauri updater](https://v2.tauri.app/plugin/updater/).

## 배포 고지

공식 updater를 포함한 데스크톱의 잠긴 Mac/Windows normal·build 의존성과 Rust 런타임의 원문 고지를 `THIRD_PARTY_NOTICES.txt`로 번들에 동봉한다. 빌드 도구·선택적 원본 고지가 포함될 수 있어 나열된 모든 패키지가 앱에 링크됐다는 뜻은 아니다. 패키지 원본 아카이브 주소를 제공하며 구현 소스를 앱에 복사하지 않는다. Cargo 패키지에 원문 고지가 누락된 경우 같은 upstream 커밋의 고지 또는 표준 라이선스 원문만 `apps/desktop/LegalSources`에 URL·해시와 보존한다. 기존 iOS·Android 고지나 앱 구현은 변경하지 않는다. `apps/desktop/scripts/generate-notices.py --check`가 두 타깃 그래프와 원문 해시의 일치를 검사한다.
