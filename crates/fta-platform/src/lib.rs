//! Platform integration is isolated so headless builds never require a WebView.
#![warn(clippy::undocumented_unsafe_blocks)]

use std::path::PathBuf;
pub mod notification_payload;

/// OS-derived locations prevent assumptions about a user's home or Downloads folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub receive_dir: PathBuf,
}

// Each module is compiled only on its native OS to keep platform FFI out of other builds.
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::paths;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::paths;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::paths;

/// Publish a complete temporary file without a delete-first gap or overwrite race.
pub fn publish_noreplace(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            source,
            rustix::fs::CWD,
            destination,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(Into::into)
    }
    #[cfg(windows)]
    {
        windows::publish_noreplace(source, destination)
    }
}
