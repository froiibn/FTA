# Linux 서버 설치와 실행

CLI·엔진은 Rust만 사용한다. Tauri, Node.js, WebView, X11 또는 Wayland는 서버 실행에 필요하지 않다.

```sh
cargo build --release -p fta-cli -p fta-engine
./target/release/fta --help
./target/release/fta-engine serve
```

별도 터미널에서 `fta status`, `fta devices`, `fta pair open` 등으로 엔진을 제어한다. 이름만 같은 다른 프로그램과 혼동하지 않도록 설치 시 두 실행 파일을 같은 사용자 소유 디렉터리에 둔다.

최초 등록은 한쪽에서 `fta pair open`으로 표시한 LAN 주소에 다른 기기가 `fta pair connect ADDRESS`로 연결한다. 받는 쪽은 `fta pair status`로 비교값을 확인하고 양쪽에서 같은 비교값을 확인한 다음 `fta pair confirm SESSION_ID`로 확정한다. 등록 시간 초과나 값 불일치 시 다시 등록한다.

systemd 사용자 서비스로 설치하려면:

```sh
fta service install --engine /absolute/path/to/fta-engine
fta service start
fta service status
fta status
```

`service status`는 systemd 프로세스 상태이고 `status`는 엔진 IPC를 통한 실제 상태다. 설치만으로 즉시 시작하지 않는다. `service install`은 실행 파일과 현재 설정·상태·IPC 경로를 사용자 unit에 기록하고 daemon-reload와 enable을 실행한다. 변경한 경로로 운영할 경우 `--config-dir`, `--state-dir`, `--runtime-dir`를 설치 명령에도 지정한다.

SSH 로그아웃 후와 로그인 전 부팅 시에도 유지하려면 해당 사용자에 대해 `loginctl enable-linger`가 필요할 수 있다. 설치 결과의 `lingering_enabled`와 안내를 확인한다. 이 설정은 설치기가 자동 변경하지 않는다. 서버의 systemd 정책에 따라 관리자 권한이 필요할 수 있다.

systemd가 없으면 기존 서비스 관리자에서 `fta-engine serve`를 관리한다. 단순 foreground 실행은 터미널 종료 이후 유지되지 않는다. 엔진이 실제 LAN 주소에서 수신하는지, 수신 폴더가 서비스 사용자에게 쓰기 가능한지, 방화벽이 허용됐는지 확인한다.

## 텍스트와 출력

```sh
printf '%s' '서버에서 보낸 메시지' | fta text --to DEVICE_ID --stdin
fta url --to DEVICE_ID 'https://example.com'
fta inbox --json
fta inbox show TRANSFER_ID
fta inbox show TRANSFER_ID --output ./received-message.txt
```

표준 출력은 JSON으로 직렬화해 수신 본문의 터미널 제어 문자를 실행하지 않는다. `--json`은 `{ "ok": ..., "result": ... }` 또는 `{ "ok": false, "error": ... }` 형태를 출력한다. `--output`은 명시한 새 파일에 원문을 저장하며 기존 파일을 덮어쓰지 않는다. URL을 서버에서 자동 실행하지 않는다.

실제 Linux 부팅·SSH 종료·방화벽 및 Windows·macOS와의 교차 전송은 대상 서버에서 별도 검증해야 한다.
