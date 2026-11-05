//! Local UI verification only: send real TLS receipts after the iOS recipient explicitly approves registration.
use fta_core::{config::AppPaths, engine::Engine, identity::Identity, model::Device};
use serde_json::{Value, json};
use std::{net::SocketAddr, path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("verification directory required")?);
    let endpoint: Value = serde_json::from_slice(&std::fs::read(
        args.next().ok_or("Bonjour endpoint file required")?,
    )?)?;
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let target: SocketAddr = endpoint["address"]
        .as_str()
        .ok_or("transfer address required")?
        .parse()?;
    let id = endpoint["device_id"]
        .as_str()
        .ok_or("receiver ID required")?
        .to_string();
    let engine = Engine::open_with_identity(
        AppPaths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
            runtime_dir: root.join("run"),
            default_receive_dir: root.join("received"),
        },
        Identity::generate_ephemeral()?,
    )?;
    // Simulator and Mac share the selected private interface; no relay or trust exception is involved.
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(target.ip(), 0)).await?;
    {
        let mut config = engine.config.write().map_err(|_| "configuration lock")?;
        config.display_name = "FTA notification verification".into();
        config.listen_addr = listener.local_addr()?;
        config.allowed_networks.push(
            endpoint["network"]
                .as_str()
                .ok_or("approved LAN required")?
                .parse()?,
        );
    }
    engine
        .discovered
        .lock()
        .map_err(|_| "discovery lock")?
        .insert(
            id.clone(),
            Device {
                device_id: id.clone(),
                display_name: "FTA iOS verification".into(),
                os: "ios".into(),
                address: target.to_string(),
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
            endpoint["pairing_address"]
                .as_str()
                .ok_or("registration address required")?
                .into(),
        );
    let serving = engine.clone();
    let server =
        tokio::spawn(async move { fta_core::network::serve(serving, listener, false).await });
    let source = root.join("FTA-receipt-check.txt");
    std::fs::write(&source, b"FTA receive notification verification")?;
    let result = engine
        .dispatch(
            "transfers.send_discovered",
            json!({"to":id,"kind":"files","paths":[source]}),
        )
        .await?;
    if result["state"] != "completed" {
        return Err("iOS file receipt did not finish".into());
    }
    for (kind, body) in [
        ("text", "FTA notification verification"),
        ("url", "https://github.com/froiibn/FTA"),
    ] {
        let result = engine
            .dispatch(
                "transfers.send_discovered",
                json!({"to":id,"kind":kind,"body":body}),
            )
            .await?;
        if result["state"] != "completed" {
            return Err("iOS message receipt did not finish".into());
        }
    }
    // Keep the genuine peer reachable while the UI checks its receipt banner and record navigation.
    tokio::time::sleep(Duration::from_secs(10)).await;
    server.abort();
    println!("Real iOS TLS file/text/URL final receipts completed after recipient approval");
    Ok(())
}
