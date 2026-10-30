//! Local verification fixture only: real first-send approval and final storage across Android JNI and Rust TLS.
use fta_core::{config::AppPaths, engine::Engine, identity::Identity, model::Device};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("fixture output directory required")?);
    let target: Value = serde_json::from_slice(&std::fs::read(
        args.next().ok_or("Android forwarded endpoints required")?,
    )?)?;
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let paths = AppPaths {
        config_dir: root.join("state"),
        state_dir: root.join("state"),
        runtime_dir: root.join("run"),
        default_receive_dir: root.join("received"),
    };
    let engine = Engine::open_with_identity(paths, Identity::generate_ephemeral()?)?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    {
        let mut config = engine.config.write().map_err(|_| "config lock")?;
        config.listen_addr = listener.local_addr()?;
        if target["background"].as_bool().unwrap_or(false) {
            // The Android test-only TCP relay preserves this LAN source while keeping every TLS byte intact.
            let android: std::net::SocketAddr = target["actual_address"]
                .as_str()
                .ok_or("actual LAN endpoint required")?
                .parse()?;
            config.listen_addr.set_ip(android.ip());
            config.allowed_networks.push(
                target["lan_cidr"]
                    .as_str()
                    .ok_or("actual LAN CIDR required")?
                    .parse()?,
            );
        }
        config.display_name = "FTA Mac verification".into();
    }
    let receiver = engine.clone();
    let server =
        tokio::spawn(async move { fta_core::network::serve(receiver, listener, false).await });
    std::fs::write(
        root.join("mac-endpoints.json"),
        serde_json::to_vec(&engine.own_device()?)?,
    )?;
    let start = tokio::time::Instant::now();
    while !root.join("reverse-ready").exists() {
        if start.elapsed() > Duration::from_secs(30) {
            return Err("ADB reverse setup timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let id = target["device_id"]
        .as_str()
        .ok_or("Android device ID required")?
        .to_string();
    engine
        .discovered
        .lock()
        .map_err(|_| "discovery lock")?
        .insert(
            id.clone(),
            Device {
                device_id: id.clone(),
                display_name: "FTA Android verification".into(),
                os: "android".into(),
                address: target["address"]
                    .as_str()
                    .ok_or("Android forwarding address required")?
                    .into(),
                online: false,
                paired: false,
                certificate: String::new(),
            },
        );
    engine
        .pairing_addresses
        .lock()
        .map_err(|_| "pairing lock")?
        .insert(
            id.clone(),
            target["pairing_address"]
                .as_str()
                .ok_or("pair forwarding required")?
                .into(),
        );
    let source = root.join("android-transport-check.txt");
    std::fs::write(&source, "actual TLS to Android ✓")?;
    let mut result = engine
        .dispatch(
            "transfers.send_discovered",
            json!({"to":id,"kind":"files","paths":[source]}),
        )
        .await?;
    if target["background"].as_bool().unwrap_or(false) && result["state"] != "completed" {
        // Registration pins the genuine peer first. Its device-LAN address is not host-routable through emulator NAT;
        // retry the same real file via the verified TCP relay, preserving the certificate and receiver consent.
        let mut registered = engine.store.device(&id)?;
        registered.address = target["address"]
            .as_str()
            .ok_or("forwarded endpoint required")?
            .into();
        engine.store.save_device(&registered)?;
        result = engine
            .dispatch(
                "transfers.send_discovered",
                json!({"to":id,"kind":"files","paths":[root.join("android-transport-check.txt")]}),
            )
            .await?;
    }
    if result["state"] != "completed" {
        return Err(format!("Android final receipt failed: {result}").into());
    }
    if engine.store.device(&id)?.os != "android" {
        return Err("Android did not identify itself as mobile".into());
    }
    let start = tokio::time::Instant::now();
    loop {
        let history = engine.store.history(100)?;
        let reply = history.iter().find(|record| {
            record.direction == "receive"
                && record.state == "completed"
                && record
                    .items
                    .iter()
                    .any(|item| item.name == "android-reply.txt")
        });
        if let Some(reply) = reply {
            let path = reply.items[0]
                .path
                .as_ref()
                .ok_or("committed path required")?;
            if std::fs::read_to_string(path)? != "actual Android JNI stream ✓" {
                return Err("Android reply bytes differ".into());
            }
            println!(
                "Actual Android↔Mac TLS passed: first receiver approval, stored file receipts, exact payloads in both directions"
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(30) {
            return Err("Android reply timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // The Android Service fixture keeps real LAN sockets alive through Home, screen off and Activity recreation.
    if target["background"].as_bool().unwrap_or(false) {
        const LARGE_BYTES: usize = 32 * 1024 * 1024;
        let expected: Vec<u8> = (0..LARGE_BYTES).map(|index| (index % 251) as u8).collect();
        let source = root.join("mac-background.bin");
        std::fs::write(&source, &expected)?;
        let stored = engine
            .dispatch(
                "transfers.send_discovered",
                json!({"to":id,"kind":"files","paths":[source]}),
            )
            .await?;
        if stored["state"] != "completed" {
            return Err("background Android final commit failed".into());
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
        loop {
            let rows = engine.store.history(100)?;
            if let Some(item) = rows
                .iter()
                .filter(|row| row.direction == "receive" && row.state == "completed")
                .flat_map(|row| &row.items)
                .find(|item| item.name == "android-background.bin")
            {
                if std::fs::read(item.path.as_ref().ok_or("background final path required")?)?
                    != expected
                {
                    return Err("background Android byte stream differs".into());
                }
                println!(
                    "Actual background TLS passed: 32 MiB each direction, final receipts and exact bytes"
                );
                break;
            }
            if tokio::time::Instant::now() > deadline {
                return Err("background Android reply timed out".into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    for (kind, body) in [
        ("text", "Android text ✓\n둘째 줄"),
        ("url", "https://example.com/fta?from=android-check"),
    ] {
        let sent = engine
            .dispatch(
                "transfers.send_discovered",
                json!({"to":id,"kind":kind,"body":body}),
            )
            .await?;
        if sent["state"] != "completed" {
            return Err(format!("Android {kind} receipt failed").into());
        }
    }
    println!("Actual Android text/URL final receipts passed");
    server.abort();
    Ok(())
}
