//! Native network setup is split from the UI and the privileged helper transport.
mod native;
// Elevation exchanges bounded data through a current-user pipe, never a shell command payload.
mod elevation;

use crate::network_setup::{
    FirewallStatus, NetworkInterface, NetworkProfile, NetworkSetupRequest, NetworkSetupStatus,
};
use native::{Address, Connection, Rule, error};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    NET_FW_IP_PROTOCOL_TCP, NET_FW_IP_PROTOCOL_UDP, NET_FW_MODIFY_STATE_OK, NET_FW_PROFILE2_PRIVATE,
};

const ENGINE_NAME: &str = "fta-engine.exe";
pub const HELPER_NAME: &str = "fta-network-helper.exe";
const PLAN_VERSION: u8 = 1;
const PLAN_LIFETIME: u64 = 180;
// Reject a second invoke while the first request owns the consent/mutation flow.
static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: u8,
    created_at: u64,
    expires_at: u64,
    user_sid: String,
    engine: PathBuf,
    connection: Connection,
    address: Address,
    make_private: bool,
    configure_firewall: bool,
    discovery_enabled: bool,
    confirmed_home_network: bool,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    code: String,
    message: String,
}

fn now() -> io::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(error)?
        .as_secs())
}
fn failure(code: &str, detail: impl std::fmt::Display) -> io::Error {
    let kind = match code {
        "CANCELLED" => io::ErrorKind::Interrupted,
        "DENIED" => io::ErrorKind::PermissionDenied,
        "STALE" => io::ErrorKind::InvalidInput,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, format!("NETWORK_SETUP_{code}: {detail}"))
}
fn selection_id(connection: &Connection, address: &Address) -> io::Result<String> {
    // Bind the displayed selection to its exact network GUID/adapter/addresses. A changed LAN
    // with the same adapter and IP cannot reuse the screen's previous home-network approval.
    let mut identity = b"FTA.NetworkSelection.v1\0".to_vec();
    // Public -> Private is an explicitly requested result, not a different connection. Preserve
    // the selection ID across that category change so the host can save the verified LAN scope.
    let mut stable = connection.clone();
    stable.profile.clear();
    identity.extend(serde_json::to_vec(&(stable, address)).map_err(error)?);
    Ok(format!(
        "windows:{}",
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, &identity)
    ))
}
fn selected(facts: &[Connection], id: &str, ip: &str) -> io::Result<(Connection, Address)> {
    for connection in facts.iter().filter(|c| c.connected) {
        for address in &connection.addresses {
            if address.address == ip && selection_id(connection, address)? == id {
                return Ok((connection.clone(), address.clone()));
            }
        }
    }
    Err(failure(
        "STALE",
        "선택한 연결이 변경되었습니다. 네트워크 상태를 다시 확인해주세요.",
    ))
}
fn checked_engine(path: &Path) -> io::Result<PathBuf> {
    checked_engine_beside(path, &std::env::current_exe()?)
}
// The host path always comes from current_exe in production. A separate resolver lets native
// identity tests use disposable installations instead of writing beside the running app.
fn checked_engine_beside(path: &Path, executable: &Path) -> io::Result<PathBuf> {
    let path = dunce::canonicalize(path)?;
    let executable = dunce::canonicalize(executable)?;
    // A host-side engine override is not permission to elevate an arbitrary external program.
    // Compare the fixed installed file's identity: folded names and a shared parent could admit
    // a different case-only sibling on NTFS. Real aliases remain valid; missing files fail closed.
    let installed = executable
        .parent()
        .ok_or_else(|| error("app parent missing"))?
        .join(ENGINE_NAME);
    if !same_file::is_same_file(&path, &installed).unwrap_or(false) {
        return Err(failure(
            "DENIED",
            "앱과 같은 설치 폴더의 FTA 엔진만 자동 설정할 수 있습니다.",
        ));
    }
    let mut signature = [0u8; 2];
    fs::File::open(&path)?.read_exact(&mut signature)?;
    if signature != *b"MZ" {
        return Err(failure(
            "DENIED",
            "설치된 Windows 엔진 실행 파일을 확인할 수 없습니다.",
        ));
    }
    // Accept an input alias only as identity evidence. Returning that alias could redirect
    // the subsequent helper lookup to its external directory; always return our fixed sibling.
    dunce::canonicalize(installed)
}
fn scopes(plan: &Plan) -> (Rule, Rule) {
    let group = format!("{}.{}", native::RULE_GROUP, plan.user_sid);
    let tcp = Rule {
        name: format!("FTA-HomeNetwork-TCP-{}", plan.user_sid),
        group: group.clone(),
        program: plan.engine.to_string_lossy().into_owned(),
        interface: plan.connection.name.clone(),
        local_address: plan.address.address.clone(),
        remote_address: plan.address.cidr.clone(),
        protocol: NET_FW_IP_PROTOCOL_TCP.0,
        local_port: "*".into(),
    };
    // Multicast packets target the mDNS group, not the adapter's unicast address. Restricting
    // only LocalAddress to the unicast IP would silently prevent automatic discovery.
    let udp = Rule {
        name: format!("FTA-HomeNetwork-UDP-{}", plan.user_sid),
        group,
        local_address: format!("{},{}", plan.address.address, native::MDNS_ADDRESS),
        protocol: NET_FW_IP_PROTOCOL_UDP.0,
        local_port: native::MDNS_PORT.into(),
        ..tcp.clone()
    };
    (tcp, udp)
}
fn plan(engine: &Path, request: &NetworkSetupRequest, facts: &[Connection]) -> io::Result<Plan> {
    if !request.confirmed_home_network {
        return Err(failure(
            "CANCELLED",
            "선택한 연결이 집 네트워크인지 먼저 확인해주세요.",
        ));
    }
    let (connection, address) = selected(facts, &request.interface_id, &request.address)?;
    if !matches!(connection.profile.as_str(), "private" | "public")
        || (request.make_private && connection.profile == "public" && connection.shared_profile)
    {
        return Err(failure(
            "DENIED",
            "관리되는 네트워크나 여러 연결이 공유하는 공용 프로필은 자동 변경할 수 없습니다.",
        ));
    }
    if request.configure_firewall && connection.profile == "public" && !request.make_private {
        return Err(failure(
            "DENIED",
            "공용 연결에는 수신을 허용하지 않습니다. 집 연결 확인 후 개인 네트워크 전환을 선택해주세요.",
        ));
    }
    let created_at = now()?;
    Ok(Plan {
        version: PLAN_VERSION,
        created_at,
        expires_at: created_at + PLAN_LIFETIME,
        user_sid: super::current_user_sid()?,
        engine: engine.into(),
        connection,
        address,
        make_private: request.make_private,
        configure_firewall: request.configure_firewall,
        discovery_enabled: request.discovery_enabled,
        confirmed_home_network: true,
    })
}

/// Read-only native status; call on a blocking worker, never the WebView/UI event thread.
pub fn status(engine_path: &Path) -> io::Result<NetworkSetupStatus> {
    let engine = checked_engine(engine_path)?;
    let facts = native::connections()?;
    let mut interfaces = Vec::new();
    for connection in &facts {
        for address in &connection.addresses {
            if !connection.connected {
                continue;
            }
            interfaces.push(NetworkInterface {
                id: selection_id(connection, address)?,
                name: connection.name.clone(),
                address: address.address.clone(),
                prefix: address.prefix_length,
                network: address.cidr.clone(),
                profile: match connection.profile.as_str() {
                    "private" => NetworkProfile::Private,
                    "public" => NetworkProfile::Public,
                    "domain" => NetworkProfile::Domain,
                    _ => NetworkProfile::Unknown,
                },
            });
        }
    }
    let firewall = inspect_firewall(&engine, &facts).unwrap_or_else(|e| FirewallStatus {
        enabled: None,
        block_all: None,
        engine_allowed: None,
        detail: format!("방화벽 상태를 읽지 못했습니다: {e}"),
    });
    Ok(NetworkSetupStatus {
        platform: "windows".into(),
        interfaces,
        firewall,
        local_network_permission: "필요한 경우 Windows 관리자 승인을 요청합니다.".into(),
    })
}
fn inspect_firewall(engine: &Path, facts: &[Connection]) -> io::Result<FirewallStatus> {
    let _apartment = native::Apartment::enter()?;
    let policy = native::policy()?;
    // SAFETY: these profile-specific getters are read-only and the COM guard stays alive.
    let (enabled, block_all) = unsafe {
        (
            policy
                .get_FirewallEnabled(NET_FW_PROFILE2_PRIVATE)
                .map_err(error)?
                .as_bool(),
            policy
                .get_BlockAllInboundTraffic(NET_FW_PROFILE2_PRIVATE)
                .map_err(error)?
                .as_bool(),
        )
    };
    let sid = super::current_user_sid()?;
    let mut engine_allowed = None;
    let rule = native::find_rule(&policy, &format!("FTA-HomeNetwork-TCP-{sid}"))?;
    if let Some(rule) = rule {
        engine_allowed = Some(false);
        for connection in facts
            .iter()
            .filter(|c| c.connected && c.profile == "private")
        {
            for address in &connection.addresses {
                let p = Plan {
                    version: PLAN_VERSION,
                    created_at: 0,
                    expires_at: 0,
                    user_sid: sid.clone(),
                    engine: engine.into(),
                    connection: connection.clone(),
                    address: address.clone(),
                    make_private: false,
                    configure_firewall: true,
                    discovery_enabled: false,
                    confirmed_home_network: false,
                };
                if enabled && !block_all && native::matches(&rule, &scopes(&p).0)? {
                    engine_allowed = Some(true);
                }
            }
        }
    }
    let detail = match (enabled, block_all, engine_allowed) {
        (false, _, _) => "개인 프로필 방화벽이 꺼져 있습니다. 자동 설정은 전체 방화벽 정책을 변경하지 않습니다.",
        (_, true, _) => "개인 프로필이 모든 수신을 차단하고 있습니다. 기존 보안 정책은 유지됩니다.",
        (_, _, Some(true)) => "현재 연결과 설치된 FTA 엔진에 한정한 개인 네트워크 수신 규칙을 확인했습니다.",
        (_, _, Some(false)) => "FTA 관리 규칙이 현재 연결 또는 설치 경로와 다릅니다. 집 연결을 선택해 갱신할 수 있습니다.",
        _ => "FTA 자동 설정 규칙이 없습니다. 별도로 만든 사용자 규칙의 연결 허용 여부는 판단하지 않았습니다.",
    }.into();
    Ok(FirewallStatus {
        enabled: Some(enabled),
        block_all: Some(block_all),
        engine_allowed,
        detail,
    })
}

/// Freeze/revalidate the selected OS facts before requesting standard Windows UAC consent.
pub fn apply(engine_path: &Path, request: &NetworkSetupRequest) -> io::Result<NetworkSetupStatus> {
    let _guard = APPLY_LOCK
        .try_lock()
        .map_err(|_| failure("FAILED", "다른 네트워크 설정 요청이 진행 중입니다."))?;
    // Check confirmation before resolving executables or reaching any elevation path.
    if !request.confirmed_home_network {
        return Err(failure(
            "CANCELLED",
            "선택한 연결이 집 네트워크인지 먼저 확인해주세요.",
        ));
    }
    let engine = checked_engine(engine_path)?;
    let plan = plan(&engine, request, &native::connections()?)?;
    if plan.configure_firewall {
        let _apartment = native::Apartment::enter()?;
        check_firewall_policy(&native::policy()?)?;
    }
    if !plan.make_private && !plan.configure_firewall {
        return status(&engine);
    }
    if already_applied(&plan)? {
        return status(&engine);
    }
    let helper = engine
        .parent()
        .ok_or_else(|| error("engine parent missing"))?
        .join(HELPER_NAME);
    if !helper.is_file() {
        return Err(failure(
            "FAILED",
            "네트워크 설정 도우미가 없습니다. 앱 설치 상태를 확인해주세요.",
        ));
    }
    let response = elevation::exchange(&helper, &serde_json::to_vec(&plan).map_err(error)?)
        .map_err(|e| {
            let code = match (e.kind(), e.raw_os_error()) {
                (io::ErrorKind::ConnectionAborted, _) => "PARTIAL",
                (_, Some(1223)) => "CANCELLED",
                (_, Some(5)) => "DENIED",
                _ => "FAILED",
            };
            failure(code, e)
        })?;
    let reply: Reply = serde_json::from_slice(&response).map_err(error)?;
    if reply.code != "APPLIED" {
        return Err(failure(&reply.code, reply.message));
    }
    // Do not equate helper exit/success text with an applied OS policy: read the actual facts.
    let current = current_connection(&plan, true).map_err(|e| failure("PARTIAL", e))?;
    if (plan.make_private && current.profile != "private")
        || !already_applied(&Plan {
            connection: current,
            ..plan.clone()
        })
        .map_err(|e| failure("PARTIAL", e))?
    {
        return Err(failure(
            "PARTIAL",
            "설정 후 상태 확인이 일치하지 않습니다. 현재 네트워크 상태를 다시 확인해주세요.",
        ));
    }
    status(&engine).map_err(|e| failure("PARTIAL", e))
}
fn current_connection(plan: &Plan, allow_private_transition: bool) -> io::Result<Connection> {
    let actual = native::connections()?
        .into_iter()
        .find(|c| c.id == plan.connection.id)
        .ok_or_else(|| failure("STALE", "선택한 연결이 사라졌습니다."))?;
    let mut expected = plan.connection.clone();
    if allow_private_transition && plan.make_private && actual.profile == "private" {
        expected.profile = "private".into();
    }
    if actual != expected {
        return Err(failure(
            "STALE",
            "선택한 집 연결이 변경되었습니다. 다시 확인해주세요.",
        ));
    }
    Ok(actual)
}
fn already_applied(plan: &Plan) -> io::Result<bool> {
    if plan.make_private && plan.connection.profile != "private" {
        return Ok(false);
    }
    if !plan.configure_firewall {
        return Ok(true);
    }
    let _apartment = native::Apartment::enter()?;
    let policy = native::policy()?;
    let (tcp, udp) = scopes(plan);
    check_firewall_policy(&policy)?;
    let Some(actual_tcp) = native::find_rule(&policy, &tcp.name)? else {
        return Ok(false);
    };
    if !native::matches(&actual_tcp, &tcp)? {
        return Ok(false);
    }
    let actual_udp = native::find_rule(&policy, &udp.name)?;
    if plan.discovery_enabled {
        Ok(actual_udp.is_some_and(|actual| native::matches(&actual, &udp).unwrap_or(false)))
    } else {
        Ok(actual_udp.is_none())
    }
}

fn check_firewall_policy(
    policy: &windows::Win32::NetworkManagement::WindowsFirewall::INetFwPolicy2,
) -> io::Result<()> {
    // SAFETY: only inspect current profile restrictions; never relax global or GPO policy.
    unsafe {
        if !policy
            .get_FirewallEnabled(NET_FW_PROFILE2_PRIVATE)
            .map_err(error)?
            .as_bool()
            || policy
                .get_BlockAllInboundTraffic(NET_FW_PROFILE2_PRIVATE)
                .map_err(error)?
                .as_bool()
            || policy.LocalPolicyModifyState().map_err(error)? != NET_FW_MODIFY_STATE_OK
        {
            return Err(failure(
                "DENIED",
                "기존 방화벽 보안 정책 때문에 자동 규칙을 적용할 수 없습니다. 정책은 변경하지 않았습니다.",
            ));
        }
    }
    Ok(())
}

fn validate_helper_plan(plan: &Plan) -> io::Result<()> {
    let current_time = now()?;
    if plan.version != PLAN_VERSION
        || !plan.confirmed_home_network
        || plan.created_at > current_time
        || plan.expires_at != plan.created_at.saturating_add(PLAN_LIFETIME)
        || plan.expires_at < current_time
    {
        return Err(failure(
            "STALE",
            "설정 요청이 만료되었습니다. 집 연결을 다시 선택해주세요.",
        ));
    }
    if plan.user_sid != super::current_user_sid()? {
        return Err(failure(
            "DENIED",
            "다른 사용자 계정의 설정 요청은 적용하지 않습니다.",
        ));
    }
    if checked_engine(&plan.engine)? != plan.engine {
        return Err(failure("DENIED", "설치된 엔진 경로가 변경되었습니다."));
    }
    let current = current_connection(plan, false)?;
    if !current.addresses.contains(&plan.address)
        || !matches!(current.profile.as_str(), "public" | "private")
        || (plan.make_private && current.profile == "public" && current.shared_profile)
        || (plan.configure_firewall && current.profile == "public" && !plan.make_private)
    {
        return Err(failure(
            "DENIED",
            "현재 선택한 네트워크에서는 이 설정을 적용할 수 없습니다.",
        ));
    }
    Ok(())
}
fn perform(plan: &Plan) -> io::Result<()> {
    let _mutation = elevation::MutationLock::acquire()?;
    validate_helper_plan(plan)?;
    let _apartment = native::Apartment::enter()?;
    let policy = native::policy()?;
    let (tcp, udp) = scopes(plan);
    if plan.configure_firewall {
        // Independently requery after UAC; the host's earlier preflight is not authority to
        // relax existing global firewall/GPO settings or skip their current restrictions.
        check_firewall_policy(&policy)?;
        for desired in [&tcp, &udp] {
            if let Some(rule) = native::find_rule(&policy, &desired.name)? {
                // SAFETY: read the group before changing profile/rules, preserving foreign names.
                if unsafe { rule.Grouping() }.map_err(error)? != desired.group {
                    return Err(failure(
                        "DENIED",
                        "같은 이름의 다른 방화벽 규칙이 있어 덮어쓰지 않았습니다.",
                    ));
                }
            }
        }
    }
    if plan.make_private {
        native::make_private(&plan.connection).map_err(|e| failure("PARTIAL", e))?;
    }
    // Revalidate after profile change; do not attach a grant to an address/network that drifted.
    current_connection(plan, true).map_err(|e| failure("PARTIAL", e))?;
    if plan.configure_firewall {
        native::put_rule(&policy, &tcp).map_err(|e| failure("PARTIAL", e))?;
        if plan.discovery_enabled {
            native::put_rule(&policy, &udp).map_err(|e| failure("PARTIAL", e))?;
        } else {
            native::remove_rule(&policy, &udp.name, &udp.group)
                .map_err(|e| failure("PARTIAL", e))?;
        }
    }
    let current = current_connection(plan, true).map_err(|e| failure("PARTIAL", e))?;
    if !already_applied(&Plan {
        connection: current,
        ..plan.clone()
    })
    .map_err(|e| failure("PARTIAL", e))?
    {
        return Err(failure(
            "PARTIAL",
            "설정된 규칙의 재조회가 일치하지 않습니다.",
        ));
    }
    Ok(())
}

/// Sole elevated entry point: fixed arguments, same-user pipe, then independently validated plan.
pub fn helper_main(arguments: &[String]) -> io::Result<()> {
    if arguments == ["--inspect"] {
        let engine = std::env::current_exe()?
            .parent()
            .ok_or_else(|| error("helper parent missing"))?
            .join(ENGINE_NAME);
        println!(
            "{}",
            serde_json::to_string(&status(&engine)?).map_err(error)?
        );
        return Ok(());
    }
    if arguments.len() != 3 || arguments[0] != "--apply" {
        return Err(error(
            "expected --apply <one-shot pipe instance> <parent pid> or --inspect",
        ));
    }
    // SAFETY: this only checks the current token; it never prompts for or acquires privilege.
    if unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } == 0 {
        return Err(failure("DENIED", "Windows 관리자 승인이 필요합니다."));
    }
    let parent_pid = arguments[2].parse::<u32>().map_err(error)?;
    elevation::serve(&arguments[1], parent_pid, |bytes| {
        let plan: Plan = serde_json::from_slice(bytes).map_err(error)?;
        let reply = match perform(&plan) {
            Ok(()) => Reply {
                code: "APPLIED".into(),
                message: "현재 집 연결의 FTA 수신 설정을 확인했습니다.".into(),
            },
            Err(e) => {
                let message = e.to_string();
                let code = ["STALE", "DENIED", "PARTIAL"]
                    .into_iter()
                    .find(|code| message.starts_with(&format!("NETWORK_SETUP_{code}:")))
                    .unwrap_or("FAILED");
                // The transport carries the category separately; do not duplicate its prefix
                // when the desktop turns the result back into a categorized error.
                let prefix = format!("NETWORK_SETUP_{code}: ");
                let message = message.strip_prefix(&prefix).unwrap_or(&message).to_owned();
                Reply {
                    code: code.into(),
                    message,
                }
            }
        };
        serde_json::to_vec(&reply).map_err(error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Synthetic LANs exercise authorization boundaries without requesting UAC or touching any
    // live profile/rule. Native read-only and detached COM tests cover the actual Windows ABI.
    fn fixture() -> (Connection, NetworkSetupRequest) {
        let address = Address {
            address: "192.168.42.7".into(),
            prefix_length: 24,
            cidr: "192.168.42.0/24".into(),
        };
        let connection = Connection {
            id: uuid::Uuid::new_v4().to_string(),
            name: "home test adapter".into(),
            network_id: uuid::Uuid::new_v4().to_string(),
            profile_name: "home test profile".into(),
            profile: "public".into(),
            connected: true,
            shared_profile: false,
            addresses: vec![address.clone()],
        };
        let request = NetworkSetupRequest {
            interface_id: selection_id(&connection, &address).expect("fixture serialization"),
            address: address.address,
            make_private: true,
            configure_firewall: true,
            discovery_enabled: false,
            confirmed_home_network: true,
        };
        (connection, request)
    }
    fn fixture_plan() -> io::Result<Plan> {
        let (connection, request) = fixture();
        plan(
            Path::new(r"C:\fixture\fta-engine.exe"),
            &request,
            &[connection],
        )
    }

    // Confirmation reaches Rust as an explicit value; missing/false consent cannot even reach
    // executable resolution, pipe creation or ShellExecuteExW.
    #[test]
    fn missing_confirmation_stops_before_elevation() {
        let (_, mut request) = fixture();
        request.confirmed_home_network = false;
        let error = apply(Path::new("not-an-installed-engine"), &request).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(error.to_string().starts_with("NETWORK_SETUP_CANCELLED:"));
    }

    // An old screen must not approve a new network that reused the same adapter/address.
    #[test]
    fn selection_identity_detects_network_and_prefix_drift() -> io::Result<()> {
        let (connection, request) = fixture();
        assert!(
            selected(
                std::slice::from_ref(&connection),
                &request.interface_id,
                &request.address
            )
            .is_ok()
        );
        let mut changed = connection.clone();
        changed.network_id = uuid::Uuid::new_v4().to_string();
        assert!(selected(&[changed], &request.interface_id, &request.address).is_err());
        let mut changed = connection.clone();
        changed.addresses[0].prefix_length = 16;
        changed.addresses[0].cidr = "192.168.0.0/16".into();
        assert!(selected(&[changed], &request.interface_id, &request.address).is_err());
        let mut changed = connection;
        changed.profile = "private".into();
        assert!(selected(&[changed], &request.interface_id, &request.address).is_ok());
        Ok(())
    }

    // Domain and shared profiles require a separate administrator decision; a single home
    // connection selection is never authority to reclassify them or open a Public exception.
    #[test]
    fn domain_shared_and_unconfirmed_public_scopes_are_rejected() -> io::Result<()> {
        for (profile, shared, make_private) in [
            ("domain", false, true),
            ("public", true, true),
            ("public", false, false),
        ] {
            let (mut connection, mut request) = fixture();
            connection.profile = profile.into();
            connection.shared_profile = shared;
            request.make_private = make_private;
            request.interface_id = selection_id(&connection, &connection.addresses[0])?;
            let error = plan(Path::new("unused"), &request, &[connection]).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        }
        Ok(())
    }

    // CIDR/program/commands are not browser inputs; the typed request fails deserialization
    // rather than silently accepting data that a later refactor might feed to elevation.
    #[test]
    fn browser_request_rejects_privileged_inputs() -> io::Result<()> {
        let (_, request) = fixture();
        for field in ["cidr", "engine_path", "command", "script", "network"] {
            let mut value = serde_json::to_value(&request).map_err(error)?;
            value[field] = "untrusted".into();
            assert!(serde_json::from_value::<NetworkSetupRequest>(value).is_err());
        }
        Ok(())
    }

    // One-shot plans are versioned, expire, and bind to the helper's own account before file
    // access or OS mutation. Different-account UAC cannot impersonate the original owner.
    #[test]
    fn helper_rejects_expired_versioned_and_foreign_user_plans() -> io::Result<()> {
        let mut expired = fixture_plan()?;
        expired.created_at = 0;
        expired.expires_at = PLAN_LIFETIME;
        assert!(
            validate_helper_plan(&expired)
                .unwrap_err()
                .to_string()
                .starts_with("NETWORK_SETUP_STALE:")
        );
        let mut incompatible = fixture_plan()?;
        incompatible.version += 1;
        assert!(validate_helper_plan(&incompatible).is_err());
        let mut foreign = fixture_plan()?;
        foreign.user_sid = "another-user".into();
        assert_eq!(
            validate_helper_plan(&foreign).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        Ok(())
    }

    // Both protocols bind the same actual engine/interface/subnet. Discovery is separate from
    // dynamic TCP registration, and enabling it does not create a wildcard address exception.
    #[test]
    fn generated_rules_are_stable_and_discovery_is_narrow() -> io::Result<()> {
        let plan = fixture_plan()?;
        let (tcp, udp) = scopes(&plan);
        assert_eq!(tcp.program, udp.program);
        assert_eq!(tcp.interface, plan.connection.name);
        assert_eq!(tcp.remote_address, plan.address.cidr);
        assert_eq!(udp.remote_address, plan.address.cidr);
        assert_eq!(tcp.local_address, plan.address.address);
        assert_eq!(
            udp.local_address,
            format!("{},224.0.0.251", plan.address.address)
        );
        assert_eq!(udp.local_port, "5353");
        assert_eq!(tcp.local_port, "*");
        assert_ne!(tcp.name, udp.name);
        assert_eq!(scopes(&plan).0, tcp);
        Ok(())
    }

    // Only the installed sibling engine can obtain a rule; an arbitrary existing executable
    // from a raw path is rejected even if it is a valid Windows PE.
    #[test]
    fn unrelated_executable_cannot_be_an_engine_override() -> io::Result<()> {
        let error = checked_engine(&std::env::current_exe()?).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        Ok(())
    }

    // Hard-link aliases identify the installed file; another file with identical bytes
    // does not. Deleting the fixed sibling must revoke acceptance even while its alias survives.
    #[test]
    fn installed_engine_uses_file_identity_and_requires_fixed_sibling() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let host = directory.path().join(HELPER_NAME);
        let engine = directory.path().join(ENGINE_NAME);
        let alias = directory.path().join("engine-alias.exe");
        let other = directory.path().join("other-engine.exe");
        fs::write(&host, b"MZ host fixture")?;
        fs::write(&engine, b"MZ engine fixture")?;
        fs::hard_link(&engine, &alias)?;
        fs::copy(&engine, &other)?;
        assert!(checked_engine_beside(&engine, &host).is_ok());
        assert!(checked_engine_beside(&alias, &host).is_ok());
        // An external alias proves identity but must never select its own helper directory.
        let external_directory = directory.path().join("external-aliases");
        fs::create_dir(&external_directory)?;
        let external_alias = external_directory.join("engine-alias.exe");
        fs::hard_link(&engine, &external_alias)?;
        assert_eq!(
            checked_engine_beside(&external_alias, &host)?,
            dunce::canonicalize(&engine)?
        );
        assert_eq!(
            checked_engine_beside(&other, &host).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        fs::remove_file(&engine)?;
        assert_eq!(
            checked_engine_beside(&alias, &host).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(checked_engine_beside(&engine, &host).is_err());
        Ok(())
    }

    // Run explicitly with TEMP on an already case-sensitive filesystem. This fixture never
    // changes directory attributes, requests privileges, or touches real firewall rules.
    #[test]
    #[ignore = "requires an already case-sensitive temporary filesystem"]
    fn case_sensitive_engine_rejects_case_only_different_file() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let host = directory.path().join(HELPER_NAME);
        let engine = directory.path().join(ENGINE_NAME);
        let other = directory.path().join(ENGINE_NAME.to_ascii_uppercase());
        fs::write(&host, b"MZ host fixture")?;
        fs::write(&engine, b"MZ engine fixture")?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&other)?
            .write_all(b"MZ different engine fixture")?;
        assert!(!same_file::is_same_file(&engine, &other)?);
        assert_eq!(
            checked_engine_beside(&other, &host).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        Ok(())
    }
}
