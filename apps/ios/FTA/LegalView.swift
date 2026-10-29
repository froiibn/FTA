// Localize visible copy while preserving identifiers, user data, and existing storage folder names.
import SwiftUI
import UIKit

// Keep legal documents inside the app so LAN-only use and a failed web request never hide the policy.
enum AppLegal {
    static let supportURL = URL(string:"https://github.com/froiibn/FTA/issues")
    // English is the legal-document fallback; the full interface has its own native localized tables.
    static var koreanPolicy: Bool { Bundle.main.preferredLocalizations.first?.hasPrefix("ko") == true }
    static var privacyResource: String { koreanPolicy ? "PrivacyPolicy" : "PrivacyPolicy-en" }
    static var privacyURL: URL? { URL(string:"https://github.com/froiibn/FTA/blob/main/docs/" + (koreanPolicy ? "privacy-policy.md" : "privacy-policy.en.md")) }
}

struct LegalDocumentView: View {
    let title: String
    let text: String
    let publicURL: URL?
    init(title: String, resource: String, publicURL: URL? = nil) {
        self.title = title; self.publicURL = publicURL
        if let url = Bundle.main.url(forResource:resource,withExtension:"txt"),
           let text = try? String(contentsOf:url,encoding:.utf8) { self.text = text }
        else { self.text = String(localized:"문서를 불러오지 못했습니다. 지원 페이지에서 문의해 주세요.") }
    }
    init(title: String, text: String) { self.title = title; self.text = text; publicURL = nil }
    var body: some View {
        NativeLegalText(text:text)
            .navigationTitle(title).navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier("legal-document")
            // The reviewed offline policy remains available even if its public GitHub page is unreachable.
            .toolbar { if let publicURL { Link(String(localized:"웹에서 보기"),destination:publicURL) } }
    }
}

// Read each dependency separately: the full inventory creates thousands of VoiceOver paragraphs.
private struct LicenseInventory: Decodable {
    struct Reference: Decodable { let file: String; let digest: String }
    struct Package: Decodable, Identifiable {
        let name: String; let version: String; let license: String; let source: String
        let notices: [Reference]
        var id: String { "\(name)-\(version)" }
    }
    let packages: [Package]
    let texts: [String:String]
    func document(for package: Package) -> String {
        "\(package.name) \(package.version)\n\(package.license)\n\(package.source)\n\n" + package.notices.map {
            String(localized:"\($0.file)\n\n\(texts[$0.digest] ?? String(localized:"문서를 불러오지 못했습니다."))")
        }.joined(separator:"\n\n")
    }
}

struct ThirdPartyNoticesView: View {
    @State private var inventory: LicenseInventory?
    @State private var query = ""
    @State private var failed = false
    var body: some View {
        Group {
            if let inventory {
                List {
                    Section {
                        ForEach(inventory.packages.filter { query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) }) { package in
                            NavigationLink {
                                LegalDocumentView(title:package.name,text:inventory.document(for:package))
                            } label: {
                                VStack(alignment:.leading) {
                                    Text("\(package.name) \(package.version)")
                                    Text(package.license).font(.caption).foregroundStyle(.secondary)
                                }
                            }.accessibilityIdentifier("license-\(package.id)")
                        }
                    } footer:{ Text(String(localized:"iOS 빌드의 기존 라이브러리와 빌드 도구 고지입니다. 원문 전체는 앱에 함께 포함됩니다.")) }
                }.searchable(text:$query,placement:.navigationBarDrawer(displayMode:.always),prompt:String(localized:"라이브러리 검색"))
            } else if failed { Text(String(localized:"문서를 불러오지 못했습니다. 지원 페이지에서 문의해 주세요.")) }
            else {
                ProgressView().task {
                    do {
                        guard let url = Bundle.main.url(forResource:"ThirdPartyNotices",withExtension:"json") else { failed = true; return }
                        inventory = try JSONDecoder().decode(LicenseInventory.self,from:Data(contentsOf:url))
                    } catch { failed = true }
                }
            }
        }.navigationTitle(String(localized:"오픈소스 고지")).navigationBarTitleDisplayMode(.inline)
    }
}

// Native selectable text preserves Dynamic Type and paragraph navigation for individual documents.
private struct NativeLegalText: UIViewRepresentable {
    let text: String
    func makeUIView(context: Context) -> UITextView {
        let view = UITextView()
        view.isEditable = false; view.isSelectable = true
        view.adjustsFontForContentSizeCategory = true
        view.font = .preferredFont(forTextStyle:.body)
        view.backgroundColor = .systemBackground; view.textColor = .label
        view.textContainerInset = UIEdgeInsets(top:16,left:16,bottom:16,right:16)
        view.text = text
        view.accessibilityIdentifier = "legal-document-text"
        return view
    }
    func updateUIView(_ uiView: UITextView, context: Context) {
        // NavigationStack can reuse the representable for another license; never retain its old text.
        if uiView.text != text { uiView.text = text; uiView.setContentOffset(.zero,animated:false) }
    }
}
