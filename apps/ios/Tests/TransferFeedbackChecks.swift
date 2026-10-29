import Foundation

// Check receipt timing and cross-transfer races without inventing any online UI peers or successful sends.
@main struct TransferFeedbackChecks {
    static func main() {
        let start = Date(timeIntervalSince1970:100)
        var feedback = TransferFeedback()
        feedback.begin(id:"first",peer:"recipient-a")
        feedback.update(id:"first",fraction:1)
        precondition(feedback.isActive && feedback.completedUntil == nil,"Uploading every byte is not a stored receipt")
        feedback.finish(id:"first",completed:true,now:start)
        precondition(feedback.peerID=="recipient-a" && feedback.completedUntil==start.addingTimeInterval(1.5))
        feedback.expire(id:"first",now:start.addingTimeInterval(1.49))
        precondition(feedback.peerID != nil)
        feedback.expire(id:"first",now:start.addingTimeInterval(1.5))
        precondition(feedback.peerID == nil)
        feedback.begin(id:"old",peer:"recipient-a",now:start)
        feedback.finish(id:"old",completed:true,now:start)
        feedback.begin(id:"new",peer:"recipient-b",now:start.addingTimeInterval(0.5))
        precondition(feedback.completionDeadline(for:"recipient-a")==start.addingTimeInterval(1.5), "Sending to another recipient must not shorten the previous receipt badge")
        feedback.update(id:"old",fraction:0.9)
        feedback.expire(id:"old",now:start.addingTimeInterval(3))
        precondition(feedback.peerID=="recipient-b" && feedback.fraction == nil && feedback.isActive)
        feedback.finish(id:"old",completed:true,now:start)
        precondition(feedback.completedUntil == nil)
        feedback.finish(id:"new",completed:false,now:start)
        precondition(feedback.peerID == nil,"Partial, failed, unknown or cancelled outcomes must not display success")
        print("Transfer feedback checks passed: saved receipt, exact 1.5s expiry, new-recipient isolation, late polls, stale timers, non-success cleanup")
    }
}
