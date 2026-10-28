# Linux 서버 상세 설계안

작성일: 2026-10-25. 화면 없이 터미널로 사용하는 서버 1대를 대상으로 한다. 배포판·CPU·init 시스템은 미확인이다. systemd 기반 사용자 서비스를 우선 설계하고 비-systemd는 foreground 실행을 제공한다.

## 1. 실행과 서비스

`fta` CLI와 `fta-engine`으로 분리한다. Rust로 구현한 CLI와 엔진만 빌드·배포하며 Tauri·TypeScript 화면·WebView·GUI 라이브러리를 의존성에 포함하지 않는다. GUI·X11·Wayland·데스크톱 키링 없이 동작한다. CLI 종료나 SSH 접속 종료가 자동 수신을 중단시키지 않도록 엔진을 사용자 서비스로 관리한다. root로 파일을 수신하지 않는다.

systemd 사용자 서비스 예시이며 실제 설치 경로는 설치기가 치환한다.

```ini
[Unit]
Description=FTA local transfer engine

[Service]
Type=simple
ExecStart=/absolute/install/path/fta-engine serve
Restart=on-failure
RestartSec=3
UMask=0077
NoNewPrivileges=true

[Install]
WantedBy=default.target
```

서비스 설치와 시작은 CLI가 담당하되 실패 원인을 그대로 보여준다. SSH 로그아웃 이후·로그인 전 부팅 시에도 사용자 서비스를 유지하려면 해당 사용자에 대한 lingering 설정이 필요할 수 있으므로 설치 상태에서 이를 검사하고 필요한 관리 명령을 안내한다. 자동으로 root 정책을 바꾸지 않는다. [systemd: loginctl](https://www.freedesktop.org/software/systemd/man/257/loginctl.html)

비-systemd 환경에서는 `fta-engine serve` foreground와 외부 서비스 관리자의 사용 방법을 제공한다. 터미널을 닫아도 계속 실행되는지는 해당 관리자가 책임지며 CLI가 백그라운드 실행에 성공했다고 잘못 표시하지 않는다.

## 2. 경로와 권한

| 항목 | 기준 경로 |
|---|---|
| 설정 | `${XDG_CONFIG_HOME:-~/.config}/fta/config.toml` |
| 상태 DB·인증키 | `${XDG_STATE_HOME:-~/.local/state}/fta/` |
| IPC socket | `$XDG_RUNTIME_DIR/fta/engine.sock` |
| 기본 수신 폴더 | `~/FTA-Inbox` |
| 로그 | systemd journal 또는 foreground stderr |

위 표는 경로 규칙 표기이며 셸의 `~` 확장에 의존해 구현하지 않는다. 사용자 홈과 XDG 경로를 직접 해석한다. 런타임 디렉터리가 없으면 상태 경로 아래 0700 디렉터리를 사용하고 파일 잠금으로 단일 인스턴스를 보장한다. socket과 키는 현재 사용자만 접근 가능하도록 설정한다. 키는 0600 파일을 기본으로 하고 암호화된 홈·디스크 사용 여부는 설치 환경에 따른다. 데스크톱 키링은 필수로 하지 않는다.

설정 수신 폴더는 서비스 사용자에게 쓰기 권한이 있어야 한다. `/srv` 등의 경로를 선택하면 사전 권한 검증을 수행하고 권한을 임의로 전체 개방하지 않는다. SELinux/AppArmor 차단은 진단에서 구분하고 전체 비활성화하지 않는다.

## 3. 서버 사용 흐름

1. SSH에서 설치·수신 폴더·기기 이름·허용 LAN 인터페이스를 설정한다.
2. 서비스를 시작하고 `fta status`로 수신 가능 여부를 확인한다.
3. 최초 송신은 `fta --request-registration`으로 요청하고 수신자는 `fta registration requests`와 `registration accept <session-id>`로 한 번 확인한다. 고급 수동 비교에는 기존 `pair` 명령을 사용할 수 있다.
4. `fta devices`로 온라인 기기를 확인하고 `send`, `text`, `url`로 전송한다.
5. 수신 파일은 자동 저장되고 `inbox`에서 파일 위치·메시지·결과를 확인한다.

URL을 서버 브라우저로 열지 않으며 데스크톱 알림도 요구하지 않는다. 텍스트는 조회·파일 출력으로 사용할 수 있다. 수신 내용의 escape sequence가 터미널 동작을 바꾸지 않도록 기본 출력에서 무해화한다.

## 4. 네트워크와 운영

실제 사용 LAN 인터페이스에 한정해 발견·수신한다. 컨테이너 브리지·VPN·관리 전용 인터페이스를 자동 허용하지 않는다. 서버 방화벽에서 해당 로컬 대역의 TCP 수신 포트와 mDNS를 확인한다. mDNS가 막힌 경우 수동 LAN 주소 등록으로 기능을 유지할 수 있게 한다.

엔진은 네트워크가 아직 준비되지 않아도 실행되고 인터페이스 준비를 감시한다. 서비스 활성 상태와 실제 수신 가능 상태를 구분한다. 로그에는 시각·전송 ID·기기 ID·오류를 기록하고 메시지 본문·개인키는 남기지 않는다. `fta status --json`은 엔진 버전, 리스너 상태, 수신 경로 쓰기 가능 여부, 온라인 기기 수를 출력한다.

## 5. 배포·검증

서버 배포판·CPU·libc를 확인한 뒤 바이너리 호환 기준을 고정한다. 업데이트는 서비스 중지·바이너리 교체·시작으로 수행하고 진행 중 전송 중단을 사전에 알린다. DB 변경은 백업과 스키마 버전을 사용하며 실패 시 구버전이 손상된 상태로 실행되지 않게 한다.

필수 검증: SSH 종료 후 수신, 재부팅 후 로그인 없이 서비스 시작, 사용자 권한·키 권한, stdout JSON 계약과 종료 코드, SIGINT·SIGTERM, 대용량 파일의 메모리 사용, 가용 공간 부족, 읽기 전용 폴더, mDNS 차단 시 수동 연결, Windows·맥 2대와 양방향 전송.

## 최초 전송 시 등록 확인 변경 (2026-10-28)

기본 전송은 감지된 컴퓨터 선택 → 최초 수신자 확인 → 같은 전송 계속 → 이후 즉시 전송으로 변경한다. 일반 MCP 전송은 등록 기기 한정이며, Linux의 명시적인 요청·승인 CLI와 데스크톱 네이티브 수신 확인은 [최초 전송 등록 설계](first-send-registration.md)를 따른다. 이전 실기 검증 기록은 이 변경의 검증을 의미하지 않는다.
