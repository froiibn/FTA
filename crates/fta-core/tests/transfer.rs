//! Real TLS sockets exercise trust and durable file delivery, rather than mocking success responses.
use fta_core::{
    config::{AppPaths, Config},
    engine::Engine,
    model::Device,
    network,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};

async fn instance() -> (
    tempfile::TempDir,
    Arc<Engine>,
    tokio::task::JoinHandle<fta_core::Result<()>>,
) {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        config_dir: dir.path().join("config"),
        state_dir: dir.path().join("state"),
        runtime_dir: dir.path().join("run"),
        default_receive_dir: dir.path().join("inbox"),
    };
    let mut config = Config::load_or_create(&paths).unwrap();
    config.listen_addr.set_port(0);
    config.save(&paths).unwrap();
    let engine = Engine::open_with_identity(
        paths,
        fta_core::identity::Identity::generate_ephemeral().unwrap(),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    engine.config.write().unwrap().listen_addr = listener.local_addr().unwrap();
    let e = engine.clone();
    let task = tokio::spawn(async move { network::serve(e, listener, false).await });
    (dir, engine, task)
}
async fn pair(a: &Arc<Engine>, b: &Arc<Engine>) {
    let open = network::pair_open(b).await.unwrap();
    let connect = network::pair_connect(a, open["address"].as_str().unwrap())
        .await
        .unwrap();
    let id = connect["session_id"].as_str().unwrap();
    let receiver = network::pair_status(b, Some(id)).unwrap();
    assert_eq!(receiver["comparison"], connect["comparison"]);
    network::pair_confirm(a, id).await.unwrap();
    network::pair_confirm(b, id).await.unwrap();
    for _ in 0..50 {
        if !a.store.devices().unwrap().is_empty() && !b.store.devices().unwrap().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("pairing did not finish");
}
fn target(b: &Arc<Engine>) -> Device {
    b.own_device().unwrap()
}
// Names must refresh through authenticated hello even with no discovery event or address change.
#[tokio::test]
async fn renamed_peer_refreshes_registration_without_repairing() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let id = b.own_device().unwrap().device_id;
    let certificate = a.store.device(&id).unwrap().certificate;
    b.dispatch(
        "settings.update",
        json!({"values":{"display_name":"Renamed peer"}}),
    )
    .await
    .unwrap();
    let listed = a.dispatch("devices.list", json!({})).await.unwrap();
    let peer = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|peer| peer["device_id"] == id)
        .unwrap();
    assert_eq!(peer["display_name"], "Renamed peer");
    assert_eq!(peer["online"], true);
    let saved = a.store.device(&id).unwrap();
    assert_eq!(saved.display_name, "Renamed peer");
    assert_eq!(saved.certificate, certificate);
    at.abort();
    bt.abort();
}

#[tokio::test]
async fn paired_file_text_url_and_collision_roundtrip() {
    let (ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let to = target(&b).device_id;
    let source = ad.path().join("사진.txt");
    std::fs::write(&source, "hello\n한글").unwrap();
    let first = network::send(&a, json!({"to":to,"kind":"files","paths":[source]}))
        .await
        .unwrap();
    assert_eq!(first["state"], "completed");
    let second = network::send(&a, json!({"to":to,"kind":"files","paths":[source]}))
        .await
        .unwrap();
    assert_ne!(
        first["items"][0]["saved_name"],
        second["items"][0]["saved_name"]
    );
    let records = b.store.history(10).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        std::fs::read(records[0].items[0].path.as_ref().unwrap()).unwrap(),
        std::fs::read(source).unwrap()
    );
    for (kind, body) in [
        ("text", "메시지\n두 줄"),
        ("url", "https://example.com/a?q=1"),
    ] {
        let result = network::send(&a, json!({"to":to,"kind":kind,"body":body}))
            .await
            .unwrap();
        let record = b
            .store
            .transfer(result["transfer_id"].as_str().unwrap())
            .unwrap();
        assert_eq!(record.body.as_deref(), Some(body));
    }
    at.abort();
    bt.abort();
}
#[tokio::test]
async fn unregistered_and_unpaired_peers_cannot_transfer() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    let mut remote = target(&b);
    remote.paired = true;
    a.store.save_device(&remote).unwrap();
    assert!(network::probe(&a, &remote).await.is_err());
    a.store.unpair(&remote.device_id).unwrap();
    pair(&a, &b).await;
    assert!(network::probe(&a, &remote).await.is_ok());
    b.store.unpair(&a.config().unwrap().device_id).unwrap();
    assert!(network::probe(&a, &remote).await.is_err());
    at.abort();
    bt.abort();
}
#[tokio::test]
async fn batch_settings_are_atomic_and_pending_network_changes_are_preserved() {
    let (_d, e, t) = instance().await;
    e.dispatch("settings.update",json!({"values":{"listen_addr":"127.0.0.2:47322","allowed_networks":["127.0.0.0/8"],"display_name":"new"}})).await.unwrap();
    e.dispatch(
        "settings.update",
        json!({"key":"display_name","value":"newer"}),
    )
    .await
    .unwrap();
    let stored = Config::load_or_create(&e.paths).unwrap();
    assert_eq!(stored.listen_addr.to_string(), "127.0.0.2:47322");
    assert_eq!(stored.display_name, "newer");
    assert_ne!(e.config().unwrap().listen_addr, stored.listen_addr);
    t.abort();
}

// Exercise remote endpoints directly so regression tests can reproduce races and retries independently of the sender UI.
fn client(a: &Arc<Engine>, b: &Arc<Engine>) -> reqwest::Client {
    reqwest::Client::builder()
        .use_preconfigured_tls(
            a.identity
                .client(vec![b.identity.cert.clone()], false)
                .unwrap(),
        )
        .no_proxy()
        .build()
        .unwrap()
}
async fn reserve(
    a: &Arc<Engine>,
    b: &Arc<Engine>,
    name: &str,
    bytes: &[u8],
) -> (reqwest::Client, String, String) {
    let c = client(a, b);
    let id = uuid::Uuid::new_v4().to_string();
    let base = format!("https://{}/v1/transfers", b.config().unwrap().listen_addr);
    let result=c.post(&base).json(&json!({"transfer_id":id,"peer_id":"ignored","direction":"send","kind":"files","state":"sending","created_at":0,"items":[{"name":name,"size":bytes.len().to_string(),"state":"pending"}]})).send().await.unwrap();
    assert!(
        result.status().is_success(),
        "{}",
        result.text().await.unwrap()
    );
    (c, format!("{base}/{id}"), id)
}
#[tokio::test]
async fn large_escaped_message_and_empty_file_are_supported() {
    let (ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let to = b.config().unwrap().device_id;
    // Escaped control characters use more JSON bytes than UTF-8 payload bytes.
    let body = "\u{0001}".repeat(fta_core::config::MAX_MESSAGE_BYTES);
    let result = network::send(&a, json!({"to":to,"kind":"text","body":body}))
        .await
        .unwrap();
    assert_eq!(result["state"], "completed");
    let stored = b
        .store
        .transfer(result["transfer_id"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        stored.body.unwrap().len(),
        fta_core::config::MAX_MESSAGE_BYTES
    );
    let path = ad.path().join("empty.bin");
    std::fs::write(&path, []).unwrap();
    let result = network::send(&a, json!({"to":to,"kind":"files","paths":[path]}))
        .await
        .unwrap();
    assert_eq!(result["state"], "completed");
    at.abort();
    bt.abort();
}
#[tokio::test]
async fn receiver_folder_is_frozen_and_duplicate_commit_does_not_create_extra_files() {
    use sha2::{Digest, Sha256};
    let (_ad, a, at) = instance().await;
    let (bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let original = b.config().unwrap().receive_dir;
    let bytes = b"durable contents";
    let (c, base, id) = reserve(&a, &b, "same.txt", bytes).await;
    let body = c
        .put(format!("{base}/items/0/body"))
        .body(bytes.to_vec())
        .send()
        .await
        .unwrap();
    assert!(body.status().is_success());
    b.dispatch(
        "settings.update",
        json!({"key":"receive_dir","value":bd.path().join("new-inbox")}),
    )
    .await
    .unwrap();
    let hash = hex::encode(Sha256::digest(bytes));
    let (x, y) = tokio::join!(
        c.post(format!("{base}/items/0/commit"))
            .json(&json!({"sha256":hash}))
            .send(),
        c.post(format!("{base}/items/0/commit"))
            .json(&json!({"sha256":hash}))
            .send()
    );
    for response in [x.unwrap(), y.unwrap()] {
        assert!(
            response.status().is_success(),
            "{}",
            response.text().await.unwrap()
        );
    }
    let saved = b.store.transfer(&id).unwrap();
    assert_eq!(saved.state, "completed");
    assert_eq!(std::fs::read(original.join("same.txt")).unwrap(), bytes);
    let cancelled = c
        .delete(&base)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(cancelled["state"], "completed");
    assert!(!original.join("same (1).txt").exists());
    let repeat = c
        .put(format!("{base}/items/0/body"))
        .body(bytes.to_vec())
        .send()
        .await
        .unwrap();
    assert!(!repeat.status().is_success());
    assert_eq!(b.store.transfer(&id).unwrap().state, "completed");

    at.abort();
    bt.abort();
}
#[tokio::test]
async fn incorrect_hash_is_never_committed_and_status_hides_receiver_path() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let (c, base, id) = reserve(&a, &b, "bad.txt", b"abc").await;
    assert!(
        c.put(format!("{base}/items/0/body"))
            .body("abc")
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let result = c
        .post(format!("{base}/items/0/commit"))
        .json(&json!({"sha256":"wrong"}))
        .send()
        .await
        .unwrap();
    assert!(!result.status().is_success());
    assert_ne!(b.store.transfer(&id).unwrap().state, "completed");
    let status = c
        .get(&base)
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert!(status["items"][0]["path"].is_null());
    c.delete(&base).send().await.unwrap();
    at.abort();
    bt.abort();
}

#[tokio::test]
async fn uncertain_sender_result_is_reconciled_without_resending() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let result = network::send(
        &a,
        json!({"to":b.config().unwrap().device_id,"kind":"text","body":"one copy"}),
    )
    .await
    .unwrap();
    let id = result["transfer_id"].as_str().unwrap();
    let mut local = a.store.transfer(id).unwrap();
    local.state = "unknown".into();
    a.store.save_transfer(&local).unwrap();
    let reconciled = a
        .dispatch("transfers.get", json!({"transfer_id":id}))
        .await
        .unwrap();
    assert_eq!(reconciled["state"], "completed");
    assert_eq!(b.store.history(100).unwrap().len(), 1);
    at.abort();
    bt.abort();
}
#[tokio::test]
async fn pairing_rejects_claimed_identity_and_confirmation_from_another_key() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    let open = network::pair_open(&b).await.unwrap();
    let address = open["address"].as_str().unwrap();
    let c = client(&a, &b);
    let mut claimed = a.own_device().unwrap();
    claimed.certificate = hex::encode(
        fta_core::identity::Identity::generate_ephemeral()
            .unwrap()
            .cert
            .clone(),
    );
    let forged = c
        .post(format!("https://{address}/pair/hello"))
        .json(&json!({"device":claimed,"nonce":"test-nonce"}))
        .send()
        .await
        .unwrap();
    assert!(!forged.status().is_success());
    let session = network::pair_connect(&a, address).await.unwrap();
    let id = session["session_id"].as_str().unwrap();
    let attacker = fta_core::identity::Identity::generate_ephemeral().unwrap();
    let attacker = reqwest::Client::builder()
        .use_preconfigured_tls(
            attacker
                .client(vec![b.identity.cert.clone()], false)
                .unwrap(),
        )
        .no_proxy()
        .build()
        .unwrap();
    let confirmation = attacker
        .post(format!("https://{address}/pair/status"))
        .json(&json!({"session_id":id,"confirmed":true}))
        .send()
        .await
        .unwrap();
    assert!(!confirmation.status().is_success());
    assert!(b.store.devices().unwrap().is_empty());
    a.dispatch("pairing.cancel", json!({"session_id":id}))
        .await
        .unwrap();
    b.dispatch("pairing.cancel", json!({"session_id":id}))
        .await
        .unwrap();
    at.abort();
    bt.abort();
}

// Progress must be observable during a real body stream, not synthesized from a final result.
#[tokio::test]
async fn receiver_reports_intermediate_bytes_and_final_count() {
    use bytes::Bytes;
    use sha2::{Digest, Sha256};
    const CHUNKS: usize = 4;
    const CHUNK_BYTES: usize = 64 * 1024;
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let contents = vec![b'x'; CHUNKS * CHUNK_BYTES];
    let (c, base, id) = reserve(&a, &b, "progress.bin", &contents).await;
    let stream = futures_util::stream::unfold(0usize, |index| async move {
        if index == CHUNKS {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        Some((
            Ok::<_, std::io::Error>(Bytes::from(vec![b'x'; CHUNK_BYTES])),
            index + 1,
        ))
    });
    let upload = c
        .put(format!("{base}/items/0/body"))
        .header(reqwest::header::CONTENT_LENGTH, contents.len())
        .body(reqwest::Body::wrap_stream(stream));
    let task = tokio::spawn(async move { upload.send().await });
    tokio::time::sleep(Duration::from_millis(750)).await;
    let progress = b.store.transfer(&id).unwrap();
    let bytes = progress.items[0].bytes_transferred.parse::<u64>().unwrap();
    assert!(
        bytes > 0 && bytes < contents.len() as u64,
        "counter must advance before completion: {bytes}"
    );
    let response = task.await.unwrap().unwrap();
    assert!(response.status().is_success());
    let response = c
        .post(format!("{base}/items/0/commit"))
        .json(&json!({"sha256":hex::encode(Sha256::digest(&contents))}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let completed = b.store.transfer(&id).unwrap();
    assert_eq!(
        completed.items[0].bytes_transferred,
        contents.len().to_string()
    );
    assert_eq!(completed.state, "completed");
    at.abort();
    bt.abort();
}

// Revoking trust must terminate a connected sender even when its body is deliberately idle.
#[tokio::test]
async fn unpair_closes_an_existing_idle_upload_and_preserves_failure() {
    use bytes::Bytes;
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    let (c, base, id) = reserve(&a, &b, "revoked.bin", b"ab").await;
    let stream = futures_util::stream::unfold(0usize, |index| async move {
        match index {
            0 => Some((Ok::<_, std::io::Error>(Bytes::from_static(b"a")), 1)),
            1 => {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Some((Ok(Bytes::from_static(b"b")), 2))
            }
            _ => None,
        }
    });
    let upload = c
        .put(format!("{base}/items/0/body"))
        .header(reqwest::header::CONTENT_LENGTH, 2)
        .body(reqwest::Body::wrap_stream(stream));
    let mut task = tokio::spawn(async move { upload.send().await });
    for _ in 0..100 {
        if b.store.transfer(&id).unwrap().items[0].state == "receiving" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(b.store.transfer(&id).unwrap().items[0].state, "receiving");
    b.dispatch(
        "devices.unpair",
        json!({"device_id":a.config().unwrap().device_id}),
    )
    .await
    .unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(2), &mut task).await;
    assert!(closed.is_ok(), "revoked idle connection was left open");
    let result = closed.unwrap().unwrap();
    assert!(result.is_err() || !result.unwrap().status().is_success());
    assert_eq!(b.store.transfer(&id).unwrap().state, "failed");
    assert!(!b.config().unwrap().receive_dir.join("revoked.bin").exists());
    at.abort();
    bt.abort();
}

// A failed home-network check closes the existing listener and blocks later outbound operations.
#[tokio::test]
async fn changed_home_network_fails_closed_without_disabling_local_status() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    pair(&a, &b).await;
    b.suspend_network().unwrap();
    let stopped = tokio::time::timeout(Duration::from_secs(1), bt)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(stopped.code, "NETWORK_CHANGED");
    let blocked = network::probe(&b, &a.own_device().unwrap())
        .await
        .unwrap_err();
    assert_eq!(blocked.code, "NETWORK_CHANGED");
    let status = b.dispatch("status", json!({})).await.unwrap();
    assert_eq!(status["network_available"], false);
    at.abort();
}
