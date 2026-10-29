//! A narrow C ABI embeds the existing engine; file bytes never cross this JSON control boundary.
use fta_core::{
    Error, Result,
    config::{AppPaths, Config},
    engine::Engine,
    model::Device,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    ffi::{CStr, CString, c_char},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::runtime::Runtime;
struct Inflight(Arc<AtomicUsize>);
impl Drop for Inflight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Host {
    inflight: Arc<AtomicUsize>,
    runtime: Runtime,
    engine: Option<Arc<Engine>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
static HOST: OnceLock<Mutex<Host>> = OnceLock::new();
fn host() -> &'static Mutex<Host> {
    HOST.get_or_init(|| {
        Mutex::new(Host {
            inflight: Arc::new(AtomicUsize::new(0)),
            runtime: Runtime::new().expect("iOS runtime"),
            engine: None,
            tasks: vec![],
        })
    })
}
// Tests use fresh ephemeral identities and never touch the developer's Keychain.
fn open_engine(paths: AppPaths) -> Result<Arc<Engine>> {
    #[cfg(target_os = "ios")]
    {
        // Installation may move the sandbox. Repair owned paths before crash recovery inspects commits.
        let mut config = Config::load_or_create(&paths)?;
        let current = std::fs::canonicalize(&paths.default_receive_dir)?;
        if config.receive_dir != current {
            let store = fta_core::storage::Store::open(&paths)?;
            let old_partials = fta_core::storage::partials_dir(&config.receive_dir)?;
            let new_partials = fta_core::storage::partials_dir(&current)?;
            if old_partials.exists() && old_partials != new_partials {
                if new_partials.exists() {
                    return Err(Error::new(
                        "CONFLICT",
                        "both staging roots exist; recovery required",
                    ));
                }
                std::fs::rename(old_partials, new_partials)?;
            }
            store.relocate_receive_root(&config.receive_dir, &current)?;
            config.receive_dir = current;
            config.save(&paths)?;
        }
    }
    #[cfg(test)]
    {
        Engine::open_with_identity(paths, fta_core::identity::Identity::generate_ephemeral()?)
    }
    #[cfg(not(test))]
    {
        Engine::open(paths)
    }
}
fn field(v: &Value, key: &str) -> Result<String> {
    Ok(fta_core::engine::required(v, key)?.into())
}
fn stop(h: &mut Host) -> Result<Value> {
    if let Some(e) = &h.engine {
        // Cancel sockets before interrupting records; commit reconciliation preserves uncertain outcomes.
        e.suspend_network()?;
        for task in h.tasks.drain(..) {
            task.abort();
        }
    }
    Ok(json!({"stopped":true}))
}
fn start(v: &Value) -> Result<Value> {
    let mut h = host()
        .lock()
        .map_err(|_| Error::new("INTERNAL", "host lock"))?;
    stop(&mut h)?;
    // Do not recover the DB under a previous send still resolving an interrupted socket.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    while h.inflight.load(Ordering::Acquire) != 0
        || h.engine
            .as_ref()
            .is_some_and(|engine| Arc::strong_count(engine) > 1)
    {
        if std::time::Instant::now() >= deadline {
            return Err(Error::new(
                "BUSY",
                "previous transfer is still stopping; retry receiving",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.engine.take();
    let state = PathBuf::from(field(v, "state_dir")?);
    let receive = PathBuf::from(field(v, "receive_dir")?);
    if !state.is_absolute() || !receive.is_absolute() {
        return Err(Error::new("INVALID_PATH", "absolute sandbox required"));
    }
    let paths = AppPaths {
        config_dir: state.clone(),
        runtime_dir: state.join("run"),
        state_dir: state,
        default_receive_dir: receive,
    };
    #[cfg(target_os = "ios")]
    fta_platform::ios::configure_paths(fta_platform::PlatformPaths {
        config_dir: paths.config_dir.clone(),
        state_dir: paths.state_dir.clone(),
        runtime_dir: paths.runtime_dir.clone(),
        receive_dir: paths.default_receive_dir.clone(),
    })?;
    let mut config = Config::load_or_create(&paths)?;
    config.display_name = field(v, "display_name")?;
    config.receive_dir = paths.default_receive_dir.clone();
    config.listen_addr = field(v, "listen_addr")?
        .parse()
        .map_err(|_| Error::new("INVALID_REQUEST", "invalid address"))?;
    config.allowed_networks = serde_json::from_value(v["allowed_networks"].clone())?;
    config.discovery_enabled = true;
    config.save(&paths)?;
    let engine = open_engine(paths)?;
    let e = engine.clone();
    let listener = h.runtime.block_on(async {
        tokio::net::TcpListener::bind(e.config()?.listen_addr)
            .await
            .map_err(Error::from)
    })?;
    engine
        .config
        .write()
        .map_err(|_| Error::new("INTERNAL", "config lock"))?
        .listen_addr = listener.local_addr()?;
    let e = engine.clone();
    let task = h.runtime.spawn(async move {
        let result = fta_core::network::serve(e.clone(), listener, false).await;
        if result.is_err() {
            let _ = e.suspend_network();
        }
    });
    h.tasks.push(task);
    // Native Bonjour needs the real temporary registration port before it publishes readiness.
    let pairing = h.runtime.block_on(fta_core::network::pair_open(&engine))?;
    let e = engine.clone();
    let task = h.runtime.spawn(async move {
        let _ = fta_core::network::approval::listen(e).await;
    });
    h.tasks.push(task);
    let result = json!({"device_id":engine.config()?.device_id,"address":engine.config()?.listen_addr.to_string(),"pairing_address":pairing["address"]});
    h.engine = Some(engine);
    Ok(result)
}
fn invoke(v: Value) -> Result<Value> {
    let method = field(&v, "method")?;
    let mut params = v.get("params").cloned().unwrap_or(json!({}));
    if method == "host.prepare" {
        let mut h = host()
            .lock()
            .map_err(|_| Error::new("INTERNAL", "host lock"))?;
        if h.engine.is_none() {
            let state = PathBuf::from(field(&params, "state_dir")?);
            let receive = PathBuf::from(field(&params, "receive_dir")?);
            if !state.is_absolute() || !receive.is_absolute() {
                return Err(Error::new("INVALID_PATH", "absolute sandbox required"));
            }
            let paths = AppPaths {
                config_dir: state.clone(),
                runtime_dir: state.join("run"),
                state_dir: state,
                default_receive_dir: receive,
            };
            #[cfg(target_os = "ios")]
            fta_platform::ios::configure_paths(fta_platform::PlatformPaths {
                config_dir: paths.config_dir.clone(),
                state_dir: paths.state_dir.clone(),
                runtime_dir: paths.runtime_dir.clone(),
                receive_dir: paths.default_receive_dir.clone(),
            })?;
            let e = open_engine(paths)?;
            e.suspend_network()?;
            h.engine = Some(e);
        }
        return Ok(json!({"prepared":true}));
    }
    if method == "host.start" {
        return start(&params);
    }
    if method == "host.stop" {
        let mut h = host()
            .lock()
            .map_err(|_| Error::new("INTERNAL", "host lock"))?;
        return stop(&mut h);
    }
    // Release the host mutex before awaiting send: approval, cancel and progress must remain concurrent.
    let (engine, handle, _inflight) = {
        let h = host()
            .lock()
            .map_err(|_| Error::new("INTERNAL", "host lock"))?;
        h.inflight.fetch_add(1, Ordering::AcqRel);
        let guard = Inflight(h.inflight.clone());
        (
            h.engine
                .clone()
                .ok_or_else(|| Error::new("OFFLINE", "receiving is paused"))?,
            h.runtime.handle().clone(),
            guard,
        )
    };
    // Selected input copies are the only file grants; a path typo must never send identity/state files.
    if method == "transfers.send_discovered" && params["kind"] == "files" {
        params["allowed_roots"] = json!([std::fs::canonicalize(
            engine.paths.state_dir.join("Outgoing")
        )?]);
    }
    match method.as_str() {
        "host.advertisement" => {
            engine.ensure_network()?;
            let c = engine.config()?;
            let sessions = engine
                .sessions
                .lock()
                .map_err(|_| Error::new("INTERNAL", "session lock"))?;
            let address = sessions
                .values()
                .find(|s| {
                    !s.initiated && s.state == "waiting" && s.expires_at >= fta_core::model::now()
                })
                .map(|s| s.address.clone());
            Ok(
                json!({"device_id":c.device_id,"address":c.listen_addr.to_string(),"pairing_address":address}),
            )
        }
        "host.discovered" => {
            let mut device: Device = serde_json::from_value(params["device"].clone())?;
            let c = engine.config()?;
            let addr: std::net::SocketAddr = device
                .address
                .parse()
                .map_err(|_| Error::new("INVALID_REQUEST", "bad discovery address"))?;
            if device.device_id == c.device_id
                || uuid::Uuid::parse_str(&device.device_id).is_err()
                || !c.allows(addr.ip())
            {
                return Err(Error::new("UNTRUSTED", "discovery outside selected LAN"));
            }
            device.certificate.clear();
            device.online = false;
            device.paired = false;
            if params["pairing"].as_bool() == Some(true) {
                engine
                    .pairing_addresses
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "discovery lock"))?
                    .insert(device.device_id, device.address);
            } else {
                engine
                    .discovered
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "discovery lock"))?
                    .insert(device.device_id.clone(), device);
            }
            Ok(json!({}))
        }
        "host.removed" => {
            let id = field(&params, "device_id")?;
            if params["pairing"].as_bool() == Some(true) {
                engine
                    .pairing_addresses
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "discovery lock"))?
                    .remove(&id);
            } else {
                engine
                    .discovered
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "discovery lock"))?
                    .remove(&id);
            }
            Ok(json!({}))
        }
        // Explicit allowlist keeps native UI mistakes away from legacy pairing and arbitrary engine controls.
        "devices.list"
        | "devices.unpair"
        | "registration.requests"
        | "registration.accept"
        | "registration.reject"
        | "transfers.send_discovered"
        | "transfers.get"
        | "transfers.cancel"
        | "inbox.list"
        | "inbox.get"
        | "inbox.delete"
        | "events.list"
        | "events.cursor"
        | "events.ack"
        | "settings.get"
        | "settings.update" => handle.block_on(engine.dispatch(&method, params)),
        _ => Err(Error::new("INVALID_REQUEST", "unsupported mobile command")),
    }
}

/// # Safety
/// `input` must point to a NUL-terminated UTF-8 request for the duration of this call.
/// The caller owns the returned string and must release it exactly once with `fta_ios_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fta_ios_call(input: *const c_char) -> *mut c_char {
    // Catch panics at the ABI boundary so unwinding never crosses into Swift.
    let result = std::panic::catch_unwind(|| {
        if input.is_null() {
            return Err(Error::new("INVALID_REQUEST", "null request"));
        }
        let text = unsafe { CStr::from_ptr(input) }
            .to_str()
            .map_err(|_| Error::new("INVALID_REQUEST", "invalid UTF-8"))?;
        if text.len() > fta_core::config::MAX_CONTROL_BYTES {
            return Err(Error::new("TOO_LARGE", "control request too large"));
        }
        invoke(serde_json::from_str(text)?)
    })
    .unwrap_or_else(|_| Err(Error::new("INTERNAL", "engine operation failed")));
    let value = match result {
        Ok(result) => json!({"result":result}),
        Err(error) => json!({"error":error}),
    };
    CString::new(value.to_string())
        .expect("JSON contains no NUL")
        .into_raw()
}
/// # Safety
/// `pointer` must be a live allocation returned by `fta_ios_call`, released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fta_ios_free(pointer: *mut c_char) {
    if !pointer.is_null() {
        drop(unsafe { CString::from_raw(pointer) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fta_core::{identity::Identity, model::Transfer};
    fn command(method: &str, params: Value) -> Result<Value> {
        invoke(json!({"method":method,"params":params}))
    }

    // Real TLS/file commit plus paused history catch regressions invisible in a Swift screen preview.
    #[test]
    fn mobile_abi_streams_only_granted_files_and_keeps_history_when_paused() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let state = root.join("sender");
        let outgoing = state.join("Outgoing");
        std::fs::create_dir_all(&outgoing).unwrap();
        let start_params = json!({"state_dir":state,"receive_dir":root.join("inbox"),"display_name":"Test phone","listen_addr":"127.0.0.1:0","allowed_networks":["127.0.0.0/8"]});
        command("host.start", start_params).unwrap();
        let (sender, runtime) = {
            let h = host().lock().unwrap();
            (h.engine.clone().unwrap(), h.runtime.handle().clone())
        };
        let peer_paths = AppPaths {
            config_dir: root.join("peer"),
            state_dir: root.join("peer"),
            runtime_dir: root.join("peer/run"),
            default_receive_dir: root.join("peer-inbox"),
        };
        let peer = Engine::open_with_identity(peer_paths, Identity::generate_ephemeral().unwrap())
            .unwrap();
        let listener =
            runtime.block_on(async { tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap() });
        peer.config.write().unwrap().listen_addr = listener.local_addr().unwrap();
        sender
            .store
            .save_device(&peer.own_device().unwrap())
            .unwrap();
        peer.store
            .save_device(&sender.own_device().unwrap())
            .unwrap();
        let receiver = peer.clone();
        let task =
            runtime.spawn(async move { fta_core::network::serve(receiver, listener, false).await });
        let secret = state.join("private-key-test");
        std::fs::write(&secret, b"never send").unwrap();
        let target = peer.config().unwrap().device_id;
        let denied = command(
            "transfers.send_discovered",
            json!({"to":target,"kind":"files","paths":[secret]}),
        )
        .unwrap_err();
        assert_eq!(denied.code, "PATH_DENIED");
        let source = outgoing.join("한글.txt");
        std::fs::write(&source, b"FTA native bridge payload").unwrap();
        let result: Transfer = serde_json::from_value(
            command(
                "transfers.send_discovered",
                json!({"to":target,"kind":"files","paths":[source]}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(result.state, "completed");
        let received = peer.store.transfer(&result.transfer_id).unwrap();
        assert_eq!(
            std::fs::read(received.items[0].path.as_ref().unwrap()).unwrap(),
            b"FTA native bridge payload"
        );
        assert_eq!(
            sender.store.transfer(&result.transfer_id).unwrap().state,
            "completed"
        );
        assert_eq!(
            command("events.cursor", json!({"consumer":"ios-notifications-v1"})).unwrap()["sequence"],
            0
        );
        command("host.stop", json!({})).unwrap();
        assert!(
            !command("inbox.list", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            command("engine.shutdown", json!({})).unwrap_err().code,
            "INVALID_REQUEST"
        );
        // A pointer returned at the boundary must survive decoding and a single explicit free.
        let pointer = unsafe { fta_ios_call(std::ptr::null()) };
        let value: Value =
            serde_json::from_str(unsafe { CStr::from_ptr(pointer) }.to_str().unwrap()).unwrap();
        unsafe {
            fta_ios_free(pointer);
        }
        assert_eq!(value["error"]["code"], "INVALID_REQUEST");
        task.abort();
        host().lock().unwrap().engine.take();
    }
}
