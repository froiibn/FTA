//! App notification choices persist independently of OS permission and the engine's receive settings.

use std::{fs, io, path::Path};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "notification-preference.json";
const DEFAULT_ENABLED: bool = true;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    enabled: bool,
}

pub fn read(path: &Path) -> Result<bool, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        // Preserve existing installations, but report corrupt saved state instead of guessing enabled.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(DEFAULT_ENABLED),
        Err(error) => return Err(format!("알림 설정을 읽지 못했습니다: {error}")),
    };
    serde_json::from_slice::<Preference>(&bytes)
        .map(|preference| preference.enabled)
        .map_err(|error| format!("알림 설정 형식이 잘못되었습니다: {error}"))
}

pub fn write(path: &Path, enabled: bool) -> Result<bool, String> {
    let parent = path.parent().ok_or("알림 설정 경로를 찾지 못했습니다")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(&Preference { enabled }).map_err(|error| error.to_string())?;
    // Reuse the synced atomic replacement so a crash cannot leave a half-written off/on preference.
    fta_core::config::atomic_write(path, &bytes).map_err(|error| error.to_string())?;
    read(path)
}
