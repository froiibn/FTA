//! The desktop bridge stays separate from the engine so headless installs need no WebView.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use fta_core::{config::AppPaths, ipc};
use serde_json::{Value, json};
use tauri::{
    DragDropEvent, Emitter, Manager, State, WindowEvent,
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

mod drop_coordinates;

#[cfg(target_os = "macos")]
mod native_notification;

const ENGINE_EXECUTABLE: &str = if cfg!(windows) {
    "fta-engine.exe"
} else {
    "fta-engine"
};
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const ENGINE_START_ATTEMPTS: usize = 30;
const ENGINE_START_DELAY: Duration = Duration::from_millis(100);
const ENGINE_STOP_ATTEMPTS: usize = 50;
const ENGINE_STOP_DELAY: Duration = Duration::from_millis(200);
const ENGINE_SHUTDOWN_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const EVENT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const EVENT_CONSUMER: &str = "desktop-notifications";
const ACTIVATION_SCHEME: &str = "fta";
const MAX_PENDING_ACTIVATIONS: usize = 32;
const WINDOW_LABEL: &str = "main";
const FOLDED_WINDOW_SIDE: f64 = 44.0;

// Expanded geometry is kept separately so a collapsed badge can always return to the user's working size.
#[derive(Default)]
struct FoldState {
    collapsed: bool,
    expanded_size: Option<tauri::LogicalSize<f64>>,
}

/// Narrow grants prevent a compromised UI from turning a generic IPC bridge into file exfiltration.
#[derive(Default)]
struct DesktopState {
    granted_paths: Mutex<HashSet<PathBuf>>,
    fold: Mutex<FoldState>,
    child: Mutex<Option<Child>>,
    notifications_ready: AtomicBool,
    exit_allowed: AtomicBool,
    lifecycle_busy: AtomicBool,
    pending_activations: Mutex<VecDeque<String>>,
}

/// Serialize restarts and exits so a second click cannot exit while the first is still draining files.
struct LifecycleGuard<'a>(&'a AtomicBool);
impl<'a> LifecycleGuard<'a> {
    fn acquire(state: &'a DesktopState) -> Result<Self, String> {
        state
            .lifecycle_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "서비스 시작·종료 처리가 진행 중입니다")?;
        Ok(Self(&state.lifecycle_busy))
    }
}
impl Drop for LifecycleGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Recoverable engine errors reach the UI instead of terminating the tray process.
async fn request(method: &str, params: Value) -> Result<Value, String> {
    let paths = AppPaths::discover().map_err(|error| error.to_string())?;
    ipc::request(&paths, method, params)
        .await
        .map_err(|error| error.to_string())
}

/// Read operations and explicit settings/pairing actions are the only generic bridge surface.
#[tauri::command]
async fn engine_request(method: String, params: Value) -> Result<Value, String> {
    match method.as_str() {
        "status" | "devices.list" | "devices.probe" | "devices.unpair" | "inbox.list"
        | "inbox.get" | "inbox.delete" | "settings.get" | "settings.update" | "pairing.open"
        | "pairing.connect" | "pairing.confirm" | "pairing.cancel" | "pairing.status"
        | "transfers.get" | "transfers.cancel" => request(&method, params).await,
        _ => Err("이 데스크톱 명령은 허용되지 않습니다".into()),
    }
}

// Secondary windows must not borrow file grants or consume notification work owned by the transfer window.
fn require_main_window(label: &str) -> Result<(), String> {
    if label == WINDOW_LABEL {
        Ok(())
    } else {
        Err("이 작업은 전송 창에서만 사용할 수 있습니다".into())
    }
}

/// Canonicalize grants at the OS-selection boundary, not from paths invented by JavaScript.
fn grant_paths(state: &DesktopState, paths: Vec<PathBuf>) -> Result<Vec<String>, String> {
    let canonical = paths
        .into_iter()
        .map(|path| {
            let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(format!(
                    "일반 파일만 전송할 수 있습니다: {}",
                    path.display()
                ));
            }
            path.canonicalize().map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut grants = state
        .granted_paths
        .lock()
        .map_err(|_| "파일 선택 상태 오류")?;
    grants.clear();
    grants.extend(canonical.iter().cloned());
    canonical
        .iter()
        .map(|path| {
            path.to_str()
                .map(str::to_owned)
                .ok_or_else(|| "UTF-8로 표현할 수 없는 파일 경로입니다".into())
        })
        .collect()
}

/// The native picker runs outside WebView filesystem permissions and grants only chosen files.
#[tauri::command]
async fn choose_files(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Vec<String>, String> {
    require_main_window(window.label())?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_files(move |selection| {
        let _ = sender.send(selection);
    });
    let selected = receiver.await.map_err(|_| "파일 선택이 취소되었습니다")?;
    let paths = selected
        .unwrap_or_default()
        .into_iter()
        .map(|file| file.into_path().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    grant_paths(&state, paths)
}

/// Revalidate selected paths when sending; selection never grants directory or wildcard access.
#[tauri::command]
async fn send_files(
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
    to: String,
    paths: Vec<String>,
    transfer_id: String,
) -> Result<Value, String> {
    require_main_window(window.label())?;
    if paths.is_empty() {
        return Err("전송할 파일을 선택해주세요".into());
    }
    {
        let grants = state
            .granted_paths
            .lock()
            .map_err(|_| "파일 선택 상태 오류")?;
        for path in &paths {
            let path = Path::new(path);
            let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || !grants.contains(path) {
                return Err("파일 선택 또는 드롭으로 허용한 일반 파일만 전송할 수 있습니다".into());
            }
        }
    }
    request(
        "transfers.send",
        json!({ "to": to, "kind": "files", "paths": paths, "transfer_id": transfer_id }),
    )
    .await
}

/// Message limits and URL schemes are checked before forwarding content to the engine.
#[tauri::command]
async fn send_message(
    window: tauri::WebviewWindow,
    to: String,
    kind: String,
    body: String,
    transfer_id: String,
) -> Result<Value, String> {
    require_main_window(window.label())?;
    if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
        return Err("메시지는 비어 있지 않은 1 MiB 이하의 텍스트여야 합니다".into());
    }
    match kind.as_str() {
        "text" => {}
        "url" => {
            validated_url(&body)?;
        }
        _ => return Err("지원하지 않는 메시지 종류입니다".into()),
    }
    request(
        "transfers.send",
        json!({ "to": to, "kind": kind, "body": body, "transfer_id": transfer_id }),
    )
    .await
}

/// Received text must not turn file:, javascript: or another scheme into an executable action.
fn validated_url(value: &str) -> Result<url::Url, String> {
    if value.chars().any(char::is_whitespace) {
        return Err("공백 없는 단일 HTTP 또는 HTTPS 주소만 열 수 있습니다".into());
    }
    let parsed = url::Url::parse(value).map_err(|_| "올바른 URL이 아닙니다")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("HTTP 또는 HTTPS 주소만 열 수 있습니다".into());
    }
    Ok(parsed)
}

/// URL opening always comes from an explicit user action, never a receipt event.
#[tauri::command]
fn open_link(app: tauri::AppHandle, value: String) -> Result<(), String> {
    let parsed = validated_url(&value)?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|error| error.to_string())
}

/// The host resolves a transfer ID so the UI cannot ask the opener to run an arbitrary local path.
#[tauri::command]
async fn reveal_transfer(app: tauri::AppHandle, transfer_id: String) -> Result<(), String> {
    let record = request("inbox.get", json!({ "transfer_id": transfer_id })).await?;
    let path = record
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find_map(|item| item.get("path").and_then(Value::as_str))
        })
        .ok_or("저장된 파일 경로를 찾을 수 없습니다")?;
    if !Path::new(path).exists() {
        return Err("파일이 이동되었거나 삭제되었습니다".into());
    }
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| error.to_string())
}

/// Clipboard contents are changed only after the user presses Copy.
#[tauri::command]
fn copy_text(app: tauri::AppHandle, value: String) -> Result<(), String> {
    app.clipboard()
        .write_text(value)
        .map_err(|error| error.to_string())
}

// Native size and UI visibility change as one operation; a failed resize must not leave an invisible panel.
fn apply_fold_state(window: &tauri::WebviewWindow, folded: bool) -> Result<bool, String> {
    require_main_window(window.label())?;
    let state = window.state::<DesktopState>();
    let mut fold = state.fold.lock().map_err(|_| "창 접기 상태 오류")?;
    if fold.collapsed == folded {
        return Ok(fold.collapsed);
    }
    let config = window.app_handle().config();
    let defaults = config
        .app
        .windows
        .iter()
        .find(|entry| entry.label == WINDOW_LABEL)
        .ok_or("전송 창 설정을 찾을 수 없습니다")?;
    window.unmaximize().map_err(|error| error.to_string())?;
    let current = window
        .inner_size()
        .map_err(|error| error.to_string())?
        .to_logical::<f64>(window.scale_factor().map_err(|error| error.to_string())?);
    let expanded = fold
        .expanded_size
        .unwrap_or(tauri::LogicalSize::new(defaults.width, defaults.height));
    let target = if folded {
        tauri::LogicalSize::new(FOLDED_WINDOW_SIDE, FOLDED_WINDOW_SIDE)
    } else {
        expanded
    };
    let expanded_min = tauri::LogicalSize::new(
        defaults.min_width.unwrap_or(defaults.width),
        defaults.min_height.unwrap_or(defaults.height),
    );
    window
        .set_min_size(Some(if folded {
            tauri::LogicalSize::new(FOLDED_WINDOW_SIDE, FOLDED_WINDOW_SIDE)
        } else {
            expanded_min
        }))
        .map_err(|error| error.to_string())?;
    if let Err(error) = window.set_size(target) {
        let _ = window.set_min_size(Some(if fold.collapsed {
            tauri::LogicalSize::new(FOLDED_WINDOW_SIDE, FOLDED_WINDOW_SIDE)
        } else {
            expanded_min
        }));
        return Err(error.to_string());
    }
    if let Err(error) = window.set_resizable(!folded) {
        let _ = window.set_min_size(Some(if fold.collapsed {
            tauri::LogicalSize::new(FOLDED_WINDOW_SIDE, FOLDED_WINDOW_SIDE)
        } else {
            expanded_min
        }));
        let _ = window.set_size(current);
        return Err(error.to_string());
    }
    if folded {
        fold.expanded_size = Some(current);
    }
    fold.collapsed = folded;
    // A tray/notification can unfold natively while the frontend is busy; it must update the same visible state.
    if let Err(error) = window.emit_to(WINDOW_LABEL, "fta:fold-state", folded) {
        eprintln!("failed to synchronize folded window state: {error}");
    }
    Ok(fold.collapsed)
}

#[tauri::command]
fn set_folded(window: tauri::WebviewWindow, folded: bool) -> Result<bool, String> {
    apply_fold_state(&window, folded)
}

// Readiness reconnects must learn the real native state instead of assuming the window is expanded.
#[tauri::command]
fn fold_state(
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<bool, String> {
    require_main_window(window.label())?;
    Ok(state
        .fold
        .lock()
        .map_err(|_| "창 접기 상태 오류")?
        .collapsed)
}

// Replacing system decorations exposes only move and close operations for bundled window roles.
#[tauri::command]
fn start_window_drag(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != WINDOW_LABEL {
        utility_label(window.label())?;
    }
    window.start_dragging().map_err(|error| error.to_string())
}
#[tauri::command]
fn close_app_window(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != WINDOW_LABEL {
        utility_label(window.label())?;
    }
    window.close().map_err(|error| error.to_string())
}

/// A pinned transfer surface is an app window, not a separate platform widget extension.
#[tauri::command]
fn set_pinned(window: tauri::WebviewWindow, pinned: bool) -> Result<(), String> {
    window
        .set_always_on_top(pinned)
        .map_err(|error| error.to_string())
}

/// Startup registration is explicit; installation never silently enables login execution.
#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|error| error.to_string())
}

/// Read the OS registration rather than guessing startup state from a UI preference.
#[tauri::command]
fn autostart_status(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

/// Notifications may arrive while the panel is hidden, so opening detail must first reveal it.
#[tauri::command]
fn show_window(app: tauri::AppHandle) {
    show_main(&app);
}

/// The UI confirms interrupted work before asking the host to stop its owned engine.
#[tauri::command]
async fn exit_app(app: tauri::AppHandle, state: State<'_, DesktopState>) -> Result<(), String> {
    let _guard = LifecycleGuard::acquire(&state)?;
    let owned = state
        .child
        .lock()
        .map_err(|_| "엔진 프로세스 상태 오류")?
        .take();
    if let Some(mut child) = owned
        && let Err(error) = shutdown_child(&mut child).await
    {
        *state.child.lock().map_err(|_| "엔진 프로세스 상태 오류")? = Some(child);
        return Err(error);
    }
    state.exit_allowed.store(true, Ordering::Release);
    app.exit(0);
    Ok(())
}

/// Engine location is deployment-derived or explicitly configured, never a developer home path.
fn engine_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("FTA_ENGINE_BIN") {
        return Ok(PathBuf::from(path));
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    if let Some(parent) = executable.parent() {
        let sibling = parent.join(ENGINE_EXECUTABLE);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    if cfg!(debug_assertions) {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let candidate = manifest
            .join("../../../target/debug")
            .join(ENGINE_EXECUTABLE);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("전송 엔진을 찾을 수 없습니다. fta-engine을 앱 옆에 설치하거나 FTA_ENGINE_BIN을 지정해주세요".into())
}

/// A failed start remains visible; it must never fabricate online peers or successful transfers.
#[tauri::command]
async fn start_engine(state: State<'_, DesktopState>) -> Result<Value, String> {
    if let Ok(status) = request("status", json!({})).await {
        return Ok(status);
    }
    {
        let mut child = state.child.lock().map_err(|_| "엔진 프로세스 상태 오류")?;
        let running = match child.as_mut() {
            Some(process) => process
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_none(),
            None => false,
        };
        if !running {
            let mut command = Command::new(engine_path()?);
            command.arg("serve").stdin(Stdio::null());
            // The GUI owns a headless engine; a Windows console must not appear for every app launch.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
            }
            *child = Some(command.spawn().map_err(|error| error.to_string())?);
        }
    }
    for _ in 0..ENGINE_START_ATTEMPTS {
        if let Ok(status) = request("status", json!({})).await {
            return Ok(status);
        }
        tokio::time::sleep(ENGINE_START_DELAY).await;
    }
    Err("엔진이 준비되지 않았습니다. 설정과 엔진 로그를 확인해주세요".into())
}

/// Ask the owned engine to drain transfers before using a bounded forced-stop fallback.
async fn shutdown_child(child: &mut Child) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    match tokio::time::timeout(
        ENGINE_SHUTDOWN_REQUEST_TIMEOUT,
        request("engine.shutdown", json!({})),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => eprintln!("graceful engine shutdown request failed: {error}"),
        Err(_) => eprintln!("graceful engine shutdown request timed out"),
    }
    for _ in 0..ENGINE_STOP_ATTEMPTS {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Ok(());
        }
        tokio::time::sleep(ENGINE_STOP_DELAY).await;
    }
    // A hung process must not block desktop shutdown forever; crash recovery handles incomplete files.
    child.kill().map_err(|error| error.to_string())?;
    child.wait().map_err(|error| error.to_string())?;
    Ok(())
}

/// Only a child owned by this desktop may be restarted; an independent server is never killed.
#[tauri::command]
async fn restart_engine(state: State<'_, DesktopState>) -> Result<Value, String> {
    let guard = LifecycleGuard::acquire(&state)?;
    let mut child = state
        .child
        .lock()
        .map_err(|_| "엔진 프로세스 상태 오류")?
        .take()
        .ok_or("이 앱에서 시작한 서비스가 아닙니다. 해당 서비스를 직접 재시작하세요")?;
    if let Err(error) = shutdown_child(&mut child).await {
        *state.child.lock().map_err(|_| "엔진 프로세스 상태 오류")? = Some(child);
        return Err(error);
    }
    drop(guard);
    start_engine(state).await
}

/// Register readiness only after the UI listener exists; early native events must remain unacknowledged.
#[tauri::command]
fn notifications_ready(
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    require_main_window(window.label())?;
    state.notifications_ready.store(true, Ordering::Release);
    Ok(())
}

/// Only a transfer UUID crosses the protocol boundary; external launch URLs never become file or shell actions.
fn activation_transfer(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != ACTIVATION_SCHEME
        || url.host_str() != Some("inbox")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let id = url.path().strip_prefix('/')?;
    uuid::Uuid::parse_str(id).ok().map(|id| id.to_string())
}

/// Keep cold-start activations until the frontend is ready instead of emitting into an absent listener.
fn queue_activation(app: &tauri::AppHandle, value: &str) {
    let Some(id) = activation_transfer(value) else {
        return;
    };
    let state = app.state::<DesktopState>();
    if let Ok(mut queue) = state.pending_activations.lock()
        && !queue.contains(&id)
    {
        if queue.len() == MAX_PENDING_ACTIVATIONS {
            queue.pop_front();
        }
        queue.push_back(id);
    }
    if state.notifications_ready.load(Ordering::Acquire) {
        show_main(app);
        let _ = app.emit("fta:activation-available", ());
    }
}

/// Pending IDs remain queued until UI acknowledgement so an unavailable engine cannot lose an activation.
#[tauri::command]
fn pending_activations(state: State<'_, DesktopState>) -> Result<Vec<String>, String> {
    Ok(state
        .pending_activations
        .lock()
        .map_err(|_| "알림 열기 상태 오류")?
        .iter()
        .cloned()
        .collect())
}

/// Remove only the resolved UUID, preserving other notifications that arrived while details were opening.
#[tauri::command]
fn ack_activation(state: State<'_, DesktopState>, transfer_id: String) -> Result<(), String> {
    state
        .pending_activations
        .lock()
        .map_err(|_| "알림 열기 상태 오류")?
        .retain(|id| id != &transfer_id);
    Ok(())
}

/// Cursor acknowledgement happens after UI processing and native notification submission were attempted.
#[tauri::command]
async fn ack_receipt(window: tauri::WebviewWindow, sequence: u64) -> Result<Value, String> {
    require_main_window(window.label())?;
    request(
        "events.ack",
        json!({ "consumer": EVENT_CONSUMER, "sequence": sequence }),
    )
    .await
}

/// Native payload preparation errors prevent acknowledgement; OS display itself is outside this API's guarantee.
#[tauri::command]
async fn notify_receipt(
    window: tauri::WebviewWindow,
    _app: tauri::AppHandle,
    sequence: u64,
    transfer_id: String,
) -> Result<(), String> {
    require_main_window(window.label())?;
    let result = request(
        "events.list",
        json!({ "after": sequence.saturating_sub(1), "limit": 1 }),
    )
    .await?;
    let event = result
        .get("events")
        .and_then(Value::as_array)
        .and_then(|events| events.first())
        .filter(|event| {
            event.get("sequence").and_then(Value::as_u64) == Some(sequence)
                && event.get("transfer_id").and_then(Value::as_str) == Some(transfer_id.as_str())
        })
        .ok_or("수신 이벤트를 찾을 수 없습니다")?;
    let record = &event["record"];
    if record["direction"] != "receive"
        || !matches!(record["state"].as_str(), Some("completed" | "partial"))
    {
        return Ok(());
    }
    let body = if record["state"] == "partial" {
        "일부 파일이 저장되었습니다. 전송 기록에서 결과를 확인하세요."
    } else if record["kind"] == "files" {
        "파일이 수신 폴더에 저장되었습니다."
    } else {
        "텍스트 또는 링크가 도착했습니다."
    };
    #[cfg(target_os = "macos")]
    {
        native_notification::show(&transfer_id, "FTA · 새 항목 수신", body).await
    }
    #[cfg(windows)]
    {
        fta_platform::windows::notifications::show(
            &_app.config().identifier,
            ACTIVATION_SCHEME,
            &transfer_id,
            "FTA · 새 항목 수신",
            body,
        )
        .map_err(|error| error.to_string())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = _app;
        Err("데스크톱 알림은 Windows와 macOS에서 지원합니다".into())
    }
}

/// Query native notification authorization without interpreting receipt approval as OS permission.
#[tauri::command]
async fn notification_permission(request: bool) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        native_notification::permission(request).await
    }
    #[cfg(windows)]
    {
        let _ = request;
        Ok("os-managed".into())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = request;
        Ok("unavailable".into())
    }
}

/// Read one durable event at a time so a failed earlier notification cannot be skipped by a later ack.
fn monitor_receipts(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            if app
                .state::<DesktopState>()
                .notifications_ready
                .load(Ordering::Acquire)
                && let Ok(cursor) =
                    request("events.cursor", json!({ "consumer": EVENT_CONSUMER })).await
                && let Some(sequence) = cursor.get("sequence").and_then(Value::as_u64)
                && let Ok(result) =
                    request("events.list", json!({ "after": sequence, "limit": 1 })).await
                && let Some(event) = result
                    .get("events")
                    .and_then(Value::as_array)
                    .and_then(|events| events.first())
            {
                // The frontend acknowledges only after handling this event; emit failure leaves it pending.
                if let Err(error) = app.emit_to(WINDOW_LABEL, "fta:receipt", event) {
                    eprintln!("failed to deliver receipt event to desktop: {error}");
                }
            }
            tokio::time::sleep(EVENT_POLL_INTERVAL).await;
        }
    });
}

// Utility labels are an allowlist, never a URL or executable supplied by the WebView.
fn utility_label(kind: &str) -> Result<&str, String> {
    match kind {
        "inbox" | "settings" => Ok(kind),
        _ => Err("허용되지 않은 보조 창입니다".into()),
    }
}

// Async creation avoids blocking a Windows IPC/UI thread while constructing another WebView.
#[tauri::command]
async fn open_utility_window(app: tauri::AppHandle, kind: String) -> Result<(), String> {
    let label = utility_label(&kind)?;
    if let Some(window) = app.get_webview_window(label) {
        return window
            .unminimize()
            .and_then(|_| window.show())
            .and_then(|_| window.set_focus())
            .map_err(|error| error.to_string());
    }
    let config = app.config();
    let window_config = config
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .ok_or("보조 창 설정을 찾을 수 없습니다")?;
    let window = tauri::WebviewWindowBuilder::from_config(&app, window_config)
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

// A child close must destroy only its own window, never hide the transfer panel or stop its engine.
#[tauri::command]
fn close_utility_window(window: tauri::WebviewWindow) -> Result<(), String> {
    utility_label(window.label())?;
    window.close().map_err(|error| error.to_string())
}

/// Tray, Dock, and activation requests must restore hidden or minimized panels.
fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        if let Err(error) = apply_fold_state(&window, false) {
            eprintln!("failed to expand transfer window: {error}");
        }
        if let Err(error) = window
            .unminimize()
            .and_then(|_| window.show())
            .and_then(|_| window.set_focus())
        {
            eprintln!("failed to show transfer window: {error}");
        }
    }
}

/// Keep shutdown scoped to the engine this UI started, not an independent user service.
fn stop_owned_engine(app: &tauri::AppHandle) {
    let state = app.state::<DesktopState>();
    match state.child.lock() {
        Ok(mut slot) => {
            if let Some(mut child) = slot.take() {
                if let Err(error) = child.kill() {
                    eprintln!("failed to stop owned engine: {error}");
                }
                if let Err(error) = child.wait() {
                    eprintln!("failed to reap owned engine: {error}");
                }
            }
        }
        Err(_) => eprintln!("failed to lock owned engine during shutdown"),
    }
}

/// Platform plugins stay in the desktop binary so Linux CLI builds remain display-independent.
fn main() {
    let app = tauri::Builder::default()
        .manage(DesktopState::default())
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            for argument in args {
                queue_activation(app, &argument);
            }
            show_main(app)
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        // Restore geometry without restoring a hidden state that could strand a tray-only app.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // A persisted 44px badge must never overwrite the next expanded startup size.
                .skip_initial_state(WINDOW_LABEL)
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            engine_request,
            choose_files,
            send_files,
            send_message,
            open_link,
            reveal_transfer,
            copy_text,
            set_pinned,
            set_folded,
            fold_state,
            start_window_drag,
            close_app_window,
            open_utility_window,
            close_utility_window,
            set_autostart,
            autostart_status,
            start_engine,
            restart_engine,
            exit_app,
            show_window,
            notifications_ready,
            ack_receipt,
            notify_receipt,
            notification_permission,
            pending_activations,
            ack_activation
        ])
        .setup(|app| {
            // Restore only main position; child windows keep full geometry restoration through the plugin.
            if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
                use tauri_plugin_window_state::WindowExt;
                let _ = window.restore_state(tauri_plugin_window_state::StateFlags::POSITION);
            }
            let activation_app = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    queue_activation(&activation_app, url.as_str());
                }
            });
            if let Some(urls) = app.deep_link().get_current()? {
                for url in urls {
                    queue_activation(app.handle(), url.as_str());
                }
            }
            for argument in std::env::args().skip(1) {
                queue_activation(app.handle(), &argument);
            }
            #[cfg(windows)]
            if let Err(error) = fta_platform::windows::notifications::register(
                &std::env::current_exe()?,
                &app.config().identifier,
                "FTA",
                ACTIVATION_SCHEME,
            ) {
                eprintln!("native notification registration failed: {error}");
            }
            let open = MenuItem::with_id(app, "open", "전송 창 열기", true, None::<&str>)?;
            let inbox = MenuItem::with_id(app, "inbox", "수신 기록", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "설정", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "앱 종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &inbox, &settings, &quit])?;
            TrayIconBuilder::new()
                .icon(Image::from_bytes(include_bytes!("../icons/icon.png"))?)
                .tooltip("FTA · 내 기기로 보내기")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main(app),
                    "inbox" | "settings" => {
                        let app = app.clone();
                        let kind = event.id.as_ref().to_owned();
                        tauri::async_runtime::spawn(async move {
                            if let Err(error) = open_utility_window(app, kind).await {
                                eprintln!("failed to open utility window: {error}");
                            }
                        });
                    }
                    "quit" => {
                        let _ = app.emit("fta:quit-requested", ());
                        show_main(app);
                    }
                    _ => {}
                })
                .build(app)?;
            monitor_receipts(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == WINDOW_LABEL => {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    eprintln!("failed to hide transfer window: {error}");
                }
                if let Ok(mut paths) = window.state::<DesktopState>().granted_paths.lock() {
                    paths.clear();
                }
                let _ = window.emit("fta:panel-hidden", ());
            }
            // Normalize backend-specific units once; Cocoa drag points must not be divided by Retina scale twice.
            WindowEvent::DragDrop(DragDropEvent::Enter { position, .. } | DragDropEvent::Over { position }) => {
                if window.label() == WINDOW_LABEL
                    && let Ok(scale) = window.scale_factor()
                    && let Ok(point) = drop_coordinates::css_position(*position, scale)
                {
                    let _ = window.emit_to(WINDOW_LABEL, "fta:drag-position", json!({ "x": point.x, "y": point.y }));
                }
            }
            WindowEvent::DragDrop(DragDropEvent::Leave) => {
                let _ = window.emit_to(WINDOW_LABEL, "fta:drag-left", ());
            }
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, position }) if window.label() == WINDOW_LABEL => {
                let state = window.state::<DesktopState>();
                match grant_paths(&state, paths.clone()) {
                    Ok(paths) => {
                        match window.scale_factor().map_err(|error| error.to_string())
                            .and_then(|scale| drop_coordinates::css_position(*position, scale).map_err(str::to_owned)) {
                            Ok(point) => { let _ = window.emit_to(WINDOW_LABEL, "fta:files-dropped", json!({ "paths": paths, "position": { "x": point.x, "y": point.y } })); }
                            Err(error) => { let _ = window.emit_to(WINDOW_LABEL, "fta:drop-error", error); }
                        }
                    }
                    Err(error) => {
                        let _ = window.emit_to(WINDOW_LABEL, "fta:drop-error", error);
                    }
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!());
    match app {
        Ok(app) => {
            // Apple requires this delegate before applicationDidFinishLaunching, not Tauri's later setup callback.
            #[cfg(target_os = "macos")]
            match native_notification::install(app.handle()) {
                Ok(delegate) => {
                    app.manage(delegate);
                }
                Err(error) => eprintln!("native notification integration unavailable: {error}"),
            }
            app.run(|app, event| {
                // Dock clicks reopen an existing macOS process rather than launching a second instance.
                // Handle both hidden-main and visible-utility cases so closing the panel cannot strand it.
                #[cfg(target_os = "macos")]
                if matches!(&event, tauri::RunEvent::Reopen { .. }) {
                    show_main(app);
                }
                if let tauri::RunEvent::ExitRequested { api, .. } = &event
                    && !app
                        .state::<DesktopState>()
                        .exit_allowed
                        .load(Ordering::Acquire)
                {
                    api.prevent_exit();
                    show_main(app);
                    let _ = app.emit("fta:quit-requested", ());
                }
                if matches!(event, tauri::RunEvent::Exit) {
                    stop_owned_engine(app);
                }
            });
        }
        Err(error) => {
            eprintln!("failed to start FTA desktop: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A history/settings window cannot reuse a file selection or acknowledge the main notification cursor.
    #[test]
    fn transfer_and_notification_actions_belong_to_main_window() {
        assert!(require_main_window("main").is_ok());
        assert!(require_main_window("inbox").is_err());
        assert!(require_main_window("settings").is_err());
    }

    // User-controlled labels cannot become arbitrary local paths, external pages, or the main window.
    #[test]
    fn utility_windows_only_accept_known_roles() {
        for label in ["inbox", "settings"] {
            assert!(utility_label(label).is_ok());
        }
        for label in [
            "main",
            "../settings",
            "https://example.com",
            "",
            "settings?view=main",
        ] {
            assert!(utility_label(label).is_err());
        }
    }

    // A received URL must never gain local-file or script execution privileges.
    #[test]
    fn only_http_links_can_open() {
        assert!(validated_url("https://example.com/path").is_ok());
        for value in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.com",
            "https://",
            "https://example.com\nhttps://example.org",
        ] {
            assert!(validated_url(value).is_err(), "{value}");
        }
    }

    // A repeated quit/restart request must not bypass an in-flight graceful engine shutdown.
    #[test]
    fn lifecycle_operations_are_serialized_and_released() {
        let state = DesktopState::default();
        let first = LifecycleGuard::acquire(&state).unwrap();
        assert!(LifecycleGuard::acquire(&state).is_err());
        drop(first);
        assert!(LifecycleGuard::acquire(&state).is_ok());
    }

    // Protocol activation is a record locator, never an arbitrary URL or path execution surface.
    #[test]
    fn activation_urls_accept_only_a_transfer_uuid() {
        let id = uuid::Uuid::new_v4();
        assert_eq!(
            activation_transfer(&format!("fta://inbox/{id}")),
            Some(id.to_string())
        );
        for value in [
            format!("https://inbox/{id}"),
            format!("fta://other/{id}"),
            format!("fta://user@inbox/{id}"),
            format!("fta://inbox/{id}?path=/etc/passwd"),
            format!("fta://inbox/{id}/extra"),
            "fta://inbox/../../file".into(),
        ] {
            assert!(activation_transfer(&value).is_none(), "{value}");
        }
    }

    // Replacing a selection revokes old path grants instead of accumulating filesystem access.
    #[test]
    fn a_new_selection_replaces_the_previous_file_grants() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let first = directory.path().join("first.txt");
        let second = directory.path().join("second.txt");
        std::fs::write(&first, "first")?;
        std::fs::write(&second, "second")?;
        let state = DesktopState::default();
        grant_paths(&state, vec![first.clone()])?;
        grant_paths(&state, vec![second.clone()])?;
        let grants = state.granted_paths.lock().map_err(|_| "lock poisoned")?;
        assert!(!grants.contains(&first.canonicalize()?));
        assert!(grants.contains(&second.canonicalize()?));
        assert!(
            grant_paths(
                &DesktopState::default(),
                vec![directory.path().to_path_buf()]
            )
            .is_err()
        );
        Ok(())
    }

    // Symlink selection must not smuggle a different target behind a user-approved path.
    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_a_file_selection_grant() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target.txt");
        let link = directory.path().join("link.txt");
        std::fs::write(&target, "content")?;
        std::os::unix::fs::symlink(&target, &link)?;
        assert!(grant_paths(&DesktopState::default(), vec![link]).is_err());
        Ok(())
    }
}
