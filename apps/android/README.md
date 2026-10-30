# FTA Android 초안

Android 기본 Java 컨트롤과 JNI로 기존 Rust 전송 엔진을 구동한다. UI 라이브러리·복제 앱 소스·새 전송 프로토콜을 추가하지 않는다. Android Studio에서 이 폴더를 프로젝트로 연다. 데스크톱 Tauri/TypeScript와 iOS 화면은 별도로 유지한다.

## 빌드

검증 환경은 사용자 Mac, Android Studio 번들 JDK 25, SDK API 37, Build Tools 36.0.0, NDK 28.2.13676358, Rust 1.99.1이다. Gradle 9.6.0과 Android Gradle Plugin 9.4.0은 고정된 빌드 도구이며 앱 UI 의존성이 아니다. 최초 wrapper 다운로드는 공식 배포 SHA-256으로 확인한다.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"
rustup target add aarch64-linux-android
./gradlew assembleDebug assembleDebugAndroidTest lintDebug
```

NDK 버전은 Android Studio의 SDK Manager에서 설치하거나 `ANDROID_NDK_HOME`으로 기존 설치를 지정한다. SDK 위치는 `ANDROID_HOME`/`ANDROID_SDK_ROOT`, Gradle의 SDK 위치는 로컬 `local.properties`로 지정한다. 개인 경로·서명·기기 식별자를 저장소에 넣지 않는다. 현재 초안은 사설 IPv4 LAN의 Android 10(API 29)+ ARM64이며 실제 실행 검증은 API 37 에뮬레이터에서 수행했다. x86/ChromeOS·다른 Android 버전의 실기 호환을 완료한 것은 아니다.

출력은 `app/build/outputs/apk/debug/app-debug.apk`다. 개발용 debug 키로 서명되며 Play 배포용 최종 서명/릴리스 패키지가 아니다. Rust 파일 스트림은 JNI를 통과하지 않는다. 네이티브 ELF와 APK는 16 KiB 페이지 정렬을 적용한다. `build-native.sh`는 실제 Cargo workspace·lockfile을 사용하며 GitHub Actions는 사용하지 않는다.

## 동작

- 설정에서 실제 Wi-Fi/유선 LAN의 인터페이스+CIDR을 한 번 신뢰하면 재시작·재접속 시 유지한다. SSID/공유기 신원 인증을 뜻하지 않는다. VPN·셀룰러는 자동으로 신뢰하지 않는다.
- Android 17(API 37)+의 로컬 네트워크 권한을 요청한다. 권한 거절 시 리스너·검색을 시작하지 않으며 설정 화면에서 권한 설정을 열 수 있다.
- 온라인 대상만 가로 아이콘으로 표시하고, 첫 전송은 수신자 한 번 승인 후에만 본문을 전송한다. 등록 후 재승인은 없다.
- 사진 버튼에서 시스템 사진 선택기/카메라를 고르고 파일 버튼은 SAF 문서 선택기를 사용한다. API 29–32의 사진 선택은 이미지 SAF로 대체한다. 카메라는 시스템 앱에 단일 출력 URI만 위임하며 전체 미디어·저장소·위치·마이크 권한을 요구하지 않는다.
- 선택 파일은 비공개 Outgoing 사본으로 준비한 뒤 Rust가 스트리밍한다. 텍스트/URL은 하나의 1–4줄 입력창에서 보내며 HTTP(S) 단일 주소만 링크 동작으로 구분한다.
- 실제 합산 바이트 진행률을 녹색 테두리로 표시한다. 준비/비율 불명은 회전 점선·가짜 퍼센트 없이 빈 테두리로 표시한다. 최종 저장 완료 응답 후 체크 1.5초·두 번 깜빡임을 표시하고 시스템 애니메이션 끄기를 존중한다.
- 파일 앱의 FTA 문서 공급자는 최종 수신 파일만 열기/삭제하도록 공개한다. 엔진·키·중간 파일은 공개하지 않는다. 수신 파일/URL은 자동 실행하지 않는다.
- 기록에서 본문을 확인·복사·링크/파일을 열고, 길게 눌러 기록만 삭제할 수 있다. 등록 해제·기기 이름·수신 사용·알림·개인정보/의존성 고지는 설정에 있다.

신뢰 LAN·필요 권한·송수신 사용을 확인하고 보이는 앱에서 connectedDevice 전경서비스를 시작한다. 지속 알림이 실행되는 동안 Home·화면 꺼짐·Activity 재생성에서도 송수신을 유지한다. 알림 중지/수신 해제/미신뢰 LAN/권한 거절은 전송과 소켓을 정리한다. LAN 상실 뒤 60초의 기존 FGS 재연결 유예가 끝나면 앱을 다시 열어 재개하며, 강제 종료·OS/Doze/제조사 정책을 우회하지 않는다. 부팅 자동 시작·오프라인 대기열·공유 확장·임의 SAF 폴더 자동 수신은 포함하지 않는다.

## 로컬 검증

```sh
# 저장소 루트에서 공통 회귀
cargo fmt --all --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo test -p fta-ios
cargo clippy -p fta-android --all-targets -- -D warnings
cargo build -p fta-android --example transport_fixture

# Android 프로젝트 폴더에서 디버그 앱·검사 APK 설치 후 실행
cd apps/android
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb install -r app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb shell pm grant app.fta.android android.permission.ACCESS_LOCAL_NETWORK
ANDROID_SERIAL=emulator-5554 python3 tests/verify-emulator.py
ANDROID_SERIAL=emulator-5554 python3 tests/ui-smoke.py
```

`verify-emulator.py`는 실제 Mac Rust 엔진과 Android JNI 엔진 사이에 adb forward/reverse를 일시 구성하고 해제한다. 수신자 최초 등록 승인, 양방향 파일 최종 저장·바이트 대조, Android 텍스트/URL 수신·키 보관·경로 차단을 검사한다. 테스트 포트/ID는 실제 리스너가 동적으로 제공하며 가짜 온라인 기기를 제품 UI에 넣지 않는다. 실제 Wi-Fi 발견/동일 LAN 실기 상호운용 검증을 대신하지 않는다. `ui-smoke.py`는 실제 에뮬레이터 Wi-Fi를 끊었다 복원하고 앱을 재시작해 설정 유지·기록/탭/사진 메뉴를 확인한다.

의존성이 바뀌면 `PATH="$HOME/.cargo/bin:$PATH" python3 scripts/generate-notices.py` 후 고지 변경을 검토한다. 빌드는 `--check`로 고지가 정확한 Android lockfile 그래프와 일치하는지 확인한다. 자세한 경계·검증 결과는 [Android 설계](../../docs/platforms/android.md)를 따른다.

API 37 에뮬레이터의 자동 GPU에서 셰이더 캐시 대기 ANR이 관찰되어 4코어·소프트웨어 GPU로 재검증했다. 앱 자체의 하드웨어 가속은 유지한다. 같은 환경에서는 AVD 이름을 실제 값으로 지정해 `emulator -avd <AVD 이름> -no-snapshot -cores 4 -memory 4096 -gpu swiftshader -feature -Vulkan`로 실행할 수 있다.

첫 초안 APK와 검증 기록은 `output/android`에 있다. 이전 기본 디자인 수정 기록은 `output/android-native-ui-v2`에 있다. 이전 시스템 설정 기준 UI 0.1.0(3)의 APK·SHA-256·검증 요약과 라이트/다크 세 화면 캡처는 `output/android-settings-ui-v3`에 있다. Android 종합 검사 35개·추가 입력 UI 경계 6개·실제 Wi-Fi 재접속/설정 유지/사진 선택/가상 카메라 준비가 통과했다. Android 실기 및 실제 LAN NSD 상호 발견은 미검증이다.

화면은 SDK 기본 라이트/다크 테마, Toolbar 새로고침 액션, 아이콘과 선택 상태의 하단 탐색, 경계가 있는 EditText, 배경/테두리 작업 버튼·Switch·그룹 표면 목록을 사용한다. 외부 UI/아이콘 라이브러리를 추가하지 않았다. 외관 변경 시 탭을 복원하고 키보드가 열리면 하단 탐색을 숨긴다. 실제 녹색 전송 진행률·최종 저장 완료 피드백과 기존 사진/파일/메시지·신뢰 정책은 유지한다.

실제 Android 시스템 설정 화면을 관찰해 설정 그룹·제목/설명/값·끝쪽 스위치/액션 구조를 적용했다. 기기 이름 행은 기본 편집 대화상자를 열고 빈 값/길이를 검증한다. 계측 검사 `-e settingsUi true`는 기본 편집 대화상자의 현재 값·취소·빈 값/UTF-8 길이 검증·정확한 저장·표시 갱신을 확인한 뒤 원래 이름을 같은 대화상자로 복원한다. `tests/capture-native-ui.py`는 목적 화면과 선택 탐색 항목을 확인한 뒤 두 테마를 캡처한다.

백그라운드 검증은 `ANDROID_SERIAL=<전용 에뮬레이터> python3 tests/verify-background.py`와 `tests/background-policy.py`를 사용한다. 전자는 실제 Service/TLS/최종 저장·Home·화면 꺼짐·32 MiB 양방향·Activity 재생성·키 보관·명시 중지를, 후자는 계측 없는 정상 앱의 권한/OS 중지/유예 만료를 검사한다. 최신 0.1.0(4) APK·해시·기록·상태별 화면은 루트 `output/android-background-v4`에 보관한다. 실기·물리 mDNS·PIN 잠금·제조사 배터리 동작을 검증한 것으로 해석하지 않는다.

0.1.0(5)는 세 페이지의 상단 헤더를 본문과 함께 스크롤하도록 변경한다. 하단 입력/탭바와 기존 디자인은 유지하며 최신 APK·화면·Galaxy S23 업데이트 검증은 `output/android-scroll-v5`에 있다. 전용 에뮬레이터의 `tests/scroll-pages.py`가 창 크기를 원상 복구하며 실제 헤더/하단 좌표·탭별 위치를 검증한다.
