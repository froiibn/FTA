//! Launch the real binary: every stdout line must be an MCP JSON-RPC message, never a log.

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{Receiver, channel},
    time::{Duration, Instant},
};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);

struct McpProcess {
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<Value>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl McpProcess {
    fn start(runtime: &Path, roots: &[&Path]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fta-mcp"));
        command
            .arg("--runtime-dir")
            .arg(runtime)
            .arg("--config-dir")
            .arg(runtime.join("config"))
            .arg("--state-dir")
            .arg(runtime.join("state"))
            .env_remove("FTA_MCP_ALLOW_DIRS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for root in roots {
            command.arg("--allow-dir").arg(root);
        }
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (sender, messages) = channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let line = line.unwrap();
                let value: Value =
                    serde_json::from_str(&line).expect("stdout must contain JSON-RPC only");
                assert_eq!(value["jsonrpc"], "2.0");
                if sender.send(value).is_err() {
                    return;
                }
            }
        });
        let mut process = Self {
            child,
            input,
            messages,
            reader: Some(reader),
        };
        process.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-11-25", "capabilities":{}, "clientInfo":{"name":"fta-test","version":"1"}
        }}));
        let initialized = process.receive(1);
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(initialized["result"]["serverInfo"]["name"], "fta-mcp");
        process.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        process
    }

    fn send(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        serde_json::to_writer(&mut *input, &value).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }

    fn receive(&self, id: u64) -> Value {
        let response = self
            .messages
            .recv_timeout(PROCESS_TIMEOUT)
            .expect("MCP response timed out");
        assert_eq!(response["id"], id, "unexpected response: {response}");
        response
    }

    fn call(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}));
        self.receive(id)
    }

    fn finish(mut self) -> String {
        drop(self.input.take());
        let deadline = Instant::now() + PROCESS_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                let mut diagnostic = String::new();
                self.child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut diagnostic)
                    .unwrap();
                self.reader.take().unwrap().join().unwrap();
                assert!(
                    status.success(),
                    "MCP process did not exit cleanly: {diagnostic}"
                );
                return diagnostic;
            }
            assert!(
                Instant::now() < deadline,
                "MCP failed to exit after stdin EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        // An assertion failure must not leave a real stdio server waiting for a missing client.
        if self.child.try_wait().ok().flatten().is_none() {
            if let Err(error) = self.child.kill() {
                eprintln!("MCP test cleanup failed: {error}");
            }
            if let Err(error) = self.child.wait() {
                eprintln!("MCP test wait failed: {error}");
            }
        }
    }
}

#[test]
fn initialization_tools_metadata_and_missing_engine_are_honest() {
    let temp = tempfile::tempdir().unwrap();
    let mut process = McpProcess::start(temp.path(), &[]);
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let response = process.receive(2);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 8);
    let files = tools
        .iter()
        .find(|tool| tool["name"] == "fta_send_files")
        .unwrap();
    assert_eq!(files["annotations"]["readOnlyHint"], false);
    assert_eq!(files["annotations"]["destructiveHint"], false);
    assert_eq!(files["annotations"]["idempotentHint"], false);
    assert_eq!(files["annotations"]["openWorldHint"], true);
    assert_eq!(files["inputSchema"]["additionalProperties"], false);
    assert_eq!(files["outputSchema"]["type"], "object");
    let cancel = tools
        .iter()
        .find(|tool| tool["name"] == "fta_cancel_transfer")
        .unwrap();
    assert_eq!(cancel["annotations"]["destructiveHint"], true);
    let result = process.call(3, "fta_status", json!({}));
    assert_eq!(result["result"]["isError"], true);
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        "SERVICE_UNAVAILABLE"
    );
    assert!(process.finish().is_empty());
}

#[test]
fn disabled_file_access_and_bad_arguments_fail_before_engine_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let physical = temp.path().canonicalize().unwrap();
    let file = physical.join("source.txt");
    std::fs::write(&file, "data").unwrap();
    let mut process = McpProcess::start(temp.path(), &[]);
    let result = process.call(2, "fta_send_files", json!({"to":"peer","paths":[file]}));
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        "FILE_ACCESS_DENIED"
    );
    let result = process.call(
        3,
        "fta_send_text",
        json!({"to":"peer","text":"private-content","arbitrary":"command"}),
    );
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        "INVALID_REQUEST"
    );
    let result = process.call(
        4,
        "fta_send_url",
        json!({"to":"peer","url":"file:///etc/passwd"}),
    );
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        "INVALID_REQUEST"
    );
    let result = process.call(5, "fta_list_inbox", json!({"limit":0}));
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        "INVALID_REQUEST"
    );
    let diagnostic = process.finish();
    assert!(!diagnostic.contains("private-content"));
}

#[test]
fn oversized_and_malformed_lines_do_not_break_later_requests() {
    let temp = tempfile::tempdir().unwrap();
    let mut process = McpProcess::start(temp.path(), &[]);
    let input = process.input.as_mut().unwrap();
    input
        .write_all(&vec![b'x'; fta_core::config::MAX_CONTROL_BYTES + 1])
        .unwrap();
    input.write_all(b"\n{malformed-json}\n").unwrap();
    input.flush().unwrap();
    process.send(json!({"jsonrpc":"2.0","id":2,"method":"ping"}));
    assert!(process.receive(2)["result"].is_object());
    process.send(json!({"jsonrpc":"2.0","id":3,"method":"not/a/method"}));
    assert_eq!(process.receive(3)["error"]["code"], -32601);
    process.finish();
}

#[cfg(unix)]
mod private_ipc {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};

    fn accept(listener: &UnixListener) -> (UnixStream, Value) {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + PROCESS_TIMEOUT;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("MCP did not call private IPC: {error}"),
            }
        };
        // macOS inherits the listener's O_NONBLOCK; framed IPC reads need blocking accepted streams.
        // Keep the listener nonblocking for deadlines and rejection checks without racing client writes.
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(PROCESS_TIMEOUT)).unwrap();
        let mut header = [0; 4];
        stream.read_exact(&mut header).unwrap();
        let length = u32::from_be_bytes(header) as usize;
        assert!(length <= fta_core::config::MAX_CONTROL_BYTES);
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).unwrap();
        (stream, serde_json::from_slice(&bytes).unwrap())
    }

    fn respond(mut stream: UnixStream, request: &Value, result: Value) {
        let response = json!({"request_id":request["request_id"],"result":result,"error":null});
        let bytes = serde_json::to_vec(&response).unwrap();
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(&bytes).unwrap();
    }

    fn online() -> Value {
        json!([{"device_id":"peer-full-id","paired":true,"online":true}])
    }

    #[test]
    fn sends_keep_partial_and_unknown_records_and_never_log_the_body() {
        for (state, error, expected_error) in [
            ("completed", Value::Null, false),
            ("partial", json!("HASH_MISMATCH"), true),
            ("unknown", json!("UNKNOWN"), true),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let listener = UnixListener::bind(temp.path().join("engine.sock")).unwrap();
            let server = std::thread::spawn(move || {
                let (stream, request) = accept(&listener);
                assert_eq!(request["method"], "devices.list");
                respond(stream, &request, online());
                let (stream, request) = accept(&listener);
                assert_eq!(request["method"], "transfers.send");
                assert_eq!(request["params"]["to"], "peer-full-id");
                assert_eq!(request["params"]["body"], "private-message-한글");
                let record = json!({"transfer_id":request["params"]["transfer_id"],"state":state,"error":error,"body":"private-message-한글","items":[]});
                respond(stream, &request, record.clone());
                record
            });
            let mut process = McpProcess::start(temp.path(), &[]);
            let result = process.call(
                2,
                "fta_send_text",
                json!({"to":"peer","text":"private-message-한글"}),
            );
            let record = server.join().unwrap();
            assert_eq!(result["result"]["structuredContent"]["result"], record);
            assert_eq!(result["result"]["isError"], expected_error);
            assert!(!process.finish().contains("private-message"));
        }
    }

    #[test]
    fn unregistered_or_offline_targets_do_not_reach_send() {
        for (paired, online, expected) in [(false, true, "UNTRUSTED"), (true, false, "OFFLINE")] {
            let temp = tempfile::tempdir().unwrap();
            let listener = UnixListener::bind(temp.path().join("engine.sock")).unwrap();
            let server = std::thread::spawn(move || {
                let (stream, request) = accept(&listener);
                respond(
                    stream,
                    &request,
                    json!([{"device_id":"peer","paired":paired,"online":online}]),
                );
                // There must be no subsequent transfer request after the device-list gate rejects it.
                std::thread::sleep(Duration::from_millis(100));
                assert!(listener.accept().is_err());
            });
            let mut process = McpProcess::start(temp.path(), &[]);
            let result = process.call(2, "fta_send_text", json!({"to":"peer","text":"message"}));
            assert_eq!(
                result["result"]["structuredContent"]["error"]["code"],
                expected
            );
            server.join().unwrap();
            process.finish();
        }
    }

    #[test]
    fn allowed_file_paths_and_roots_are_passed_to_the_engine() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("data.txt");
        std::fs::write(&file, "file").unwrap();
        let listener = UnixListener::bind(temp.path().join("engine.sock")).unwrap();
        let expected_file = file.clone();
        let expected_root = root.clone();
        let server = std::thread::spawn(move || {
            let (stream, request) = accept(&listener);
            respond(stream, &request, online());
            let (stream, request) = accept(&listener);
            assert_eq!(request["params"]["paths"], json!([expected_file]));
            assert_eq!(request["params"]["allowed_roots"], json!([expected_root]));
            respond(
                stream,
                &request,
                json!({"state":"completed","error":null,"items":[]}),
            );
        });
        let mut process = McpProcess::start(temp.path(), &[&root]);
        let result = process.call(2, "fta_send_files", json!({"to":"peer","paths":[file]}));
        assert_eq!(result["result"]["isError"], false);
        server.join().unwrap();
        process.finish();
    }

    #[test]
    fn cancellation_reaches_engine_and_suppresses_the_cancelled_rpc_result() {
        let temp = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(temp.path().join("engine.sock")).unwrap();
        let (started_sender, started_receiver) = channel();
        let server = std::thread::spawn(move || {
            let (stream, request) = accept(&listener);
            respond(stream, &request, online());
            let (_pending_stream, request) = accept(&listener);
            let id = request["params"]["transfer_id"].clone();
            started_sender.send(()).unwrap();
            let (stream, cancellation) = accept(&listener);
            assert_eq!(cancellation["method"], "transfers.cancel");
            assert_eq!(cancellation["params"]["transfer_id"], id);
            respond(
                stream,
                &cancellation,
                json!({"transfer_id":id,"state":"cancelled"}),
            );
        });
        let mut process = McpProcess::start(temp.path(), &[]);
        process.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"fta_send_text","arguments":{"to":"peer","text":"long-running"}}}));
        started_receiver.recv_timeout(PROCESS_TIMEOUT).unwrap();
        process.send(
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}),
        );
        server.join().unwrap();
        process.send(json!({"jsonrpc":"2.0","id":3,"method":"ping"}));
        assert!(process.receive(3)["result"].is_object());
        assert!(
            process
                .messages
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
        process.finish();
    }

    #[test]
    fn real_engine_and_tls_receiver_store_file_text_and_url() {
        use fta_core::{
            config::{AppPaths, Config},
            engine::Engine,
            identity::Identity,
            network,
        };
        use std::sync::Arc;

        struct EngineThread {
            stop: Option<tokio::sync::oneshot::Sender<()>>,
            task: Option<std::thread::JoinHandle<()>>,
        }
        impl Drop for EngineThread {
            fn drop(&mut self) {
                // The test owns both engine instances; stop their runtime even if a later assertion fails.
                if let Some(stop) = self.stop.take() {
                    // A closed receiver means the test runtime has already stopped, which needs no action.
                    let _ = stop.send(());
                }
                if let Some(task) = self.task.take() {
                    task.join().expect("test engine thread failed");
                }
            }
        }
        fn instance(root: &Path) -> Arc<Engine> {
            let paths = AppPaths {
                config_dir: root.join("config"),
                state_dir: root.join("state"),
                runtime_dir: root.into(),
                default_receive_dir: root.join("inbox"),
            };
            let mut config = Config::load_or_create(&paths).unwrap();
            config.listen_addr.set_port(0);
            config.save(&paths).unwrap();
            Engine::open_with_identity(paths, Identity::generate_ephemeral().unwrap()).unwrap()
        }

        let temp = tempfile::tempdir().unwrap();
        let physical = temp.path().canonicalize().unwrap();
        let sender_root = physical.join("sender");
        let receiver_root = physical.join("receiver");
        let share = physical.join("share");
        std::fs::create_dir(&share).unwrap();
        let source = share.join("사진 문서.txt");
        let file_body = "실제 전송 파일\n두 번째 줄";
        std::fs::write(&source, file_body).unwrap();
        let outside = physical.join("not-authorized.txt");
        std::fs::write(&outside, "must not send").unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (ready, ready_receiver) = channel();
        let engine_sender_root = sender_root.clone();
        let task = std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(async move {
                let sender = instance(&engine_sender_root); let receiver = instance(&receiver_root);
                let network_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                receiver.config.write().unwrap().listen_addr = network_listener.local_addr().unwrap();
                sender.store.save_device(&receiver.own_device().unwrap()).unwrap();
                receiver.store.save_device(&sender.own_device().unwrap()).unwrap();
                let server_receiver = receiver.clone();
                let network_task = tokio::spawn(async move { network::serve(server_receiver, network_listener, false).await });
                let control = tokio::net::UnixListener::bind(sender.paths.socket_path()).unwrap();
                ready.send(receiver).unwrap();
                let mut requests = tokio::task::JoinSet::new();
                tokio::pin!(stopped);
                loop { tokio::select! {
                    connection = control.accept() => {
                        let (stream, _) = connection.unwrap(); let engine = sender.clone();
                        requests.spawn(async move { fta_core::ipc::serve(stream, engine).await });
                    },
                    _ = &mut stopped => break,
                    Some(result) = requests.join_next() => { result.unwrap().unwrap(); },
                }}
                requests.abort_all(); network_task.abort();
            });
        });
        let engine_thread = EngineThread {
            stop: Some(stop),
            task: Some(task),
        };
        let receiver = ready_receiver.recv_timeout(PROCESS_TIMEOUT).unwrap();
        let target = receiver.config().unwrap().device_id;
        let mut process = McpProcess::start(&sender_root, &[&share]);

        let file = process.call(2, "fta_send_files", json!({"to":target,"paths":[source]}));
        assert_eq!(file["result"]["isError"], false, "{file}");
        let file_id = file["result"]["structuredContent"]["result"]["transfer_id"]
            .as_str()
            .unwrap();
        let saved = receiver.store.transfer(file_id).unwrap();
        assert_eq!(saved.state, "completed");
        assert_eq!(
            std::fs::read_to_string(saved.items[0].path.as_ref().unwrap()).unwrap(),
            file_body
        );
        for (id, name, field, body) in [
            (3, "fta_send_text", "text", "실제 MCP 메시지\n줄바꿈"),
            (
                4,
                "fta_send_url",
                "url",
                "https://example.com/private?q=한글",
            ),
        ] {
            let mut arguments = json!({"to":target});
            arguments[field] = json!(body);
            let result = process.call(id, name, arguments);
            assert_eq!(result["result"]["isError"], false, "{result}");
            let transfer_id = result["result"]["structuredContent"]["result"]["transfer_id"]
                .as_str()
                .unwrap();
            let saved = receiver.store.transfer(transfer_id).unwrap();
            assert_eq!(saved.state, "completed");
            assert_eq!(saved.body.as_deref(), Some(body));
        }
        let denied = process.call(5, "fta_send_files", json!({"to":target,"paths":[outside]}));
        assert_eq!(
            denied["result"]["structuredContent"]["error"]["code"],
            "FILE_ACCESS_DENIED"
        );
        assert_eq!(receiver.store.history(10).unwrap().len(), 3);
        let stderr = process.finish();
        assert!(!stderr.contains("실제 MCP 메시지"));
        assert!(!stderr.contains("private?q"));
        drop(engine_thread);
    }
}
