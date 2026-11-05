import Foundation

// These are event bookkeeping checks, not invented peers/receipts in product UI.
@main struct ReceiptStateChecks {
    static func main() {
        let suite = "fta-receipt-check-" + UUID().uuidString
        let defaults = UserDefaults(suiteName:suite)!
        defer { defaults.removePersistentDomain(forName:suite) }
        let state = ReceiptState(defaults:defaults)
        precondition(state.count == 0 && state.needsDelivery(1))
        state.track("first",sequence:1,alreadyOpen:false)
        let beforeAcknowledgement = ReceiptState(defaults:defaults)
        precondition(beforeAcknowledgement.count == 1 && beforeAcknowledgement.needsDelivery(1))
        state.delivered(1)
        let restored = ReceiptState(defaults:defaults)
        precondition(restored.count == 1 && !restored.needsDelivery(1))
        restored.read("first")
        precondition(restored.count == 0 && !restored.needsDelivery(1))
        restored.track("second",sequence:2,alreadyOpen:false);restored.delivered(2)
        restored.track("second",sequence:3,alreadyOpen:false);restored.delivered(3)
        precondition(restored.count == 1 && !restored.needsDelivery(2))
        restored.track("visible",sequence:4,alreadyOpen:true);restored.delivered(4)
        precondition(restored.count == 1 && !restored.isUnread("visible"))
        precondition(ReceiptState(defaults:defaults).isUnread("second"))
        print("Receipt state checks passed: durable unread, submission retry, acknowledgement dedupe, read/restart, updated result, visible record")
    }
}
