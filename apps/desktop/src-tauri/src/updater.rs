//! Signed GitHub updates stay native: the WebView cannot choose a URL, key or installer.
use std::{collections::HashSet, sync::Mutex, time::Duration};

use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{DesktopState, LifecycleGuard, ensure_engine, request};

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const START_DELAY: Duration = Duration::from_secs(30);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);
const STOP_ATTEMPTS: usize = 100;
const STOP_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Serialize)]
pub struct Status {
    phase: String,
    message: String,
}

pub struct UpdateState {
    busy: tokio::sync::Mutex<()>,
    status: Mutex<Status>,
    prompted: Mutex<HashSet<String>>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            busy: tokio::sync::Mutex::new(()),
            status: Mutex::new(Status {
                phase: "idle".into(),
                message: format!(
                    "현재 버전 {} · 새 버전을 자동으로 확인합니다.",
                    env!("CARGO_PKG_VERSION")
                ),
            }),
            prompted: Mutex::new(HashSet::new()),
        }
    }
}

fn set_status(app: &tauri::AppHandle, phase: &str, message: impl Into<String>) {
    if let Ok(mut status) = app.state::<UpdateState>().status.lock() {
        *status = Status {
            phase: phase.into(),
            message: message.into(),
        };
    }
}

#[tauri::command]
pub fn update_status(state: State<'_, UpdateState>) -> Result<Status, String> {
    state
        .status
        .lock()
        .map(|s| s.clone())
        .map_err(|_| "업데이트 상태를 읽지 못했습니다".into())
}

#[tauri::command]
pub async fn check_update(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if window.label() != "settings" {
        return Err("설정 창에서만 업데이트를 확인할 수 있습니다".into());
    }
    run_check(&app, true).await
}

// Separate target manifests let a Mac release advance without advertising an older Windows installer as new.
fn release_url_allowed(url: &url::Url, version: &str) -> bool {
    let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && parts.len() == 6
        && parts[..4] == ["froiibn", "FTA", "releases", "download"]
        && parts[4] == format!("v{version}")
        && parts[5]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && (parts[5].ends_with(".tar.gz") || parts[5].ends_with("-setup.exe"))
}

async fn run_check(app: &tauri::AppHandle, manual: bool) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    let _busy = state
        .busy
        .try_lock()
        .map_err(|_| "업데이트 확인 또는 설치가 진행 중입니다")?;
    let result = check_inner(app, manual).await;
    if let Err(error) = &result {
        set_status(app, "error", error.clone());
    }
    result
}

async fn check_inner(app: &tauri::AppHandle, manual: bool) -> Result<(), String> {
    set_status(app, "checking", "새 버전을 확인하고 있습니다.");
    let updater = app
        .updater_builder()
        .timeout(NETWORK_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|_| "업데이트를 확인하지 못했습니다. 인터넷 연결을 확인하고 다시 시도하세요.")?
    else {
        set_status(
            app,
            "current",
            format!("현재 버전 {} · 최신 버전입니다.", env!("CARGO_PKG_VERSION")),
        );
        return Ok(());
    };
    if !release_url_allowed(&update.download_url, &update.version) {
        return Err("공식 FTA 릴리스의 업데이트 파일이 아니므로 설치하지 않았습니다.".into());
    }
    set_status(
        app,
        "available",
        format!(
            "{} 업데이트가 있습니다. 확인 아이콘으로 설치할 수 있습니다.",
            update.version
        ),
    );
    let first_prompt = app
        .state::<UpdateState>()
        .prompted
        .lock()
        .map_err(|_| "업데이트 상태 오류")?
        .insert(update.version.clone());
    if !manual && !first_prompt {
        return Ok(());
    }
    // A native prompt preserves drafts and lets the user choose when a tray receiver may restart.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().message(format!("FTA {} 업데이트를 다운로드하고 설치할까요?\n\n완료 후 FTA가 다시 시작됩니다. 진행 중인 송수신이 있으면 설치를 보류합니다.", update.version))
        .title("FTA 업데이트")
        .buttons(MessageDialogButtons::OkCancelCustom("업데이트".into(), "나중에".into()))
        .show(move |accepted| { let _ = sender.send(accepted); });
    if !receiver.await.unwrap_or(false) {
        return Ok(());
    }
    set_status(
        app,
        "downloading",
        "업데이트 다운로드 및 서명 검증 중입니다.",
    );
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|_| "다운로드 또는 서명 검증에 실패했습니다. 기존 앱을 유지합니다.")?;
    install(app, update, bytes).await
}

async fn install(app: &tauri::AppHandle, update: Update, bytes: Vec<u8>) -> Result<(), String> {
    let state = app.state::<DesktopState>();
    let _lifecycle = LifecycleGuard::acquire(&state)?;
    // Never replace binaries under a separately managed CLI/MCP service or kill a busy receiver.
    if state.child.lock().map_err(|_| "엔진 상태 오류")?.is_none() {
        return Err("별도로 실행한 전송 엔진을 종료한 뒤 FTA를 다시 열고 업데이트하세요.".into());
    }
    request("engine.update_shutdown", serde_json::json!({}))
        .await
        .map_err(
            |_| "송수신 또는 연결 작업이 진행 중입니다. 완료 후 업데이트를 다시 확인하세요.",
        )?;
    wait_owned_engine(&state).await?;
    set_status(app, "installing", "검증한 업데이트를 설치하고 있습니다.");
    state
        .exit_allowed
        .store(true, std::sync::atomic::Ordering::Release);
    let result = tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await;
    match result {
        Ok(Ok(())) => {
            app.restart();
        }
        _ => {
            // A failed replacement must restore receiving and must never be reported as an update success.
            state
                .exit_allowed
                .store(false, std::sync::atomic::Ordering::Release);
            let recovery = ensure_engine(&state).await;
            Err(if recovery.is_ok() {
                "설치하지 못했습니다. 기존 전송 엔진을 다시 시작했습니다."
            } else {
                "설치하지 못했습니다. 전송 엔진을 다시 시작하려면 FTA를 재실행하세요."
            }
            .into())
        }
    }
}

async fn wait_owned_engine(state: &DesktopState) -> Result<(), String> {
    for _ in 0..STOP_ATTEMPTS {
        {
            let mut child = state.child.lock().map_err(|_| "엔진 상태 오류")?;
            if let Some(process) = child.as_mut() {
                if process.try_wait().map_err(|e| e.to_string())?.is_some() {
                    *child = None;
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }
        tokio::time::sleep(STOP_DELAY).await;
    }
    Err("엔진 종료 확인이 지연되어 설치하지 않았습니다. FTA를 다시 열고 시도하세요.".into())
}

pub fn start(app: tauri::AppHandle) {
    // Debug previews never contact a release feed or offer to replace a development build.
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(START_DELAY).await;
        loop {
            let _ = run_check(&app, false).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "requires the signed release feed to be published on GitHub"]
    async fn published_mac_update_installs_only_into_isolated_app() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), config["plugins"]["updater"].clone());
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("FTA.app");
        let executable = bundle.join("Contents/MacOS/fta-desktop");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"old isolated test app").unwrap();
        let sentinel = directory.path().join("user-data");
        std::fs::write(&sentinel, b"preserve settings and received files").unwrap();
        let update = app
            .updater_builder()
            .timeout(NETWORK_TIMEOUT)
            .executable_path(&executable)
            // This test requests the current public package to exercise a real signature/install, not a fake newer release.
            .version_comparator(|_, _| true)
            .build()
            .unwrap()
            .check()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(update.version, env!("CARGO_PKG_VERSION"));
        assert!(release_url_allowed(&update.download_url, &update.version));
        let bytes = update.download(|_, _| {}, || {}).await.unwrap();
        let mut corrupted = bytes.clone();
        corrupted[0] ^= 1;
        assert!(update.install(corrupted).is_err());
        assert_eq!(
            std::fs::read(&executable).unwrap(),
            b"old isolated test app"
        );
        update.install(bytes).unwrap();
        let plist = std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        // The downloaded bundle must match the requested release, including future versions.
        assert!(plist.contains(&format!("<string>{}</string>", update.version)));
        assert!(bundle.join("Contents/MacOS/fta-engine").is_file());
        assert!(bundle.join("Contents/MacOS/fta-mcp").is_file());
        assert!(
            bundle
                .join("Contents/Resources/THIRD_PARTY_NOTICES.txt")
                .is_file()
        );
        assert!(
            std::process::Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&bundle)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"preserve settings and received files"
        );
    }

    // Release preparation runs this against the final, notarized bytes before any public feed exists.
    #[test]
    #[ignore = "requires FTA_TEST_UPDATER_ARTIFACT pointing to a locally signed release"]
    fn signed_release_rejects_corruption_and_version_tampering() {
        use base64::Engine;
        let path = std::env::var("FTA_TEST_UPDATER_ARTIFACT").expect("release artifact path");
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let decode = |s: &str| {
            String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(s.trim())
                    .unwrap(),
            )
            .unwrap()
        };
        let public = minisign_verify::PublicKey::decode(&decode(
            config["plugins"]["updater"]["pubkey"].as_str().unwrap(),
        ))
        .unwrap();
        let encoded = std::fs::read_to_string(format!("{path}.sig")).unwrap();
        let signature_text = decode(&encoded);
        let signature = minisign_verify::Signature::decode(&signature_text).unwrap();
        let bytes = std::fs::read(path).unwrap();
        public.verify(&bytes, &signature, true).unwrap();
        let signed = format!("version:{}", config["version"].as_str().unwrap());
        assert!(
            signature
                .trusted_comment()
                .split('\t')
                .any(|field| field == signed)
        );
        let mut corrupted = bytes.clone();
        corrupted[0] ^= 1;
        assert!(public.verify(&corrupted, &signature, true).is_err());
        let changed = signature_text.replace(&signed, "version:999.0.0");
        let forged = minisign_verify::Signature::decode(&changed).unwrap();
        assert!(public.verify(&bytes, &forged, true).is_err());
    }
    #[test]
    fn accepts_only_exact_release_origin_and_signed_version_path() {
        for name in ["FTA.app.tar.gz", "FTA_0.1.5_x64-setup.exe"] {
            assert!(release_url_allowed(
                &format!("https://github.com/froiibn/FTA/releases/download/v0.1.5/{name}")
                    .parse()
                    .unwrap(),
                "0.1.5"
            ));
        }
        for url in [
            "http://github.com/froiibn/FTA/releases/download/v0.1.5/FTA.app.tar.gz",
            "https://github.com/other/FTA/releases/download/v0.1.5/FTA.app.tar.gz",
            "https://github.com/froiibn/FTA/releases/download/v0.1.4/FTA.app.tar.gz",
            "https://github.com/froiibn/FTA/releases/download/v0.1.5/FTA.app.tar.gz?x=1",
            "https://user@github.com/froiibn/FTA/releases/download/v0.1.5/FTA.app.tar.gz",
            "https://github.com/froiibn/FTA/releases/download/v0.1.5/%2e%2e%2ffile.tar.gz",
        ] {
            assert!(
                !release_url_allowed(&url.parse().unwrap(), "0.1.5"),
                "{url}"
            );
        }
    }
}
