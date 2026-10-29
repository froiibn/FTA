import Foundation

// One composer preserves the exact draft; only a whole HTTP(S) URL gets link behavior on receipt.
// Whitespace/multiline prose and other schemes remain ordinary text, matching desktop semantics.
enum MessageContent {
    static func kind(for body: String) -> String {
        guard !body.contains(where: { $0.isWhitespace }),
              let url = URL(string: body),
              let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme),
              let host = url.host, !host.isEmpty else { return "text" }
        return "url"
    }
}
