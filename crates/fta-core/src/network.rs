//! LAN transport authenticates every request and streams bodies outside the WebView and database.
use crate::{
    Error, Result,
    config::*,
    engine::{Engine, required},
    identity::{public_key_fingerprint, same_public_key},
    model::{Device, Item, Transfer, now},
    storage::{aggregate, commit_path, receiving_dir},
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming, service::service_fn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};
use tokio::io::AsyncWriteExt;

// First-send approval reuses the isolated registration transport, never the file receiver.
pub mod approval;

#[cfg(not(target_os = "ios"))]
const SERVICE_TYPE: &str = "_fta._tcp.local.";
const MAX_META: usize = 256 * 1024;
const CHUNK_SIZE: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 32;
#[cfg(not(target_os = "ios"))]
const MAX_DISCOVERED_DEVICES: usize = 1000;
#[cfg(not(target_os = "ios"))]
const PAIR_SERVICE_TYPE: &str = "_fta-pair._tcp.local.";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const REVOCATION_POLL_INTERVAL: Duration = Duration::from_millis(250);
// Registration advertisements may arrive after a click or a stale passive browse cache.
const PAIR_LOOKUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Serialize, Deserialize)]
pub struct PairSession {
    pub session_id: String,
    pub comparison: String,
    pub state: String,
    pub expires_at: u64,
    pub address: String,
    pub local_confirmed: bool,
    pub remote_confirmed: bool,
    pub peer: Option<Device>,
    pub nonce: String,
    pub initiated: bool,
    // Absent on the explicit legacy/CLI comparison flow and on older peers.
    #[serde(default)]
    pub approval: Option<approval::Intent>,
}
fn lock_error() -> Error {
    Error::new("INTERNAL", "state lock poisoned")
}
fn net_error(e: impl std::fmt::Display) -> Error {
    Error::new("OFFLINE", e.to_string())
}
fn active_session(e: &Engine, id: &str) -> Result<PairSession> {
    let session = e
        .sessions
        .lock()
        .map_err(|_| lock_error())?
        .get(id)
        .cloned()
        .ok_or_else(|| Error::new("NOT_FOUND", "registration session not found"))?;
    if session.expires_at < now() {
        return Err(Error::new("TIMEOUT", "registration session expired"));
    }
    Ok(session)
}
fn store_session(e: &Engine, mut s: PairSession) -> Result<PairSession> {
    {
        let mut sessions = e.sessions.lock().map_err(|_| lock_error())?;
        let previous = sessions
            .get(&s.session_id)
            .ok_or_else(|| Error::new("NOT_FOUND", "registration was cancelled"))?;
        // A delayed status response must never resurrect a recipient's rejection/cancellation.
        if previous.state == "rejected" {
            return Err(Error::new(
                "REGISTRATION_REJECTED",
                "registration was rejected or cancelled",
            ));
        }
        // A response started before expiry must not save trust after the approval budget ended.
        if previous.state == "expired" || previous.expires_at < now() {
            return Err(Error::new("TIMEOUT", "registration session expired"));
        }
        s.local_confirmed |= previous.local_confirmed;
        s.remote_confirmed |= previous.remote_confirmed;
        if s.local_confirmed
            && s.remote_confirmed
            && let Some(mut peer) = s.peer.clone()
        {
            peer.paired = true;
            peer.online = true;
            e.store.save_device(&peer)?;
            s.state = "paired".into();
        }
        sessions.insert(s.session_id.clone(), s.clone());
    }
    Ok(s)
}
pub fn pair_status(e: &Engine, id: Option<&str>) -> Result<Value> {
    if let Some(id) = id {
        return Ok(serde_json::to_value(active_session(e, id)?)?);
    }
    Ok(serde_json::to_value(
        e.sessions
            .lock()
            .map_err(|_| lock_error())?
            .values()
            .filter(|s| s.expires_at >= now())
            .cloned()
            .collect::<Vec<_>>(),
    )?)
}
pub async fn pair_open(e: &Arc<Engine>) -> Result<Value> {
    e.ensure_network()?;
    // Background readiness and explicit CLI opens must not create competing listeners.
    let _opening = e.registration.opening.lock().await;
    e.sessions
        .lock()
        .map_err(|_| lock_error())?
        .retain(|_, s| s.expires_at >= now());
    if let Some(waiting) = e
        .sessions
        .lock()
        .map_err(|_| lock_error())?
        .values()
        .find(|s| !s.initiated && s.state == "waiting" && s.expires_at >= now())
        .cloned()
    {
        return Ok(serde_json::to_value(waiting)?);
    }
    if e.sessions
        .lock()
        .map_err(|_| lock_error())?
        .values()
        .any(|s| s.expires_at >= now() && ["waiting", "compare"].contains(&s.state.as_str()))
    {
        return Err(Error::new("BUSY", "a registration session is already open"));
    }
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(e.config()?.listen_addr.ip(), 0)).await?;
    let session = PairSession {
        session_id: uuid::Uuid::new_v4().to_string(),
        comparison: String::new(),
        state: "waiting".into(),
        expires_at: now() + PAIR_SECONDS,
        address: listener.local_addr()?.to_string(),
        local_confirmed: false,
        remote_confirmed: false,
        peer: None,
        nonce: uuid::Uuid::new_v4().to_string(),
        initiated: false,
        approval: None,
    };
    let advertisement = PairAdvertisement(advertise_pairing(e, &session)?);
    e.sessions
        .lock()
        .map_err(|_| lock_error())?
        .insert(session.session_id.clone(), session.clone());
    let engine = e.clone();
    let session_id = session.session_id.clone();
    tokio::spawn(async move {
        let mut advertisement = advertisement;
        let session_engine = engine.clone();
        let monitor = async {
            loop {
                tokio::time::sleep(REVOCATION_POLL_INTERVAL).await;
                let state = session_engine.sessions.lock().ok().and_then(|sessions| {
                    sessions
                        .get(&session_id)
                        .filter(|session| session.expires_at >= now())
                        .map(|session| session.state.clone())
                });
                match state.as_deref() {
                    None => break,
                    Some("paired" | "rejected") => {
                        advertisement.0.take().inspect(|daemon| {
                            let _ = daemon.shutdown();
                        });
                    }
                    _ => {}
                }
            }
        };
        tokio::select! {
            _=serve(engine.clone(),listener,true)=>{},
            // The first-send approval budget begins when a peer connects, not at engine startup.
            _=engine.network_token.cancelled()=>{},
            _=monitor=>{},
        }
    });
    Ok(serde_json::to_value(session)?)
}
fn client(e: &Engine, pins: Vec<Vec<u8>>, pairing: bool) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .use_preconfigured_tls(e.identity.client(pins, pairing)?)
        .tls_info(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(CONNECT_SECONDS))
        .read_timeout(Duration::from_secs(IDLE_SECONDS))
        .http1_only()
        .build()
        .map_err(net_error)
}
// Registration and read-only queries also stop immediately when the selected home interface changes.
async fn network_io<T>(
    e: &Engine,
    operation: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    e.ensure_network()?;
    tokio::select! {
        result=operation=>result,
        _=e.network_token.cancelled()=>Err(Error::new("NETWORK_CHANGED","home network changed during request")),
    }
}
fn allowed_address(e: &Engine, address: &str) -> Result<SocketAddr> {
    e.ensure_network()?;
    let address: SocketAddr = address
        .parse()
        .map_err(|_| Error::new("INVALID_REQUEST", "use a literal LAN IP:port address"))?;
    if !e.config()?.allows(address.ip()) {
        return Err(Error::new(
            "NETWORK_DENIED",
            "address is outside explicitly allowed LAN networks",
        ));
    }
    Ok(address)
}
async fn response_value(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|n| n > MAX_CONTROL_BYTES as u64)
    {
        return Err(Error::new("TOO_LARGE", "response exceeds limit"));
    }
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) =
        tokio::time::timeout(Duration::from_secs(IDLE_SECONDS), response.chunk())
            .await
            .map_err(|_| Error::new("TIMEOUT", "response idle timeout"))?
            .map_err(net_error)?
    {
        if bytes.len() + chunk.len() > MAX_CONTROL_BYTES {
            return Err(Error::new("TOO_LARGE", "response exceeds limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    if !status.is_success() {
        return Err(serde_json::from_value(value)
            .unwrap_or_else(|_| Error::new("REMOTE_ERROR", format!("remote HTTP {status}"))));
    }
    Ok(value)
}
// A cache miss is not proof that the peer's temporary listener is closed.
fn cached_pairing_target(e: &Engine, target: &str) -> Result<Option<String>> {
    let addresses = e.pairing_addresses.lock().map_err(|_| lock_error())?;
    let mut matches = addresses
        .iter()
        .filter(|(id, _)| !target.is_empty() && id.starts_with(target));
    let address = matches.next().map(|(_, address)| address.clone());
    if matches.next().is_some() {
        return Err(Error::new(
            "CONFLICT",
            "device ID matches more than one open registration",
        ));
    }
    Ok(address)
}

// Refresh only registration discovery; never guess the ordinary transfer port or replay /pair/hello.
#[cfg(not(target_os = "ios"))]
async fn resolve_pairing_target(e: &Engine, target: &str) -> Result<String> {
    if target.parse::<SocketAddr>().is_ok() {
        return Ok(target.into());
    }
    if let Some(address) = cached_pairing_target(e, target)? {
        return Ok(address);
    }
    let missing = || {
        Error::new(
            "PAIRING_NOT_FOUND",
            "registration endpoint not discovered; reopen registration on the peer or use its temporary registration address",
        )
    };
    let config = e.config()?;
    if !config.discovery_enabled || config.listen_addr.ip().is_loopback() {
        return Err(missing());
    }
    // A short-lived browser does not replace the running daemon's single listener for this type.
    // The guard stops its worker on success, error, cancellation, and future drop.
    let daemon = discovery_daemon(&config)?;
    let _guard = PairAdvertisement(Some(daemon.clone()));
    let events = daemon.browse(PAIR_SERVICE_TYPE).map_err(net_error)?;
    let deadline = tokio::time::sleep(Duration::from_secs(CONNECT_SECONDS));
    tokio::pin!(deadline);
    let mut poll = tokio::time::interval(PAIR_LOOKUP_POLL_INTERVAL);
    loop {
        e.ensure_network()?;
        if let Some(address) = cached_pairing_target(e, target)? {
            return Ok(address);
        }
        tokio::select! {
            _ = &mut deadline => return Err(missing()),
            _ = e.network_token.cancelled() => return Err(Error::new("NETWORK_CHANGED", "home network changed during registration discovery")),
            _ = poll.tick() => {},
            event = events.recv_async() => {
                let event = event.map_err(net_error)?;
                apply_discovery_event(e, &config, event, true)?;
            }
        }
    }
}

pub async fn pair_connect(e: &Arc<Engine>, target: &str) -> Result<Value> {
    pair_connect_mode(e, target, None).await
}

// Only a send explicitly requesting first-use registration supplies an approval intent.
async fn pair_connect_mode(
    e: &Arc<Engine>,
    target: &str,
    approval: Option<approval::Intent>,
) -> Result<Value> {
    e.ensure_network()?;
    let target = resolve_pairing_target(e, target.trim()).await?;
    let address = allowed_address(e, &target)?;
    let c = client(e, vec![], true)?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let response = network_io(e, async {
        c.post(format!("https://{address}/pair/hello"))
            .json(&json!({"device":e.own_device()?,"nonce":nonce,"approval":approval}))
            .send()
            .await
            .map_err(net_error)
    })
    .await?;
    let observed = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(|i| i.peer_certificate())
        .map(|c| c.to_vec())
        .ok_or_else(|| Error::new("TLS_ERROR", "missing pairing TLS certificate"))?;
    let mut session: PairSession =
        serde_json::from_value(network_io(e, response_value(response)).await?)?;
    let peer = session
        .peer
        .clone()
        .ok_or_else(|| Error::new("INVALID_REQUEST", "missing remote registration identity"))?;
    // A legacy peer must not silently downgrade recipient approval into a different trust flow.
    if session.approval != approval {
        return Err(Error::new(
            "VERSION_UNSUPPORTED",
            "peer does not support first-send registration approval",
        ));
    }
    if hex::decode(&peer.certificate).ok().as_deref() != Some(observed.as_slice()) {
        return Err(Error::new(
            "KEY_CHANGED",
            "pairing identity differs from TLS certificate",
        ));
    }
    let expected = comparison(&e.identity.cert, &observed, &nonce, &session.nonce)?;
    if session.comparison != expected {
        return Err(Error::new(
            "KEY_CHANGED",
            "registration transcript mismatch",
        ));
    }
    session.address = address.to_string();
    session.initiated = true;
    session.local_confirmed = false;
    session.remote_confirmed = false;
    e.sessions
        .lock()
        .map_err(|_| lock_error())?
        .insert(session.session_id.clone(), session.clone());
    // Poll only the bounded registration session, never keep an offline file-delivery queue.
    let engine = e.clone();
    let id = session.session_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _=tokio::time::sleep(Duration::from_secs(1))=>{},
                _=engine.network_token.cancelled()=>break,
            }
            let Ok(mut s) = active_session(&engine, &id) else {
                break;
            };
            if ["paired", "rejected", "expired"].contains(&s.state.as_str()) {
                break;
            }
            let Some(peer) = &s.peer else {
                break;
            };
            let Ok(cert) = hex::decode(&peer.certificate) else {
                break;
            };
            let Ok(c) = client(&engine, vec![cert], false) else {
                break;
            };
            let url = format!("https://{}/pair/status", s.address);
            let request = if s.local_confirmed {
                c.post(url).json(&json!({"session_id":id,"confirmed":true}))
            } else {
                c.post(url)
                    .json(&json!({"session_id":id,"confirmed":false}))
            }
            .send();
            let response = network_io(&engine, async { request.await.map_err(net_error) }).await;
            if let Ok(response) = response {
                match network_io(&engine, response_value(response)).await {
                    Ok(value) => {
                        if value["state"] == "rejected" {
                            s.state = "rejected".into();
                        }
                        s.remote_confirmed = value["local_confirmed"].as_bool().unwrap_or(false);
                        let _ = store_session(&engine, s);
                    }
                    Err(error)
                        if s.approval.is_some()
                            && ["NOT_FOUND", "REGISTRATION_REJECTED"]
                                .contains(&error.code.as_str()) =>
                    {
                        // Explicit rejection/removal is terminal; do not wait until the old deadline.
                        s.state = "rejected".into();
                        let _ = store_session(&engine, s);
                        break;
                    }
                    Err(error) if s.approval.is_some() && error.code == "TIMEOUT" => {
                        s.state = "expired".into();
                        let _ = store_session(&engine, s);
                        break;
                    }
                    Err(_) => {}
                }
            }
        }
    });
    Ok(serde_json::to_value(session)?)
}
pub async fn pair_confirm(e: &Arc<Engine>, id: &str) -> Result<Value> {
    let mut session = active_session(e, id)?;
    if session.state == "rejected" {
        return Err(Error::new(
            "REGISTRATION_REJECTED",
            "registration was rejected or cancelled",
        ));
    }
    if session.peer.is_none() {
        return Err(Error::new(
            "INVALID_REQUEST",
            "wait for the other device to connect before comparing",
        ));
    }
    session.local_confirmed = true;
    Ok(serde_json::to_value(store_session(e, session)?)?)
}
fn comparison(a: &[u8], b: &[u8], client_nonce: &str, server_nonce: &str) -> Result<String> {
    let mut keys = [public_key_fingerprint(a)?, public_key_fingerprint(b)?];
    keys.sort();
    Ok(crate::identity::fingerprint(
        format!(
            "fta-pair-v1:{}:{}:{}:{}",
            keys[0], keys[1], client_nonce, server_nonce
        )
        .as_bytes(),
    ))
}

pub async fn serve(e: Arc<Engine>, listener: tokio::net::TcpListener, pairing: bool) -> Result<()> {
    let connections = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, address) = tokio::select! {
            result=listener.accept()=>result?,
            _=e.network_token.cancelled()=>return Err(Error::new("NETWORK_CHANGED","home network must be confirmed again")),
        };
        e.ensure_network()?;
        if !e.config()?.allows(address.ip()) {
            continue;
        }
        let Ok(permit) = connections.clone().try_acquire_owned() else {
            continue;
        };
        let pins = e
            .store
            .devices()?
            .into_iter()
            .filter_map(|d| hex::decode(d.certificate).ok())
            .collect();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(e.identity.server(pins, pairing)?));
        let engine = e.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let handshake = tokio::time::timeout(
                Duration::from_secs(CONNECT_SECONDS),
                acceptor.accept(stream),
            );
            let tls = tokio::select! {
                result=handshake=>match result {Ok(Ok(tls))=>tls,_=>return},
                _=engine.network_token.cancelled()=>return,
            };
            let cert = tls
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|c| c.first())
                .map(|c| c.to_vec());
            let guard_engine = engine.clone();
            let guard_cert = cert.clone();
            let service = service_fn(move |request| {
                let e = engine.clone();
                let cert = cert.clone();
                async move {
                    let response = match route(&e, request, cert, pairing, address).await {
                        Ok(v) => http_json(200, v),
                        Err(err) => http_json(
                            400,
                            serde_json::to_value(err).unwrap_or(
                                json!({"code":"INTERNAL","message":"error serialization failed"}),
                            ),
                        ),
                    };
                    Ok::<_, Infallible>(response)
                }
            });
            let mut http = hyper::server::conn::http1::Builder::new();
            http.timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(Duration::from_secs(CONNECT_SECONDS))
                .keep_alive(false);
            let connection = http.serve_connection(hyper_util::rt::TokioIo::new(tls), service);
            tokio::pin!(connection);
            let mut revocation = tokio::time::interval(REVOCATION_POLL_INTERVAL);
            let pairing_deadline = tokio::time::sleep(Duration::from_secs(PAIR_SECONDS));
            tokio::pin!(pairing_deadline);
            loop {
                tokio::select! {
                    _=guard_engine.network_token.cancelled()=>break,
                    _=&mut pairing_deadline,if pairing=>break,
                    _=revocation.tick()=>{
                        if !pairing && authenticated(&guard_engine,guard_cert.as_deref()).is_err() {break;}
                    },
                    result=&mut connection=>{
                        if let Err(error)=result {eprintln!("HTTP connection closed: {}",error.is_incomplete_message());}
                        break;
                    },
                }
            }
        });
    }
}
fn http_json(status: u16, value: Value) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(value.to_string())));
    *response.status_mut() =
        hyper::StatusCode::from_u16(status).unwrap_or(hyper::StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response
}
async fn json_body(request: Request<Incoming>) -> Result<Value> {
    let limit = if request.uri().path().ends_with("/message") {
        MAX_CONTROL_BYTES
    } else {
        MAX_META
    };
    let collect = http_body_util::Limited::new(request.into_body(), limit).collect();
    let bytes = tokio::time::timeout(Duration::from_secs(IDLE_SECONDS), collect)
        .await
        .map_err(|_| Error::new("TIMEOUT", "metadata body idle timeout"))?
        .map_err(|_| Error::new("TOO_LARGE", "request exceeds limit"))?
        .to_bytes();
    Ok(serde_json::from_slice(&bytes)?)
}
fn authenticated(e: &Engine, cert: Option<&[u8]>) -> Result<Device> {
    let certificate =
        cert.ok_or_else(|| Error::new("UNTRUSTED", "TLS peer certificate is missing"))?;
    e.store
        .devices()?
        .into_iter()
        .find(|device| {
            hex::decode(&device.certificate)
                .ok()
                .is_some_and(|pin| same_public_key(&pin, certificate))
        })
        .ok_or_else(|| Error::new("UNTRUSTED", "device no longer registered"))
}

async fn route(
    e: &Arc<Engine>,
    request: Request<Incoming>,
    cert: Option<Vec<u8>>,
    pairing: bool,
    address: SocketAddr,
) -> Result<Value> {
    e.ensure_network()?;
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    if pairing {
        return pair_route(e, request, cert.as_deref(), address).await;
    }
    let peer = authenticated(e, cert.as_deref())?;
    if path == "/v1/hello" {
        return Ok(
            // Names are mutable metadata; expose the current value only after mutual TLS authentication.
            json!({"device_id":e.config()?.device_id,"display_name":e.config()?.display_name,"version":1,"features":["files","text","url"]}),
        );
    }
    if path == "/v1/transfers" && method == hyper::Method::POST {
        let mut transfer: Transfer = serde_json::from_value(json_body(request).await?)?;
        authenticated(e, cert.as_deref())?;
        e.ensure_network()?;
        uuid::Uuid::parse_str(&transfer.transfer_id)
            .map_err(|_| Error::new("INVALID_REQUEST", "invalid transfer UUID"))?;
        if transfer.items.len() > MAX_ITEMS
            || !["files", "text", "url"].contains(&transfer.kind.as_str())
            || (transfer.kind == "files" && transfer.items.is_empty())
            || transfer.body.is_some()
        {
            return Err(Error::new("INVALID_REQUEST", "invalid transfer metadata"));
        }
        for item in &mut transfer.items {
            item.size
                .parse::<u64>()
                .map_err(|_| Error::new("INVALID_REQUEST", "invalid item size"))?;
            item.bytes_transferred = "0".into();
            item.state = "pending".into();
            item.path = None;
            item.saved_name = None;
            item.sha256 = None;
            item.error = None;
        }
        transfer.peer_id = peer.device_id;
        transfer.direction = "receive".into();
        transfer.state = "receiving".into();
        transfer.created_at = now();
        let root = receiving_dir(&e.config()?.receive_dir)?;
        let mut record = e.store.reserve_receive(&transfer, &root)?;
        for item in &mut record.items {
            item.path = None;
        }
        return Ok(serde_json::to_value(record)?);
    }
    let parts: Vec<_> = path.trim_matches('/').split('/').collect();
    if parts.len() < 3 || parts[0] != "v1" || parts[1] != "transfers" {
        return Err(Error::new("NOT_FOUND", "unknown endpoint"));
    }
    let id = parts[2];
    let initial = e.store.transfer(id)?;
    if initial.peer_id != peer.device_id || initial.direction != "receive" {
        return Err(Error::new("UNTRUSTED", "transfer belongs to another peer"));
    }
    // Signal cancellation before awaiting the operation lock so an active body can stop itself.
    if method == hyper::Method::DELETE && parts.len() == 3 && initial.state != "completed" {
        e.cancelled
            .lock()
            .map_err(|_| lock_error())?
            .insert(id.into());
    }
    let operation = {
        let mut operations = e.operations.lock().map_err(|_| lock_error())?;
        operations.retain(|_, weak| weak.strong_count() > 0);
        match operations.get(id).and_then(std::sync::Weak::upgrade) {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                operations.insert(id.into(), Arc::downgrade(&lock));
                lock
            }
        }
    };
    let _operation = operation.lock().await;
    let mut transfer = e.store.transfer(id)?;
    if transfer.peer_id != peer.device_id || transfer.direction != "receive" {
        return Err(Error::new("UNTRUSTED", "transfer belongs to another peer"));
    }
    if parts.len() == 3 {
        if method == hyper::Method::DELETE && transfer.state != "completed" {
            e.cancelled
                .lock()
                .map_err(|_| lock_error())?
                .insert(id.into());
            for item in &mut transfer.items {
                if item.state != "completed" {
                    item.state = "cancelled".into();
                }
            }
            transfer.state = if transfer.items.iter().any(|i| i.state == "completed") {
                "partial".into()
            } else {
                "cancelled".into()
            };
            e.store.save_transfer(&transfer)?;
            let root = e.store.receive_root(id)?;
            for (index, item) in transfer.items.iter().enumerate() {
                if item.state != "completed" {
                    let path =
                        crate::storage::partials_dir(&root)?.join(format!("{id}-{index}.part"));
                    match tokio::fs::remove_file(path).await {
                        Ok(()) => {}
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                        Err(err) => eprintln!("partial cleanup failed: {}", err.kind()),
                    }
                }
            }
        }
        // Remote status never exposes the receiver's absolute filesystem locations.
        for item in &mut transfer.items {
            item.path = None;
        }
        return Ok(serde_json::to_value(transfer)?);
    }
    if e.is_cancelled(id) {
        return Err(Error::new("CANCELLED", "transfer cancelled"));
    }
    if parts.len() == 4 && parts[3] == "message" {
        let body = json_body(request).await?;
        let text = required(&body, "body")?;
        if e.is_cancelled(id) || authenticated(e, cert.as_deref()).is_err() {
            return Err(Error::new(
                "CANCELLED",
                "transfer cancelled or device unregistered",
            ));
        }
        e.ensure_network()?;
        if text.len() > MAX_MESSAGE_BYTES || !["text", "url"].contains(&transfer.kind.as_str()) {
            return Err(Error::new(
                "TOO_LARGE",
                "invalid message or message too large",
            ));
        }
        if transfer.kind == "url" {
            validate_url(text)?;
        }
        if transfer.state == "completed" {
            if transfer.body.as_deref() != Some(text) {
                return Err(Error::new("CONFLICT", "completed message differs"));
            }
        } else {
            transfer.body = Some(text.into());
            transfer.state = "completed".into();
            e.store.save_transfer(&transfer)?;
        }
        return Ok(serde_json::to_value(transfer)?);
    }
    if parts.len() != 6 || parts[3] != "items" || transfer.kind != "files" {
        return Err(Error::new("NOT_FOUND", "unknown transfer operation"));
    }
    let index: usize = parts[4]
        .parse()
        .map_err(|_| Error::new("INVALID_REQUEST", "invalid item index"))?;
    if index >= transfer.items.len() {
        return Err(Error::new("INVALID_REQUEST", "item index out of range"));
    }
    let root = e.store.receive_root(id)?;
    receiving_dir(&root)?;
    let temp = crate::storage::partials_dir(&root)?.join(format!("{id}-{index}.part"));
    if parts[5] == "body" && method == hyper::Method::PUT {
        // A duplicate upload must not replace a terminal record with a new failure or remove committed data.
        if transfer.items[index].state != "pending" {
            return Err(Error::new(
                "CONFLICT",
                "item body has already been submitted",
            ));
        }
        let _permit = e
            .incoming
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::new("BUSY", "receiver stream limit reached"))?;
        let token = format!("{id}:{index}");
        {
            let mut active = e.active.lock().map_err(|_| lock_error())?;
            if !active.insert(token.clone()) {
                return Err(Error::new("BUSY", "item is already being received"));
            }
        }
        let result = receive_body(e, &mut transfer, index, &temp, request).await;
        e.active.lock().map_err(|_| lock_error())?.remove(&token);
        if let Err(error) = &result {
            transfer.items[index].state = "failed".into();
            transfer.items[index].error = Some(error.code.clone());
            transfer.state = aggregate(&transfer.items);
            e.store.save_transfer(&transfer)?;
            match tokio::fs::remove_file(&temp).await {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => eprintln!("partial cleanup failed: {}", err.kind()),
            }
        }
        result?;
        return Ok(json!({"state":"verifying"}));
    }
    if parts[5] == "commit" && method == hyper::Method::POST {
        let expected = required(&json_body(request).await?, "sha256")?.to_string();
        if e.is_cancelled(id) || authenticated(e, cert.as_deref()).is_err() {
            return Err(Error::new(
                "CANCELLED",
                "transfer cancelled or device unregistered",
            ));
        }
        e.ensure_network()?;
        let item = &transfer.items[index];
        if item.sha256.as_ref() != Some(&expected) {
            if item.state != "completed" {
                transfer.items[index].state = "failed".into();
                transfer.items[index].error = Some("HASH_MISMATCH".into());
                transfer.state = aggregate(&transfer.items);
                e.store.save_transfer(&transfer)?;
                match tokio::fs::remove_file(&temp).await {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => eprintln!("partial cleanup failed: {}", err.kind()),
                }
            }
            return Err(Error::new(
                "HASH_MISMATCH",
                "sender and receiver hashes differ",
            ));
        }
        if item.state == "completed" {
            let mut remote = transfer.clone();
            for i in &mut remote.items {
                i.path = None;
            }
            return Ok(serde_json::to_value(remote)?);
        }
        if item.state != "verifying" {
            return Err(Error::new("CONFLICT", "file body is not ready"));
        }
        let name = item.name.clone();
        let saved = commit_path(&temp, &root, &name, |target| {
            transfer.items[index].path = Some(target.to_string_lossy().into());
            transfer.items[index].state = "committing".into();
            transfer.state = "committing".into();
            e.store.save_transfer(&transfer)
        })?;
        transfer.items[index].saved_name = saved.file_name().map(|n| n.to_string_lossy().into());
        transfer.items[index].path = Some(saved.to_string_lossy().into());
        transfer.items[index].state = "completed".into();
        transfer.state = if transfer.items.iter().all(|i| i.state == "completed") {
            "completed".into()
        } else {
            "receiving".into()
        };
        e.store.save_transfer(&transfer)?;
        for i in &mut transfer.items {
            i.path = None;
        }
        return Ok(serde_json::to_value(transfer)?);
    }
    Err(Error::new("NOT_FOUND", "unknown transfer operation"))
}
async fn pair_route(
    e: &Engine,
    request: Request<Incoming>,
    cert: Option<&[u8]>,
    address: SocketAddr,
) -> Result<Value> {
    let path = request.uri().path().to_string();
    let body = json_body(request).await?;
    if path == "/pair/hello" {
        let peer: Device = serde_json::from_value(body["device"].clone())?;
        let nonce = required(&body, "nonce")?;
        let certificate = hex::decode(&peer.certificate)
            .map_err(|_| Error::new("INVALID_REQUEST", "invalid peer certificate"))?;
        uuid::Uuid::parse_str(&peer.device_id)
            .map_err(|_| Error::new("INVALID_REQUEST", "invalid device ID"))?;
        if peer.display_name.is_empty()
            || peer.display_name.len() > 128
            || cert != Some(certificate.as_slice())
        {
            return Err(Error::new(
                "UNTRUSTED",
                "registration identity does not match the TLS client certificate",
            ));
        }
        // Database failures must not be mistaken for a new, unregistered identity.
        let registered = match e.store.device(&peer.device_id) {
            Ok(old) => Some(old),
            Err(error) if error.code == "UNTRUSTED" => None,
            Err(error) => return Err(error),
        };
        if let Some(old) = &registered
            && !hex::decode(&old.certificate)
                .ok()
                .is_some_and(|old| same_public_key(&old, &certificate))
        {
            return Err(Error::new(
                "KEY_CHANGED",
                "remove the previous registration before changing a key",
            ));
        }
        let peer_address = allowed_address(e, &peer.address)?;
        if peer_address.ip() != address.ip() {
            return Err(Error::new(
                "INVALID_REQUEST",
                "peer address must match connection source",
            ));
        }
        let intent: Option<approval::Intent> =
            serde_json::from_value(body.get("approval").cloned().unwrap_or(Value::Null))?;
        if let Some(intent) = &intent {
            approval::validate_intent(intent)?;
            approval::check_request(e, &peer)?;
        }
        let mut sessions = e.sessions.lock().map_err(|_| lock_error())?;
        let session = sessions
            .values_mut()
            .find(|s| !s.initiated && s.expires_at >= now() && s.peer.is_none())
            .ok_or_else(|| Error::new("BUSY", "no registration slot open"))?;
        session.comparison = comparison(&certificate, &e.identity.cert, nonce, &session.nonce)?;
        session.peer = Some(peer);
        session.state = "compare".into();
        session.approval = intent;
        if session.approval.is_some() {
            session.expires_at = now() + PAIR_SECONDS;
            // A previously accepted key is already trusted on this receiver; do not prompt twice.
            session.local_confirmed = registered.is_some();
        }
        let mut remote = session.clone();
        remote.peer = Some(e.own_device()?);
        return Ok(serde_json::to_value(remote)?);
    }
    if path == "/pair/status" || path == "/pair/cancel" {
        let mut session = active_session(e, required(&body, "session_id")?)?;
        if session.initiated {
            return Err(Error::new(
                "INVALID_REQUEST",
                "not a receiver registration session",
            ));
        }
        // Confirmation must be bound to the client identity, not merely a guessable network address.
        let peer = session
            .peer
            .as_ref()
            .ok_or_else(|| Error::new("UNTRUSTED", "registration has no connected peer"))?;
        if hex::decode(&peer.certificate).ok().as_deref() != cert
            || allowed_address(e, &peer.address)?.ip() != address.ip()
        {
            return Err(Error::new("UNTRUSTED", "session source changed"));
        }
        if path == "/pair/cancel" {
            // Cancellation is authenticated exactly like status and never removes an accepted pin.
            if let Some(current) = e
                .sessions
                .lock()
                .map_err(|_| lock_error())?
                .get_mut(&session.session_id)
            {
                current.state = "rejected".into();
            }
            return Ok(json!({"cancelled":true}));
        }
        if session.state == "rejected" {
            return Ok(serde_json::to_value(session)?);
        }
        if body["confirmed"] == true {
            session.remote_confirmed = true;
        }
        return Ok(serde_json::to_value(store_session(e, session)?)?);
    }
    Err(Error::new("NOT_FOUND", "registration API only"))
}
async fn receive_body(
    e: &Engine,
    t: &mut Transfer,
    index: usize,
    temp: &std::path::Path,
    request: Request<Incoming>,
) -> Result<()> {
    let size = t.items[index]
        .size
        .parse::<u64>()
        .map_err(|_| Error::new("INVALID_REQUEST", "invalid size"))?;
    let declared = request
        .headers()
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.parse::<u64>().ok());
    if declared != Some(size) {
        return Err(Error::new(
            "INVALID_REQUEST",
            "Content-Length must match item size",
        ));
    }
    if t.items[index].state != "pending" {
        return Err(Error::new("CONFLICT", "item body already received"));
    }
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let mut file = opts.open(temp).await?;
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut last_progress = tokio::time::Instant::now();
    t.items[index].state = "receiving".into();
    e.store.save_transfer(t)?;
    let mut body = request.into_body();
    while let Some(frame) = tokio::time::timeout(Duration::from_secs(IDLE_SECONDS), body.frame())
        .await
        .map_err(|_| Error::new("TIMEOUT", "file data idle timeout"))?
    {
        let frame = frame.map_err(net_error)?;
        if let Ok(data) = frame.into_data() {
            if e.is_cancelled(&t.transfer_id) || e.store.device(&t.peer_id).is_err() {
                return Err(Error::new("CANCELLED", "transfer cancelled"));
            }
            received = received
                .checked_add(data.len() as u64)
                .ok_or_else(|| Error::new("TOO_LARGE", "file size overflow"))?;
            if received > size {
                return Err(Error::new("TOO_LARGE", "body exceeds declared file size"));
            }
            e.ensure_network()?;
            file.write_all(&data).await?;
            hash.update(&data);
            t.items[index].bytes_transferred = received.to_string();
            if last_progress.elapsed() >= PROGRESS_INTERVAL {
                e.store.update_progress(&t.transfer_id, index, received)?;
                last_progress = tokio::time::Instant::now();
            }
        }
    }
    if received != size {
        return Err(Error::new("INCOMPLETE", "body ended before declared size"));
    }
    if e.is_cancelled(&t.transfer_id) || e.store.device(&t.peer_id).is_err() {
        return Err(Error::new(
            "CANCELLED",
            "transfer cancelled or device unregistered",
        ));
    }
    e.ensure_network()?;
    file.sync_all().await?;
    t.items[index].bytes_transferred = received.to_string();
    t.items[index].sha256 = Some(hex::encode(hash.finalize()));
    t.items[index].state = "verifying".into();
    e.store.save_transfer(t)?;
    Ok(())
}
pub fn validate_url(text: &str) -> Result<()> {
    let url =
        reqwest::Url::parse(text).map_err(|_| Error::new("INVALID_REQUEST", "invalid URL"))?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || text.chars().any(char::is_whitespace)
    {
        return Err(Error::new(
            "INVALID_REQUEST",
            "only a single HTTP or HTTPS URL is allowed",
        ));
    }
    Ok(())
}
/// A discovered device is still untrusted. A bounded TCP check only keeps dead advertisements
/// out of the picker; first-send TLS identity binding and recipient consent remain mandatory.
pub async fn discovered_reachable(e: &Engine, device: &Device) -> bool {
    if e.ensure_network().is_err() {
        return false;
    }
    let Ok(address) = allowed_address(e, &device.address) else {
        return false;
    };
    tokio::select! {
        result=tokio::time::timeout(Duration::from_secs(CONNECT_SECONDS),tokio::net::TcpStream::connect(address))=>matches!(result,Ok(Ok(_))),
        _=e.network_token.cancelled()=>false,
    }
}

pub async fn probe(e: &Engine, device: &Device) -> Result<Value> {
    let address = allowed_address(e, &device.address)?;
    let cert = hex::decode(&device.certificate)
        .map_err(|_| Error::new("IDENTITY_ERROR", "invalid registered certificate"))?;
    let request = async {
        response_value(
            client(e, vec![cert], false)?
                .get(format!("https://{address}/v1/hello"))
                .send()
                .await
                .map_err(net_error)?,
        )
        .await
    };
    let value = tokio::select! {
        result=request=>result?,
        _=e.network_token.cancelled()=>return Err(Error::new("NETWORK_CHANGED","home network must be confirmed again")),
    };
    if value["device_id"] != device.device_id || value["version"] != 1 {
        return Err(Error::new(
            "VERSION_UNSUPPORTED",
            "unexpected device identity or protocol version",
        ));
    }
    Ok(value)
}
// The exact handle validated against MCP grants is retained until streaming completes.
struct OpenedSource {
    file: std::fs::File,
    before: std::fs::Metadata,
}
pub async fn send(e: &Arc<Engine>, params: Value) -> Result<Value> {
    e.ensure_network()?;
    let _permit = e
        .outgoing
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::new("BUSY", "sender stream limit reached"))?;
    let peer = e.store.device(required(&params, "to")?)?;
    let kind = required(&params, "kind")?;
    if !["files", "text", "url"].contains(&kind) {
        return Err(Error::new("INVALID_REQUEST", "unknown content kind"));
    }
    let id = params
        .get("transfer_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    uuid::Uuid::parse_str(&id)
        .map_err(|_| Error::new("INVALID_REQUEST", "invalid transfer UUID"))?;
    if e.store.transfer(&id).is_ok() {
        return Err(Error::new("CONFLICT", "local transfer ID already exists"));
    }
    let mut t = Transfer {
        transfer_id: id,
        peer_id: peer.device_id.clone(),
        direction: "send".into(),
        kind: kind.into(),
        state: "sending".into(),
        created_at: now(),
        body: None,
        items: vec![],
        error: None,
    };
    let paths: Vec<std::path::PathBuf> = if kind == "files" {
        serde_json::from_value(params.get("paths").cloned().unwrap_or(json!([])))?
    } else {
        vec![]
    };
    if kind == "files" && (paths.is_empty() || paths.len() > MAX_ITEMS) {
        return Err(Error::new(
            "INVALID_REQUEST",
            "select 1..100 ordinary files",
        ));
    }
    let allowed_roots: Option<Vec<std::path::PathBuf>> = params
        .get("allowed_roots")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?;
    let mut sources = Vec::with_capacity(paths.len());
    for path in &paths {
        let path_for_open = path.clone();
        let roots = allowed_roots.clone();
        let file = tokio::task::spawn_blocking(move || {
            crate::source::open_file(&path_for_open, roots.as_deref())
        })
        .await
        .map_err(|error| Error::new("IO_ERROR", error.to_string()))??;
        let metadata = file.metadata()?;
        t.items.push(Item {
            name: path
                .file_name()
                .ok_or_else(|| Error::new("INVALID_REQUEST", "missing filename"))?
                .to_string_lossy()
                .into(),
            size: metadata.len().to_string(),
            bytes_transferred: "0".into(),
            state: "pending".into(),
            saved_name: None,
            path: None,
            sha256: None,
            error: None,
        });
        sources.push(OpenedSource {
            file,
            before: metadata,
        });
    }
    if kind != "files" {
        let text = required(&params, "body")?;
        if text.len() > MAX_MESSAGE_BYTES {
            return Err(Error::new("TOO_LARGE", "message exceeds limit"));
        }
        if kind == "url" {
            validate_url(text)?;
        }
        t.body = Some(text.into());
    }
    e.store.save_transfer(&t)?;
    let result = tokio::select! {
        result=send_inner(e,&peer,&mut t,sources)=>result,
        _=e.network_token.cancelled()=>Err(Error::new("NETWORK_CHANGED","network changed during transfer")),
    };
    if let Err(error) = result {
        if !["UNKNOWN", "NETWORK_CHANGED"].contains(&error.code.as_str())
            && let (Ok(address), Ok(cert)) = (
                allowed_address(e, &peer.address),
                hex::decode(&peer.certificate),
            )
            && let Ok(c) = client(e, vec![cert], false)
        {
            let _ = c
                .delete(format!("https://{address}/v1/transfers/{}", t.transfer_id))
                .send()
                .await;
        }
        // The body stream may have persisted newer counters than this command's initial snapshot.
        if let Ok(latest) = e.store.transfer(&t.transfer_id) {
            for (item, latest) in t.items.iter_mut().zip(latest.items) {
                item.bytes_transferred = latest.bytes_transferred;
            }
        }
        t.error = Some(error.code.clone());
        for item in &mut t.items {
            if item.state != "completed" {
                item.state = if ["UNKNOWN", "NETWORK_CHANGED"].contains(&error.code.as_str()) {
                    "unknown".into()
                } else if e.is_cancelled(&t.transfer_id) {
                    "cancelled".into()
                } else {
                    "failed".into()
                };
                item.error = Some(error.code.clone());
            }
        }
        t.state = if ["UNKNOWN", "NETWORK_CHANGED"].contains(&error.code.as_str()) {
            "unknown".into()
        } else if e.is_cancelled(&t.transfer_id) {
            "cancelled".into()
        } else if t.items.iter().any(|i| i.state == "completed") {
            "partial".into()
        } else {
            "failed".into()
        };
        e.store.save_transfer(&t)?;
        return Ok(serde_json::to_value(t)?);
    }
    e.store.save_transfer(&t)?;
    Ok(serde_json::to_value(t)?)
}
async fn send_inner(
    e: &Arc<Engine>,
    peer: &Device,
    t: &mut Transfer,
    sources: Vec<OpenedSource>,
) -> Result<()> {
    // Cancellation can arrive while first-use consent or source validation is finishing.
    if e.is_cancelled(&t.transfer_id) {
        return Err(Error::new("CANCELLED", "transfer cancelled before sending"));
    }
    probe(e, peer).await?;
    let address = allowed_address(e, &peer.address)?;
    let cert =
        hex::decode(&peer.certificate).map_err(|_| Error::new("IDENTITY_ERROR", "invalid pin"))?;
    let c = client(e, vec![cert], false)?;
    let base = format!("https://{address}/v1/transfers");
    let mut metadata = t.clone();
    metadata.body = None;
    response_value(
        c.post(&base)
            .json(&metadata)
            .send()
            .await
            .map_err(net_error)?,
    )
    .await?;
    let target = format!("{base}/{}", t.transfer_id);
    if e.is_cancelled(&t.transfer_id) {
        return Err(Error::new("CANCELLED", "transfer cancelled before payload"));
    }
    if t.kind != "files" {
        let response = c
            .post(format!("{target}/message"))
            .json(&json!({"body":t.body}))
            .send()
            .await;
        let result = completion_response(&c, &target, response).await?;
        if result["state"] != "completed" {
            return Err(Error::new("UNKNOWN", "message completion is not confirmed"));
        }
        t.state = "completed".into();
        return Ok(());
    }
    for (index, source) in sources.into_iter().enumerate() {
        if e.is_cancelled(&t.transfer_id) {
            let _ = c.delete(&target).send().await;
            return Err(Error::new("CANCELLED", "transfer cancelled"));
        }
        let integrity = tokio::fs::File::from_std(source.file.try_clone()?);
        let file = tokio::fs::File::from_std(source.file);
        let before = source.before;
        let size = before.len();
        if size.to_string() != t.items[index].size {
            return Err(Error::new(
                "SOURCE_CHANGED",
                "source size changed before sending",
            ));
        }
        // The body stream hashes exactly the bytes sent, without a pre-read or whole-file buffer.
        let digest = Arc::new(std::sync::Mutex::new(Sha256::new()));
        let digest_stream = digest.clone();
        let sending_engine = e.clone();
        let sending_id = t.transfer_id.clone();
        let peer_id_for_stream = peer.device_id.clone();
        let mut sent = 0u64;
        let mut last_progress = tokio::time::Instant::now();
        use futures_util::TryStreamExt;
        let stream =
            tokio_util::io::ReaderStream::with_capacity(file, CHUNK_SIZE).and_then(move |bytes| {
                let cancelled = sending_engine.is_cancelled(&sending_id)
                    || sending_engine.ensure_network().is_err()
                    || sending_engine.store.device(&peer_id_for_stream).is_err();
                sent = sent.saturating_add(bytes.len() as u64);
                let progress = if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    last_progress = tokio::time::Instant::now();
                    sending_engine
                        .store
                        .update_progress(&sending_id, index, sent)
                        .map_err(|error| std::io::Error::other(error.to_string()))
                } else {
                    Ok(())
                };
                if let Ok(mut hash) = digest_stream.lock() {
                    hash.update(&bytes);
                }
                async move {
                    progress?;
                    if cancelled {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "transfer cancelled",
                        ))
                    } else {
                        Ok(bytes)
                    }
                }
            });
        let response = c
            .put(format!("{target}/items/{index}/body"))
            .header(reqwest::header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(stream))
            .send()
            .await
            .map_err(net_error)?;
        response_value(response).await?;
        let after = integrity.metadata().await?;
        if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
            return Err(Error::new(
                "SOURCE_CHANGED",
                "source changed while being sent",
            ));
        }
        if e.is_cancelled(&t.transfer_id) {
            let _ = c.delete(&target).send().await;
            return Err(Error::new("CANCELLED", "transfer cancelled"));
        }
        let hash = hex::encode(digest.lock().map_err(|_| lock_error())?.clone().finalize());
        let response = c
            .post(format!("{target}/items/{index}/commit"))
            .json(&json!({"sha256":hash}))
            .send()
            .await;
        let value = completion_response(&c, &target, response).await?;
        if value["items"][index]["state"] != "completed" {
            return Err(Error::new(
                "UNKNOWN",
                "receiver did not confirm final storage",
            ));
        }
        t.items[index].bytes_transferred = size.to_string();
        t.items[index].state = "completed".into();
        t.items[index].sha256 = Some(hash);
        t.items[index].saved_name = value["items"][index]["saved_name"]
            .as_str()
            .map(str::to_string);
        e.store.save_transfer(t)?;
    }
    t.state = "completed".into();
    Ok(())
}

struct PairAdvertisement(Option<mdns_sd::ServiceDaemon>);
impl Drop for PairAdvertisement {
    fn drop(&mut self) {
        if let Some(daemon) = self.0.take() {
            let _ = daemon.shutdown();
        }
    }
}
#[cfg(not(target_os = "ios"))]
fn discovery_daemon(config: &Config) -> Result<mdns_sd::ServiceDaemon> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(net_error)?;
    daemon
        .disable_interface(mdns_sd::IfKind::All)
        .map_err(net_error)?;
    daemon
        .enable_interface(mdns_sd::IfKind::Addr(config.listen_addr.ip()))
        .map_err(net_error)?;
    Ok(daemon)
}
#[cfg(not(target_os = "ios"))]
fn service_info(config: &Config, service_type: &str, port: u16) -> Result<mdns_sd::ServiceInfo> {
    let host = format!("fta-{}.local.", config.device_id);
    let properties = [
        ("device_id", config.device_id.as_str()),
        ("display_name", config.display_name.as_str()),
        ("os", std::env::consts::OS),
        ("version", "1"),
    ];
    mdns_sd::ServiceInfo::new(
        service_type,
        &config.device_id,
        &host,
        config.listen_addr.ip(),
        port,
        &properties[..],
    )
    .map_err(net_error)
}
#[cfg(not(target_os = "ios"))]
fn advertise_pairing(e: &Engine, session: &PairSession) -> Result<Option<mdns_sd::ServiceDaemon>> {
    let config = e.config()?;
    if !config.discovery_enabled || config.listen_addr.ip().is_loopback() {
        return Ok(None);
    }
    let address: SocketAddr = session
        .address
        .parse()
        .map_err(|_| Error::new("INVALID_REQUEST", "invalid pairing address"))?;
    let daemon = discovery_daemon(&config)?;
    daemon
        .register(service_info(&config, PAIR_SERVICE_TYPE, address.port())?)
        .map_err(net_error)?;
    Ok(Some(daemon))
}
/// Only address hints are accepted from discovery; authentication always happens on the TLS socket.
#[cfg(not(target_os = "ios"))]
fn apply_discovery_event(
    e: &Engine,
    config: &Config,
    event: mdns_sd::ServiceEvent,
    pairing: bool,
) -> Result<()> {
    match event {
        mdns_sd::ServiceEvent::ServiceResolved(info) => {
            let Some(id) = info.get_property_val_str("device_id") else {
                return Ok(());
            };
            if id == config.device_id
                || uuid::Uuid::parse_str(id).is_err()
                || info.get_property_val_str("version") != Some("1")
            {
                return Ok(());
            }
            let Some(ip) = info
                .get_addresses()
                .iter()
                .find(|ip| config.allows(ip.to_ip_addr()))
            else {
                return Ok(());
            };
            let address = SocketAddr::new(ip.to_ip_addr(), info.get_port()).to_string();
            if pairing {
                let mut found = e.pairing_addresses.lock().map_err(|_| lock_error())?;
                if found.len() < MAX_DISCOVERED_DEVICES || found.contains_key(id) {
                    found.insert(id.into(), address);
                }
            } else {
                let device = Device {
                    device_id: id.into(),
                    display_name: info
                        .get_property_val_str("display_name")
                        .unwrap_or(id)
                        .into(),
                    os: info.get_property_val_str("os").unwrap_or("unknown").into(),
                    address,
                    online: false,
                    paired: false,
                    certificate: String::new(),
                };
                let mut found = e.discovered.lock().map_err(|_| lock_error())?;
                if found.len() < MAX_DISCOVERED_DEVICES || found.contains_key(id) {
                    found.insert(id.into(), device);
                }
            }
        }
        mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
            if pairing {
                e.pairing_addresses
                    .lock()
                    .map_err(|_| lock_error())?
                    .retain(|id, _| !fullname.starts_with(&format!("{id}.")));
            } else {
                e.discovered
                    .lock()
                    .map_err(|_| lock_error())?
                    .retain(|id, _| !fullname.starts_with(&format!("{id}.")));
            }
        }
        _ => {}
    }
    Ok(())
}
/// Browse temporary registration endpoints separately from ordinary transfer listeners.
#[cfg(not(target_os = "ios"))]
pub fn start_discovery(e: Arc<Engine>) -> Result<Option<mdns_sd::ServiceDaemon>> {
    let config = e.config()?;
    e.ensure_network()?;
    if !config.discovery_enabled || config.listen_addr.ip().is_loopback() {
        return Ok(None);
    }
    let daemon = discovery_daemon(&config)?;
    daemon
        .register(service_info(
            &config,
            SERVICE_TYPE,
            config.listen_addr.port(),
        )?)
        .map_err(net_error)?;
    for (service_type, pairing) in [(SERVICE_TYPE, false), (PAIR_SERVICE_TYPE, true)] {
        let receiver = daemon.browse(service_type).map_err(net_error)?;
        let engine = e.clone();
        let config = config.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _=engine.network_token.cancelled()=>break,
                    event=receiver.recv_async()=>{
                        let Ok(event)=event else {break;};
                        if let Err(error)=apply_discovery_event(&engine,&config,event,pairing) {eprintln!("discovery update failed: {}",error.code);}
                    },
                }
            }
        });
    }
    Ok(Some(daemon))
}

/// Unknown completion is resolved by a read-only status query, never by automatically resending payloads.
pub async fn reconcile(e: &Arc<Engine>, id: &str) -> Result<Value> {
    let mut local = e.store.transfer(id)?;
    if local.direction != "send" || local.state != "unknown" {
        return Ok(serde_json::to_value(local)?);
    }
    let peer = e.store.device(&local.peer_id)?;
    let address = allowed_address(e, &peer.address)?;
    let certificate = hex::decode(peer.certificate)
        .map_err(|_| Error::new("IDENTITY_ERROR", "invalid registered certificate"))?;
    let response = network_io(e, async {
        client(e, vec![certificate], false)?
            .get(format!("https://{address}/v1/transfers/{id}"))
            .send()
            .await
            .map_err(net_error)
    })
    .await;
    if let Ok(response) = response
        && let Ok(value) = network_io(e, response_value(response)).await
    {
        let remote: Transfer = serde_json::from_value(value)?;
        if ["completed", "partial", "failed", "cancelled"].contains(&remote.state.as_str()) {
            local.state = remote.state;
            local.items = remote.items;
            local.error = remote.error;
            e.store.save_transfer(&local)?;
        }
    }
    Ok(serde_json::to_value(local)?)
}

/// A response can lose its body after headers arrive; both kinds of transport loss require reconciliation.
async fn completion_response(
    client: &reqwest::Client,
    target: &str,
    response: std::result::Result<reqwest::Response, reqwest::Error>,
) -> Result<Value> {
    let result = match response {
        Ok(response) => response_value(response).await,
        Err(error) => Err(net_error(error)),
    };
    match result {
        Ok(value) => Ok(value),
        Err(error) if ["OFFLINE", "TIMEOUT", "INVALID_REQUEST"].contains(&error.code.as_str()) => {
            if let Ok(response) = client.get(target).send().await
                && let Ok(value) = response_value(response).await
            {
                return Ok(value);
            }
            Err(Error::new(
                "UNKNOWN",
                "completion response was lost; receiver result remains unconfirmed",
            ))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use crate::{config::AppPaths, identity::Identity};

    fn instance() -> (tempfile::TempDir, Arc<Engine>) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            config_dir: dir.path().join("config"),
            state_dir: dir.path().join("state"),
            runtime_dir: dir.path().join("run"),
            default_receive_dir: dir.path().join("inbox"),
        };
        let engine =
            Engine::open_with_identity(paths, Identity::generate_ephemeral().unwrap()).unwrap();
        (dir, engine)
    }
    // A late discovery event from the ongoing browser must unblock a cache-miss lookup.
    #[tokio::test]
    async fn registration_lookup_waits_for_late_discovery() {
        let (_dir, engine) = instance();
        let mut config = engine.config().unwrap();
        // Documentation-only addresses keep this test off real LAN interfaces.
        config.listen_addr = "192.0.2.1:47321".parse().unwrap();
        config.allowed_networks = vec!["192.0.2.0/24".parse().unwrap()];
        config.discovery_enabled = true;
        *engine.config.write().unwrap() = config.clone();
        let mut peer = config.clone();
        peer.device_id = uuid::Uuid::new_v4().to_string();
        peer.listen_addr = "192.0.2.2:47321".parse().unwrap();
        let expected = "192.0.2.2:49152";
        let target = peer.device_id.clone();
        let lookup_engine = engine.clone();
        let lookup =
            tokio::spawn(async move { resolve_pairing_target(&lookup_engine, &target).await });
        tokio::time::sleep(Duration::from_millis(25)).await;
        let info = service_info(&peer, PAIR_SERVICE_TYPE, 49152).unwrap();
        apply_discovery_event(
            &engine,
            &config,
            mdns_sd::ServiceEvent::ServiceResolved(Box::new(info.as_resolved_service())),
            true,
        )
        .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), lookup)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result, expected);
        assert!(engine.sessions.lock().unwrap().is_empty());
    }
    // A network change must stop discovery without creating a registration or waiting out its budget.
    #[tokio::test]
    async fn registration_lookup_stops_on_network_change() {
        let (_dir, engine) = instance();
        {
            let mut config = engine.config.write().unwrap();
            config.listen_addr = "192.0.2.1:47321".parse().unwrap();
            config.allowed_networks = vec!["192.0.2.0/24".parse().unwrap()];
            config.discovery_enabled = true;
        }
        let lookup_engine = engine.clone();
        let lookup = tokio::spawn(async move {
            resolve_pairing_target(&lookup_engine, &uuid::Uuid::new_v4().to_string()).await
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        engine.network_token.cancel();
        let error = tokio::time::timeout(Duration::from_secs(1), lookup)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, "NETWORK_CHANGED");
        assert!(engine.sessions.lock().unwrap().is_empty());
    }
    // The mDNS event path must select the temporary registration port, never the normal transfer port.
    #[tokio::test]
    async fn discovered_registration_endpoint_connects_by_device_id() {
        let (_sender_dir, sender) = instance();
        let (_receiver_dir, receiver) = instance();
        let open = pair_open(&receiver).await.unwrap();
        let address: SocketAddr = open["address"].as_str().unwrap().parse().unwrap();
        let info = service_info(
            &receiver.config().unwrap(),
            PAIR_SERVICE_TYPE,
            address.port(),
        )
        .unwrap();
        let fullname = info.get_fullname().to_string();
        apply_discovery_event(
            &sender,
            &sender.config().unwrap(),
            mdns_sd::ServiceEvent::ServiceResolved(Box::new(info.as_resolved_service())),
            true,
        )
        .unwrap();
        let id = receiver.config().unwrap().device_id;
        assert_eq!(
            sender.pairing_addresses.lock().unwrap()[&id],
            address.to_string()
        );
        assert!(sender.discovered.lock().unwrap().is_empty());
        let session = pair_connect(&sender, &id).await.unwrap();
        assert_eq!(session["address"], address.to_string());
        apply_discovery_event(
            &sender,
            &sender.config().unwrap(),
            mdns_sd::ServiceEvent::ServiceRemoved(PAIR_SERVICE_TYPE.into(), fullname),
            true,
        )
        .unwrap();
        assert!(!sender.pairing_addresses.lock().unwrap().contains_key(&id));
        let session_id = session["session_id"].as_str().unwrap();
        sender
            .dispatch("pairing.cancel", json!({"session_id":session_id}))
            .await
            .unwrap();
        receiver
            .dispatch("pairing.cancel", json!({"session_id":session_id}))
            .await
            .unwrap();
    }
    // Losing the selected network after receiver storage must not falsely claim delivery failed.
    #[tokio::test]
    async fn network_loss_after_message_commit_preserves_unknown_until_reconciled() {
        let (_sender_dir, sender) = instance();
        let (_receiver_dir, receiver) = instance();
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
        let stored = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let server_engine = receiver.clone();
        let server_stored = stored.clone();
        let server_release = release.clone();
        let server = tokio::spawn(async move {
            loop {
                let (stream, address) = listener.accept().await.unwrap();
                let engine = server_engine.clone();
                let stored = server_stored.clone();
                let release = server_release.clone();
                tokio::spawn(async move {
                    let pins = engine
                        .store
                        .devices()
                        .unwrap()
                        .into_iter()
                        .map(|device| hex::decode(device.certificate).unwrap())
                        .collect();
                    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(
                        engine.identity.server(pins, false).unwrap(),
                    ));
                    let tls = acceptor.accept(stream).await.unwrap();
                    let certificate = tls.get_ref().1.peer_certificates().unwrap()[0].to_vec();
                    let service = service_fn(move |request: Request<Incoming>| {
                        let engine = engine.clone();
                        let certificate = certificate.clone();
                        let stored = stored.clone();
                        let release = release.clone();
                        async move {
                            let message = request.uri().path().ends_with("/message");
                            let value = route(&engine, request, Some(certificate), false, address)
                                .await
                                .unwrap();
                            if message {
                                stored.notify_one();
                                release.notified().await;
                            }
                            Ok::<_, Infallible>(http_json(200, value))
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(tls), service)
                        .await;
                });
            }
        });
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let sending_engine = sender.clone();
        let id = transfer_id.clone();
        let receiver_id = receiver.config().unwrap().device_id;
        let sending = tokio::spawn(async move {
            send(
                &sending_engine,
                json!({"to":receiver_id,"kind":"text","body":"one durable copy","transfer_id":id}),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(3), stored.notified())
            .await
            .unwrap();
        assert_eq!(
            receiver.store.transfer(&transfer_id).unwrap().state,
            "completed"
        );
        sender.suspend_network().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), sending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result["state"], "unknown");
        assert_eq!(result["error"], "NETWORK_CHANGED");
        release.notify_one();
        let restarted =
            Engine::open_with_identity(sender.paths.clone(), sender.identity.clone()).unwrap();
        let reconciled = reconcile(&restarted, &transfer_id).await.unwrap();
        assert_eq!(reconciled["state"], "completed");
        assert_eq!(receiver.store.history(100).unwrap().len(), 1);
        server.abort();
    }
}

// The iOS host publishes actual bound ports with NetService and feeds resolved LAN hints back.
#[cfg(target_os = "ios")]
fn advertise_pairing(_: &Engine, _: &PairSession) -> Result<Option<mdns_sd::ServiceDaemon>> {
    Ok(None)
}
#[cfg(target_os = "ios")]
pub fn start_discovery(_: Arc<Engine>) -> Result<Option<mdns_sd::ServiceDaemon>> {
    Ok(None)
}

// Native Bonjour may resolve after the send tap; wait only within the existing online budget.
#[cfg(target_os = "ios")]
async fn resolve_pairing_target(e: &Engine, target: &str) -> Result<String> {
    if target.parse::<SocketAddr>().is_ok() {
        return Ok(target.into());
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(CONNECT_SECONDS);
    loop {
        e.ensure_network()?;
        if let Some(address) = cached_pairing_target(e, target)? {
            return Ok(address);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::new(
                "PAIRING_NOT_FOUND",
                "registration endpoint not discovered",
            ));
        }
        tokio::time::sleep(PAIR_LOOKUP_POLL_INTERVAL).await;
    }
}
