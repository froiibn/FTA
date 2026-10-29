import Foundation

// Compile the shipped classification rule directly: prose, URL schemes and exact draft preservation matter.
@main struct ContentRulesChecks {
    static func main() {
        let cases: [(String,String)] = [
            ("안녕하세요\n두 번째 줄", "text"),
            ("https://example.com/path?x=1#part", "url"),
            ("HTTP://example.com", "url"),
            ("https://example.com 내용", "text"),
            ("https://example.com\n", "text"),
            (" https://example.com", "text"),
            ("ftp://example.com/file", "text"),
            ("javascript:alert(1)", "text"),
            ("https://", "text"),
            ("example.com", "text"),
            ("", "text")
        ]
        for (body,expected) in cases { precondition(MessageContent.kind(for:body) == expected, "Unexpected kind for \(body)") }
        print("Message classification: \(cases.count) cases passed")
    }
}
