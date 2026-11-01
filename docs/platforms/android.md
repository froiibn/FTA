# Android 초안 설계·구현 (2026-10-30)

사용자 지시로 iOS와 독립된 `apps/android`에서 Android 개발을 시작했다. 공통 전송 엔진은 Rust를 유지하며 첫 실행 가능한 호스트는 Android SDK의 Java 기본 컨트롤·JNI·NSD·SAF·Keystore를 사용한다. Java/Android 기본 컨트롤은 이번 초안의 구현 선택이며, Windows·macOS의 사용자 확정 TypeScript/Tauri 2 스택을 바꾸지 않는다. 새 UI/전송 라이브러리나 복제 앱 소스를 도입하지 않는다. 사설 IPv4 LAN, 최소 API 29, 첫 ABI ARM64, target/compile SDK 37은 초안 기준이며 API 37 에뮬레이터 외의 호환 검증을 완료한 결정은 아니다.

## 화면 계획·검토

사용자의 최신 지시인 “안드로이드 설정화면과 같은 UI”를 기준으로 실제 API 37 에뮬레이터의 시스템 설정 앱을 읽기 전용으로 관찰했다. 외부 UI 라이브러리 없이 SDK의 라이트/다크 테마·시스템 sans·Toolbar·Switch·Button·EditText·AlertDialog를 유지하고, 설정 앱처럼 바탕과 둥근 그룹 표면을 구분했다. 밝은 바탕 `#F0EEF5`/표면 `#FCFAFF`, 어두운 바탕 `#17171C`/표면 `#27262E`, 강조색 `#24466B`/`#AAC9EE`를 사용한다. 제조사 스킨/외부 아이콘 패키지를 복제하지 않는다.

설정은 네트워크·기기·파일/권한·앱 정보 그룹 안에 제목/보조 설명/현재 값을 표시하고 Switch 또는 끝쪽 액션 표식을 둔다. 기기 이름은 현재 값이 보이는 행에서 기본 편집 대화상자를 열고 검증 후 저장한다. OS 권한 설정에서 돌아오면 표시값을 갱신한다. 보내기/기록도 같은 표면과 행 구조를 사용한다. 사진/파일은 채움·외곽선이 있는 기본 버튼, 파일 전송은 강조 배경, 메시지는 포커스 외곽선을 가진 입력 영역과 별도 강조 보내기 버튼으로 역할을 구분한다.

Toolbar 56dp·본문 16dp·작업 터치 영역 최소 48dp를 유지한다. Activity가 외관 변경에 맞춰 다시 생성되며 탭을 복원한다. 상태/내비게이션 바와 IME insets를 적용하고 키보드가 열리면 하단 탐색을 숨긴다. 입력 1–4줄/초과 스크롤·빈 영역 키보드 내리기를 유지한다. 온라인/진행/완료는 실제 엔진 상태의 녹색이며 밝은 표면 `#188038`, 어두운 표면 `#81C995`로 대비를 확보한다. 실제 바이트 테두리 진행률과 최종 저장 후 체크 1.5초·두 번 깜빡임은 변경하지 않는다.

주기적인 엔진 조회가 같은 기기/기록/선택 초안 컨트롤을 계속 교체하지 않도록 실제 내용이 바뀔 때만 행을 갱신한다. 가짜 온라인 대상·가짜 퍼센트·가짜 성공 상태는 넣지 않는다. 이번 디자인 수정은 Android 화면·리소스에 한정하며 공통 Rust·iOS·데스크톱 코드는 수정하지 않는다.

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
| 백그라운드 | connectedDevice 전경서비스가 네트워크·NSD·단일 JNI 호스트·송신·알림을 소유, Home/화면 꺼짐/Activity 재생성에 유지, 명시 중지/권한/LAN 경계에서 정리 |

Keystore는 래핑 키를 보호한다. 기존 Rust TLS 엔진이 DER를 필요로 하므로 TLS 서명 키 자체가 항상 하드웨어 안에서만 사용된다고 주장하지 않는다. 하드웨어 보호 수준은 기기에 따라 다르다. 공개 수신 루트는 OS 컨테이너 안이며 Source FD는 컨테이너 아래 각 허용 폴더를 no-follow로 열어 시스템 부모 접근 제한과 링크 바꿔치기를 함께 처리한다.

사진/파일은 최대 합산 100개로 준비하고 취소/실패 때 이전 초안을 보존한다. 메시지 원문은 그대로 보내며 단일 HTTP(S)만 URL로 분류한다. 실제 바이트 전체 합산 비율을 녹색 테두리에 표시한다. 전송 업로드 100%도 최종 저장 완료가 아니며 저장 응답 뒤에만 체크 1.5초와 0.6초 주기의 두 번 흐림/복원을 표시한다. 온라인 점·기기 이름·체크는 깜빡이지 않는다. 실패/부분/취소/결과 불명은 성공 표시를 하지 않고, 이전 전송의 늦은 조회/타이머는 ID로 분리한다. 시스템 애니메이션 끄기에서는 체크만 표시한다.

전경서비스 실행 중에는 사진/파일 선택기·카메라·외부 뷰어·Home으로 전환해도 송수신을 유지한다. 부팅 자동 시작·오프라인 대기열·공유 확장·임의 SAF 폴더 자동 수신은 범위 밖이다. OS 강제 중지 뒤에는 정상적인 앱 재진입으로만 재개한다.

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

### Android 기본 디자인 수정 검증

0.1.0 내부 빌드 2에서 플랫폼 기본 Toolbar·탐색·입력·설정·기록과 라이트/다크 화면을 적용했다. API 37 ARM64 에뮬레이터에서 빌드/Android lint, 양쪽 외관의 입력 검사 각 34개, 양쪽 외관의 실제 Wi-Fi/신뢰 설정 유지 UI 검사, Mac Rust와 실제 JNI TLS/최종 저장 검사 35개를 통과했다. 시스템 글꼴 130%에서도 입력 검사 34개가 통과했다. 실제 Android 실기/TalkBack과 모든 화면의 초대형 글꼴 평가는 별도 검증이 필요하다. 최신 세 화면의 실제 캡처는 `output/android-native-ui-v2/light`와 `output/android-native-ui-v2/dark`, APK/해시/검증 요약은 `output/android-native-ui-v2`에 보관한다. 첫 초안의 기존 엔진 검사 기록을 이번 UI 작업에서 모두 재실행한 것으로 해석하지 않는다.

### 시스템 설정 기준 UI 수정 (내부 빌드 3)

실제 시스템 설정 참고 캡처는 `output/android-system-settings-reference.png`다. 새 그룹 표면·설정값/스위치 행·이름 편집 대화상자·배경/외곽선 작업 버튼·입력 경계를 구현했다. 최신 APK/해시/검증 요약과 실제 보내기/기록/설정 라이트·다크 화면은 `output/android-settings-ui-v3`에 보관한다. 검증 요약에는 이번 변경에서 실제 수행한 UI·입력·설정값 편집·TLS 검사를 별도 기록한다. Android 실기·물리 LAN 발견·TalkBack 및 전체 화면의 초대형 글꼴 검증은 완료하지 않았다.

이번 수정의 로컬 검증은 assembleDebug/assembleDebugAndroidTest/lintDebug(오류 0·경고 5), 설정 대화상자 검사 35개(기본 28+추가 7), 라이트/다크 및 글꼴 130% 입력 검사 각 34개, 두 테마의 Wi-Fi 상실/재접속·신뢰/송수신 설정 유지·탭/사진 메뉴 UI 검사, 실제 Android↔Mac TLS/최종 저장 35개를 통과했다. 기본 글꼴과 130% 글꼴에서 보내기/기록/설정을 두 테마로 실제 캡처했다. APK 서명과 16 KiB 정렬을 확인했으며 Android 실기 설치로 보고하지 않는다.


## 백그라운드 송수신 (0.1.0 내부 빌드 4)

`TransferService`가 NetworkCallback·NSD·프로세스 LAN 바인딩·Rust 호스트 시작/중지·조회·송신 작업·DB 이벤트 알림을 소유한다. Activity는 보이는 동안 로컬 Binder로 상태를 읽고 중지 때 unbind한다. Service는 Activity나 View 콜백을 보관하지 않는다. 즉시 완료 표시 갱신에는 내용 없는 패키지/서명 권한 제한 브로드캐스트를 쓰며 화면이 사라지면 수신기를 해제한다. 전송 ID/실제 바이트·최종 저장 결과는 Service에 남아 화면 재생성/전환이 작업을 다시 시작하지 않는다. 파일과 메시지 초안은 불변 전송 시도와 비교해 성공한 부분만 지우며 미확정 결과는 성공 처리하지 않는다.

보이는 앱 진입에서 송수신 사용·실제 신뢰 LAN·LAN 권한을 확인한 뒤 `startForegroundService`를 호출하고 즉시 `connectedDevice`로 승격한다. 지속 알림은 상태/열기/중지를 제공하며 알림 중지는 송수신 설정도 끈다. `START_NOT_STICKY`, 부팅 수신기/재시작 알람/작업 스케줄 없음으로 시스템/사용자 중지를 우회하지 않는다. 시작 거절·영구 시작 실패는 자동 반복하지 않고 다음 정상 앱 진입/명시 설정 변경을 기다린다. 호스트가 중단된 소켓을 정리하는 BUSY 상황만 기존 FGS 안에서 제한 시간으로 재시도한다.

LAN 상실 시 발견/광고/온라인 목록·소켓/진행 작업을 즉시 정리한다. 기존 FGS는 최대 60초의 단일 재연결 유예를 둔다. 그 안에 같은 승인 범위의 실제 LAN이 복귀하면 재개하며, 미신뢰 LAN/권한 거절/송수신 해제는 유예 없이 중지한다. 유예가 끝난 뒤 백그라운드에서 새 FGS를 시작하지 않는다. 앱을 다시 열면 저장된 신뢰를 재사용한다. OS 절전으로 타이머 실행이 늦어져도 경과 시간 확인으로 만료 후 네트워크 복귀를 새 시작 예외로 사용하지 않는다.

| OS | 구분 |
| --- | --- |
| API 29–30 | 최소 지원 기준, typed FGS/지속 알림 사용. 현재 실제 실행 검증은 API 37에 한정 |
| API 31–32 | 백그라운드 신규 FGS 시작 제한을 따르고 보이는 앱에서 시작 |
| API 33+ | 알림 권한 거부로 FGS 자체를 막지는 않음. UI에 표시 거부를 안내하고 OS Task Manager 중지를 보존 |
| API 34+ | connectedDevice 타입과 FOREGROUND_SERVICE_CONNECTED_DEVICE 선언. 실제 NSD에 쓰는 CHANGE_WIFI_MULTICAST_STATE가 타입의 선언 권한 전제 충족 |
| API 35+ | dataSync의 24시간 중 6시간 제한과 구별. LAN 기기 발견/연결/직접 송수신이 connectedDevice 사용 근거이며 타임아웃 회피용 타입 대체가 아님 |
| API 37+ | ACCESS_LOCAL_NETWORK 런타임 동의를 Service에서도 재검증. 거부 상태에서는 LAN 소켓/검색을 열지 않음 |

상시 로컬 수신 대기와 실제 외부 LAN 기기 상호작용에는 connectedDevice를 사용한다. 클라우드 동기화용 dataSync나 발신만 다루는 사용자 시작 전송 작업은 즉시 LAN 수신 리스너를 대신하지 못한다. Google Play 제출 시 FGS 유형·사용자 영향·시작/중지 흐름과 시연 자료를 별도 선언/심사해야 하며 이 구현이 승인을 보장하지 않는다.

화면 꺼짐 자체는 중지 조건이 아니다. 유휴 백그라운드 조회는 3초, 화면/활성 전송 조회는 0.75초로 구분한다. 실제 진행 작업에만 2분 제한 partial wake lock을 사용하고 실제 바이트 변화에 따라 갱신하며 유휴 수신에는 보유하지 않는다. Doze/제조사 절전/네트워크 재연결 지연을 우회하지 않고 절전 알림 상태를 구분한다. 배터리 최적화 일괄 제외를 요청하지 않는다. 기존 Keystore AES-GCM 키 속성과 암호화 저장소를 변경하지 않았다. API 37 에뮬레이터의 첫 잠금 해제 이후 화면 꺼짐에서 기존 키 복호화를 확인했지만 PIN 보안 잠금·제조사 하드웨어 키·재부팅 전 첫 잠금 해제 전은 별도 실기 검증이 필요하다. 앱은 Direct Boot/부팅 자동 수신을 지원하지 않는다.

공식 근거(2026-10-30 확인): [FGS 유형](https://developer.android.com/develop/background-work/services/fgs/service-types), [백그라운드 시작 제한](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start), [타입별 시간 제한](https://developer.android.com/develop/background-work/services/fgs/timeout), [알림 권한](https://developer.android.com/develop/ui/compose/notifications/notification-permission), [사용자 중지](https://developer.android.com/develop/background-work/services/fgs/handle-user-stopping), [LAN 권한](https://developer.android.com/privacy-and-security/local-network-permission), [Doze](https://developer.android.com/training/monitoring-device-state/doze-standby), [Play FGS 요건](https://support.google.com/googleplay/android-developer/answer/13392821).

`verify-background.py`는 실제 Service의 LAN 바인딩을 유지하면서 에뮬레이터 NAT용 테스트 전용 TCP 릴레이로 암호화 TLS 바이트만 연결한다. 최초 승인·키/인증 정책을 우회하지 않는다. 실기 LAN mDNS 발견·배터리 성능 검증과 구별한다. Home/화면 꺼짐 수신·송신, 양방향 32 MiB 최종 저장·바이트 대조, 전송 중 Activity 재생성, 키 복호화, Wi-Fi 상실/복귀, 알림 중지·송수신 설정 경계를 검사한다. API 37 에뮬레이터가 화면 꺼짐에서 Wi-Fi 재연결을 지연해 재연결 검사는 FTA를 열지 않고 Home 화면을 켠 상태로 수행한다. 계측 실행이 프로세스 우선순위에 미치는 영향과 실제 제조사 절전 차이는 별도 실기로 확인해야 한다. `background-policy.py`는 계측 없이 정상 앱에서 권한 거부·Task Manager/force-stop·유예 만료·명시 재진입을 검증한다.

### 백그라운드 최종 로컬 검증

API 37 ARM64 에뮬레이터에서 실제 Service 통합 29개와 계측 없는 정상 앱 OS 경계 15개가 통과했다. 후자는 POST_NOTIFICATIONS/LAN 권한 거절, Task Manager/force-stop 뒤 자동 부활 없음, 수신 해제 후 cold start의 실제 기록, 신뢰·기기 ID 유지, 단일 60초 유예 만료와 정상 앱 재진입을 포함한다. 실제 TCP 리스너가 알림 중지/설정 해제 뒤 닫히는지도 검사했다. 기본 입력 묶음 34개·설정 대화상자 묶음 35개·실제 Wi-Fi/UI 스모크가 통과했으며 각 묶음의 기본 JNI 검사 28개는 서로 중복된다.

사용자 Mac에서 기본 headless 83 통과·1 ignored, workspace fmt·headless Clippy, Android host/all-targets Clippy 및 Android 타깃 lib Clippy를 통과했다. Android assembleDebug/assembleDebugAndroidTest/lintDebug, 서명·APK 16 KiB/ELF 정렬을 확인했다. iOS/데스크톱 구현을 변경하거나 이번 작업에서 해당 OS 앱 전체 UI 검증을 재실행한 것으로 해석하지 않는다. 최종 APK·SHA-256·실제 화면·검증 요약은 `output/android-background-v4`에 있다. Android 실기·API 29–36 실제 실행·제조사 절전·보안 PIN 잠금·Deep Doze에서의 수신 보장·물리 LAN mDNS 상호 발견은 미검증이다.


### 상단 헤더 스크롤 (내부 빌드 5)

보내기·기록·설정의 Toolbar와 구분선을 각 페이지의 단일 ScrollView 첫 컨텐츠로 이동했다. 헤더 바깥의 고정 높이/공백을 제거하고 같은 Toolbar를 재사용한다. 하단 메시지 입력·탭바·시스템 인셋과 기존 디자인은 유지한다. 탭별 스크롤 위치를 저장하고 비동기 복원 중의 임시 0 위치를 다시 저장하지 않아 탭 재구성/Activity 복원에서 상단으로 초기화되지 않게 한다. 새로고침/서비스 조회는 컨텐츠 갱신 경로를 유지한다.

`tests/scroll-pages.py`는 실제 컨텐츠만 있는 전용 에뮬레이터의 창 높이를 일시 축소해 세 헤더가 실제 터치 스크롤로 화면 밖으로 나가는지, 하단 좌표·서비스 갱신·동일 탭 재구성/탭 왕복 위치를 확인하고 화면 크기를 복원한다. 입력 검사 34개와 로컬 빌드/lint를 통과했다. Galaxy S23에는 삭제 없이 install -r로 업데이트하고 신뢰/송수신/기기 이름·기기 ID 해시가 유지되며 프로세스/전경서비스가 실행 중임을 확인했다. 실기기에서 전송·등록 승인이나 권한/신뢰 설정 변경은 하지 않았다. APK·실제 전후 화면·설치 검증은 `output/android-scroll-v5`에 보관한다.


### 물리 LAN 미등록 기기 표시 수정 (내부 빌드 6)

Galaxy S23(Android 16/API 36)를 Mac과 동일 LAN에 연결해 확인했다. Android NSD는 정상 광고를 받고 resolve를 완료했으며, 엔진의 `devices.list(available_only=true)`도 실제 TCP 도달성을 통과한 미등록 기기 두 개를 반환했다. 화면이 `online=false`를 일괄 제외해 이를 숨기고 있었다. 공통 모델에서 미등록 발견 기기의 `online`은 등록된 인증 세션의 온라인 상태와 다르므로, Android는 이미 도달성을 검사한 미등록 결과를 표시하고 등록된 기기에만 `online` 필터를 적용한다. 등록 승인/본문 전송/엔진 신뢰 검사는 변경하지 않는다.

표시 판정 네 가지(도달 가능한 미등록·등록 온라인·등록 오프라인·null)를 UI에 가짜 기기를 삽입하지 않는 순수 검사로 추가했다. 기존 JNI/Keystore/입력 UI 검사와 합계 38개, 빌드/lint, APK 서명·16KiB ZIP 정렬 검사를 통과했다. 실기에는 앱 삭제 없이 업데이트하고 실제 미등록 목록 두 개(컴퓨터 아이콘/녹색 점)·선택, Mac의 Galaxy mDNS 발견, 설정/기기 ID 보존과 전경서비스 실행을 확인했다. 이번 검증은 물리 LAN 발견과 표시이며 사용자 기기에 등록 승인이나 파일·본문 전송을 실행하지 않는다. 제조사 절전/Deep Doze 및 물리 LAN 실전송 검증을 대신하지 않는다. 증빙은 `output/android-nsd-v6`에 보관한다.
