# MCP 설정 예시 생성

저장소 루트에서 `cargo build --release -p fta-mcp`로 빌드한 뒤 다음 명령으로 현재 컴퓨터의 절대 경로를 반영한 설정 예시를 출력한다.

```sh
python3 packaging/mcp/config-example.py --client codex --allow-dir ./share
python3 packaging/mcp/config-example.py --client claude --allow-dir ./share
```

`share`는 사용자가 전송을 허용할 실제 디렉터리여야 한다. `--allow-dir`는 여러 번 지정할 수 있고, 생략하면 파일 전송이 비활성화된다. 별도 설치 바이너리는 `--binary`, 개발·테스트 엔진은 `--config-dir`, `--state-dir`, `--runtime-dir`로 지정한다. 출력만 수행하며 Codex·Claude의 설정 파일을 생성·수정하지 않는다.

등록 명령과 도구 계약, 검증 한계는 [MCP 사용 문서](../../docs/mcp.md)를 참고한다.
