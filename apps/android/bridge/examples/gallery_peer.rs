//! Isolated physical-LAN gallery verification: genuine recipient approval and TLS, with generated pictures only.
use fta_core::{config::AppPaths, engine::Engine, identity::Identity, model::Device};
use serde_json::{Value, json};
use std::{net::SocketAddr, path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let request = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("verification request required")?,
    );
    let input: Value = serde_json::from_slice(&std::fs::read(request)?)?;
    let root = PathBuf::from(input["root"].as_str().ok_or("test root required")?).canonicalize()?;
    let engine = Engine::open_with_identity(
        AppPaths {
            config_dir: root.join("state"),
            state_dir: root.join("state"),
            runtime_dir: root.join("run"),
            default_receive_dir: root.join("received"),
        },
        Identity::generate_ephemeral()?,
    )?;
    // All network coordinates come from actual OS/discovery data, never a fixed device/IP exception.
    let bind: SocketAddr = input["sender_bind"]
        .as_str()
        .ok_or("actual sender interface required")?
        .parse()?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    {
        let mut config = engine.config.write().map_err(|_| "config lock")?;
        config.listen_addr = listener.local_addr()?;
        config.allowed_networks = vec![
            input["lan_cidr"]
                .as_str()
                .ok_or("actual LAN required")?
                .parse()?,
        ];
        config.display_name = "FTA Gallery Verification".into();
        config.discovery_enabled = true;
    }
    let peer: Device = serde_json::from_value(input["target"].clone())?;
    let id = peer.device_id.clone();
    engine
        .discovered
        .lock()
        .map_err(|_| "discovery lock")?
        .insert(id.clone(), peer);
    engine
        .pairing_addresses
        .lock()
        .map_err(|_| "pairing lock")?
        .insert(
            id.clone(),
            input["pairing_address"]
                .as_str()
                .ok_or("discovered pairing endpoint required")?
                .into(),
        );
    let receiver = engine.clone();
    let server =
        tokio::spawn(async move { fta_core::network::serve(receiver, listener, false).await });
    let discovery = fta_core::network::start_discovery(engine.clone())?;
    let supplied: Vec<PathBuf> = serde_json::from_value(input["photos"].clone())?;
    let paths: Vec<PathBuf> = supplied
        .iter()
        .map(std::fs::canonicalize)
        .collect::<std::io::Result<_>>()?;
    if paths.is_empty() || paths.iter().any(|p| !p.starts_with(&root) || !p.is_file()) {
        return Err("only this isolated test's generated photos may be sent".into());
    }
    // The regular first-send protocol must receive real recipient approval before any file body is sent.
    let outcome = engine
        .dispatch(
            "transfers.send_discovered",
            json!({"to":id,"kind":"files","paths":paths}),
        )
        .await?;
    std::fs::write(
        root.join("receipt.json"),
        serde_json::to_vec_pretty(
            &json!({"state":outcome["state"],"transfer_id":outcome["transfer_id"],"file_count":paths.len()}),
        )?,
    )?;
    if outcome["state"] != "completed" {
        return Err("gallery test did not receive final stored confirmation".into());
    }
    // Give the phone time to exercise gallery UI, then tear down this independent peer without touching the user's engine.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    while !root.join("stop").exists() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if let Some(discovery) = discovery {
        let _ = discovery.shutdown();
    }
    server.abort();
    Ok(())
}
