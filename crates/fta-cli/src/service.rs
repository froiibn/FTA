//! Service actions use systemd directly and never claim that an engine listener is healthy.

use crate::{CliError, ServiceCommand};
use fta_core::config::AppPaths;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const SERVICE_NAME: &str = "fta-engine.service";

/// Non-Linux builds expose an honest unsupported result instead of spawning an unrelated tool.
pub fn execute(command: &ServiceCommand, paths: &AppPaths) -> Result<Value, CliError> {
    if !cfg!(target_os = "linux") {
        return Err(CliError::invalid(
            "service commands require Linux with systemd; run fta-engine serve directly on other systems",
        ));
    }
    match command {
        ServiceCommand::Install { engine } => install(engine.as_deref(), paths),
        ServiceCommand::Start => control("start"),
        ServiceCommand::Stop => control("stop"),
        ServiceCommand::Status => control("status"),
    }
}

fn control(action: &str) -> Result<Value, CliError> {
    let output = Command::new("systemctl").args(["--user", action, SERVICE_NAME, "--no-pager"])
        .output().map_err(|error| CliError::service(format!("cannot run systemctl --user: {error}; use fta-engine serve if systemd is unavailable")))?;
    if !output.status.success() {
        return Err(CliError::service(format!(
            "systemctl --user {action} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(
        json!({"action":action,"unit":SERVICE_NAME,"service_output":String::from_utf8_lossy(&output.stdout),"listener_checked":false}),
    )
}

fn install(engine: Option<&Path>, paths: &AppPaths) -> Result<Value, CliError> {
    let engine = if let Some(path) = engine {
        path.canonicalize().map_err(|error| {
            CliError::invalid(format!("cannot resolve engine executable: {error}"))
        })?
    } else {
        let mut sibling =
            std::env::current_exe().map_err(|error| CliError::service(error.to_string()))?;
        sibling.set_file_name("fta-engine");
        sibling.canonicalize().map_err(|error| {
            CliError::invalid(format!(
                "cannot find sibling fta-engine; supply --engine: {error}"
            ))
        })?
    };
    if !engine.is_file() {
        return Err(CliError::invalid(
            "engine executable must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::metadata(&engine)
            .map_err(|error| CliError::invalid(error.to_string()))?
            .permissions();
        if permissions.mode() & 0o111 == 0 {
            return Err(CliError::invalid(
                "engine file has no executable permission",
            ));
        }
    }
    let unit_path = user_unit_dir()?.join(SERVICE_NAME);
    let parent = unit_path
        .parent()
        .ok_or_else(|| CliError::invalid("invalid systemd unit directory"))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| CliError::service(format!("cannot create systemd directory: {error}")))?;
    let unit = render_unit(&engine, paths)?;
    // Do not partially truncate an existing unit if a write or flush fails.
    let temporary_path = unit_path.with_extension(format!("service.{}.tmp", uuid::Uuid::new_v4()));
    let write_result = (|| -> Result<(), CliError> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary_path)
            .map_err(|error| CliError::service(error.to_string()))?;
        file.write_all(unit.as_bytes())
            .map_err(|error| CliError::service(error.to_string()))?;
        file.sync_all()
            .map_err(|error| CliError::service(error.to_string()))?;
        std::fs::rename(&temporary_path, &unit_path)
            .map_err(|error| CliError::service(error.to_string()))?;
        Ok(())
    })();
    if write_result.is_err()
        && let Err(error) = std::fs::remove_file(&temporary_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("could not remove temporary unit file: {error}");
    }
    write_result?;
    for args in [
        vec!["--user", "daemon-reload"],
        vec!["--user", "enable", SERVICE_NAME],
    ] {
        let output = Command::new("systemctl")
            .args(&args)
            .output()
            .map_err(|error| {
                CliError::service(format!("unit written but systemctl failed: {error}"))
            })?;
        if !output.status.success() {
            return Err(CliError::service(format!(
                "unit written but systemctl {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    // Report linger separately: installation must not silently change administrator policy.
    let linger = Command::new("loginctl")
        .args(["show-user", "--property=Linger", "--value"])
        .output();
    let (lingering_enabled, linger_note) = match linger {
        Ok(output) if output.status.success() => {
            let enabled = String::from_utf8_lossy(&output.stdout).trim() == "yes";
            (
                Some(enabled),
                if enabled {
                    "enabled"
                } else {
                    "run loginctl enable-linger for this user if reception must survive logout and start before login"
                },
            )
        }
        _ => (
            None,
            "could not inspect lingering; verify loginctl enable-linger for this user before relying on reception after logout",
        ),
    };
    Ok(
        json!({"installed":true,"unit_path":unit_path,"started":false,"lingering_enabled":lingering_enabled,"linger_note":linger_note}),
    )
}

fn user_unit_dir() -> Result<PathBuf, CliError> {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        let base = PathBuf::from(base);
        if !base.is_absolute() {
            return Err(CliError::invalid("XDG_CONFIG_HOME must be absolute"));
        }
        return Ok(base.join("systemd/user"));
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::invalid("HOME is not set"))?;
    let home = PathBuf::from(home);
    if !home.is_absolute() {
        return Err(CliError::invalid("HOME must be absolute"));
    }
    Ok(home.join(".config/systemd/user"))
}

/// Environment directives expand percent specifiers but do not expand dollar signs.
pub(crate) fn unit_quote(value: &str) -> Result<String, CliError> {
    if value.contains(['\n', '\r', '\0']) {
        return Err(CliError::invalid(
            "service values cannot contain newline or NUL",
        ));
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    ))
}

pub(crate) fn render_unit(engine: &Path, paths: &AppPaths) -> Result<String, CliError> {
    let engine = engine
        .to_str()
        .ok_or_else(|| CliError::invalid("systemd engine path must be UTF-8"))?;
    let mut environment = String::new();
    for (name, path) in [
        ("FTA_CONFIG_DIR", &paths.config_dir),
        ("FTA_STATE_DIR", &paths.state_dir),
        ("FTA_RUNTIME_DIR", &paths.runtime_dir),
    ] {
        let path = path
            .to_str()
            .ok_or_else(|| CliError::invalid("systemd data paths must be UTF-8"))?;
        environment.push_str(&format!(
            "Environment={}\n",
            unit_quote(&format!("{name}={path}"))?
        ));
    }
    Ok(format!(
        "[Unit]\nDescription=FTA local transfer engine\n\n[Service]\nType=simple\nExecStart={} serve\n{}Restart=on-failure\nRestartSec=3\nUMask=0077\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n",
        // ExecStart performs dollar expansion, unlike Environment.
        unit_quote(engine)?.replace('$', "$$"),
        environment
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_values_escape_systemd_expansion_and_reject_line_injection() {
        // Unit interpolation must not reinterpret paths chosen by users as directives or specifiers.
        assert_eq!(
            unit_quote("/a path/100%/$x/\"name\"").unwrap(),
            "\"/a path/100%%/$x/\\\"name\\\"\""
        );
        assert!(unit_quote("safe\nExecStart=evil").is_err());
        assert!(unit_quote("safe\0bad").is_err());
    }

    #[test]
    fn unit_rendering_keeps_config_dollars_literal_and_exec_dollars_escaped() {
        // systemd expands command arguments and Environment assignments differently.
        let paths = AppPaths {
            config_dir: "/home/user/$config".into(),
            state_dir: "/home/user/state%test".into(),
            runtime_dir: "/run/user/123/fta".into(),
            default_receive_dir: "/home/user/inbox".into(),
        };
        let unit = render_unit(Path::new("/home/user/$binary/fta-engine"), &paths).unwrap();
        assert!(unit.contains("ExecStart=\"/home/user/$$binary/fta-engine\" serve"));
        assert!(unit.contains("Environment=\"FTA_CONFIG_DIR=/home/user/$config\""));
        assert!(unit.contains("Environment=\"FTA_STATE_DIR=/home/user/state%%test\""));
    }
}
