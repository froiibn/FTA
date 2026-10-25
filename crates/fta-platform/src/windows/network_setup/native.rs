//! OS objects stay on their initializing thread; callers exchange only owned facts.
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, mem::ManuallyDrop};
use windows::{
    Win32::{
        Foundation::{RPC_E_CHANGED_MODE, VARIANT_FALSE, VARIANT_TRUE},
        NetworkManagement::WindowsFirewall::*,
        Networking::NetworkListManager::*,
        System::{
            Com::{
                CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance,
                CoInitializeEx, CoUninitialize,
            },
            Ole::{
                SafeArrayCreateVector, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
                SafeArrayPutElement,
            },
            Variant::{
                VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_ARRAY, VT_BSTR, VT_VARIANT,
                VariantClear,
            },
        },
    },
    core::{BSTR, GUID},
};

// These identify this app's rules and the standardized IPv4 mDNS destination, not a user's LAN.
pub(super) const RULE_GROUP: &str = "FTA.HomeNetwork.v1";
pub(super) const MDNS_ADDRESS: &str = "224.0.0.251";
pub(super) const MDNS_PORT: &str = "5353";

pub(super) fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

pub(super) struct Apartment(bool);
impl Apartment {
    pub(super) fn enter() -> io::Result<Self> {
        // SAFETY: no reserved pointer is supplied; the guard balances only this thread's init.
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if status == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        status.ok().map_err(error)?;
        Ok(Self(true))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this guard owns one successful COM initialization on the current thread.
            unsafe { CoUninitialize() };
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Address {
    pub address: String,
    pub prefix_length: u8,
    pub cidr: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Connection {
    pub id: String,
    pub name: String,
    pub network_id: String,
    pub profile_name: String,
    pub profile: String,
    pub connected: bool,
    pub shared_profile: bool,
    pub addresses: Vec<Address>,
}

fn guid_text(value: GUID) -> String {
    uuid::Uuid::from_u128(value.to_u128()).to_string()
}

pub(super) fn connections() -> io::Result<Vec<Connection>> {
    let _apartment = Apartment::enter()?;
    let addresses = if_addrs::get_if_addrs()?;
    // SAFETY: COM is initialized and the typed CLSID/interface pair is the SDK's NLM service.
    let manager: INetworkListManager =
        unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL) }.map_err(error)?;
    // SAFETY: manager is a live interface on this initialized thread.
    let enumerator = unsafe { manager.GetNetworkConnections() }.map_err(error)?;
    let mut result = Vec::new();
    loop {
        let mut item = [None];
        let mut fetched = 0;
        // SAFETY: item and fetched are writable; Next's returned interface is owned by item.
        unsafe { enumerator.Next(&mut item, Some(&mut fetched)) }.map_err(error)?;
        if fetched == 0 {
            break;
        }
        let connection = item[0]
            .take()
            .ok_or_else(|| error("NLM returned no connection"))?;
        // SAFETY: all methods borrow live COM objects on this thread and return owned values.
        let (id, network, connected) = unsafe {
            (
                guid_text(connection.GetAdapterId().map_err(error)?),
                connection.GetNetwork().map_err(error)?,
                connection.IsConnected().map_err(error)?.as_bool(),
            )
        };
        // SAFETY: network is live for every property query.
        let (network_id, profile_name, category) = unsafe {
            (
                guid_text(network.GetNetworkId().map_err(error)?),
                network.GetName().map_err(error)?.to_string(),
                network.GetCategory().map_err(error)?,
            )
        };
        let mut name = None;
        let mut selected = Vec::new();
        for interface in &addresses {
            let Ok(adapter_id) =
                uuid::Uuid::parse_str(interface.adapter_name.trim_matches(['{', '}']))
            else {
                continue;
            };
            if adapter_id.to_string() != id {
                continue;
            }
            name = Some(interface.name.clone());
            if !interface.is_oper_up() {
                continue;
            }
            // The setup flow initially supports concrete RFC1918 IPv4 LANs, never /0 or APIPA.
            if let if_addrs::IfAddr::V4(value) = &interface.addr
                && value.ip.is_private()
                && (1..=32).contains(&value.prefixlen)
            {
                let cidr = Ipv4Net::new(value.ip, value.prefixlen)
                    .map_err(error)?
                    .trunc();
                // An unusually broad adapter mask must not turn a private host address into
                // a firewall grant spanning public addresses outside the selected home LAN.
                if !cidr.network().is_private() || !cidr.broadcast().is_private() {
                    continue;
                }
                selected.push(Address {
                    address: value.ip.to_string(),
                    prefix_length: value.prefixlen,
                    cidr: cidr.to_string(),
                });
            }
        }
        selected.sort_by(|a, b| a.address.cmp(&b.address));
        selected.dedup();
        let profile = match category {
            NLM_NETWORK_CATEGORY_PUBLIC => "public",
            NLM_NETWORK_CATEGORY_PRIVATE => "private",
            NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED => "domain",
            _ => "unknown",
        };
        result.push(Connection {
            id,
            name: name.unwrap_or(profile_name.clone()),
            network_id,
            profile_name,
            profile: profile.into(),
            connected,
            shared_profile: false,
            addresses: selected,
        });
    }
    // Changing an NLM profile can affect several adapters; never silently expand that choice.
    let mut counts = BTreeMap::new();
    for item in &result {
        *counts.entry(item.network_id.clone()).or_insert(0usize) += 1;
    }
    for item in &mut result {
        item.shared_profile = counts[&item.network_id] > 1;
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

pub(super) fn make_private(expected: &Connection) -> io::Result<()> {
    let actual = connections()?
        .into_iter()
        .find(|c| c.id == expected.id)
        .ok_or_else(|| error("selected connection disappeared"))?;
    if &actual != expected {
        return Err(error("selected network changed; inspect again"));
    }
    if actual.profile == "private" {
        return Ok(());
    }
    if actual.profile != "public" || actual.shared_profile {
        return Err(error("only an unshared Public profile may become Private"));
    }
    let _apartment = Apartment::enter()?;
    let guid = GUID::from_u128(
        uuid::Uuid::parse_str(&actual.network_id)
            .map_err(error)?
            .as_u128(),
    );
    // SAFETY: use the selected network's immutable GUID, not a reusable interface index, so a
    // newly connected network cannot accidentally receive this user's Private classification.
    unsafe {
        let manager: INetworkListManager =
            CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).map_err(error)?;
        manager
            .GetNetwork(guid)
            .map_err(error)?
            .SetCategory(NLM_NETWORK_CATEGORY_PRIVATE)
            .map_err(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rule {
    pub name: String,
    pub group: String,
    pub program: String,
    pub interface: String,
    pub local_address: String,
    pub remote_address: String,
    pub protocol: i32,
    pub local_port: String,
}

// VARIANT is an FFI record without Drop; this owner releases the array and its BSTR copies once.
struct OwnedVariant(VARIANT);
impl Drop for OwnedVariant {
    fn drop(&mut self) {
        // SAFETY: all members are either zero-initialized or uniquely owned COM allocations.
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}
fn string_variant(value: &str) -> OwnedVariant {
    OwnedVariant(VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BSTR,
                Anonymous: VARIANT_0_0_0 {
                    bstrVal: ManuallyDrop::new(BSTR::from(value)),
                },
                ..Default::default()
            }),
        },
    })
}
fn interface_variant(value: &str) -> io::Result<OwnedVariant> {
    // SAFETY: request one VARIANT slot; an owned VARIANT below always frees a successful array.
    let array = unsafe { SafeArrayCreateVector(VT_VARIANT, 0, 1) };
    if array.is_null() {
        return Err(error("could not allocate firewall interface array"));
    }
    let result = OwnedVariant(VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_ARRAY | VT_VARIANT,
                Anonymous: VARIANT_0_0_0 { parray: array },
                ..Default::default()
            }),
        },
    });
    let item = string_variant(value);
    // SAFETY: slot zero exists; SafeArrayPutElement copies the complete BSTR variant.
    unsafe { SafeArrayPutElement(array, &0, (&item.0 as *const VARIANT).cast()) }.map_err(error)?;
    Ok(result)
}
fn interfaces(value: VARIANT) -> io::Result<Vec<String>> {
    let value = OwnedVariant(value);
    // SAFETY: this is a live VARIANT returned by INetFwRule; inspect the type before its union.
    unsafe {
        if value.0.Anonymous.Anonymous.vt != (VT_ARRAY | VT_VARIANT) {
            return Ok(Vec::new());
        }
        let array = value.0.Anonymous.Anonymous.Anonymous.parray;
        let lower = SafeArrayGetLBound(array, 1).map_err(error)?;
        let upper = SafeArrayGetUBound(array, 1).map_err(error)?;
        if upper.saturating_sub(lower) > 128 {
            return Err(error("unexpected firewall interface array size"));
        }
        let mut result = Vec::new();
        for index in lower..=upper {
            let mut item = OwnedVariant(VARIANT::default());
            SafeArrayGetElement(array, &index, (&mut item.0 as *mut VARIANT).cast())
                .map_err(error)?;
            if item.0.Anonymous.Anonymous.vt != VT_BSTR {
                return Err(error("firewall interface is not a name"));
            }
            result.push(item.0.Anonymous.Anonymous.Anonymous.bstrVal.to_string());
        }
        Ok(result)
    }
}

pub(super) fn policy() -> io::Result<INetFwPolicy2> {
    // SAFETY: the caller owns an Apartment guard for the entire lifetime of the returned object.
    unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }.map_err(error)
}
pub(super) fn find_rule(policy: &INetFwPolicy2, name: &str) -> io::Result<Option<INetFwRule>> {
    // SAFETY: policy is live on its initialized thread; the BSTR is borrowed only for this call.
    match unsafe { policy.Rules().map_err(error)?.Item(&BSTR::from(name)) } {
        Ok(rule) => Ok(Some(rule)),
        Err(e) if e.code().0 as u32 == 0x8007_0002 => Ok(None),
        Err(e) => Err(error(e)),
    }
}
pub(super) fn matches(rule: &INetFwRule, expected: &Rule) -> io::Result<bool> {
    // Windows aliases may name one file, while case-only paths may name different NTFS files.
    // Verify file identity and fail closed on absent/inaccessible programs instead of folding text.
    // SAFETY: all getters borrow a live rule; returned strings/variants have distinct ownership.
    unsafe {
        Ok(rule.Grouping().map_err(error)? == expected.group
            && same_file::is_same_file(
                rule.ApplicationName().map_err(error)?.to_string(),
                &expected.program,
            )
            .unwrap_or(false)
            && rule.Profiles().map_err(error)? == NET_FW_PROFILE2_PRIVATE.0
            && rule.Direction().map_err(error)? == NET_FW_RULE_DIR_IN
            && rule.Action().map_err(error)? == NET_FW_ACTION_ALLOW
            && rule.Enabled().map_err(error)?.as_bool()
            && !rule.EdgeTraversal().map_err(error)?.as_bool()
            && rule.Protocol().map_err(error)? == expected.protocol
            && rule.LocalPorts().map_err(error)? == expected.local_port
            && rule.RemotePorts().map_err(error)? == "*"
            && canonical_addresses(&rule.LocalAddresses().map_err(error)?.to_string()).ok()
                == Some(canonical_addresses(&expected.local_address)?)
            && canonical_addresses(&rule.RemoteAddresses().map_err(error)?.to_string()).ok()
                == Some(canonical_addresses(&expected.remote_address)?)
            && interfaces(rule.Interfaces().map_err(error)?)? == [expected.interface.clone()])
    }
}
// Firewall COM can serialize CIDRs using dotted netmasks and reorder multicast addresses.
// Compare the actual address sets, while wildcard/range scopes fail comparison with ours.
fn canonical_addresses(value: &str) -> io::Result<Vec<Ipv4Net>> {
    let mut result = Vec::new();
    for part in value.split(',').map(str::trim) {
        let network = if let Ok(network) = part.parse::<Ipv4Net>() {
            network
        } else if let Some((ip, mask)) = part.split_once('/') {
            Ipv4Net::with_netmask(ip.parse().map_err(error)?, mask.parse().map_err(error)?)
                .map_err(error)?
        } else {
            Ipv4Net::new(part.parse().map_err(error)?, 32).map_err(error)?
        };
        result.push(network.trunc());
    }
    result.sort();
    result.dedup();
    Ok(result)
}
fn build_rule(expected: &Rule) -> io::Result<INetFwRule> {
    // SAFETY: the new object is detached from the system policy until Add. Every restriction is
    // installed before enabling it, so changing protocol/ports never exposes a broad live rule.
    unsafe {
        let rule: INetFwRule =
            CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER).map_err(error)?;
        rule.SetName(&BSTR::from(expected.name.as_str()))
            .map_err(error)?;
        rule.SetGrouping(&BSTR::from(expected.group.as_str()))
            .map_err(error)?;
        rule.SetDescription(&BSTR::from(
            "FTA selected home connection; managed by the network setup helper",
        ))
        .map_err(error)?;
        rule.SetEnabled(VARIANT_FALSE).map_err(error)?;
        rule.SetApplicationName(&BSTR::from(expected.program.as_str()))
            .map_err(error)?;
        rule.SetProfiles(NET_FW_PROFILE2_PRIVATE.0).map_err(error)?;
        rule.SetDirection(NET_FW_RULE_DIR_IN).map_err(error)?;
        rule.SetAction(NET_FW_ACTION_ALLOW).map_err(error)?;
        rule.SetEdgeTraversal(VARIANT_FALSE).map_err(error)?;
        rule.SetProtocol(expected.protocol).map_err(error)?;
        rule.SetLocalPorts(&BSTR::from(expected.local_port.as_str()))
            .map_err(error)?;
        rule.SetRemotePorts(&BSTR::from("*")).map_err(error)?;
        rule.SetLocalAddresses(&BSTR::from(expected.local_address.as_str()))
            .map_err(error)?;
        rule.SetRemoteAddresses(&BSTR::from(expected.remote_address.as_str()))
            .map_err(error)?;
        rule.SetInterfaces(&interface_variant(&expected.interface)?.0)
            .map_err(|e| error(format!("firewall interface scope: {e}")))?;
        rule.SetEnabled(VARIANT_TRUE).map_err(error)?;
        Ok(rule)
    }
}
pub(super) fn put_rule(policy: &INetFwPolicy2, expected: &Rule) -> io::Result<()> {
    let previous = find_rule(policy, &expected.name)?;
    if let Some(previous) = &previous {
        // SAFETY: querying an existing rule does not change its policy.
        if unsafe { previous.Grouping() }.map_err(error)? != expected.group {
            return Err(error("a foreign firewall rule owns this name"));
        }
        if matches(previous, expected)? {
            return Ok(());
        }
    }
    let replacement = build_rule(expected)?;
    // SAFETY: only an app-owned stable rule name is replaced. Disable before remove/add; any
    // interrupted replacement fails closed and is reported as a partial result, never success.
    unsafe {
        let rules = policy.Rules().map_err(error)?;
        if let Some(previous) = previous {
            previous.SetEnabled(VARIANT_FALSE).map_err(error)?;
            rules
                .Remove(&BSTR::from(expected.name.as_str()))
                .map_err(error)?;
        }
        rules.Add(&replacement).map_err(error)
    }
}
pub(super) fn remove_rule(policy: &INetFwPolicy2, name: &str, group: &str) -> io::Result<()> {
    if let Some(rule) = find_rule(policy, name)? {
        // SAFETY: a rule is removed only after its app ownership marker has been checked.
        unsafe {
            if rule.Grouping().map_err(error)? != group {
                return Err(error("a foreign firewall rule owns this name"));
            }
            rule.SetEnabled(VARIANT_FALSE).map_err(error)?;
            policy
                .Rules()
                .map_err(error)?
                .Remove(&BSTR::from(name))
                .map_err(error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Setting detached COM properties verifies native ABI/SAFEARRAY ownership without touching
    // any live firewall rule, requesting elevation, or changing the machine's network category.
    #[test]
    fn detached_rule_preserves_all_scope_filters() -> io::Result<()> {
        let _apartment = Apartment::enter()?;
        let directory = tempfile::tempdir()?;
        let program = directory.path().join("fta-engine.exe");
        std::fs::write(&program, b"MZ detached rule fixture")?;
        let expected = Rule {
            name: "FTA detached test".into(),
            group: RULE_GROUP.into(),
            program: program.to_string_lossy().into_owned(),
            interface: connections()?
                .into_iter()
                .find(|c| c.connected)
                .ok_or_else(|| error("native test needs a connected interface"))?
                .name,
            local_address: "192.168.4.2".into(),
            remote_address: "192.168.4.0/24".into(),
            protocol: NET_FW_IP_PROTOCOL_TCP.0,
            local_port: "*".into(),
        };
        let rule = build_rule(&expected)?;
        assert!(matches(&rule, &expected)?);
        let mut changed = expected.clone();
        changed.remote_address = "192.168.5.0/24".into();
        assert!(!matches(&rule, &changed)?);
        changed = expected;
        changed.program = r"C:\other.exe".into();
        assert!(!matches(&rule, &changed)?);
        Ok(())
    }

    // A detached rule must recognize an alias by native file ID, reject a byte-for-byte copy,
    // and stop claiming permission when the rule's executable disappears from disk.
    #[test]
    fn detached_rule_program_uses_existing_file_identity() -> io::Result<()> {
        let _apartment = Apartment::enter()?;
        let directory = tempfile::tempdir()?;
        let program = directory.path().join("fta-engine.exe");
        let alias = directory.path().join("engine-alias.exe");
        let other = directory.path().join("other-engine.exe");
        std::fs::write(&program, b"MZ identity fixture")?;
        std::fs::hard_link(&program, &alias)?;
        std::fs::copy(&program, &other)?;
        let expected = Rule {
            name: "FTA detached identity test".into(),
            group: RULE_GROUP.into(),
            program: program.to_string_lossy().into_owned(),
            interface: connections()?
                .into_iter()
                .find(|c| c.connected)
                .ok_or_else(|| error("native test needs a connected interface"))?
                .name,
            local_address: "192.168.4.2".into(),
            remote_address: "192.168.4.0/24".into(),
            protocol: NET_FW_IP_PROTOCOL_TCP.0,
            local_port: "*".into(),
        };
        let rule = build_rule(&expected)?;
        assert!(matches(&rule, &expected)?);
        let mut changed = expected.clone();
        changed.program = alias.to_string_lossy().into_owned();
        assert!(matches(&rule, &changed)?);
        changed.program = other.to_string_lossy().into_owned();
        assert!(!matches(&rule, &changed)?);
        changed.program = directory
            .path()
            .join("missing.exe")
            .to_string_lossy()
            .into_owned();
        assert!(!matches(&rule, &changed)?);
        std::fs::remove_file(&program)?;
        assert!(!matches(&rule, &expected)?);
        Ok(())
    }

    // Real adapter discovery is read-only; this catches GUID marshalling mistakes that command
    // names or mocks would hide, without asserting a developer-specific address or adapter name.
    #[test]
    fn native_connection_snapshot_is_read_only_and_consistent() -> io::Result<()> {
        for connection in connections()? {
            uuid::Uuid::parse_str(&connection.id).map_err(error)?;
            uuid::Uuid::parse_str(&connection.network_id).map_err(error)?;
            for address in connection.addresses {
                let network: Ipv4Net = address.cidr.parse().map_err(error)?;
                let ip: std::net::Ipv4Addr = address.address.parse().map_err(error)?;
                assert!(network.contains(&ip));
                assert_eq!(network.prefix_len(), address.prefix_length);
                assert!(ip.is_private());
            }
        }
        Ok(())
    }

    // Read-only rule lookup must distinguish absence from permission or service failures.
    #[test]
    fn missing_rule_is_not_reported_as_configured() -> io::Result<()> {
        let _apartment = Apartment::enter()?;
        let policy = policy()?;
        assert!(
            find_rule(
                &policy,
                &format!("FTA absent test {}", uuid::Uuid::new_v4())
            )?
            .is_none()
        );
        Ok(())
    }

    // Equivalent CIDR spellings are accepted, while any broader or wildcard scope remains
    // different; treating these strings loosely could otherwise falsely certify a rule.
    #[test]
    fn address_comparison_preserves_exact_scope() -> io::Result<()> {
        assert_eq!(
            canonical_addresses("192.168.4.0/24")?,
            canonical_addresses("192.168.4.0/255.255.255.0")?
        );
        assert_ne!(
            canonical_addresses("192.168.4.0/24")?,
            canonical_addresses("192.168.0.0/16")?
        );
        assert_eq!(
            canonical_addresses("224.0.0.251,192.168.4.2")?,
            canonical_addresses("192.168.4.2/32,224.0.0.251")?
        );
        assert!(canonical_addresses("*").is_err());
        assert!(canonical_addresses("LocalSubnet").is_err());
        Ok(())
    }

    // The COM interface array holds literal names, including Unicode and wildcard characters;
    // unlike shell wildcard parameters, this round-trip cannot widen the selected interface.
    #[test]
    fn interface_array_preserves_literal_names() -> io::Result<()> {
        let _apartment = Apartment::enter()?;
        let value = interface_variant("한글 [literal] * interface")?;
        // SAFETY: interfaces takes ownership, so transfer the record and suppress the old owner.
        let value = ManuallyDrop::new(value);
        // SAFETY: the old owner is suppressed, transferring its record to the new sole owner.
        let raw = unsafe { std::ptr::read(&value.0) };
        assert_eq!(interfaces(raw)?, ["한글 [literal] * interface"]);
        Ok(())
    }
}
