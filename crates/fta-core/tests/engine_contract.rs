//! Exercise engine contracts without persistent key-store access or a desktop session.

use fta_core::{
    config::{AppPaths, Config},
    engine::Engine,
    identity::Identity,
    model::{Device, Item, Transfer},
    network,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let directory = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        config_dir: directory.path().join("config"),
        state_dir: directory.path().join("state"),
        runtime_dir: directory.path().join("run"),
        default_receive_dir: directory.path().join("inbox"),
    };
    let identity = Identity::generate_ephemeral().unwrap();
    (
        directory,
        Engine::open_with_identity(paths, identity).unwrap(),
    )
}

fn transfer(id: &str, state: &str) -> Transfer {
    Transfer {
        transfer_id: id.into(),
        peer_id: "peer".into(),
        direction: "send".into(),
        kind: "text".into(),
        state: state.into(),
        created_at: fta_core::model::now(),
        body: Some("message".into()),
        items: vec![],
        error: None,
    }
}

#[tokio::test]
async fn online_count_requires_actual_mutual_authentication() {
    let (_a_directory, a) = engine();
    let (_b_directory, b) = engine();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    b.config.write().unwrap().listen_addr = listener.local_addr().unwrap();
    let mut old_address = b.own_device().unwrap();
    old_address.address = "127.0.0.1:1".into();
    a.store.save_device(&old_address).unwrap();
    b.store.save_device(&a.own_device().unwrap()).unwrap();
    let b_server = b.clone();
    let task = tokio::spawn(async move { network::serve(b_server, listener, false).await });
    let mut moved_device = b.own_device().unwrap();
    moved_device.certificate = "an advertisement cannot change this key".into();
    a.discovered
        .lock()
        .unwrap()
        .insert(moved_device.device_id.clone(), moved_device);
    // A discovered advertisement alone is not evidence that a trusted peer is online.
    a.discovered.lock().unwrap().insert(
        "untrusted".into(),
        Device {
            device_id: "untrusted".into(),
            display_name: "untrusted".into(),
            os: "linux".into(),
            address: "127.0.0.1:1".into(),
            online: true,
            paired: false,
            certificate: String::new(),
        },
    );
    let status = a.dispatch("status", json!({})).await.unwrap();
    assert_eq!(status["online_devices"], 1);
    let saved = a.store.device(&b.config().unwrap().device_id).unwrap();
    assert_eq!(saved.address, b.config().unwrap().listen_addr.to_string());
    assert_eq!(saved.certificate, old_address.certificate);
    b.store.unpair(&a.config().unwrap().device_id).unwrap();
    let status = a.dispatch("status", json!({})).await.unwrap();
    assert_eq!(status["online_devices"], 0);
    task.abort();
}

#[tokio::test]
async fn cancelling_completed_transfer_preserves_actual_completion() {
    let (_directory, engine) = engine();
    engine
        .store
        .save_transfer(&transfer("complete", "completed"))
        .unwrap();
    let result = engine
        .dispatch("transfers.cancel", json!({"transfer_id":"complete"}))
        .await
        .unwrap();
    assert_eq!(result["state"], "completed");
    assert!(!engine.is_cancelled("complete"));
    assert_eq!(
        engine.store.transfer("complete").unwrap().body.as_deref(),
        Some("message")
    );
}

#[tokio::test]
async fn cancellation_waits_for_observed_terminal_state() {
    let (_directory, engine) = engine();
    engine
        .store
        .save_transfer(&transfer("running", "sending"))
        .unwrap();
    let writer = engine.clone();
    let writer_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(writer.is_cancelled("running"));
        writer
            .store
            .save_transfer(&transfer("running", "cancelled"))
            .unwrap();
    });
    let result = engine
        .dispatch("transfers.cancel", json!({"transfer_id":"running"}))
        .await
        .unwrap();
    writer_task.await.unwrap();
    assert_eq!(result["state"], "cancelled");
}

#[tokio::test]
async fn stalled_cancellation_reports_request_without_claiming_completion() {
    let (_directory, engine) = engine();
    engine
        .store
        .save_transfer(&transfer("stalled", "sending"))
        .unwrap();
    let result = engine
        .dispatch("transfers.cancel", json!({"transfer_id":"stalled"}))
        .await
        .unwrap();
    assert_eq!(result["state"], "cancel_requested");
    assert_eq!(result["confirmed"], false);
    assert_eq!(engine.store.transfer("stalled").unwrap().state, "sending");
    let missing = engine
        .dispatch("transfers.cancel", json!({"transfer_id":"missing"}))
        .await
        .unwrap_err();
    assert_eq!(missing.code, "NOT_FOUND");
    assert!(!engine.is_cancelled("missing"));
}

#[tokio::test]
async fn inbox_summaries_omit_bulk_text_and_private_paths() {
    let (_directory, engine) = engine();
    let mut record = transfer("large", "completed");
    record.body = Some("a".repeat(1024 * 1024));
    record.items.push(Item {
        bytes_transferred: "0".into(),
        name: "file".into(),
        size: "0".into(),
        state: "completed".into(),
        saved_name: Some("file".into()),
        path: Some("/private/location/file".into()),
        sha256: None,
        error: None,
    });
    engine.store.save_transfer(&record).unwrap();
    let summary = engine
        .dispatch("inbox.list", json!({"limit":20}))
        .await
        .unwrap();
    assert!(summary[0].get("body").is_none());
    assert!(summary[0]["items"][0].get("path").is_none());
    assert!(serde_json::to_vec(&summary).unwrap().len() < 16 * 1024);
    let full = engine
        .dispatch("inbox.get", json!({"transfer_id":"large"}))
        .await
        .unwrap();
    assert_eq!(full["body"].as_str().unwrap().len(), 1024 * 1024);
    assert_eq!(full["items"][0]["path"], "/private/location/file");
}

#[tokio::test]
async fn pending_network_settings_survive_later_live_changes() {
    let (directory, engine) = engine();
    let original_listener = engine.config().unwrap().listen_addr;
    engine
        .dispatch(
            "settings.update",
            json!({"values":{
                "listen_addr":"127.0.0.2:47322", "allowed_networks":["127.0.0.0/8"]
            }}),
        )
        .await
        .unwrap();
    let response = engine
        .dispatch(
            "settings.update",
            json!({"key":"display_name","value":"renamed"}),
        )
        .await
        .unwrap();
    assert_eq!(response["restart_required"], true);
    let saved = Config::load_or_create(&engine.paths).unwrap();
    assert_eq!(saved.listen_addr.to_string(), "127.0.0.2:47322");
    assert_eq!(engine.config().unwrap().listen_addr, original_listener);
    assert_eq!(engine.config().unwrap().display_name, "renamed");
    let invalid = engine.dispatch("settings.update", json!({"values":{
        "display_name":"must-not-save", "receive_dir":directory.path().join("inbox-two"), "allowed_networks":[]
    }})).await;
    assert!(invalid.is_err());
    assert_eq!(
        Config::load_or_create(&engine.paths).unwrap().display_name,
        "renamed"
    );
}

#[cfg(unix)]
mod lifecycle {
    use super::*;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;

    const CHILD_ROOT_ENV: &str = "FTA_TEST_ENGINE_CHILD_ROOT";
    const CHILD_RUNTIME_ENV: &str = "FTA_TEST_ENGINE_CHILD_RUNTIME";
    const CHILD_TIMEOUT: Duration = Duration::from_secs(8);

    struct ChildGuard(Option<Child>);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            // Failed assertions must not leave a real background listener running after the test.
            if let Some(mut child) = self.0.take() {
                if let Err(error) = child.kill()
                    && error.kind() != std::io::ErrorKind::InvalidInput
                {
                    eprintln!("test child cleanup failed: {error}");
                }
                if let Err(error) = child.wait() {
                    eprintln!("test child wait failed: {error}");
                }
            }
        }
    }

    fn child_paths(root: &Path, runtime: &Path) -> AppPaths {
        AppPaths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
            runtime_dir: runtime.into(),
            default_receive_dir: root.join("inbox"),
        }
    }

    fn spawn_child(root: &Path, runtime: &Path) -> ChildGuard {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "lifecycle::engine_lifecycle_fixture",
                "--nocapture",
            ])
            .env(CHILD_ROOT_ENV, root)
            .env(CHILD_RUNTIME_ENV, runtime)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        ChildGuard(Some(child))
    }

    fn wait_ready(child: &mut ChildGuard, paths: &AppPaths) {
        let deadline = Instant::now() + CHILD_TIMEOUT;
        while !paths.socket_path().exists() {
            if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                let output = child.0.take().unwrap().wait_with_output().unwrap();
                panic!(
                    "engine fixture exited before readiness ({status}): {}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert!(
                Instant::now() < deadline,
                "engine fixture startup timed out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // A successful real IPC round-trip proves startup advanced into the signal-aware accept loop.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime
            .block_on(fta_core::ipc::request(paths, "status", json!({})))
            .unwrap();
        assert_eq!(result["online_devices"], 0);
    }

    fn wait_exit(child: &mut ChildGuard) -> std::process::Output {
        let deadline = Instant::now() + CHILD_TIMEOUT;
        loop {
            if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                return child.0.take().unwrap().wait_with_output().unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "engine fixture shutdown timed out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn terminate(child: &ChildGuard) {
        let pid = child.0.as_ref().unwrap().id() as libc::pid_t;
        // SAFETY: the PID belongs to the child we still own; SIGTERM has no memory preconditions.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    }

    #[test]
    fn sigterm_cleans_socket_and_releases_state_lock() {
        #[cfg(target_os = "linux")]
        if fta_platform::linux::ensure_unprivileged().is_err() {
            eprintln!(
                "lifecycle process test requires a non-root user, matching production policy"
            );
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let runtime_dir = directory.path().join("run");
        let paths = child_paths(directory.path(), &runtime_dir);
        let mut config = Config::load_or_create(&paths).unwrap();
        config.listen_addr.set_port(0);
        config.save(&paths).unwrap();
        let mut first = spawn_child(directory.path(), &runtime_dir);
        wait_ready(&mut first, &paths);

        // A different IPC directory must not bypass the lock for the same persistent state.
        let mut duplicate = spawn_child(directory.path(), &directory.path().join("other-run"));
        let output = wait_exit(&mut duplicate);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("ALREADY_RUNNING"));

        terminate(&first);
        let output = wait_exit(&mut first);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!paths.socket_path().exists());

        let mut restarted = spawn_child(directory.path(), &runtime_dir);
        wait_ready(&mut restarted, &paths);
        terminate(&restarted);
        assert!(wait_exit(&mut restarted).status.success());
        assert!(!paths.socket_path().exists());
    }

    #[test]
    #[ignore = "subprocess fixture, executed only by the lifecycle regression test"]
    fn engine_lifecycle_fixture() {
        // Child-only environment configuration avoids unsafe global environment mutation in parallel tests.
        let root =
            std::env::var_os(CHILD_ROOT_ENV).expect("fixture root must be explicitly supplied");
        let runtime_dir = std::env::var_os(CHILD_RUNTIME_ENV)
            .expect("fixture runtime must be explicitly supplied");
        let paths = child_paths(Path::new(&root), Path::new(&runtime_dir));
        let identity = Identity::generate_ephemeral().unwrap();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(fta_core::engine::run_with_identity(paths, identity))
            .expect("fixture engine must shut down cleanly");
    }
}
