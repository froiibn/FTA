import Foundation

// A receipt, not byte upload completion, earns the transient success badge. IDs reject late polls/timers.
struct TransferFeedback {
    static let completionSeconds: TimeInterval = 1.5
    private static let blinkCycleSeconds: TimeInterval = 0.6
    private static let blinkCount = 2
    private static let dimmedIconOpacity = 0.25
    // Keep the receipt badge steady while the device alone dims twice within its existing 1.5s window.
    static func completionIconOpacity(deadline: Date?, now: Date, reduceMotion: Bool = false) -> Double {
        guard !reduceMotion, let deadline else { return 1 }
        let elapsed = now.timeIntervalSince(deadline.addingTimeInterval(-completionSeconds))
        guard elapsed >= 0, elapsed < blinkCycleSeconds * Double(blinkCount) else { return 1 }
        let phase = elapsed.truncatingRemainder(dividingBy:blinkCycleSeconds) / blinkCycleSeconds
        let dimming = 1 - abs(2 * phase - 1)
        return 1 - (1 - dimmedIconOpacity) * dimming
    }
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
