import Foundation

// A receipt, not byte upload completion, earns the transient success badge. IDs reject late polls/timers.
struct TransferFeedback {
    static let completionSeconds: TimeInterval = 1.5
    private(set) var transferID: String?
    private(set) var peerID: String?
    private(set) var fraction: Double?
    private(set) var completedUntil: Date?
    private struct Completion { let id: String; let until: Date }
    private var completions: [String:Completion] = [:]
    func completionDeadline(for peer: String) -> Date? { completions[peer]?.until }
    var isActive: Bool { transferID != nil && completedUntil == nil }
    mutating func begin(id: String, peer: String, now: Date = Date()) {
        // A new attempt supersedes this peer's old badge, while other peers keep their full 1.5 seconds.
        completions = completions.filter { $0.value.until > now }
        completions.removeValue(forKey:peer)
        transferID = id; peerID = peer; fraction = nil; completedUntil = nil
    }
    mutating func update(id: String, fraction: Double?) {
        guard transferID == id, isActive else { return }
        self.fraction = fraction.map { min(1,max(0,$0)) }
    }
    mutating func finish(id: String, completed: Bool, now: Date = Date()) {
        guard transferID == id else { return }
        if completed, let peerID {
            fraction = nil; completedUntil = now.addingTimeInterval(Self.completionSeconds)
            completions[peerID] = Completion(id:id,until:completedUntil!)
        } else { clearCurrent() }
    }
    mutating func expire(id: String, now: Date = Date()) {
        completions = completions.filter { !($0.value.id == id && now >= $0.value.until) }
        guard transferID == id, let end = completedUntil, now >= end else { return }
        clearCurrent()
    }
    private mutating func clearCurrent() {
        transferID = nil; peerID = nil; fraction = nil; completedUntil = nil
    }
}
