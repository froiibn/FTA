//! The engine owns state so desktop and headless clients cannot diverge on transfer semantics.
use crate::{
    Error, Result,
    config::{AppPaths, Config},
    identity::Identity,
    model::Device,
    storage::Store,
};
use fs2::FileExt;
use futures_util::{StreamExt, stream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const STATUS_PROBE_CONCURRENCY: usize = 4;
const CANCEL_CONFIRM_MILLIS: u64 = 2000;
const CANCEL_POLL_MILLIS: u64 = 50;
const SHUTDOWN_GRACE_MILLIS: u64 = 2000;
const SHUTDOWN_HISTORY_LIMIT: usize = 1000;
const HOME_NETWORK_POLL_SECONDS: u64 = 2;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};
use tokio::sync::Semaphore;

pub struct Engine {
    pub paths: AppPaths,
    pub config: RwLock<Config>,
    pub identity: Identity,
    pub store: Store,
    pub sessions: Mutex<HashMap<String, crate::network::PairSession>>,
    pub discovered: Mutex<HashMap<String, Device>>,
    pub pairing_addresses: Mutex<HashMap<String, String>>,
    pub network_token: CancellationToken,
    shutdown_token: CancellationToken,
    network_available: AtomicBool,
    pub incoming: Arc<Semaphore>,
    pub outgoing: Arc<Semaphore>,
    pub cancelled: Mutex<std::collections::HashSet<String>>,
    pub active: Mutex<std::collections::HashSet<String>>,
    /// Per-transfer serialization prevents concurrent requests from overwriting one record.
    pub operations: Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
    settings_updates: Mutex<()>,
}
impl Engine {
    /// Production opens protected identity storage; tests may supply a generated in-memory identity.
    pub fn open(paths: AppPaths) -> Result<Arc<Self>> {
        let config = Config::load_or_create(&paths)?;
        config.validate()?;
        let identity = Identity::load(&paths)?;
        Self::from_parts(paths, config, identity)
    }
    pub fn open_with_identity(paths: AppPaths, identity: Identity) -> Result<Arc<Self>> {
        let config = Config::load_or_create(&paths)?;
        config.validate()?;
        Self::from_parts(paths, config, identity)
    }
    fn from_parts(paths: AppPaths, config: Config, identity: Identity) -> Result<Arc<Self>> {
        let store = Store::open(&paths)?;
        store.recover()?;
        Ok(Arc::new(Self {
            paths,
            config: RwLock::new(config),
            identity,
            store,
            sessions: Mutex::new(HashMap::new()),
            discovered: Mutex::new(HashMap::new()),
            pairing_addresses: Mutex::new(HashMap::new()),
            network_token: CancellationToken::new(),
            shutdown_token: CancellationToken::new(),
            network_available: AtomicBool::new(true),
            incoming: Arc::new(Semaphore::new(crate::config::MAX_STREAMS)),
            outgoing: Arc::new(Semaphore::new(crate::config::MAX_STREAMS)),
            cancelled: Mutex::new(Default::default()),
            active: Mutex::new(Default::default()),
            operations: Mutex::new(Default::default()),
            settings_updates: Mutex::new(()),
        }))
    }
    /// A changed home interface remains paused until configuration is confirmed and the engine restarts.
    pub fn network_available(&self) -> bool {
        self.network_available.load(Ordering::Acquire)
    }
    pub fn ensure_network(&self) -> Result<()> {
        if self.network_available() {
            Ok(())
        } else {
            Err(Error::new(
                "NETWORK_CHANGED",
                "home network changed; confirm network settings and restart the engine",
            ))
        }
    }
    pub fn suspend_network(&self) -> Result<()> {
        self.network_available.store(false, Ordering::Release);
        self.network_token.cancel();
        self.store.interrupt_active(None, "NETWORK_CHANGED")
    }
    /// Saved addresses are only candidates: online status requires a fresh authenticated probe.
    async fn registered_devices(self: &Arc<Self>) -> Result<Vec<Device>> {
        let devices = self.store.devices()?;
        let discovered = self
            .discovered
            .lock()
            .map_err(|_| Error::new("INTERNAL", "discovery lock poisoned"))?
            .clone();
        let results = stream::iter(devices)
            .map(|mut device| {
                let discovered = &discovered;
                async move {
                    // DHCP advertisements can suggest a new address, but must never replace a pinned identity.
                    if let Some(candidate) = discovered.get(&device.device_id)
                        && candidate.address != device.address
                    {
                        let mut updated = device.clone();
                        updated.address = candidate.address.clone();
                        if crate::network::probe(self, &updated).await.is_ok() {
                            updated.online = true;
                            updated.paired = true;
                            self.store.save_device(&updated)?;
                            updated.certificate.clear();
                            return Ok(updated);
                        }
                    }
                    device.online = crate::network::probe(self, &device).await.is_ok();
                    device.paired = true;
                    device.certificate.clear();
                    Ok(device)
                }
            })
            .buffered(STATUS_PROBE_CONCURRENCY)
            .collect::<Vec<Result<Device>>>()
            .await;
        results.into_iter().collect()
    }
    /// Completion wins a cancellation race; pending requests are never labelled as confirmed cancellation.
    async fn cancel_transfer(&self, id: &str) -> Result<Value> {
        let record = self.store.transfer(id)?;
        if terminal_state(&record.state) {
            return Ok(serde_json::to_value(record)?);
        }
        self.cancelled
            .lock()
            .map_err(|_| Error::new("INTERNAL", "cancel lock poisoned"))?
            .insert(id.into());
        let deadline = tokio::time::Instant::now() + Duration::from_millis(CANCEL_CONFIRM_MILLIS);
        loop {
            let record = self.store.transfer(id)?;
            if terminal_state(&record.state) {
                return Ok(serde_json::to_value(record)?);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(
                    json!({"transfer_id":id,"state":"cancel_requested","confirmed":false,"message":"cancellation was requested; final receiver result is not yet confirmed"}),
                );
            }
            tokio::time::sleep(Duration::from_millis(CANCEL_POLL_MILLIS)).await;
        }
    }
    pub fn config(&self) -> Result<Config> {
        self.config
            .read()
            .map(|c| c.clone())
            .map_err(|_| Error::new("INTERNAL", "config lock poisoned"))
    }
    pub fn own_device(&self) -> Result<Device> {
        let c = self.config()?;
        Ok(Device {
            device_id: c.device_id,
            display_name: c.display_name,
            os: std::env::consts::OS.into(),
            address: c.listen_addr.to_string(),
            online: true,
            paired: false,
            certificate: hex::encode(&self.identity.cert),
        })
    }
    pub fn is_cancelled(&self, id: &str) -> bool {
        self.cancelled.lock().map_or(true, |c| c.contains(id))
    }
    /// Dispatch uses an explicit allowlist; arbitrary method names never reach a shell or filesystem.
    pub async fn dispatch(self: &Arc<Self>, method: &str, p: Value) -> Result<Value> {
        match method {
            "engine.shutdown" => {
                self.shutdown_token.cancel();
                Ok(json!({"stopping":true}))
            }
            "status" => {
                let c = self.config()?;
                let online = self
                    .registered_devices()
                    .await?
                    .iter()
                    .filter(|device| device.online)
                    .count();
                Ok(
                    json!({"device_id":c.device_id,"display_name":c.display_name,"receive_dir":c.receive_dir,"listen_addr":c.listen_addr,"online_devices":online,"version":env!("CARGO_PKG_VERSION"),"network_available":self.network_available(),"network_reconfirmation_required":!self.network_available(),"network_reason":if self.network_available(){""}else{"Home network interface changed; confirm settings and restart"}}),
                )
            }
            "devices.list" => {
                let mut devices = self.registered_devices().await?;
                let known: std::collections::HashSet<_> =
                    devices.iter().map(|d| d.device_id.clone()).collect();
                let found = self
                    .discovered
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "discovery lock poisoned"))?;
                devices.extend(
                    found
                        .values()
                        .filter(|d| !known.contains(&d.device_id))
                        .cloned(),
                );
                Ok(serde_json::to_value(devices)?)
            }
            "devices.probe" => {
                let d = self.store.device(required(&p, "device_id")?)?;
                crate::network::probe(self, &d).await
            }
            "devices.unpair" => {
                let d = self.store.device(required(&p, "device_id")?)?;
                self.store.unpair(&d.device_id)?;
                self.store
                    .interrupt_active(Some(&d.device_id), "UNTRUSTED")?;
                // A cancelled/unpaired registration must not be resurrected by a delayed poll response.
                self.sessions
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "session lock poisoned"))?
                    .retain(|_, s| {
                        s.peer
                            .as_ref()
                            .is_none_or(|peer| peer.device_id != d.device_id)
                    });
                Ok(json!({"unpaired":d.device_id}))
            }
            "settings.get" => Ok(serde_json::to_value(Config::load_or_create(&self.paths)?)?),
            "settings.update" => {
                // Serialize load-modify-save to preserve independent updates from concurrent clients.
                let _settings_guard = self
                    .settings_updates
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "settings lock poisoned"))?;
                let changes = if let Some(values) = p.get("values").and_then(Value::as_object) {
                    values.clone()
                } else {
                    let key = required(&p, "key")?;
                    let value = p
                        .get("value")
                        .ok_or_else(|| Error::new("INVALID_REQUEST", "missing setting value"))?;
                    serde_json::Map::from_iter([(key.to_string(), value.clone())])
                };
                let mut config = Config::load_or_create(&self.paths)?;
                for (key, value) in &changes {
                    match key.as_str() {
                        "display_name" => {
                            config.display_name = value
                                .as_str()
                                .ok_or_else(|| Error::new("INVALID_REQUEST", "name must be text"))?
                                .into()
                        }
                        "receive_dir" => {
                            let path =
                                std::path::PathBuf::from(value.as_str().ok_or_else(|| {
                                    Error::new("INVALID_REQUEST", "path must be text")
                                })?);
                            if !path.is_absolute() {
                                return Err(Error::new(
                                    "INVALID_REQUEST",
                                    "receive directory must be absolute",
                                ));
                            }
                            crate::storage::receiving_dir(&path)?;
                            config.receive_dir = path;
                        }
                        "listen_addr" => {
                            config.listen_addr = value
                                .as_str()
                                .ok_or_else(|| {
                                    Error::new("INVALID_REQUEST", "address must be text")
                                })?
                                .parse()
                                .map_err(|e: std::net::AddrParseError| {
                                    Error::new("INVALID_REQUEST", e.to_string())
                                })?
                        }
                        "allowed_networks" => {
                            config.allowed_networks = serde_json::from_value(value.clone())?
                        }
                        "discovery_enabled" => {
                            config.discovery_enabled = value
                                .as_bool()
                                .ok_or_else(|| Error::new("INVALID_REQUEST", "expected boolean"))?
                        }
                        _ => return Err(Error::new("INVALID_REQUEST", "unknown setting")),
                    }
                }
                config.save(&self.paths)?;
                // Apply safe live settings without accidentally activating a pending listener change.
                {
                    let mut running = self
                        .config
                        .write()
                        .map_err(|_| Error::new("INTERNAL", "config lock poisoned"))?;
                    running.display_name = config.display_name.clone();
                    running.receive_dir = config.receive_dir.clone();
                }
                let running = self.config()?;
                let restart = running.listen_addr != config.listen_addr
                    || running.allowed_networks != config.allowed_networks
                    || running.discovery_enabled != config.discovery_enabled;
                let mut response = serde_json::to_value(config)?;
                response["restart_required"] = json!(restart);
                Ok(response)
            }
            "inbox.list" => {
                // List only summaries: full message bodies can exceed the IPC frame limit in aggregate.
                let records = self
                    .store
                    .history(p.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize)?;
                let mut result = serde_json::to_value(records)?;
                if let Some(records) = result.as_array_mut() {
                    for record in records {
                        if let Some(record) = record.as_object_mut() {
                            record.remove("body");
                            if let Some(items) =
                                record.get_mut("items").and_then(Value::as_array_mut)
                            {
                                for item in items {
                                    if let Some(item) = item.as_object_mut() {
                                        item.remove("path");
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(result)
            }
            "events.list" => self.store.events_after(
                p.get("after").and_then(Value::as_u64).unwrap_or(0),
                p.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize,
            ),
            "events.cursor" => {
                Ok(json!({"sequence":self.store.event_cursor(required(&p,"consumer")?)?}))
            }
            "events.ack" => {
                let sequence = p
                    .get("sequence")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Error::new("INVALID_REQUEST", "invalid event sequence"))?;
                self.store
                    .acknowledge_events(required(&p, "consumer")?, sequence)?;
                Ok(json!({"sequence":self.store.event_cursor(required(&p,"consumer")?)?}))
            }
            "inbox.get" => Ok(serde_json::to_value(
                self.store.transfer(required(&p, "transfer_id")?)?,
            )?),
            "transfers.get" => crate::network::reconcile(self, required(&p, "transfer_id")?).await,
            "inbox.delete" => {
                self.store.delete_record(required(&p, "transfer_id")?)?;
                Ok(json!({"deleted":true}))
            }
            "transfers.send" => crate::network::send(self, p).await,
            "transfers.cancel" => self.cancel_transfer(required(&p, "transfer_id")?).await,
            "pairing.open" => crate::network::pair_open(self).await,
            "pairing.connect" => crate::network::pair_connect(self, required(&p, "target")?).await,
            "pairing.confirm" => {
                crate::network::pair_confirm(self, required(&p, "session_id")?).await
            }
            "pairing.cancel" => {
                self.sessions
                    .lock()
                    .map_err(|_| Error::new("INTERNAL", "session lock poisoned"))?
                    .remove(required(&p, "session_id")?);
                Ok(json!({"cancelled":true}))
            }
            "pairing.status" => {
                crate::network::pair_status(self, p.get("session_id").and_then(Value::as_str))
            }
            _ => Err(Error::new("INVALID_REQUEST", "unsupported control method")),
        }
    }
}
pub fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new("INVALID_REQUEST", format!("missing string field: {key}")))
}

fn terminal_state(state: &str) -> bool {
    matches!(
        state,
        "completed" | "partial" | "failed" | "cancelled" | "unknown"
    )
}

/// Handle service-manager SIGTERM as well as interactive Ctrl+C so user services clean up IPC.
async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { signal = tokio::signal::ctrl_c() => signal?, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

/// Abort on every error path too: a failed IPC bind must not leave an orphan LAN listener.
struct ListenerTask(tokio::task::JoinHandle<Result<()>>);
impl Drop for ListenerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Discovery has its own worker thread, so dropping the engine must request its shutdown too.
struct DiscoveryTask(Option<mdns_sd::ServiceDaemon>);
impl DiscoveryTask {
    fn shutdown(&mut self) -> Result<()> {
        if let Some(discovery) = self.0.take() {
            discovery
                .shutdown()
                .map_err(|error| Error::new("DISCOVERY_ERROR", error.to_string()))?;
        }
        Ok(())
    }
}
impl Drop for DiscoveryTask {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("discovery shutdown failed: {}", error.code);
        }
    }
}
#[cfg(unix)]
struct SocketCleanup(std::path::PathBuf);
#[cfg(unix)]
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("IPC socket cleanup failed: {}", error.kind());
        }
    }
}

/// Keep a state-directory instance lock for the listener lifetime, regardless of runtime overrides.
pub async fn run(paths: AppPaths) -> Result<()> {
    run_inner(paths, None).await
}

/// Identity injection lets lifecycle tests use the real listeners without accessing a user's key store.
pub async fn run_with_identity(paths: AppPaths, identity: Identity) -> Result<()> {
    run_inner(paths, Some(identity)).await
}

async fn run_inner(paths: AppPaths, identity: Option<Identity>) -> Result<()> {
    #[cfg(target_os = "linux")]
    fta_platform::linux::ensure_unprivileged()?;
    paths.prepare()?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.state_dir.join("engine.lock"))?;
    lock.try_lock_exclusive().map_err(|_| {
        Error::new(
            "ALREADY_RUNNING",
            "an engine already owns this state directory",
        )
    })?;
    let engine = match identity {
        Some(identity) => Engine::open_with_identity(paths.clone(), identity)?,
        None => Engine::open(paths.clone())?,
    };
    let network = tokio::net::TcpListener::bind(engine.config()?.listen_addr).await?;
    // Keep the actual ephemeral address in running state without rewriting the chosen startup port.
    {
        let mut c = engine
            .config
            .write()
            .map_err(|_| Error::new("INTERNAL", "config lock poisoned"))?;
        c.listen_addr = network.local_addr()?;
    }
    let mut discovery = DiscoveryTask(crate::network::start_discovery(engine.clone())?);
    let net_engine = engine.clone();
    let mut network_task = ListenerTask(tokio::spawn(async move {
        crate::network::serve(net_engine, network, false).await
    }));
    let monitored = engine.clone();
    let _network_watcher = ListenerTask(tokio::spawn(async move {
        watch_home_interface(monitored).await
    }));
    let mut network_finished = false;
    let mut controls = tokio::task::JoinSet::new();
    let shutdown_token = engine.shutdown_token.clone();
    let shutdown = async move {
        tokio::select! {result=shutdown_signal()=>result,_=shutdown_token.cancelled()=>Ok(())}
    };
    tokio::pin!(shutdown);
    #[cfg(unix)]
    {
        if paths.socket_path().exists() {
            std::fs::remove_file(paths.socket_path())?;
        }
        let listener = tokio::net::UnixListener::bind(paths.socket_path())?;
        let _socket_cleanup = SocketCleanup(paths.socket_path());
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(paths.socket_path(), std::fs::Permissions::from_mode(0o600))?;
        loop {
            tokio::select! {
                result = listener.accept() => { let (stream, _) = result?; let e = engine.clone(); controls.spawn(async move { crate::ipc::serve(stream, e).await }); },
                signal = &mut shutdown => { signal?; break; },
                result = &mut network_task.0, if !network_finished => {network_finished=true;if engine.network_available(){return Err(listener_failure(result));}},
                Some(result) = controls.join_next() => report_control_result(result),
            }
        }
    }
    #[cfg(windows)]
    {
        let name = paths.pipe_name()?;
        let mut server = fta_platform::windows::create_user_pipe(&name, true)?;
        loop {
            tokio::select! {
                result = server.connect() => { result?; let connected = server; server = fta_platform::windows::create_user_pipe(&name, false)?; let e = engine.clone(); controls.spawn(async move { crate::ipc::serve(connected, e).await }); },
                signal = &mut shutdown => { signal?; break; },
                result = &mut network_task.0, if !network_finished => {network_finished=true;if engine.network_available(){return Err(listener_failure(result));}},
                Some(result) = controls.join_next() => report_control_result(result),
            }
        }
    }
    // Stop new connections before asking in-flight operations to finish or observe cancellation.
    engine.network_token.cancel();
    network_task.0.abort();
    discovery.shutdown()?;
    for record in engine.store.history(SHUTDOWN_HISTORY_LIMIT)? {
        if !terminal_state(&record.state) {
            engine
                .cancelled
                .lock()
                .map_err(|_| Error::new("INTERNAL", "cancel lock poisoned"))?
                .insert(record.transfer_id);
        }
    }
    let grace = tokio::time::sleep(Duration::from_millis(SHUTDOWN_GRACE_MILLIS));
    tokio::pin!(grace);
    while !controls.is_empty() {
        tokio::select! { _ = &mut grace => break, Some(result) = controls.join_next() => report_control_result(result) }
    }
    controls.abort_all();
    engine.store.interrupt_active(None, "INTERRUPTED")?;
    drop(lock);
    Ok(())
}

fn listener_failure(result: std::result::Result<Result<()>, tokio::task::JoinError>) -> Error {
    match result {
        Ok(Err(error)) => error,
        Ok(Ok(())) => Error::new("LISTENER_STOPPED", "LAN listener stopped unexpectedly"),
        Err(error) => Error::new("INTERNAL", error.to_string()),
    }
}

fn report_control_result(result: std::result::Result<Result<()>, tokio::task::JoinError>) {
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("IPC request failed: {}", error.code),
        Err(error) => eprintln!("IPC task failed: {error}"),
    }
}

/// Compare the selected address/interface binding, rather than trusting private CIDRs on a new connection.
fn interface_signature(address: std::net::IpAddr) -> Result<Vec<String>> {
    let mut signature = if_addrs::get_if_addrs()?
        .into_iter()
        .filter(|interface| interface.ip() == address)
        .map(|interface| format!("{}:{:?}", interface.name, interface.addr))
        .collect::<Vec<_>>();
    signature.sort();
    Ok(signature)
}
async fn watch_home_interface(engine: Arc<Engine>) -> Result<()> {
    let address = engine.config()?.listen_addr.ip();
    if address.is_loopback() {
        engine.network_token.cancelled().await;
        return Ok(());
    }
    let baseline = match interface_signature(address) {
        Ok(signature) => signature,
        Err(error) => {
            engine.suspend_network()?;
            return Err(error);
        }
    };
    if baseline.is_empty() {
        engine.suspend_network()?;
        return Ok(());
    }
    loop {
        tokio::select! {_=engine.network_token.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(HOME_NETWORK_POLL_SECONDS))=>{}}
        let observed = match interface_signature(address) {
            Ok(signature) => signature,
            Err(error) => {
                engine.suspend_network()?;
                return Err(error);
            }
        };
        if observed != baseline {
            engine.suspend_network()?;
            return Ok(());
        }
    }
}
