import Foundation

// Persist unread locators independently of engine event acknowledgements; never store message bodies here.
final class ReceiptState {
    private struct State: Codable { var delivered: UInt64 = 0; var unread: [String:UInt64] = [:] }
    private let defaults: UserDefaults
    private let key = "receipt-state.v1"
    private var state: State
    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        state = defaults.data(forKey:key).flatMap { try? JSONDecoder().decode(State.self,from:$0) } ?? State()
    }
    var count: Int { state.unread.count }
    func isUnread(_ id: String) -> Bool { state.unread[id] != nil }
    func needsDelivery(_ sequence: UInt64) -> Bool { sequence > state.delivered }
    // Tracking precedes native alert delivery so an interrupted submission still leaves a visible unread record.
    func track(_ id: String, sequence: UInt64, alreadyOpen: Bool) {
        if !alreadyOpen { state.unread[id] = max(state.unread[id] ?? 0,sequence); save() }
    }
    func delivered(_ sequence: UInt64) { state.delivered = max(state.delivered,sequence); save() }
    func read(_ id: String) { state.unread.removeValue(forKey:id); save() }
    private func save() { if let data = try? JSONEncoder().encode(state) { defaults.set(data,forKey:key) } }
}
