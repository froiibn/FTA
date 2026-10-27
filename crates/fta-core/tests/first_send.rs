//! Consent boundaries use isolated loopback engines; no personal profiles or real LAN peers.
use fta_core::{
    config::{AppPaths, Config},
    engine::Engine,
    identity::Identity,
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
        default_receive_dir: dir.path().join("received"),
    };
    let mut config = Config::load_or_create(&paths).unwrap();
    config.listen_addr.set_port(0);
    config.save(&paths).unwrap();
    let engine =
        Engine::open_with_identity(paths, Identity::generate_ephemeral().unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    engine.config.write().unwrap().listen_addr = listener.local_addr().unwrap();
    let serving = engine.clone();
    let task = tokio::spawn(async move { network::serve(serving, listener, false).await });
    (dir, engine, task)
}

async fn prepare(sender: &Arc<Engine>, receiver: &Arc<Engine>) -> String {
    let peer = receiver.own_device().unwrap();
    let open = network::pair_open(receiver).await.unwrap();
    sender
        .discovered
        .lock()
        .unwrap()
        .insert(peer.device_id.clone(), peer.clone());
    sender.pairing_addresses.lock().unwrap().insert(
        peer.device_id.clone(),
        open["address"].as_str().unwrap().into(),
    );
    peer.device_id
}

async fn incoming(receiver: &Arc<Engine>) -> String {
    for _ in 0..100 {
        let requests = receiver
            .dispatch("registration.requests", json!({}))
            .await
            .unwrap();
        if let Some(id) = requests[0]["session_id"].as_str() {
            return id.into();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("recipient request not observed");
}

// Neither content nor a pin may be saved until the recipient explicitly accepts.
#[tokio::test]
async fn recipient_acceptance_releases_first_send_and_is_not_repeated() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    let target = prepare(&a, &b).await;
    let id = uuid::Uuid::new_v4().to_string();
    let params = json!({"to":target,"transfer_id":id,"kind":"text","body":"first consent"});
    let sending = a.clone();
    let send =
        tokio::spawn(async move { sending.dispatch("transfers.send_discovered", params).await });
    let session = incoming(&b).await;
    assert!(a.store.devices().unwrap().is_empty());
    assert!(b.store.devices().unwrap().is_empty());
    assert!(b.store.history(10).unwrap().is_empty());
    assert_eq!(
        a.dispatch("transfers.get", json!({"transfer_id":id}))
            .await
            .unwrap()["state"],
        "awaiting_registration"
    );
    b.dispatch("registration.accept", json!({"session_id":session}))
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), send)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result["state"], "completed");
    assert_eq!(b.store.history(10).unwrap().len(), 1);
    let second = a
        .dispatch(
            "transfers.send_discovered",
            json!({"to":target,"kind":"text","body":"already registered"}),
        )
        .await
        .unwrap();
    assert_eq!(second["state"], "completed");
    assert!(
        b.dispatch("registration.requests", json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    at.abort();
    bt.abort();
}

// A rejection remains terminal even while the sender's authenticated status poll is in flight.
#[tokio::test]
async fn rejection_never_stores_a_pin_or_payload() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    let target = prepare(&a, &b).await;
    let sending = a.clone();
    let send = tokio::spawn(async move {
        sending
            .dispatch(
                "transfers.send_discovered",
                json!({"to":target,"kind":"text","body":"must not arrive"}),
            )
            .await
    });
    let session = incoming(&b).await;
    b.dispatch("registration.reject", json!({"session_id":session}))
        .await
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), send)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "REGISTRATION_REJECTED");
    assert!(a.store.devices().unwrap().is_empty());
    assert!(b.store.devices().unwrap().is_empty());
    assert!(b.store.history(10).unwrap().is_empty());
    assert!(
        b.dispatch("registration.accept", json!({"session_id":session}))
            .await
            .is_err()
    );
    at.abort();
    bt.abort();
}

// Cancellation works before a transfer row exists; ordinary send/MCP never initiates registration.
#[tokio::test]
async fn cancellation_and_strict_send_preserve_the_trust_boundary() {
    let (_ad, a, at) = instance().await;
    let (_bd, b, bt) = instance().await;
    let target = prepare(&a, &b).await;
    assert_eq!(
        a.dispatch(
            "transfers.send",
            json!({"to":target,"kind":"text","body":"strict"})
        )
        .await
        .unwrap_err()
        .code,
        "UNTRUSTED"
    );
    let id = uuid::Uuid::new_v4().to_string();
    let params = json!({"to":target,"transfer_id":id,"kind":"text","body":"cancelled"});
    let sending = a.clone();
    let send =
        tokio::spawn(async move { sending.dispatch("transfers.send_discovered", params).await });
    incoming(&b).await;
    a.dispatch("transfers.cancel", json!({"transfer_id":id}))
        .await
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), send)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "CANCELLED");
    assert!(a.store.devices().unwrap().is_empty());
    assert!(b.store.devices().unwrap().is_empty());
    assert!(b.store.history(10).unwrap().is_empty());
    at.abort();
    bt.abort();
}
