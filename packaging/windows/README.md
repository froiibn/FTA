# Windows 11 x64 설치와 검증

대상: Windows 11 / AMD Ryzen 5 5600X(x86-64). 이 문서는 Windows PC에서 직접 빌드하는 절차다. Windows 네이티브 검사·설치 파일 생성·설치·첫 실행·트레이 유지와 종료·재실행, Mac 한 대와의 실제 LAN 양방향 파일·해시·동일 이름 보존을 확인했다. GUI 드롭과 나머지 OS 통합 검증은 별도다. [검증 기록](../../docs/implementation-status.md)을 따른다.

## 1. 빌드 PC 준비

설치 파일을 만드는 PC에만 다음 도구가 필요하다. 설치 파일로 앱을 사용하는 다른 Windows PC에는 개발 도구가 필요 없다.

- Node.js 24와 npm: [공식 다운로드](https://nodejs.org/en/download).
- Rust rustup의 **MSVC x64** 도구 모음: [공식 설치](https://www.rust-lang.org/tools/install). 프로젝트의 `rust-toolchain.toml`에 지정한 버전을 사용한다.
- Visual Studio Build Tools의 **Desktop development with C++**, MSVC x64/x86와 Windows SDK: [Tauri Windows 준비 안내](https://v2.tauri.app/start/prerequisites/).
- Microsoft Edge WebView2 Runtime: Windows 11에 있는지 확인한다. 설치 프로그램은 없을 때 다운로드 부트스트래퍼를 사용하므로 최초 설치 시 인터넷이 필요할 수 있다. 앱의 실제 전송은 집 LAN에서 동작한다.

도구 설치 뒤 터미널을 새로 연다. 소스 ZIP을 로컬 쓰기 가능한 폴더에 모두 압축 해제한다. `node_modules`, Mac의 `target`·실행 파일을 복사할 필요는 없다. 빌드 중 의존성과 NSIS 도구를 다운로드하므로 인터넷과 충분한 디스크 공간이 필요하다.

기존 Rust가 GNU 호스트라면 MSVC 도구 모음을 추가하고 이 PowerShell 세션에서만 선택한다. 다른 프로젝트의 Rust 기본 설정은 바꾸지 않는다. 버전은 `rust-toolchain.toml`과 일치시킨다.

```powershell
rustup toolchain install 1.99.1-x86_64-pc-windows-msvc --profile minimal --component rustfmt --component clippy
$env:RUSTUP_TOOLCHAIN = '1.99.1-x86_64-pc-windows-msvc'
```

## 2. 한 번에 검사하고 설치 파일 만들기

압축 해제한 `FTA` 폴더에서 **일반 사용자 PowerShell**을 연다. 관리자 권한으로 앱을 실행하면 Explorer 드롭 테스트에 영향을 줄 수 있다.

```powershell
node --version
npm --version
rustc -vV
powershell.exe -NoProfile -ExecutionPolicy RemoteSigned -File .\packaging\windows\build.ps1
```

위 명령의 실행 정책은 해당 PowerShell 프로세스에만 적용한다. 인터넷에서 받은 ZIP의 차단 표시가 유지되어 스크립트가 거부되면, 확인한 소스 ZIP의 속성 → 차단 해제 후 다시 압축을 해제한다. 조직 정책이 우선 적용되는 환경에서는 허용된 개발 절차를 따른다. 스크립트는 `x86_64-pc-windows-msvc` 호스트를 확인하고 Cargo·데스크톱 검사와 `npm ci`·빌드를 실행한 후 NSIS를 생성한다. 검사가 실패하면 그 자리에서 중단한다.

성공하면 마지막에 출력되는 `output\windows-x64\<UTC 빌드 시각>\` 폴더를 연다.

| 파일 | 용도 |
|---|---|
| `*-setup.exe` | Program Files를 기본값으로 제시하고 다른 폴더 선택을 허용하는 모든 사용자용 x64 설치 프로그램 |
| `fta.exe` | 설치 앱 엔진에 연결하는 진단·CLI 도구 |
| `fta-engine.exe`, `fta-mcp.exe` | 같은 빌드의 엔진·MCP 진단용 실행 파일 |
| `SHA256SUMS.txt` | 배포 파일 무결성 비교값 |
| `build.log`, `build-info.json` | 검사·빌드 결과와 빌드 환경 |

`fta.exe`와 설치 프로그램만 먼저 사용한다. 별도 엔진을 중복 실행하지 않는다. 빌드 폴더와 로그가 존재한다는 것만으로 성공은 아니다. 스크립트의 정상 종료와 설치 프로그램 생성을 확인한다. 해시는 다음 명령으로 `SHA256SUMS.txt`의 같은 파일명 값과 비교한다.

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath '.\실제-설치파일명-setup.exe'
```

직접 빌드한 테스트 패키지에는 배포용 코드 서명을 적용하지 않는다. 배포 게시자 서명 검증은 별도 미완료 항목이다. 설치 파일 형식과 모든 사용자용 설치 방식은 [Tauri NSIS 안내](https://v2.tauri.app/distribute/windows-installer/#install-modes)를 따른다.

`apps/desktop/src-tauri/tauri.windows.conf.json`의 `installMode`는 사용자 요청에 따라 `perMachine`으로 설정한다. 기본 설치 위치를 AppData에서 OS의 Program Files 아래 `FTA`로 바꾸기 위한 선택이다. 기본 NSIS 폴더 선택 페이지를 유지하므로 다른 드라이브·폴더도 지정할 수 있고, 같은 설치 범위의 재설치는 이전 선택을 복원한다. 경로를 하드코딩하거나 이 동작만을 위해 NSIS 템플릿을 복제하지 않는다. Program Files에 실행 파일을 쓰고 모든 사용자용 설치 정보를 등록하므로 설치 시 관리자 승인이 필요하다.

네트워크 자동 설정용 Windows 빌드에는 `fta-network-helper.exe`도 엔진 옆에 동봉한다. 이 파일은 사용자가 집 연결을 확인하고 설정 적용을 요청할 때만 UAC 대상으로 실행하는 일회용 도우미이며, UI·엔진 전체를 관리자 권한으로 실행하지 않는다. 개발 시 같은 빌드 폴더에서 `fta-network-helper.exe --inspect`로 읽기 전용 상태를 확인할 수 있다. 실제 승인·취소와 규칙 적용 검증은 별도로 수행하고 [설정 어댑터의 제한](../../docs/platforms/windows.md#앱-네트워크-설정-어댑터)을 따른다.

## 3. 설치와 첫 실행

1. 전송 중인 기존 FTA가 있다면 완료를 기다리고 트레이 메뉴로 종료한다.
2. 생성된 `*-setup.exe`를 실행하고 설치 관리자 승인을 진행한다. 기본 `C:\Program Files\FTA`를 사용하거나 폴더 선택 화면에서 다른 위치를 지정하고 실제 설치 위치를 기록한다. OS의 Program Files 위치가 다른 환경에서는 해당 OS 경로를 사용한다.
3. 시작 메뉴의 FTA를 일반 사용자 권한으로 실행한다. 설치 위치에 앱과 `fta-engine.exe`, `fta-mcp.exe`가 함께 있는지 확인한다.
4. 설정에서 기기 이름과 쓰기 가능한 수신 폴더를 저장한다. 기본 수신 폴더는 Windows Downloads의 `FTA`다.
5. 알림을 사용하려면 FTA의 알림 설정과 Windows 설정의 시스템 → 알림 → FTA를 확인한다. 집중 모드는 알림 표시를 억제할 수 있다.

기본 수신 주소는 **loopback**이므로 아직 다른 컴퓨터에서 연결할 수 없다. 창의 X 버튼은 종료가 아니라 트레이로 숨기기이며 수신을 유지한다.

진단용 `fta.exe`가 있는 산출물 폴더에서 다음 명령이 성공하는지 확인한다. 이 명령은 설치된 앱이 실행한 사용자 전용 named pipe에 연결한다.

```powershell
.\fta.exe status --json
.\fta.exe config show --json
```

설정·DB·DPAPI 보호 키는 설치 폴더와 별개로 현재 사용자의 `%LOCALAPPDATA%\FTA`에 있다. 문제 보고 때 이 폴더 전체나 키 파일을 공유하지 않는다. 기존 현재 사용자용 설치와 새 모든 사용자용 설치는 설치 정보의 등록 범위가 다르므로, 이전 설치의 자동 이동·설정 이전이 끝났다고 가정하지 않는다. 기존 앱을 종료한 뒤 새 경로와 등록 정보를 확인하고, 이전 설치를 제거할 때 앱 데이터 삭제를 선택하지 않는다. Codex 같은 패키지 앱에서 실행한 이전 설치는 AppData 경로가 패키지별 LocalCache로 리디렉션됐을 수 있으므로 실제 데이터 위치도 확인한다. 이번 설정 변경은 기존 설정·키·수신 파일을 이동하거나 삭제하지 않는다.

## 4. 집 LAN 설정과 최초 기기 등록

Windows와 상대 Mac 모두 설정한다.

1. Windows의 `ipconfig` 또는 설정의 네트워크 정보에서 **현재 집 LAN IPv4 주소·서브넷**을 확인한다. VPN·가상 어댑터 주소와 구분한다.
2. FTA 설정 → 수신 주소에 `이-PC의-LAN-IP:선택한-포트`를 입력한다. 기본 포트는 47321이다. 허용 네트워크에는 실제 집 대역의 CIDR을 입력한다. 예를 들어 네트워크가 `192.168.10.0/24`인 경우만 그 값을 사용한다. 예시를 그대로 복사하지 않는다.
3. 같은 네트워크 기기 자동 발견을 선택하고 **설정 저장 → 서비스 재시작**한다. Mac에도 Mac 자신의 LAN IP와 같은 허용 대역을 설정한다.
4. 집 연결이 Windows의 Private 네트워크인지 확인한다. 방화벽은 FTA 엔진에 대해 Private·집 로컬 대역의 수신만 허용한다. 등록용 TCP 임시 포트와 발견용 UDP 5353도 사용하므로 고정 전송 포트 하나만 허용하면 등록·발견이 실패할 수 있다. 방화벽 전체 해제나 Public 프로필 허용은 필요 없다.
5. 받는 쪽 설정에서 등록 모드를 열고, 보내는 쪽에서 표시된 임시 등록 주소 또는 발견한 고유 기기 ID로 연결한다. 임시 등록 모드는 제한 시간 내에 사용한다. **양쪽 비교값을 확인하고 양쪽에서 등록 확정**한다.
6. 등록된 기기가 온라인으로 표시되는지 확인한다. 이후 수신마다 승인은 요구하지 않는다.

자동 발견만 실패하면 임시 등록 주소로 먼저 확인한다. IP가 바뀌거나 네트워크가 변경되면 설정을 재확인하고 서비스를 재시작한다. 게스트 Wi-Fi의 기기 격리나 절전은 접속을 막을 수 있다.

## 5. Windows ↔ Mac 수동 검증표

각 행을 양방향으로 실행하고 결과·파일명·전송 ID·실행 시각을 기록한다. 먼저 중요하지 않은 테스트 파일을 사용한다.

| 검사 | 실행 방법 | 통과 기준 |
|---|---|---|
| 시작·연결 | 앱 실행 후 CLI `status --json` | `ok: true`, 올바른 수신 경로, 등록 후 온라인 기기 |
| 단일·다중 파일 | Explorer/Finder에서 파일 선택·드롭 | 설정 폴더 자동 저장, 양쪽 완료 기록, 파일 수 일치 |
| 무결성 | 사진·영상·큰 파일을 전송 | 송수신 SHA-256 일치; 바이트 진행과 최종 완료 구분 |
| 이름·빈 파일 | 한글·공백 이름과 0바이트 파일 | 정상 저장·정확한 크기 |
| 충돌 | 같은 파일을 같은 폴더로 다시 보냄 | 기존 파일 보존, 새 파일은 충돌 없는 이름 |
| 텍스트·URL | 한글·여러 줄·HTTPS URL 전송 | 알림 클릭 시 내용 열기, 복사 값 일치; 링크는 클릭한 뒤만 열림 |
| 트레이 | 창 X로 숨기고 상대에서 전송 | 백그라운드 저장·알림, 트레이에서 다시 열림 |
| 알림 재실행 | 수신 알림을 남기고 트레이에서 앱 종료 후 알림 클릭 | 앱 재실행, 해당 기록 열림, 중복 인스턴스 없음 |
| 취소 | 큰 파일 전송 도중 취소 | 취소·부분 성공을 사실대로 표시, 미완성 파일을 완료 파일로 노출하지 않음 |
| 오프라인 | 상대 앱을 완전히 종료하고 전송 시도 | 실패·오프라인 표시; 나중 자동 전달하지 않음 |
| 등록 해제 | 등록 해제 후 상대에서 다시 전송 | 신뢰 실패; 같은 IP·이름만으로 자동 등록되지 않음 |
| 네트워크 변화 | 전송 없는 상태에서 어댑터·주소 변경 | 전송 차단과 설정 재확인 안내, 무단 네트워크 전환 없음 |
| 재시작·자동 실행 | 옵션 활성화 후 재로그인 | 사용자 세션에서 실행·수신; 로그인 전 수신은 범위 밖 |

Windows 해시는 `Get-FileHash -Algorithm SHA256 -LiteralPath '실제 파일 경로'`, Mac은 `shasum -a 256 '실제 파일 경로'`로 비교한다. 발신 측 완료 표시만으로 무결성 검증을 대체하지 않는다.

Linux 테스트는 서버 배포판·CPU를 확인하고 [Linux 설치](../linux/README.md)를 진행한 뒤 동일한 파일·텍스트·URL 검사를 추가한다. 전체 4대 12방향 검증은 [검증 계획](../../docs/verification-plan.md)을 따른다.

## 6. MCP와 문제 보고

설치 폴더의 `fta-mcp.exe`를 Codex·Claude stdio 서버로 등록한다. 앱 엔진을 먼저 실행하고 허용 폴더를 지정한다. 실제 등록 명령과 도구 목록은 [MCP 문서](../../docs/mcp.md)를 따른다. 빌드·설치 과정은 클라이언트 설정을 자동 수정하지 않는다.

실패하면 다음을 기록한다: 실패한 검증 행, Windows 버전, `build-info.json`, `build.log`의 첫 오류, CLI 상태, 전송 ID와 오류 코드. 로그에 개인 경로·기기 ID가 포함될 수 있으므로 공유 전에 확인한다. 연결 오류에는 엔진 중복 실행보다 앱 설정의 서비스 재시작을 먼저 사용한다.

업데이트·제거는 전송이 없는 때 수행한다. Windows 설치된 앱에서 제거하고 수신 파일·기존 설정의 보존 여부를 확인한다. 앞선 현재 사용자용 설치·재실행·프로토콜·알림 바로가기 등록과 Private 프로필의 Mac 양방향 전송 기록은 유지한다. 새 모든 사용자용 설치의 Program Files 기본값·사용자 지정 경로·이전 설치에서의 전환은 사용자가 직접 검증한다. 제거·실제 알림 표시와 클릭·로그인 시 자동 실행·Public 프로필 차단 동작도 **아직 Windows 실기 미검증**이며 이 검증표의 결과로 갱신한다.
