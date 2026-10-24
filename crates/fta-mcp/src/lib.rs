//! Official MCP transport and lifecycle wrap the existing private FTA IPC, not a second engine.

pub mod paths;

use clap::Parser;
use fta_core::{Error, Result, config::AppPaths, ipc};
use paths::FilePolicy;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::tool::schema_for_input,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
    transport::{async_rw::AsyncRwTransport, io::stdio_pipes},
};
use schemars::JsonSchema;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

const DEFAULT_INBOX_LIMIT: u32 = 20;
const MAX_INBOX_LIMIT: u32 = 1000;
const MAX_CONCURRENT_CALLS: usize = 4;
const CANCEL_RETRY_MILLIS: u64 = 50;
const CANCEL_DEADLINE_MILLIS: u64 = 3000;
const SERVICE_STOP_SECONDS: u64 = 3;
const ALLOW_DIRS_ENV: &str = "FTA_MCP_ALLOW_DIRS";

/// File access is opt-in; a text-only integration does not need any authorized file roots.
#[derive(Parser, Debug)]
#[command(
    name = "fta-mcp",
    version,
    about = "FTA local stdio MCP server for registered personal devices"
)]
pub struct Args {
    #[arg(long)]
    pub config_dir: Option<PathBuf>,
    #[arg(long)]
    pub state_dir: Option<PathBuf>,
    #[arg(long)]
    pub runtime_dir: Option<PathBuf>,
    #[arg(long = "allow-dir", value_name = "DIRECTORY")]
    pub allow_dirs: Vec<PathBuf>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SendFiles {
    /// Registered device ID or an unambiguous ID prefix from fta_list_devices.
    to: String,
    /// Absolute paths to 1..100 ordinary files inside explicitly authorized directories.
    paths: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SendText {
    /// Registered online device ID or an unambiguous ID prefix.
    to: String,
    /// UTF-8 text; maximum 1 MiB, preserving line breaks.
    text: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SendUrl {
    /// Registered online device ID or an unambiguous ID prefix.
    to: String,
    /// One HTTP or HTTPS URL, delivered as content and never automatically opened.
    url: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListInbox {
    /// Maximum records to return, from 1 to 1000; defaults to 20.
    limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TransferId {
    /// Transfer ID from a send result or fta_list_inbox.
    transfer_id: String,
}

/// One process shares immutable authorization policy and bounds engine calls without a queue.
#[derive(Clone)]
pub struct FtaMcp {
    paths: AppPaths,
    files: FilePolicy,
    tools: Arc<Vec<Tool>>,
    calls: Arc<Semaphore>,
    active_transfers: Arc<Mutex<HashSet<String>>>,
}

impl FtaMcp {
    pub fn new(paths: AppPaths, allowed_roots: Vec<PathBuf>) -> Result<Self> {
        Ok(Self {
            paths,
            files: FilePolicy::new(allowed_roots)?,
            tools: Arc::new(tool_definitions()?),
            calls: Arc::new(Semaphore::new(MAX_CONCURRENT_CALLS)),
            active_transfers: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Tool names select fixed IPC methods; MCP arguments cannot choose a shell, URL, or database query.
    async fn execute_tool(
        &self,
        name: &str,
        args: Value,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        if cancellation.is_cancelled() {
            return Err(Error::new(
                "CANCELLED",
                "request was cancelled before dispatch",
            ));
        }
        match name {
            "fta_status" => {
                parse::<NoArgs>(args)?;
                self.query("status", json!({}), cancellation).await
            }
            "fta_list_devices" => {
                parse::<NoArgs>(args)?;
                self.query("devices.list", json!({}), cancellation).await
            }
            "fta_send_files" => {
                let args: SendFiles = parse(args)?;
                let paths = self.files.validate_files(&args.paths)?;
                let to = self.online_peer(&args.to, cancellation.clone()).await?;
                self.send(json!({"to":to,"kind":"files","paths":paths,"allowed_roots":self.files.allowed_roots()}), cancellation).await
            }
            "fta_send_text" => {
                let args: SendText = parse(args)?;
                check_text(&args.text)?;
                let to = self.online_peer(&args.to, cancellation.clone()).await?;
                self.send(
                    json!({"to":to,"kind":"text","body":args.text}),
                    cancellation,
                )
                .await
            }
            "fta_send_url" => {
                let args: SendUrl = parse(args)?;
                check_text(&args.url)?;
                fta_core::network::validate_url(&args.url)?;
                let to = self.online_peer(&args.to, cancellation.clone()).await?;
                self.send(json!({"to":to,"kind":"url","body":args.url}), cancellation)
                    .await
            }
            "fta_list_inbox" => {
                let args: ListInbox = parse(args)?;
                let limit = args.limit.unwrap_or(DEFAULT_INBOX_LIMIT);
                if !(1..=MAX_INBOX_LIMIT).contains(&limit) {
                    return Err(Error::new(
                        "INVALID_REQUEST",
                        "limit must be between 1 and 1000",
                    ));
                }
                self.query("inbox.list", json!({"limit":limit}), cancellation)
                    .await
            }
            "fta_get_transfer" => {
                let args: TransferId = parse(args)?;
                check_id(&args.transfer_id)?;
                self.query(
                    "transfers.get",
                    json!({"transfer_id":args.transfer_id}),
                    cancellation,
                )
                .await
            }
            "fta_cancel_transfer" => {
                let args: TransferId = parse(args)?;
                check_id(&args.transfer_id)?;
                self.query(
                    "transfers.cancel",
                    json!({"transfer_id":args.transfer_id}),
                    cancellation,
                )
                .await
            }
            _ => Err(Error::new("UNKNOWN_TOOL", "unknown FTA MCP tool")),
        }
    }

    async fn query(
        &self,
        method: &str,
        params: Value,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        tokio::select! {
            result = ipc::request(&self.paths, method, params) => result,
            _ = cancellation.cancelled() => Err(Error::new("CANCELLED", "request was cancelled")),
        }
    }

    /// A discovery advertisement cannot authorize a send: require a unique registered, authenticated peer.
    async fn online_peer(&self, target: &str, cancellation: CancellationToken) -> Result<String> {
        check_id(target)?;
        let devices = self.query("devices.list", json!({}), cancellation).await?;
        let devices = devices
            .as_array()
            .ok_or_else(|| Error::new("IPC_ERROR", "invalid device-list response"))?;
        let matches: Vec<_> = devices
            .iter()
            .filter(|device| {
                device
                    .get("device_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.starts_with(target))
            })
            .collect();
        if matches.len() != 1 {
            return Err(Error::new(
                "UNTRUSTED",
                "device ID must identify exactly one registered device",
            ));
        }
        let device = matches[0];
        if device.get("paired").and_then(Value::as_bool) != Some(true) {
            return Err(Error::new(
                "UNTRUSTED",
                "register this device in FTA before sending",
            ));
        }
        if device.get("online").and_then(Value::as_bool) != Some(true) {
            return Err(Error::new(
                "OFFLINE",
                "target device is not online; offline delivery is not queued",
            ));
        }
        device
            .get("device_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Error::new("IPC_ERROR", "missing device ID"))
    }

    async fn send(&self, mut params: Value, cancellation: CancellationToken) -> Result<Value> {
        let transfer_id = uuid::Uuid::new_v4().to_string();
        params["transfer_id"] = json!(transfer_id);
        self.active_transfers
            .lock()
            .await
            .insert(transfer_id.clone());
        let result = tokio::select! {
            result = ipc::request(&self.paths, "transfers.send", params) => result,
            _ = cancellation.cancelled() => {
                // Closing the wait alone would leave a remote transfer running; cancel by its stable ID.
                if let Err(error) = self.cancel_pending(&transfer_id).await {
                    eprintln!("MCP request cancellation incomplete: {}", error.code);
                }
                Err(Error::new("CANCELLED", "request cancelled; inspect the transfer record for the final receiver result"))
            }
        };
        self.active_transfers.lock().await.remove(&transfer_id);
        result
    }

    async fn cancel_pending(&self, id: &str) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(CANCEL_DEADLINE_MILLIS);
        loop {
            // A cancellation can race the engine's initial record insert; retry only this cancellation.
            match tokio::time::timeout_at(
                deadline,
                ipc::request(&self.paths, "transfers.cancel", json!({"transfer_id":id})),
            )
            .await
            {
                Ok(Ok(_)) => return Ok(()),
                Ok(Err(error))
                    if error.code == "NOT_FOUND" && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(CANCEL_RETRY_MILLIS)).await;
                }
                Ok(Err(error)) => {
                    eprintln!("MCP transfer cancellation failed: {}", error.code);
                    return Err(error);
                }
                Err(_) => {
                    return Err(Error::new(
                        "TIMEOUT",
                        "transfer cancellation was not confirmed before shutdown",
                    ));
                }
            }
        }
    }

    async fn cancel_remaining(&self) {
        let active: Vec<_> = self.active_transfers.lock().await.iter().cloned().collect();
        let mut cancellations = tokio::task::JoinSet::new();
        for id in active {
            // Cancel bounded active calls concurrently so shutdown time does not grow per transfer.
            let server = self.clone();
            cancellations.spawn(async move { server.cancel_pending(&id).await });
        }
        while let Some(result) = cancellations.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("MCP shutdown cancellation incomplete: {}", error.code),
                Err(_) => eprintln!("MCP shutdown cancellation task failed"),
            }
        }
    }
}

impl ServerHandler for FtaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("fta-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("FTA transfers files, text and URLs to registered online personal computers on the home LAN. Sends immediately deliver content to the target without a receiver approval. Only send when the user requests it. File sends require explicit allow directories configured when this server starts. No external network delivery, device registration or automatic URL execution is provided by these tools.")
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        if request.and_then(|request| request.cursor).is_some() {
            return Err(McpError::invalid_params(
                "FTA tools fit on one page; no cursor is valid",
                None,
            ));
        }
        Ok(ListToolsResult::with_all_items(self.tools.as_ref().clone())
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, McpError> {
        if self.get_tool(&request.name).is_none() {
            return Err(McpError::invalid_params("unknown FTA MCP tool", None));
        }
        let _permit = match self.calls.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return Ok(tool_error(Error::new(
                    "BUSY",
                    "FTA MCP concurrent-call limit reached; retry explicitly",
                ))
                .into());
            }
        };
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = self.execute_tool(&request.name, args, context.ct).await;
        Ok(match result {
            Ok(value) => tool_result(&request.name, value),
            Err(error) => tool_error(error),
        }
        .into())
    }
}

/// Official SDK transport handles initialization, JSON-RPC, version negotiation and cancellation routing.
pub async fn run_stdio(args: Args) -> Result<()> {
    let mut paths = AppPaths::discover()?;
    if let Some(path) = args.config_dir {
        paths.config_dir = absolute(path)?;
    }
    if let Some(path) = args.state_dir {
        paths.state_dir = absolute(path)?;
    }
    if let Some(path) = args.runtime_dir {
        paths.runtime_dir = absolute(path)?;
    }
    let mut roots = args.allow_dirs;
    if let Some(value) = std::env::var_os(ALLOW_DIRS_ENV) {
        roots.extend(std::env::split_paths(&value).filter(|path| !path.as_os_str().is_empty()));
    }
    let server = FtaMcp::new(paths, roots)?;
    let cleanup = server.clone();
    let (read, write) = stdio_pipes();
    let transport = AsyncRwTransport::<RoleServer, _, _>::new_server(read, write)
        .with_max_line_length(fta_core::config::MAX_CONTROL_BYTES);
    let cancellation = CancellationToken::new();
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let service = tokio::select! {
        service = server.serve_with_ct(transport, cancellation.clone()) => service.map_err(|_| Error::new("MCP_INITIALIZE_ERROR", "MCP initialization failed"))?,
        signal = &mut shutdown => { signal?; return Ok(()); },
    };
    let waiting = service.waiting();
    tokio::pin!(waiting);
    tokio::select! {
        result = &mut waiting => { result.map_err(|_| Error::new("MCP_SERVICE_ERROR", "MCP service task failed"))?; },
        signal = &mut shutdown => {
            signal?; cancellation.cancel();
            let (stopped, ()) = tokio::join!(
                tokio::time::timeout(Duration::from_secs(SERVICE_STOP_SECONDS), &mut waiting),
                cleanup.cancel_remaining()
            );
            if !matches!(stopped, Ok(Ok(_))) { eprintln!("MCP service did not confirm clean shutdown within its grace period"); }
            return Ok(());
        },
    }
    cleanup.cancel_remaining().await;
    Ok(())
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result=tokio::signal::ctrl_c()=>result?, _=terminate.recv()=>{} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T> {
    // Do not echo deserializer diagnostics: unexpected values may contain a private message body.
    serde_json::from_value(args).map_err(|_| {
        Error::new(
            "INVALID_REQUEST",
            "tool arguments do not match the advertised schema",
        )
    })
}
fn check_id(id: &str) -> Result<()> {
    if id.trim().is_empty() || id.len() > 128 {
        return Err(Error::new(
            "INVALID_REQUEST",
            "device or transfer ID must contain 1..128 bytes",
        ));
    }
    Ok(())
}
fn check_text(text: &str) -> Result<()> {
    if text.len() > fta_core::config::MAX_MESSAGE_BYTES {
        return Err(Error::new(
            "TOO_LARGE",
            "text or URL exceeds the 1 MiB limit",
        ));
    }
    Ok(())
}
fn absolute(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// Partial and uncertain results remain visible, rather than turning successful IPC into false delivery success.
fn tool_result(name: &str, value: Value) -> CallToolResult {
    let state = value.get("state").and_then(Value::as_str);
    let failed = if name == "fta_cancel_transfer" {
        matches!(state, Some("unknown" | "failed" | "cancel_requested"))
    } else {
        matches!(state, Some("partial" | "unknown" | "failed" | "cancelled"))
    };
    let value = json!({"ok":!failed,"result":value});
    if failed {
        CallToolResult::structured_error(value)
    } else {
        CallToolResult::structured(value)
    }
}
fn tool_error(error: Error) -> CallToolResult {
    CallToolResult::structured_error(
        json!({"ok":false,"error":{"code":error.code,"message":error.message}}),
    )
}

fn definition<T: JsonSchema + 'static>(
    name: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    external: bool,
) -> Result<Tool> {
    let input = schema_for_input::<T>()
        .map_err(|_| Error::new("SCHEMA_ERROR", "cannot construct tool schema"))?;
    let output = json!({"type":"object","properties":{"ok":{"type":"boolean"},"result":{},"error":{"type":"object","properties":{"code":{"type":"string"},"message":{"type":"string"}},"required":["code","message"]}},"required":["ok"]});
    let output = output
        .as_object()
        .cloned()
        .ok_or_else(|| Error::new("SCHEMA_ERROR", "invalid output schema"))?;
    Ok(Tool::new(name, description, input)
        .with_raw_output_schema(Arc::new(output))
        .with_annotations(
            ToolAnnotations::new()
                .read_only(read_only)
                .destructive(destructive)
                .idempotent(idempotent)
                .open_world(external),
        ))
}
fn tool_definitions() -> Result<Vec<Tool>> {
    Ok(vec![
        definition::<NoArgs>(
            "fta_status",
            "Read the local FTA engine status and authenticated online peer count. Does not start the engine.",
            true,
            false,
            true,
            true,
        )?,
        definition::<NoArgs>(
            "fta_list_devices",
            "List home-LAN devices with IDs, registration and authenticated online status. A discovered unregistered device cannot receive sends.",
            true,
            false,
            true,
            true,
        )?,
        definition::<SendFiles>(
            "fta_send_files",
            "Immediately send ordinary files from configured allow directories to one registered online personal computer. This delivers content outside the current computer; the receiver saves it automatically without approval. No offline queue.",
            false,
            false,
            false,
            true,
        )?,
        definition::<SendText>(
            "fta_send_text",
            "Immediately deliver UTF-8 text to one registered online personal computer without receiver approval. This sends content outside this computer; it does not synchronize the clipboard.",
            false,
            false,
            false,
            true,
        )?,
        definition::<SendUrl>(
            "fta_send_url",
            "Immediately deliver an HTTP/HTTPS URL to one registered online personal computer without receiver approval. The receiving app shows it as content and does not automatically open it.",
            false,
            false,
            false,
            true,
        )?,
        definition::<ListInbox>(
            "fta_list_inbox",
            "Read bounded local transfer-history summaries without full message bodies. Use fta_get_transfer for a specific message or transfer result.",
            true,
            false,
            true,
            false,
        )?,
        definition::<TransferId>(
            "fta_get_transfer",
            "Read a transfer record, including received text. For an unknown sender result, query the registered receiver to reconcile status; no content is resent.",
            true,
            false,
            true,
            true,
        )?,
        definition::<TransferId>(
            "fta_cancel_transfer",
            "Request cancellation of one transfer. Uncommitted data may be discarded; already completed files remain. A pending result is not confirmation of cancellation.",
            false,
            true,
            true,
            true,
        )?,
    ])
}
