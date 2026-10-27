//! Recipient-only first-use approval; payloads remain local until both certificate pins are stored.
use super::*;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

const MAX_PENDING_SENDS: usize = MAX_STREAMS;
const DECLINE_COOLDOWN_SECONDS: u64 = 30;
const MAX_DECLINES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub transfer_id: String,
    pub kind: String,
    pub item_count: usize,
}

pub(crate) struct PendingSend {
    peer_id: String,
    kind: String,
    cancel: CancellationToken,
}

#[derive(Default)]
pub(crate) struct State {
    pub opening: tokio::sync::Mutex<()>,
    sends: std::sync::Mutex<HashMap<String, PendingSend>>,
    declines: std::sync::Mutex<HashMap<String, u64>>,
}

// A dropped IPC operation must not leave a pending intent or an uncancellable stale ID.
struct PendingGuard<'a>(&'a Engine, String);
impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut sends) = self.0.registration.sends.lock() {
            sends.remove(&self.1);
        }
        // Cancelled approval attempts have no durable transfer row and must not accumulate IDs.
        if self
            .0
            .store
            .transfer(&self.1)
            .is_err_and(|error| error.code == "NOT_FOUND")
            && let Ok(mut cancelled) = self.0.cancelled.lock()
        {
            cancelled.remove(&self.1);
        }
    }
}

pub fn validate_intent(intent: &Intent) -> Result<()> {
    uuid::Uuid::parse_str(&intent.transfer_id)
        .map_err(|_| Error::new("INVALID_REQUEST", "invalid transfer UUID"))?;
    if !["files", "text", "url"].contains(&intent.kind.as_str())
        || (intent.kind == "files" && (intent.item_count == 0 || intent.item_count > MAX_ITEMS))
        || (intent.kind != "files" && intent.item_count != 0)
    {
        return Err(Error::new("INVALID_REQUEST", "invalid registration intent"));
    }
    Ok(())
}

// Rejection is throttled by the proven TLS public key, so changing a displayed name/ID cannot spam prompts.
pub fn check_request(e: &Engine, peer: &Device) -> Result<()> {
    if peer.device_id == e.config()?.device_id
        || peer.display_name.chars().any(char::is_control)
        || peer.os.len() > 32
        || peer.os.chars().any(char::is_control)
    {
        return Err(Error::new("INVALID_REQUEST", "invalid registration peer"));
    }
    let cert = hex::decode(&peer.certificate)
        .map_err(|_| Error::new("UNTRUSTED", "invalid peer certificate"))?;
    let key = public_key_fingerprint(&cert)?;
    let mut declines = e.registration.declines.lock().map_err(|_| lock_error())?;
    declines.retain(|_, expires| *expires > now());
    if declines.contains_key(&key) {
        return Err(Error::new("BUSY", "registration was recently declined"));
    }
    if declines.len() >= MAX_DECLINES {
        return Err(Error::new("BUSY", "registration request limit reached"));
    }
    Ok(())
}

// This task only opens the isolated registration API; ordinary transfer authentication stays pinned.
pub async fn listen(e: Arc<Engine>) -> Result<()> {
    loop {
        let config = e.config()?;
        if config.discovery_enabled
            && !config.listen_addr.ip().is_loopback()
            && let Err(error) = pair_open(&e).await
            && error.code != "BUSY"
        {
            eprintln!("registration readiness failed: {}", error.code);
        }
        tokio::select! {
            _ = e.network_token.cancelled() => return Ok(()),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
    }
}

// Local approval UI/CLI receives metadata only, never certificate blobs, nonces, paths or content.
pub fn requests(e: &Engine) -> Result<Value> {
    let sessions = e.sessions.lock().map_err(|_| lock_error())?;
    Ok(json!(sessions.values().filter(|s| !s.initiated && s.state == "compare" && !s.local_confirmed && s.expires_at >= now() && s.approval.is_some() && s.peer.is_some())
        .map(|s| { let peer = s.peer.as_ref().unwrap(); json!({"session_id":s.session_id,"device_id":peer.device_id,"display_name":peer.display_name,"os":peer.os,"address":peer.address,"kind":s.approval.as_ref().unwrap().kind,"item_count":s.approval.as_ref().unwrap().item_count,"expires_at":s.expires_at}) }).collect::<Vec<_>>()))
}

pub async fn decide(e: &Arc<Engine>, id: &str, accept: bool) -> Result<Value> {
    let session = active_session(e, id)?;
    if session.initiated
        || session.state != "compare"
        || session.local_confirmed
        || session.approval.is_none()
        || session.peer.is_none()
    {
        return Err(Error::new(
            "INVALID_REQUEST",
            "not an incoming first-send request",
        ));
    }
    if accept {
        return pair_confirm(e, id).await;
    }
    let certificate = hex::decode(&session.peer.as_ref().unwrap().certificate)
        .map_err(|_| Error::new("UNTRUSTED", "invalid certificate"))?;
    let key = public_key_fingerprint(&certificate)?;
    e.registration
        .declines
        .lock()
        .map_err(|_| lock_error())?
        .insert(key, now() + DECLINE_COOLDOWN_SECONDS);
    // Keep an authenticated status tombstone long enough for the sender to observe the decision.
    let mut sessions = e.sessions.lock().map_err(|_| lock_error())?;
    let current = sessions
        .get_mut(id)
        .ok_or_else(|| Error::new("NOT_FOUND", "registration expired"))?;
    if current.local_confirmed || current.state != "compare" {
        return Err(Error::new("CONFLICT", "registration already decided"));
    }
    current.state = "rejected".into();
    Ok(json!({"rejected":true}))
}

pub fn pending(e: &Engine, id: &str) -> Result<Option<Value>> {
    Ok(e.registration.sends.lock().map_err(|_| lock_error())?.get(id).map(|s| json!({"transfer_id":id,"peer_id":s.peer_id,"direction":"send","kind":s.kind,"state":if s.cancel.is_cancelled(){"cancelled"}else{"awaiting_registration"},"items":[]})))
}

pub fn cancel_pending(e: &Engine, id: &str) -> Result<Option<Value>> {
    if let Some(send) = e
        .registration
        .sends
        .lock()
        .map_err(|_| lock_error())?
        .get(id)
    {
        send.cancel.cancel();
    }
    pending(e, id)
}

// Remote cancellation is best effort; failure never converts a cancelled send into a queued delivery.
async fn cancel_session(e: &Engine, id: &str) {
    if let Ok(session) = active_session(e, id)
        && let Some(peer) = &session.peer
        && let Ok(certificate) = hex::decode(&peer.certificate)
        && let Ok(client) = client(e, vec![certificate], false)
    {
        let _ = tokio::time::timeout(
            Duration::from_secs(CONNECT_SECONDS),
            client
                .post(format!("https://{}/pair/cancel", session.address))
                .json(&json!({"session_id":id}))
                .send(),
        )
        .await;
    }
    if let Ok(mut sessions) = e.sessions.lock() {
        sessions.remove(id);
    }
}

pub async fn send(e: &Arc<Engine>, mut params: Value) -> Result<Value> {
    e.ensure_network()?;
    let target = required(&params, "to")?.to_string();
    // Already trusted peers follow the unchanged normal send path, including MCP-compatible checks.
    match e.store.device(&target) {
        Ok(_) => return super::send(e, params).await,
        Err(error) if error.code == "UNTRUSTED" => {}
        Err(error) => return Err(error),
    }
    if !e
        .discovered
        .lock()
        .map_err(|_| lock_error())?
        .contains_key(&target)
    {
        return Err(Error::new("OFFLINE", "target is no longer discovered"));
    }
    let id = params
        .get("transfer_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let kind = required(&params, "kind")?.to_string();
    let count = if kind == "files" {
        params
            .get("paths")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new("INVALID_REQUEST", "file paths required"))?
            .len()
    } else {
        0
    };
    let intent = Intent {
        transfer_id: id.clone(),
        kind: kind.clone(),
        item_count: count,
    };
    validate_intent(&intent)?;
    if kind != "files" {
        let body = required(&params, "body")?;
        if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
            return Err(Error::new("TOO_LARGE", "invalid message size"));
        }
        if kind == "url" {
            validate_url(body)?;
        }
    }
    let cancel = CancellationToken::new();
    {
        let mut sends = e.registration.sends.lock().map_err(|_| lock_error())?;
        if sends.len() >= MAX_PENDING_SENDS
            || sends.contains_key(&id)
            || e.store.transfer(&id).is_ok()
        {
            return Err(Error::new("BUSY", "another first-send request is pending"));
        }
        sends.insert(
            id.clone(),
            PendingSend {
                peer_id: target.clone(),
                kind,
                cancel: cancel.clone(),
            },
        );
    }
    let _guard = PendingGuard(e, id.clone());
    params["transfer_id"] = json!(id);
    let connected = tokio::select! {
        result = pair_connect_mode(e, &target, Some(intent)) => result?,
        _ = cancel.cancelled() => return Err(Error::new("CANCELLED", "registration request cancelled")),
        _ = e.network_token.cancelled() => return Err(Error::new("NETWORK_CHANGED", "home network changed")),
    };
    let session_id = required(&connected, "session_id")?.to_string();
    if connected["peer"]["device_id"].as_str() != Some(target.as_str()) {
        cancel_session(e, &session_id).await;
        return Err(Error::new(
            "KEY_CHANGED",
            "registration peer differs from discovered target",
        ));
    }
    let approved: Result<()> = async {
        // Sending is the sender's consent; only the recipient is prompted before any payload leaves.
        pair_confirm(e, &session_id).await?;
        // Bound consent locally even if a remote clock or response supplies a later expiry.
        let deadline = tokio::time::sleep(Duration::from_secs(PAIR_SECONDS));
        tokio::pin!(deadline);
        loop {
            let session = active_session(e, &session_id)?;
            if session.state == "rejected" { return Err(Error::new("REGISTRATION_REJECTED", "recipient declined registration")); }
            if session.state == "expired" { return Err(Error::new("REGISTRATION_EXPIRED", "recipient approval expired")); }
            if session.state == "paired" { return Ok(()); }
            tokio::select! {
                _ = &mut deadline => return Err(Error::new("REGISTRATION_EXPIRED", "recipient approval expired")),
                _ = cancel.cancelled() => return Err(Error::new("CANCELLED", "registration request cancelled")),
                _ = e.network_token.cancelled() => return Err(Error::new("NETWORK_CHANGED", "home network changed")),
                _ = tokio::time::sleep(REVOCATION_POLL_INTERVAL) => {}
            }
        }
    }.await;
    if let Err(mut error) = approved {
        cancel_session(e, &session_id).await;
        if error.code == "TIMEOUT" {
            error = Error::new("REGISTRATION_EXPIRED", "recipient approval expired");
        }
        return Err(error);
    }
    if cancel.is_cancelled() {
        return Err(Error::new("CANCELLED", "send cancelled after approval"));
    }
    // The existing sender revalidates files and online identity, then waits for final durable storage.
    super::send(e, params).await
}
