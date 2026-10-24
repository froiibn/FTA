//! macOS paths and key protection stay outside the transfer engine's platform-neutral logic.

use std::io;

use crate::PlatformPaths;

const APPLICATION_DIRECTORY: &str = "FTA";
const KEYCHAIN_SERVICE: &str = "app.fta.local-transfer.identity";

/// Derive the user directories from OS-aware APIs, not a developer-specific home path.
pub fn paths() -> io::Result<PlatformPaths> {
    let data = dirs::data_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "macOS application data directory is unavailable",
        )
    })?;
    let downloads = dirs::download_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "macOS Downloads directory is unavailable",
        )
    })?;
    let state_dir = data.join(APPLICATION_DIRECTORY);
    let receive_dir = downloads.join(APPLICATION_DIRECTORY);
    Ok(PlatformPaths {
        config_dir: state_dir.clone(),
        runtime_dir: state_dir.join("run"),
        state_dir,
        receive_dir,
    })
}

/// Store the private identity key in Keychain instead of introducing a plaintext key fallback.
pub fn store_identity_key(account: &str, secret: &[u8]) -> io::Result<()> {
    if account.is_empty() || secret.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "identity account and key must not be empty",
        ));
    }
    security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, account, secret)
        .map_err(|error| io::Error::other(format!("keychain identity storage failed: {error}")))
}

/// Report a locked or missing key explicitly; silently minting another identity breaks peer trust.
pub fn load_identity_key(account: &str) -> io::Result<Vec<u8>> {
    if account.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "identity account must not be empty",
        ));
    }
    security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, account)
        .map_err(|error| io::Error::other(format!("keychain identity access failed: {error}")))
}

/// Registration removal must remove the actual Keychain entry, not leave a disabled credential.
pub fn delete_identity_key(account: &str) -> io::Result<()> {
    if account.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "identity account must not be empty",
        ));
    }
    security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, account)
        .map_err(|error| io::Error::other(format!("keychain identity deletion failed: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A path lookup must remain read-only and derive every returned path from the current user.
    #[test]
    fn platform_paths_are_absolute_and_runtime_is_scoped() -> io::Result<()> {
        let paths = paths()?;
        assert!(paths.config_dir.is_absolute());
        assert!(paths.state_dir.is_absolute());
        assert!(paths.receive_dir.is_absolute());
        assert_eq!(paths.runtime_dir, paths.state_dir.join("run"));
        Ok(())
    }

    // Empty inputs are rejected before invoking Keychain, so tests never create system credentials.
    #[test]
    fn empty_identity_values_are_rejected() {
        assert!(store_identity_key("", b"secret").is_err());
        assert!(store_identity_key("test", b"").is_err());
        assert!(load_identity_key("").is_err());
        assert!(delete_identity_key("").is_err());
    }
}
