//! Foundation supplies sandbox paths; Keychain protects the same DER key consumed by rustls.
use crate::PlatformPaths;
use std::{io, sync::OnceLock};
static PATHS: OnceLock<PlatformPaths> = OnceLock::new();
const KEYCHAIN_SERVICE: &str = "app.fta.ios.identity";

/// Refuse a second sandbox so one process cannot redirect identity or recovery state.
pub fn configure_paths(paths: PlatformPaths) -> io::Result<()> {
    if let Some(existing) = PATHS.get() {
        return if existing == &paths {
            Ok(())
        } else {
            Err(io::Error::other("sandbox already configured"))
        };
    }
    PATHS
        .set(paths)
        .map_err(|_| io::Error::other("sandbox initialization raced"))
}
pub fn paths() -> io::Result<PlatformPaths> {
    PATHS
        .get()
        .cloned()
        .ok_or_else(|| io::Error::other("Foundation sandbox paths required"))
}
pub fn store_identity_key(account: &str, secret: &[u8]) -> io::Result<()> {
    // ThisDeviceOnly prevents a restored backup from cloning a registered device's private key.
    use security_framework::{
        access_control::{ProtectionMode, SecAccessControl},
        passwords::PasswordOptions,
    };
    let mut options = PasswordOptions::new_generic_password(KEYCHAIN_SERVICE, account);
    let control = SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly),
        0,
    )
    .map_err(|_| io::Error::other("Keychain protection unavailable"))?;
    options.set_access_control(control);
    options.set_access_synchronized(Some(false));
    security_framework::passwords::set_generic_password_options(secret, options)
        .map_err(|error| io::Error::other(format!("Keychain identity storage failed: {error}")))
}
pub fn load_identity_key(account: &str) -> io::Result<Vec<u8>> {
    security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, account).map_err(
        |error| {
            io::Error::other(format!(
                "Keychain identity unavailable; unlock device: {error}"
            ))
        },
    )
}
