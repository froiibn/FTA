//! XDG paths keep the headless server independent of a desktop session or GUI keyring.

use crate::PlatformPaths;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

const APPLICATION_DIR: &str = "fta";
const DEFAULT_RECEIVE_DIR: &str = "FTA-Inbox";

/// Incoming network payloads must never be written with root privileges on a personal server.
pub fn ensure_unprivileged() -> io::Result<()> {
    // SAFETY: geteuid has no preconditions and returns the calling process's effective user ID.
    let effective_user = unsafe { libc::geteuid() };
    if effective_user == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "FTA must run as an unprivileged user, not root",
        ));
    }
    Ok(())
}

/// Resolve through an injected environment reader so tests never mutate process-wide environment.
pub fn paths() -> io::Result<PlatformPaths> {
    paths_from(|name| std::env::var_os(name))
}

fn paths_from(environment: impl Fn(&str) -> Option<OsString>) -> io::Result<PlatformPaths> {
    let home = absolute_env(&environment, "HOME")?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "HOME is required to resolve Linux application paths",
        )
    })?;
    let config_base =
        absolute_env(&environment, "XDG_CONFIG_HOME")?.unwrap_or_else(|| home.join(".config"));
    let state_base =
        absolute_env(&environment, "XDG_STATE_HOME")?.unwrap_or_else(|| home.join(".local/state"));
    let config_dir = config_base.join(APPLICATION_DIR);
    let state_dir = state_base.join(APPLICATION_DIR);
    let runtime_dir = absolute_env(&environment, "XDG_RUNTIME_DIR")?
        .map(|base| base.join(APPLICATION_DIR))
        .unwrap_or_else(|| state_dir.join("run"));
    Ok(PlatformPaths {
        config_dir,
        state_dir,
        runtime_dir,
        receive_dir: home.join(DEFAULT_RECEIVE_DIR),
    })
}

fn absolute_env(
    environment: &impl Fn(&str) -> Option<OsString>,
    name: &str,
) -> io::Result<Option<PathBuf>> {
    let Some(value) = environment(name).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} must be an absolute path"),
        ));
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn uses_explicit_xdg_directories_without_gui_runtime() {
        // A server may have a nonstandard home and state volume, so no username may be embedded.
        let environment = HashMap::from([
            ("HOME", "/srv/operator"),
            ("XDG_CONFIG_HOME", "/mnt/config"),
            ("XDG_STATE_HOME", "/mnt/state"),
        ]);
        let paths = paths_from(|name| environment.get(name).map(OsString::from)).unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/mnt/config/fta"));
        assert_eq!(paths.runtime_dir, PathBuf::from("/mnt/state/fta/run"));
        assert_eq!(paths.receive_dir, PathBuf::from("/srv/operator/FTA-Inbox"));
    }

    #[test]
    fn rejects_relative_environment_paths() {
        let environment = HashMap::from([("HOME", "/home/user"), ("XDG_CONFIG_HOME", "relative")]);
        assert!(paths_from(|name| environment.get(name).map(OsString::from)).is_err());
    }
}
