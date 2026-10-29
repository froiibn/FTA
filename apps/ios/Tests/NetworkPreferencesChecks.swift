import Foundation

// Recreate preferences with the same OS store to cover process restart, explicit revocation and scope isolation.
@main struct NetworkPreferencesChecks {
    static func main() {
        let domain = "fta-network-checks." + UUID().uuidString
        let defaults = UserDefaults(suiteName:domain)!
        defer { defaults.removePersistentDomain(forName:domain) }
        let home = NetworkPreferences.scope(interface:"en0",network:"192.168.50.0/24")
        let other = NetworkPreferences.scope(interface:"en0",network:"192.168.51.0/24")
        let cable = NetworkPreferences.scope(interface:"en1",network:"192.168.50.0/24")
        let first = NetworkPreferences(defaults:defaults)
        precondition(!first.trusts(home) && first.receiveEnabled)
        first.setTrusted(true,scope:home); first.selectedScope = home; first.receiveEnabled = false
        let afterRestart = NetworkPreferences(defaults:UserDefaults(suiteName:domain)!)
        precondition(afterRestart.trusts(home) && afterRestart.selectedScope == home && !afterRestart.receiveEnabled)
        precondition(!afterRestart.trusts(other) && !afterRestart.trusts(cable))
        // Temporary disconnection is deliberately not a consent write, so reconnect resolves the same scope.
        let afterReconnect = NetworkPreferences(defaults:UserDefaults(suiteName:domain)!)
        precondition(afterReconnect.trusts(home))
        afterReconnect.setTrusted(true,scope:other); afterReconnect.setTrusted(false,scope:home)
        let afterRevocation = NetworkPreferences(defaults:UserDefaults(suiteName:domain)!)
        precondition(!afterRevocation.trusts(home) && afterRevocation.trusts(other) && !afterRevocation.receiveEnabled)
        afterRevocation.adoptLegacyNetworks(["192.168.50.0/24"],interface:"en0",network:"192.168.50.0/24")
        precondition(!afterRevocation.trusts(home), "Legacy settings must not undo an explicit revocation")
        defaults.removePersistentDomain(forName:domain)
        let legacy = NetworkPreferences(defaults:defaults)
        legacy.adoptLegacyNetworks(["10.0.0.0/8","172.16.0.0/12","192.168.0.0/16"],interface:"en0",network:"192.168.50.0/24")
        precondition(!legacy.trusts(home), "New installs must never inherit broad default networks")
        legacy.adoptLegacyNetworks(["192.168.50.0/24"],interface:"en0",network:"192.168.51.0/24")
        precondition(!legacy.trusts(other))
        legacy.adoptLegacyNetworks(["192.168.50.0/24"],interface:"en0",network:"192.168.50.0/24")
        precondition(legacy.trusts(home) && legacy.selectedScope == home)
        print("Network preferences checks passed: defaults, restart, selected scope, receive setting, reconnect, network/interface isolation, revocation")
    }
}
