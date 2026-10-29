//! Run with simctl spawn on the compiled iOS-simulator test executable, never the host SQLite.
#![cfg(target_os = "ios")]

use fta_core::{
    config::AppPaths,
    engine::Engine,
    identity::Identity,
    model::Transfer,
    storage::{Store, hash_file, partials_dir},
};
use serde_json::json;
use std::{fs, path::Path};

fn paths(root: &Path, name: &str) -> AppPaths {
    AppPaths {
        config_dir: root.join(name),
        state_dir: root.join(name),
        runtime_dir: root.join(name).join("run"),
        default_receive_dir: root.join(format!("{name}-inbox")),
    }
}

// A single test owns the process-wide iOS platform anchor and exercises real engine/SQLite paths.
#[tokio::test(flavor = "multi_thread")]
async fn system_sqlite_preserves_tls_commits_collisions_reopen_and_crash_recovery() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    fta_platform::ios::configure_container_root(root.clone()).unwrap();
    fta_platform::ios::configure_paths(fta_platform::PlatformPaths {
        config_dir: root.join("platform"),
        state_dir: root.join("platform"),
        runtime_dir: root.join("platform/run"),
        receive_dir: root.join("platform/inbox"),
    })
    .unwrap();
    fs::create_dir_all(root.join("platform")).unwrap();
    assert!(rusqlite::version_number() >= 3_024_000);
    println!("iOS runtime SQLite {}", rusqlite::version());

    let sender = Engine::open_with_identity(
        paths(&root, "sender"),
        Identity::generate_ephemeral().unwrap(),
    )
    .unwrap();
    let receiver_paths = paths(&root, "receiver");
    let receiver = Engine::open_with_identity(
        receiver_paths.clone(),
        Identity::generate_ephemeral().unwrap(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    receiver.config.write().unwrap().listen_addr = listener.local_addr().unwrap();
    sender
        .store
        .save_device(&receiver.own_device().unwrap())
        .unwrap();
    receiver
        .store
        .save_device(&sender.own_device().unwrap())
        .unwrap();
    let incoming = receiver.clone();
    let task =
        tokio::spawn(async move { fta_core::network::serve(incoming, listener, false).await });
    let outgoing = root.join("sender/Outgoing");
    fs::create_dir_all(&outgoing).unwrap();
    let source = outgoing.join("native-sqlite.bin");
    let payload: Vec<u8> = (0..1024 * 1024).map(|index| (index % 251) as u8).collect();
    fs::write(&source, &payload).unwrap();
    let mut received_paths = Vec::new();
    let mut committed = None;
    for _ in 0..2 {
        let sent: Transfer = serde_json::from_value(sender.dispatch("transfers.send_discovered", json!({
            "to":receiver.config().unwrap().device_id,"kind":"files","paths":[source],"allowed_roots":[outgoing]
        })).await.unwrap()).unwrap();
        assert_eq!(sent.state, "completed");
        let received = receiver.store.transfer(&sent.transfer_id).unwrap();
        assert_eq!(received.state, "completed");
        let path = received.items[0].path.as_ref().unwrap();
        assert_eq!(fs::read(path).unwrap(), payload);
        received_paths.push(path.clone());
        committed = Some(received);
    }
    assert_ne!(
        received_paths[0], received_paths[1],
        "A same-name receive must not overwrite the earlier file"
    );
    receiver.suspend_network().unwrap();
    task.abort();
    let _ = task.await;
    let saved = committed.unwrap();
    drop(receiver);
    let store = Store::open(&receiver_paths).unwrap();
    assert_eq!(
        store.transfer(&saved.transfer_id).unwrap().state,
        "completed"
    );
    let connection =
        rusqlite::Connection::open(receiver_paths.state_dir.join("history.sqlite3")).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    let options = connection
        .prepare("PRAGMA compile_options")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    println!("System SQLite compile options: {}", options.join(", "));

    // Recover a durable intent only when publication is the same inode, not coincidentally equal bytes.
    for published in [false, true] {
        let mut transfer = saved.clone();
        transfer.transfer_id = uuid::Uuid::new_v4().to_string();
        transfer.state = "receiving".into();
        transfer.items[0].state = "pending".into();
        transfer.items[0].path = None;
        let destination = root.join(if published {
            "recovery-published"
        } else {
            "recovery-collision"
        });
        store.reserve_receive(&transfer, &destination).unwrap();
        let partial = partials_dir(&destination)
            .unwrap()
            .join(format!("{}-0.part", transfer.transfer_id));
        let target = destination.join("native-sqlite.bin");
        fs::write(&partial, &payload).unwrap();
        if published {
            fs::hard_link(&partial, &target).unwrap();
        } else {
            fs::write(&target, &payload).unwrap();
        }
        transfer.state = "committing".into();
        transfer.items[0].state = "committing".into();
        transfer.items[0].path = Some(target.to_string_lossy().into());
        transfer.items[0].sha256 = Some(hash_file(&partial).unwrap());
        store.save_transfer(&transfer).unwrap();
        store.recover().unwrap();
        assert_eq!(
            store.transfer(&transfer.transfer_id).unwrap().state,
            if published { "completed" } else { "failed" }
        );
        assert_eq!(fs::read(target).unwrap(), payload);
        assert!(!partial.exists());
    }
    assert_eq!(
        connection
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}
