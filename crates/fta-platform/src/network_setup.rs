//! Only current OS interfaces can become a user-confirmed home connection.
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkProfile {
    Unknown,
    Public,
    Private,
    Domain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub id: String,
    pub name: String,
    pub address: String,
    pub prefix: u8,
    pub network: String,
    pub profile: NetworkProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallStatus {
    pub enabled: Option<bool>,
    pub block_all: Option<bool>,
    pub engine_allowed: Option<bool>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSetupStatus {
    pub platform: String,
    pub interfaces: Vec<NetworkInterface>,
    pub firewall: FirewallStatus,
    pub local_network_permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSetupRequest {
    // Omitted consent must fail closed even when a client submits otherwise valid connection data.
    #[serde(default)]
    pub confirmed_home_network: bool,
    pub interface_id: String,
    pub address: String,
    pub make_private: bool,
    pub configure_firewall: bool,
    pub discovery_enabled: bool,
}

/// Reject stale selections rather than carrying a formerly trusted address onto a new connection.
pub fn selected_interface(
    interfaces: &[NetworkInterface],
    request: &NetworkSetupRequest,
) -> io::Result<NetworkInterface> {
    if !request.confirmed_home_network {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "집 네트워크임을 먼저 확인하세요",
        ));
    }
    interfaces
        .iter()
        .find(|i| i.id == request.interface_id && i.address == request.address)
        .cloned()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "네트워크가 변경되었습니다. 연결을 다시 선택하세요",
            )
        })
}

/// An unchanged IP alone cannot authorize a broader subnet after the user confirmed its scope.
pub fn verify_scope(expected: &NetworkInterface, actual: &NetworkInterface) -> io::Result<()> {
    if expected.id != actual.id
        || expected.address != actual.address
        || expected.prefix != actual.prefix
        || expected.network != actual.network
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "연결 주소 또는 허용 대역이 바뀌었습니다. 다시 확인하세요",
        ));
    }
    Ok(())
}

// The entire subnet, not just this address, must remain private when deriving an automatic scope.
fn private_subnet(
    address: std::net::Ipv4Addr,
    netmask: std::net::Ipv4Addr,
) -> Option<ipnet::Ipv4Net> {
    let mask = u32::from(netmask);
    let prefix = mask.leading_ones() as u8;
    if prefix == 0 || mask != u32::MAX.checked_shl(u32::from(32 - prefix)).unwrap_or(0) {
        return None;
    }
    let network = ipnet::Ipv4Net::new(address, prefix).ok()?.trunc();
    (network.network().is_private() && network.broadcast().is_private()).then_some(network)
}

/// Enumerate private IPv4 candidates; the user, not RFC1918 membership, decides whether one is home.
pub fn local_interfaces() -> io::Result<Vec<NetworkInterface>> {
    let mut result = Vec::new();
    for interface in if_addrs::get_if_addrs()? {
        if interface.oper_status != if_addrs::IfOperStatus::Up {
            continue;
        }
        let if_addrs::IfAddr::V4(address) = interface.addr else {
            continue;
        };
        if !address.ip.is_private() || address.ip.is_loopback() {
            continue;
        }
        let Some(network) = private_subnet(address.ip, address.netmask) else {
            continue;
        };
        let prefix = network.prefix_len();
        let network = network.to_string();
        result.push(NetworkInterface {
            id: interface.name.clone(),
            name: interface.name,
            address: address.ip.to_string(),
            prefix,
            network,
            profile: NetworkProfile::Unknown,
        });
    }
    result.sort_by(|a, b| (&a.id, &a.address).cmp(&(&b.id, &b.address)));
    result.dedup_by(|a, b| a.id == b.id && a.address == b.address);
    Ok(result)
}

pub fn status(engine_path: &Path) -> io::Result<NetworkSetupStatus> {
    #[cfg(target_os = "macos")]
    return crate::macos::network_setup::status(engine_path);
    #[cfg(windows)]
    return crate::windows::network_setup::status(engine_path);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = engine_path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "데스크톱 네트워크 설정은 Mac과 Windows에서 지원합니다",
        ))
    }
}

pub fn apply(engine_path: &Path, request: &NetworkSetupRequest) -> io::Result<NetworkSetupStatus> {
    #[cfg(target_os = "macos")]
    return crate::macos::network_setup::apply(engine_path, request);
    #[cfg(windows)]
    return crate::windows::network_setup::apply(engine_path, request);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (engine_path, request);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "데스크톱 네트워크 설정은 Mac과 Windows에서 지원합니다",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_scope_never_spills_into_public_addresses() {
        let ip = |text: &str| text.parse::<std::net::Ipv4Addr>().unwrap();
        assert!(private_subnet(ip("10.1.2.3"), ip("255.0.0.0")).is_some());
        assert!(private_subnet(ip("172.16.1.2"), ip("255.0.0.0")).is_none());
        assert!(private_subnet(ip("192.168.1.2"), ip("255.0.0.0")).is_none());
        assert!(private_subnet(ip("192.168.1.2"), ip("255.0.255.0")).is_none());
        assert!(private_subnet(ip("192.168.1.2"), ip("0.0.0.0")).is_none());
    }

    #[test]
    fn unchanged_address_does_not_authorize_a_wider_network() {
        let expected = NetworkInterface {
            id: "en0".into(),
            name: "Ethernet".into(),
            address: "192.168.1.2".into(),
            prefix: 24,
            network: "192.168.1.0/24".into(),
            profile: NetworkProfile::Public,
        };
        let mut actual = expected.clone();
        actual.profile = NetworkProfile::Private;
        assert!(verify_scope(&expected, &actual).is_ok());
        actual.prefix = 16;
        actual.network = "192.168.0.0/16".into();
        assert!(verify_scope(&expected, &actual).is_err());
    }

    #[test]
    fn stale_or_other_interface_cannot_reuse_a_home_confirmation() {
        let iface = NetworkInterface {
            id: "en0".into(),
            name: "Ethernet".into(),
            address: "192.168.1.2".into(),
            prefix: 24,
            network: "192.168.1.0/24".into(),
            profile: NetworkProfile::Unknown,
        };
        let mut request = NetworkSetupRequest {
            confirmed_home_network: true,
            interface_id: "en0".into(),
            address: "192.168.1.2".into(),
            make_private: false,
            configure_firewall: false,
            discovery_enabled: false,
        };
        assert!(selected_interface(std::slice::from_ref(&iface), &request).is_ok());
        request.confirmed_home_network = false;
        assert!(selected_interface(std::slice::from_ref(&iface), &request).is_err());
        request.confirmed_home_network = true;
        request.interface_id = "en1".into();
        assert!(selected_interface(std::slice::from_ref(&iface), &request).is_err());
        request.interface_id = "en0".into();
        request.address = "192.168.1.3".into();
        assert!(selected_interface(&[iface], &request).is_err());
    }
}
