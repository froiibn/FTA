import Foundation

// C strings have explicit ownership; neither Swift nor JSON ever carries file bytes.

struct EngineFailure: LocalizedError {
    let code: String
    let message: String
    var errorDescription: String? { "\(message) (\(code))" }
}
enum Engine {
    static func call(_ method: String, _ params: [String: Any] = [:]) async throws -> Any {
        // Concurrent native calls allow recipient approval/cancellation during a pending send.
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                do {
                    let data = try JSONSerialization.data(withJSONObject: ["method": method, "params": params])
                    let request = String(decoding: data, as: UTF8.self)
                    let response: Data = try request.withCString { input in
                        guard let output = fta_ios_call(input) else { throw EngineFailure(code: "INTERNAL", message: "엔진 응답이 없습니다") }
                        defer { fta_ios_free(output) }
                        return Data(bytes: output, count: strlen(output))
                    }
                    let envelope = try JSONSerialization.jsonObject(with: response) as? [String: Any] ?? [:]
                    if let error = envelope["error"] as? [String: String] {
                        throw EngineFailure(code: error["code"] ?? "INTERNAL", message: error["message"] ?? "작업 실패")
                    }
                    continuation.resume(returning: envelope["result"] ?? NSNull())
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
    static func decode<T: Decodable>(_ type: T.Type, _ method: String, _ params: [String: Any] = [:]) async throws -> T {
        let value = try await call(method, params)
        return try JSONDecoder().decode(type, from: JSONSerialization.data(withJSONObject: value))
    }
}

struct Peer: Decodable, Identifiable {
    let device_id: String
    let display_name: String
    let os: String
    let online: Bool
    let paired: Bool
    var id: String { device_id }
    var mobile: Bool { ["ios", "android"].contains(os.lowercased()) }
    var selectable: Bool { !paired || online }
    var status: String { paired ? (online ? "온라인" : "오프라인") : "발견됨 · 최초 등록 필요" }
}
struct TransferItem: Decodable, Identifiable {
    let name: String
    let size: String
    let state: String
    let bytes_transferred: String?
    let saved_name: String?
    let path: String?
    let error: String?
    var id: String { name + (saved_name ?? "") }
}
struct Transfer: Decodable, Identifiable {
    let transfer_id: String
    let peer_id: String
    let direction: String
    let kind: String
    let state: String
    let created_at: Double?
    let body: String?
    let items: [TransferItem]
    let error: String?
    var id: String { transfer_id }
    var title: String { kind == "files" ? (items.first?.name ?? "파일") + (items.count > 1 ? " 외 \(items.count - 1)개" : "") : (kind == "url" ? "URL" : "텍스트") }
    var terminal: Bool { ["completed", "partial", "failed", "cancelled", "unknown"].contains(state) }
    var status: String {
        ["completed":"완료", "partial":"부분 완료", "failed":"실패", "cancelled":"취소됨", "unknown":"결과 확인 필요", "sending":"전송 중", "receiving":"수신 중", "verifying":"무결성 확인 중", "committing":"저장 중", "awaiting_registration":"상대 기기의 등록 승인 대기", "cancel_requested":"취소 확인 중"][state] ?? state
    }
    var fraction: Double {
        let total = items.reduce(0.0) { $0 + (Double($1.size) ?? 0) }
        let sent = items.reduce(0.0) { $0 + (Double($1.bytes_transferred ?? "0") ?? 0) }
        return total > 0 ? min(1, sent / total) : 0
    }
}
struct Registration: Decodable, Identifiable {
    let session_id: String
    let display_name: String
    let os: String
    let kind: String
    let item_count: Int
    let expires_at: Double
    var id: String { session_id }
}
struct Receipt: Decodable { let sequence: UInt64; let transfer_id: String; let record: Transfer }
struct EventPage: Decodable { let events: [Receipt]; let next_cursor: UInt64 }
struct Cursor: Decodable { let sequence: UInt64 }

// Foundation derives sandbox locations so restored installations never retain an old absolute path.
enum Sandbox {
    static var state: URL { FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("FTA", isDirectory: true).resolvingSymlinksInPath() }
    static var received: URL { FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0].appendingPathComponent("받은 파일", isDirectory: true).resolvingSymlinksInPath() }
    static var outgoing: URL { state.appendingPathComponent("Outgoing", isDirectory: true) }
    static func prepare() throws {
        for url in [state, received, outgoing] {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true, attributes: [.protectionKey: FileProtectionType.complete])
        }
        // State/keys must not be cloned into another device identity by an automatic backup.
        var url = state
        var values = URLResourceValues(); values.isExcludedFromBackup = true
        try url.setResourceValues(values)
    }
    static func receivedURL(_ path: String?) -> URL? {
        guard let path else { return nil }
        let url = URL(fileURLWithPath: path).resolvingSymlinksInPath()
        let root = received.resolvingSymlinksInPath().path + "/"
        guard url.path.hasPrefix(root), FileManager.default.fileExists(atPath: url.path) else { return nil }
        return url
    }
}
