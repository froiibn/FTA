import Foundation

// Consent belongs to the approved interface/subnet, never a DHCP address or a foreground session.
// This retains the app's CIDR trust scope; it does not claim SSID/router identity verification.
final class NetworkPreferences {
    private let defaults: UserDefaults
    private enum Key {
        static let trusted = "trustedLANScopes.v1"
        static let selected = "selectedLANScope.v1"
        static let receiving = "receiveEnabled.v1"
    }
    init(defaults: UserDefaults = .standard) { self.defaults = defaults }
    static func scope(interface: String, network: String) -> String { "\(interface)|\(network)" }
    var selectedScope: String? {
        get { defaults.string(forKey:Key.selected) }
        set { defaults.set(newValue,forKey:Key.selected) }
    }
    var receiveEnabled: Bool {
        get { defaults.object(forKey:Key.receiving) == nil || defaults.bool(forKey:Key.receiving) }
        set { defaults.set(newValue,forKey:Key.receiving) }
    }
    // Legacy iOS host.start saved exactly the user-approved CIDR. Default broad RFC1918 lists
    // never qualify; an existing new-format record (including explicit revocation) always wins.
    func adoptLegacyNetworks(_ networks: [String], interface: String, network: String) {
        guard defaults.object(forKey:Key.trusted) == nil, networks == [network] else { return }
        let scope = Self.scope(interface:interface,network:network)
        setTrusted(true,scope:scope)
        selectedScope = scope
    }
    func trusts(_ scope: String) -> Bool { (defaults.stringArray(forKey:Key.trusted) ?? []).contains(scope) }
    func setTrusted(_ trusted: Bool, scope: String) {
        var scopes = Set(defaults.stringArray(forKey:Key.trusted) ?? [])
        if trusted { scopes.insert(scope) } else { scopes.remove(scope) }
        defaults.set(scopes.sorted(),forKey:Key.trusted)
    }
}
