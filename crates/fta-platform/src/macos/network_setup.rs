//! macOS has application firewall rules, not Windows public/private connection profiles.
use crate::network_setup::{self, FirewallStatus, NetworkSetupRequest, NetworkSetupStatus};
use std::{io, path::Path, process::Command};

const FIREWALL_TOOL: &str = "/usr/libexec/ApplicationFirewall/socketfilterfw";
// Arguments are passed to AppleScript separately; quoted form prevents shell interpretation of paths.
const ALLOW_ENGINE_SCRIPT: &str = r#"on run argv
set enginePath to item 1 of argv
do shell script "/usr/libexec/ApplicationFirewall/socketfilterfw --add " & quoted form of enginePath & " && /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp " & quoted form of enginePath with administrator privileges
end run"#;

fn query(arguments: &[&std::ffi::OsStr]) -> io::Result<String> {
    let output = Command::new(FIREWALL_TOOL)
        .args(arguments)
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "macOS 방화벽 상태를 읽을 수 없습니다. 시스템 설정에서 확인하세요",
        ));
    }
    String::from_utf8(output.stdout).map_err(io::Error::other)
}

// Unknown/localized output stays unknown rather than treating an unfamiliar message as permission.
fn toggle(text: &str) -> Option<bool> {
    let text = text.to_ascii_lowercase();
    if text.contains("disabled") {
        Some(false)
    } else if text.contains("enabled") {
        Some(true)
    } else {
        None
    }
}

pub fn status(engine_path: &Path) -> io::Result<NetworkSetupStatus> {
    let enabled = query(&["--getglobalstate".as_ref()])
        .ok()
        .and_then(|v| toggle(&v));
    let block_all = query(&["--getblockall".as_ref()])
        .ok()
        .and_then(|v| toggle(&v));
    let engine_allowed = if enabled == Some(true) {
        query(&["--getappblocked".as_ref(), engine_path.as_os_str()])
            .ok()
            .and_then(|v| {
                let text = v.to_ascii_lowercase();
                if text.contains(" is permitted") {
                    Some(true)
                } else if text.contains(" is blocked") {
                    Some(false)
                } else {
                    None
                }
            })
    } else {
        None
    };
    Ok(NetworkSetupStatus{platform:"macos".into(),interfaces:network_setup::local_interfaces()?,
        firewall:FirewallStatus{enabled,block_all,engine_allowed,detail:if block_all==Some(true) {
            "모든 수신 연결 차단이 켜져 있습니다. 시스템 설정 → 네트워크 → 방화벽에서 직접 확인하세요. 앱은 이 보호 설정을 끄지 않습니다."
        } else if enabled==Some(false) {"방화벽이 꺼져 있어 별도 허용 규칙은 필요하지 않습니다. 방화벽 상태는 변경하지 않습니다."}
        else {"필요하면 OS 관리자 인증 후 FTA 엔진만 수신 허용합니다. 다른 앱과 전체 방화벽 설정은 바꾸지 않습니다."}.into()},
        local_network_permission:"로컬 네트워크 권한은 자동 판정할 수 없습니다. macOS가 요청하면 허용하고, 차단한 경우 시스템 설정 → 개인정보 보호 및 보안 → 로컬 네트워크에서 FTA를 확인하세요.".into()})
}

pub fn apply(engine_path: &Path, request: &NetworkSetupRequest) -> io::Result<NetworkSetupStatus> {
    let before = status(engine_path)?;
    network_setup::selected_interface(&before.interfaces, request)?;
    if request.make_private {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Mac에는 Windows 네트워크 프로필이 없습니다",
        ));
    }
    if request.configure_firewall && before.firewall.enabled != Some(false) {
        if before.firewall.enabled != Some(true) || before.firewall.block_all != Some(false) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "방화벽 전체 차단 상태를 먼저 시스템 설정에서 확인하세요",
            ));
        }
        // Canonicalization is trusted-host derived; the UI cannot submit an executable to elevate.
        let engine = engine_path.canonicalize()?;
        if !engine.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "FTA 엔진이 없습니다",
            ));
        }
        if before.firewall.engine_allowed != Some(true) {
            let output = Command::new("/usr/bin/osascript")
                .args(["-e", ALLOW_ENGINE_SCRIPT, "--"])
                .arg(&engine)
                .output()?;
            if !output.status.success() {
                let cancelled = String::from_utf8_lossy(&output.stderr).contains("(-128)");
                return Err(io::Error::new(
                    if cancelled {
                        io::ErrorKind::Interrupted
                    } else {
                        io::ErrorKind::PermissionDenied
                    },
                    if cancelled {
                        "관리자 인증을 취소했습니다. 방화벽 상태를 다시 확인하세요"
                    } else {
                        "FTA 방화벽 허용에 실패했습니다. 관리자 권한과 시스템 설정을 확인하세요"
                    },
                ));
            }
        }
    }
    let after = status(engine_path)?;
    network_setup::selected_interface(&after.interfaces, request)?;
    if request.configure_firewall
        && after.firewall.enabled != Some(false)
        && (after.firewall.engine_allowed != Some(true) || after.firewall.block_all != Some(false))
    {
        return Err(io::Error::other(
            "방화벽 허용을 확인하지 못했습니다. 재점검 후 다시 시도하세요",
        ));
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrecognized_firewall_output_never_means_allowed() {
        assert_eq!(toggle("Firewall is disabled. (State = 0)"), Some(false));
        assert_eq!(
            toggle("Firewall has block all state set to enabled."),
            Some(true)
        );
        assert_eq!(toggle("permission denied"), None);
    }
}
