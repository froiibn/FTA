//! Validate observable CLI contracts rather than mirroring the implementation's control flow.

use clap::Parser;
use fta_cli::{Args, request_for, result_exit_code, terminal_safe};
use serde_json::{Value, json};
use std::process::Command;

#[test]
fn stdin_keeps_unicode_and_line_breaks_but_is_bounded() {
    let args = Args::try_parse_from(["fta", "text", "--to", "server", "--stdin"]).unwrap();
    let body = "안녕\nsecond line\n";
    let (method, request) = request_for(&args.command, &mut body.as_bytes()).unwrap();
    assert_eq!(method, "transfers.send");
    assert_eq!(request["body"], body);
    assert_eq!(request["kind"], "text");
    assert!(request["transfer_id"].as_str().is_some());
    let oversized = vec![b'a'; 1024 * 1024 + 1];
    assert!(request_for(&args.command, &mut oversized.as_slice()).is_err());
}

#[test]
fn mutually_exclusive_text_sources_are_rejected() {
    assert!(Args::try_parse_from(["fta", "text", "--to", "server", "body", "--stdin"]).is_err());
}

#[test]
fn send_rejects_directories_and_symbolic_links() {
    // A CLI should not silently turn a folder or symlink into a different payload.
    let directory = tempfile::tempdir().unwrap();
    let args = Args::try_parse_from([
        "fta",
        "send",
        "--to",
        "server",
        directory.path().to_str().unwrap(),
    ])
    .unwrap();
    assert!(request_for(&args.command, &mut &b""[..]).is_err());
    #[cfg(unix)]
    {
        let source = directory.path().join("source");
        std::fs::write(&source, "data").unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(source, &link).unwrap();
        let args = Args::try_parse_from(["fta", "send", "--to", "server", link.to_str().unwrap()])
            .unwrap();
        assert!(request_for(&args.command, &mut &b""[..]).is_err());
    }
}

#[test]
fn errors_under_json_mode_are_machine_readable() {
    let output = Command::new(env!("CARGO_BIN_EXE_fta"))
        .args(["text", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["error"]["code"], "INVALID_REQUEST");
    assert!(output.stderr.is_empty());
}

#[test]
fn status_without_engine_is_json_failure_not_fake_online() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fta"))
        .args([
            "--json",
            "--config-dir",
            directory.path().to_str().unwrap(),
            "--state-dir",
            directory.path().to_str().unwrap(),
            "--runtime-dir",
            directory.path().to_str().unwrap(),
            "status",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(8));
    let parsed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["ok"], false);
}

#[test]
fn final_state_and_terminal_safety_are_preserved() {
    assert_eq!(result_exit_code(&json!({"state":"partial"})), 6);
    assert_eq!(result_exit_code(&json!({"state":"unknown"})), 7);
    assert_eq!(result_exit_code(&json!({"state":"completed"})), 0);
    assert!(!terminal_safe("\u{1b}[2J\rtext\u{7}").contains(['\u{1b}', '\r', '\u{7}']));
}

#[test]
fn settings_are_normalized_to_engine_types() {
    // Booleans and network lists must not arrive at the engine as arbitrary stringly typed values.
    let args = Args::try_parse_from(["fta", "config", "set", "discovery-enabled", "true"]).unwrap();
    let (_, request) = request_for(&args.command, &mut &b""[..]).unwrap();
    assert_eq!(request, json!({"key":"discovery_enabled","value":true}));
    let args = Args::try_parse_from([
        "fta",
        "config",
        "set",
        "allowed-networks",
        "[\"192.168.1.0/24\"]",
    ])
    .unwrap();
    let (_, request) = request_for(&args.command, &mut &b""[..]).unwrap();
    assert_eq!(request["value"], json!(["192.168.1.0/24"]));
    let args = Args::try_parse_from(["fta", "config", "set", "discovery-enabled", "yes"]).unwrap();
    assert!(request_for(&args.command, &mut &b""[..]).is_err());
}

#[cfg(unix)]
mod private_ipc {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, Instant};

    fn serve_once(
        listener: UnixListener,
        result: Value,
        error: Option<Value>,
    ) -> std::thread::JoinHandle<Value> {
        std::thread::spawn(move || {
            // A finite accept loop makes a malformed CLI invocation fail rather than hang the suite.
            listener.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(error) => panic!("CLI did not connect to the private socket: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut header = [0; 4];
            stream.read_exact(&mut header).unwrap();
            let size = u32::from_be_bytes(header) as usize;
            assert!(size < 2 * 1024 * 1024);
            let mut body = vec![0; size];
            stream.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let response = json!({"request_id":request["request_id"],"result":if error.is_none() {Some(result)} else {None},"error":error});
            let body = serde_json::to_vec(&response).unwrap();
            stream
                .write_all(&(body.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&body).unwrap();
            request
        })
    }

    #[test]
    fn engine_error_preserves_code_and_cli_exit_status() {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
        let server = serve_once(
            listener,
            Value::Null,
            Some(json!({"code":"OFFLINE","message":"peer is offline"})),
        );
        let output = Command::new(env!("CARGO_BIN_EXE_fta"))
            .args([
                "--json",
                "--runtime-dir",
                directory.path().to_str().unwrap(),
                "text",
                "--to",
                "server",
                "hello",
            ])
            .output()
            .unwrap();
        let request = server.join().unwrap();
        assert_eq!(request["method"], "transfers.send");
        assert_eq!(request["params"]["body"], "hello");
        assert_eq!(output.status.code(), Some(3));
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["error"]["code"], "OFFLINE");
    }

    #[test]
    fn transfer_error_metadata_does_not_replace_the_result_envelope() {
        // Transfer.error is nullable diagnostic metadata; only the outer IPC error is a request failure.
        for (state, diagnostic, exit_code) in [
            ("completed", Value::Null, 0),
            ("partial", json!("HASH_MISMATCH"), 6),
            ("unknown", json!("UNKNOWN"), 7),
            ("failed", json!("HASH_MISMATCH"), 5),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
            let record = json!({
                "transfer_id":"returned-id", "state":state, "error":diagnostic,
                "kind":"text", "body":"원문 유지", "items":[]
            });
            let server = serve_once(listener, record.clone(), None);
            let output = Command::new(env!("CARGO_BIN_EXE_fta"))
                .args([
                    "--json",
                    "--runtime-dir",
                    directory.path().to_str().unwrap(),
                    "text",
                    "--to",
                    "server",
                    "원문 유지",
                ])
                .output()
                .unwrap();
            let request = server.join().unwrap();
            assert_eq!(request["method"], "transfers.send");
            assert_eq!(output.status.code(), Some(exit_code), "state={state}");
            let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(envelope["result"], record, "state={state}");
            assert_eq!(envelope["ok"], exit_code == 0, "state={state}");
            assert!(envelope.get("error").is_none(), "state={state}");
        }
    }

    #[test]
    fn raw_message_export_is_explicit_and_never_overwrites() {
        let directory = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
        let server = serve_once(listener, json!({"body":"\u{1b}[2J한글\n"}), None);
        let destination = directory.path().join("body.txt");
        std::fs::write(&destination, "existing").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_fta"))
            .args([
                "--json",
                "--runtime-dir",
                directory.path().to_str().unwrap(),
                "inbox",
                "show",
                "id",
                "--output",
                destination.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        server.join().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "existing");
        assert!(!output.stdout.contains(&0x1b));
    }
}
