# Android 초안 설계·구현 (2026-10-30)

사용자 지시로 iOS와 독립된 `apps/android`에서 Android 개발을 시작했다. 공통 전송 엔진은 Rust를 유지하며 첫 실행 가능한 호스트는 Android SDK의 Java 기본 컨트롤·JNI·NSD·SAF·Keystore를 사용한다. Java/Android 기본 컨트롤은 이번 초안의 구현 선택이며, Windows·macOS의 사용자 확정 TypeScript/Tauri 2 스택을 바꾸지 않는다. 새 UI/전송 라이브러리나 복제 앱 소스를 도입하지 않는다. 사설 IPv4 LAN, 최소 API 29, 첫 ABI ARM64, target/compile SDK 37은 초안 기준이며 API 37 에뮬레이터 외의 호환 검증을 완료한 결정은 아니다.

## 화면 계획·검토

FTA의 기존 색과 네이티브 조작을 따른다. 바탕 `#102032`, 상단 색조 `#17334A`, 본문/버튼 `#FFFFFF`, 보조 글자 흰색 75%, 진행/온라인/완료 `#34C759`를 사용한다. 글꼴은 Android 시스템 sans다. 상단 브랜드·상태·가로 대상·사진/파일 두 버튼·선택 초안은 본문 스크롤, 1–4줄 메시지 입력과 기본 탭 버튼은 하단에 둔다. 기기 이름·본문은 왼쪽, 기기 아이콘과 이름은 가운데 정렬한다. 반투명 표면과 옅은 흰 윤곽으로 글래스 느낌을 표현하고 글자/아이콘은 선명하게 유지한다.

검토 기준은 사용자가 정한 흰 버튼·휴대폰/기존 컴퓨터 윤곽·녹색 실제 진행률·완료 두 번 깜빡임이다. OS 기본 Ripple·키보드·PopupMenu·AlertDialog·시스템 선택기를 사용한다. 주기적인 엔진 조회가 같은 기기/기록/선택 초안 컨트롤을 계속 교체하지 않도록 실제 내용이 바뀔 때만 행을 갱신한다. 가짜 온라인 대상·가짜 퍼센트·가짜 성공 상태는 넣지 않는다. Android 전용 화면만 추가하고 iOS 기존 레이아웃과 데스크톱 배포본은 수정하지 않는다.

## 엔진 경계

`Android 기본 화면 → UTF-8 byte[] JNI 제어 → 기존 모바일 Rust 호스트 → fta-core`로 호출한다. `fta-android`는 현재 `apps/ios/bridge`에 있는 UI 독립 모바일 C ABI를 의존성으로 재사용한다. Android에서 iOS Keychain/UIKit를 링크하지 않으며 OS 어댑터는 cfg로 분리한다. 기존 iOS 호스트를 이동/이름 변경하지 않아 기존 빌드 경로를 보존한다. 파일 body는 Rust에서 스트리밍하고 JNI/JSON에는 넣지 않는다. 텍스트/URL·경로·메타데이터 제어는 공통 엔진과 같은 8 MiB 프레임 예산을 둔다.

최초 발견 정보는 인증되지 않은 주소 힌트다. NSD의 실제 IPv4/포트를 Rust에 전달하고, 등록 대상도 접속 가능성 검사를 통과해야 한다. 최초 전송 시 수신자 한 번의 승인 전에 파일/본문을 전송하지 않는다. 이후에는 기존 인증서 pin으로 온라인 확인·TLS 1.3 상호 인증·크기/해시·최종 저장 확인을 유지한다. NSD의 os는 android이며 데스크톱의 모바일 분류와 일치한다. 일반/등록 서비스는 각각 `_fta._tcp.`와 `_fta-pair._tcp.`다.

NSD 검색/광고는 선택한 실제 Wi-Fi/유선 네트워크에 바인딩한다. 오래된 Android의 단일 resolve 제한은 직렬 큐로 처리하고 주소/발견 수를 제한한다. 인터페이스+RFC1918 CIDR 승인은 SharedPreferences에 저장하며 DHCP 변경/재접속에 재사용한다. CIDR이 공개 주소까지 포함하는 supernet이면 선택하지 않는다. SSID·공유기 자체 인증과 구분한다. VPN/셀룰러가 이 승인을 상속하지 않으며 프로세스 소켓도 선택한 LAN으로 바인딩한다. Wi-Fi/LAN 상실 시 목록/선택과 광고·리스너를 즉시 정리하고 재접속 뒤 실제 온라인 확인을 다시 수행한다. 오프라인 등록은 설정의 관리용으로만 유지한다.

## Android 권한·저장소·키·백그라운드

| 경계 | 구현 |
| --- | --- |
| Android 17+ LAN | `ACCESS_LOCAL_NETWORK` 선언·런타임 동의, 거부 상태에서 검색/리스너 미시작, 앱 권한 설정 연결 |
| 수신 알림 | `POST_NOTIFICATIONS` 선택 동의, DB 이벤트/소비자 커서 사용, OS 표시 여부와 제출/권한 거부 처리를 구분 |
| 사진 | API 33+ Photo Picker, 이전 OS는 이미지 SAF, 선택 항목만 스트리밍 복사, 전체 보관함 권한 없음 |
| 촬영 | 외부 시스템 카메라 Intent·단일 임시 출력 URI만 권한 부여, 앱 자체 CAMERA/마이크 권한 없음, 빈 결과/취소는 초안 보존 |
| 파일 | SAF `ACTION_OPEN_DOCUMENT`, 선택한 공급자 URI에서 비공개 Outgoing 사본으로 복사, 내용 전체를 메모리에 적재하지 않음 |
| 수신 | 내부 Received에 원자적 최종 저장, Android Files의 FTA DocumentsProvider는 최종 파일 열기/사용자 삭제만 허용, 숨김 partial/상태/키/경로 이탈 차단 |
| 개인키 | 기존 Rust TLS DER를 Android Keystore의 추출 불가 AES-GCM 키로 감싸 no-backup vault에 보관, 계정 지문을 AAD로 인증, identity JSON에는 인증서/키 계정만 기록 |
| 메모리·오류 | 키 전용 JNI byte[]는 처리 뒤 덮어씀, 개인키/본문 로그 없음, 기존 키 실패 시 새 신뢰 키 자동 생성 없음 |
| 앱 업데이트/복구 | Context 물리 경로·컨테이너 FD를 매 프로세스 시작 때 고정, 저장 승인/초안/기기 ID 유지, 참조 없는 중단된 선택 임시 파일 정리 |
| 백업 | allowBackup=false와 구버전/Android 12+ 백업·기기 이전 명시 제외, 상태/키는 no-backup 폴더 사용 |
| 백그라운드 | 보이는 Activity 동안만 수신, 앱 전환/잠금·수신 해제·LAN 상실 시 네트워크 중지/작업 취소, 기존 Rust 기록/복구 경계 유지 |

Keystore는 래핑 키를 보호한다. 기존 Rust TLS 엔진이 DER를 필요로 하므로 TLS 서명 키 자체가 항상 하드웨어 안에서만 사용된다고 주장하지 않는다. 하드웨어 보호 수준은 기기에 따라 다르다. 공개 수신 루트는 OS 컨테이너 안이며 Source FD는 컨테이너 아래 각 허용 폴더를 no-follow로 열어 시스템 부모 접근 제한과 링크 바꿔치기를 함께 처리한다.

사진/파일은 최대 합산 100개로 준비하고 취소/실패 때 이전 초안을 보존한다. 메시지 원문은 그대로 보내며 단일 HTTP(S)만 URL로 분류한다. 실제 바이트 전체 합산 비율을 녹색 테두리에 표시한다. 전송 업로드 100%도 최종 저장 완료가 아니며 저장 응답 뒤에만 체크 1.5초와 0.6초 주기의 두 번 흐림/복원을 표시한다. 온라인 점·기기 이름·체크는 깜빡이지 않는다. 실패/부분/취소/결과 불명은 성공 표시를 하지 않고, 이전 전송의 늦은 조회/타이머는 ID로 분리한다. 시스템 애니메이션 끄기에서는 체크만 표시한다.

Foreground Service·백그라운드 지속·부팅 자동 시작·오프라인 대기열·공유 확장·임의 SAF 폴더 자동 수신은 이번 초안에 포함하지 않는다. 사진/파일 선택기·카메라·외부 뷰어로 전환하는 동안에도 Activity가 보이지 않으면 수신이 중단된다. 앱 복귀 때 승인된 LAN과 서비스 상태를 다시 확인한다.

## 빌드·검증

사용자 Mac의 JDK/SDK/NDK에서 Gradle/NDK/Cargo로 빌드한다. GitHub Actions·클라우드 빌드는 사용하지 않는다. 앱 실행 의존성은 기존 Rust 그래프와 Android 프레임워크이며 Gradle/AGP는 빌드 도구다. Android target graph의 원문 라이선스를 APK에 포함하고 설정에서 열 수 있다. 전송 데이터/권한/삭제/백업 범위를 설명하는 로컬 개인정보 안내도 포함한다. 빌드 스크립트/SDK 경로·설치 명령은 [Android README](../../apps/android/README.md)를 따른다.

최종 검증 수치·산출물은 완료 기록에서 추가한다. debug APK는 초안 개발용이며 Google Play 제출/출시 서명·스토어 등록·외부 배포 완료가 아니다. 실제 Android 기기·다른 지원 OS 버전·실제 Wi-Fi NSD 발견·Android↔iPhone/Windows/Linux 실기·대형 파일·저장 공간 부족·강제 종료 중 복구·권한 철회 중 실제 전송·외부 클라우드 공급자·제조사 절전 정책은 별도 검증 대상이다.

## 플랫폼 근거

- [Android 로컬 네트워크 권한](https://developer.android.com/privacy-and-security/local-network-permission)
- [Android NSD와 API 37 네트워크 제한](https://developer.android.com/reference/android/net/nsd/NsdManager)
- [권한 최소화와 시스템 사진/카메라 선택](https://developer.android.com/privacy-and-security/minimize-permission-requests)
- [Android Gradle Plugin 9.4 도구 호환성](https://developer.android.com/build/releases/agp-9-4-0-release-notes)

### 첫 초안 최종 검증 기록

Android 0.1.0 내부 빌드 1의 개발용 debug APK를 빌드·서명 검사·16 KiB ELF/APK 정렬 검사 후 API 37 ARM64 에뮬레이터에 설치했다. Android 기본 UI/Java에는 별도 런타임 라이브러리를 추가하지 않았으며 Cargo.lock 변화는 Android workspace 패키지 추가뿐이다.

- 실제 Android JNI 엔진과 Mac Rust 엔진 사이 수신자 첫 등록 승인, 양방향 TLS 파일 스트림·최종 저장·원문 대조, Android 텍스트/URL 수신이 통과했다. 이 검사에서 Android의 실제 os=android 메타데이터, Keystore 암호화·기기 ID 유지·원문 URL 분류·합산 진행률·두 번 깜빡임·문서 공급자/숨김 최종 파일/경로 이탈/루트 삭제 차단·비공개 상태 파일 송신 거부를 포함해 35개 검사를 통과했다. adb loopback 전달을 사용한 실제 전송이며 물리 Wi-Fi 발견 검증과 구분한다.
- 입력 UI 검사 묶음 34개가 통과했다. 기본 검사 28개는 위와 중복되며 실제 입력창의 1줄→4줄 높이 증가·네 줄 상한·초과 줄 내부 스크롤·빈 영역 터치에 따른 포커스/키보드 해제 등 6개가 추가된다. 글꼴 패딩을 포함한 실제 Layout의 줄 높이로 검증했다.
- 실제 에뮬레이터 Wi-Fi를 차단/복원하고 앱을 강제 종료/재실행해 네트워크 승인·송수신 끄기·기기 ID 유지·네이티브 탭·사진 메뉴를 확인했다. Photo Picker에서 고른 PNG의 준비 사본이 원본 SHA-256과 일치했고, 외부 시스템 카메라의 에뮬레이터 가상 센서로 촬영한 63,747바이트 JPEG가 기존 사진 초안과 함께 준비됐다. 실제 SAF 파일 선택기 실행/취소도 확인했다.
- 사용자 Mac headless 83 통과·1 ignored, fmt·headless Clippy 통과. 기존 iOS bridge의 localhost 복합 검사 1개와 iOS Rust 타깃 check 통과. Android host/Android 타깃 all-targets Clippy 통과. Mac 데스크톱 check·테스트 10개·Clippy·TypeScript/Vite build 통과. Android lint 오류 0개이며 고정 도구의 새 버전 안내·ARM64 전용/아이콘 등 비차단 경고는 남아 있다.

처음 API 37 에뮬레이터 자동 GPU 모드에서 `HardwareRenderer.setStopped → RenderThread → glGetProgramBinary → qemu_pipe_read` 대기의 ANR 덤프를 관찰했다. 같은 앱의 하드웨어 가속을 끄지 않고 에뮬레이터를 4코어/소프트웨어 GPU로 실행해 UI·키보드·파일 선택·촬영을 재검증했다. 이 결과를 실기 성능 검증으로 해석하지 않는다. 로컬 기록은 `output/android/verification.json`, UI 이미지는 `output/android-ui-checks`, 개발용 설치 파일은 `output/android/FTA-0.1.0-debug-arm64.apk`와 SHA-256이다. 실제 Android 기기에는 설치하지 않았고 외부 배포/Google Play 제출/GitHub Actions는 수행하지 않았다.
