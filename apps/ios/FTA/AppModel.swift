import SwiftUI
import UserNotifications
import Network

// Native notification delivery uses the engine's durable consumer cursor, independent of visible tabs.
final class NotificationRouter: NSObject, UNUserNotificationCenterDelegate {
    var open: ((String) -> Void)?
    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound])
    }
    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        if let id = response.notification.request.content.userInfo["transfer_id"] as? String {
            DispatchQueue.main.async { self.open?(id) }
        }
        completionHandler()
    }
}

@MainActor final class AppModel: ObservableObject {
    @Published var tab = 0
    @Published var peers: [Peer] = []
    @Published var history: [Transfer] = []
    @Published var requests: [Registration] = []
    @Published var selectedPeer: String?
    @Published var files: [URL] = []
    @Published var photos: [URL] = []
    @Published var message = ""
    @Published var progress: Transfer?
    @Published var sending = false
    @Published var importing = false
    @Published var running = false
    @Published var starting = false
    @Published var foreground = true
    @Published var confirmed = false
    @Published var receiveEnabled = true
    @Published var lans: [LAN] = LAN.available()
    @Published var selectedLAN = ""
    @Published var name = UserDefaults.standard.string(forKey: "deviceName") ?? UIDevice.current.name
    @Published var notice: String?
    @Published var error: String?
    @Published var detail: Transfer?
    @Published var activeID: String?
    @Published var deciding = false
    @Published var notificationStatus = "확인 중"
    private let bonjour = Bonjour()
    private let notifications = NotificationRouter()
    private let monitor = NWPathMonitor()
    private var lifecycle: Task<Void, Never>?
    private var polling: Task<Void, Never>?
    private var devicesPolling: Task<Void, Never>?
    private var activeLAN: LAN?
    private var deviceEpoch = 0
    private let consumer = "ios-notifications-v1"

    init() {
        selectedLAN = lans.first?.id ?? ""
        UNUserNotificationCenter.current().delegate = notifications
        notifications.open = { [weak self] id in Task { @MainActor in self?.tab = 1; await self?.openRecord(id) } }
        bonjour.failure = { [weak self] text in
            self?.error = text; self?.confirmed = false; self?.updateLifecycle()
        }
        monitor.pathUpdateHandler = { [weak self] path in
            let usableLAN = path.status == .satisfied && (path.usesInterfaceType(.wifi) || path.usesInterfaceType(.wiredEthernet))
            Task { @MainActor in
                guard let self else { return }
                self.lans = usableLAN ? LAN.available() : []
                if let active = self.activeLAN, !usableLAN || !self.lans.contains(active) {
                    // Invalidate before awaiting Rust shutdown, so a stale probe cannot restore Wi-Fi targets.
                    self.deviceEpoch += 1
                    self.selectedPeer = nil
                    self.confirmed = false
                    self.notice = "네트워크가 변경되어 송수신을 중지했습니다. 사용할 집 네트워크를 다시 확인하세요."
                    self.updateLifecycle()
                }
            }
        }
        monitor.start(queue: DispatchQueue(label: "fta.ios.network"))
        // History must be available before LAN consent and while receiving is paused.
        lifecycle = Task {
            do {
                try Sandbox.prepare()
                try? FileManager.default.removeItem(at:Sandbox.outgoing)
                try Sandbox.prepare()
                _ = try await Engine.call("host.prepare",["state_dir":Sandbox.state.path,"receive_dir":Sandbox.received.path])
                await refreshLocalHistory()
            } catch { self.error = error.localizedDescription }
            await refreshNotificationStatus()
        }
    }
    // Preserve offline trust records for settings, but never offer them as send targets.
    var targets: [Peer] { running && confirmed ? peers.filter { !$0.paired || $0.online } : [] }
    var selected: Peer? { targets.first { $0.id == selectedPeer } }
    var canSend: Bool { running && !sending && !importing && selected?.selectable == true }
    var lan: LAN? { lans.first { $0.id == selectedLAN } }
    var serviceText: String { starting ? "수신 준비 중" : (running ? "이 화면이 열려 있는 동안 수신 가능" : "송수신 중지됨") }

    func updateLifecycle() {
        let previous = lifecycle
        lifecycle = Task { [weak self] in
            await previous?.value
            guard let self else { return }
            if self.foreground && self.confirmed && self.receiveEnabled { await self.startIfNeeded() }
            else { await self.stop() }
        }
    }
    private func startIfNeeded() async {
        guard !running, let lan else { return }
        starting = true; defer { starting = false }
        do {
            try Sandbox.prepare()
            let data = try await Engine.call("host.start", ["state_dir":Sandbox.state.path,"receive_dir":Sandbox.received.path,"display_name":name,"listen_addr":"\(lan.address):0","allowed_networks":[lan.network]]) as? [String:Any] ?? [:]
            activeLAN = lan; running = true
            bonjour.start(); bonjour.advertise(data, name: name)
            polling = Task { [weak self] in
                while !Task.isCancelled, let self, self.running {
                    await self.refreshActivity()
                    try? await Task.sleep(for: .milliseconds(750))
                }
            }
            devicesPolling = Task { [weak self] in
                while !Task.isCancelled, let self, self.running {
                    await self.refreshDevices()
                    try? await Task.sleep(for: .seconds(3))
                }
            }
        } catch { self.error = error.localizedDescription; running = false }
    }
    private func stop() async {
        deviceEpoch += 1; selectedPeer = nil
        polling?.cancel(); devicesPolling?.cancel(); bonjour.stop()
        running = false
        peers = peers.filter(\.paired).map { Peer(device_id:$0.id,display_name:$0.display_name,os:$0.os,online:false,paired:true) }
        requests = []; activeLAN = nil
        do { _ = try await Engine.call("host.stop") } catch { self.error = error.localizedDescription }
        // A forced suspension can lose commit replies; sender results remain in durable history.
        if sending { notice = "앱이 백그라운드로 이동해 전송이 중단될 수 있습니다. 복귀 후 기록에서 결과를 확인하세요." }
    }
    func refreshDevices() async {
        guard running, confirmed else { return }
        let epoch = deviceEpoch
        do {
            let result = try await Engine.decode([Peer].self, "devices.list")
            guard running, confirmed, epoch == deviceEpoch else { return }
            peers = result.sorted { ($0.online ? 0 : ($0.paired ? 2 : 1), $0.display_name) < ($1.online ? 0 : ($1.paired ? 2 : 1), $1.display_name) }
            if !targets.contains(where: { $0.id == selectedPeer }) { selectedPeer = nil }
        } catch {
            if epoch == deviceEpoch { peers = peers.filter(\.paired).map { Peer(device_id:$0.id,display_name:$0.display_name,os:$0.os,online:false,paired:true) }; selectedPeer = nil }
            if running { self.error = error.localizedDescription }
        }
    }
    func refreshLocalHistory() async {
        do {
            history = try await Engine.decode([Transfer].self,"inbox.list",["limit":100])
            if !running { peers = try await Engine.decode([Peer].self,"devices.list") }
        } catch { self.error = error.localizedDescription }
    }
    func refreshActivity() async {
        if let activeLAN, !LAN.available().contains(activeLAN) {
            confirmed = false; notice = "집 네트워크가 변경되어 송수신을 중지했습니다."; updateLifecycle(); return
        }
        guard running else { await refreshLocalHistory(); return }
        do {
            let incoming = try await Engine.decode([Registration].self, "registration.requests")
            let records = try await Engine.decode([Transfer].self, "inbox.list", ["limit":100])
            let advert = try await Engine.call("host.advertisement") as? [String:Any] ?? [:]
            guard running else { return }
            requests = incoming; history = records; bonjour.advertise(advert, name:name)
            if let id = activeID {
                do { progress = try await Engine.decode(Transfer.self,"transfers.get",["transfer_id":id]) }
                catch let failure as EngineFailure where failure.code == "NOT_FOUND" { /* send is opening its first request */ }
            }
            try await consumeEvents()
        } catch { if running { self.error = error.localizedDescription } }
    }
    private func consumeEvents() async throws {
        let cursor = try await Engine.decode(Cursor.self,"events.cursor",["consumer":consumer])
        let page = try await Engine.decode(EventPage.self,"events.list",["after":cursor.sequence,"limit":100])
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        for event in page.events {
            if event.record.direction == "receive", [.authorized,.provisional,.ephemeral].contains(settings.authorizationStatus) {
                let content = UNMutableNotificationContent()
                content.title = event.record.state == "completed" ? "FTA 수신 완료" : "FTA 수신 결과"
                content.body = "\(event.record.title) · \(event.record.status)"
                content.sound = .default
                content.userInfo = ["transfer_id":event.transfer_id]
                // Stable request IDs limit duplicates after a crash between submission and acknowledgement.
                try await UNUserNotificationCenter.current().add(UNNotificationRequest(identifier:"fta.receipt.\(event.sequence)",content:content,trigger:nil))
            }
            // Denied notifications are deliberately suppressed; the receipt remains in history.
            _ = try await Engine.call("events.ack",["consumer":consumer,"sequence":event.sequence])
        }
    }
    func decide(_ request: Registration, accept: Bool) async {
        guard !deciding else { return }; deciding = true; defer { deciding = false }
        do { _ = try await Engine.call(accept ? "registration.accept" : "registration.reject",["session_id":request.id]); await refreshActivity() }
        catch { self.error = error.localizedDescription }
    }
    func sendFiles() async { await send(kind:"files", extras:["paths":files.map(\.path)]) }
    func sendPhotos() async { await send(kind:"files", extras:["paths":photos.map(\.path)], photoTransfer:true) }
    // The composer has no mode switch: classify only to preserve link actions on the receiver.
    func sendMessage() async { await send(kind:MessageContent.kind(for:message),extras:["body":message]) }
    private func send(kind: String, extras: [String:Any], photoTransfer: Bool = false) async {
        guard canSend, let peer = selected else { return }
        guard kind == "files" ? !(photoTransfer ? photos : files).isEmpty : !message.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty else { return }
        sending = true; let id = UUID().uuidString.lowercased(); activeID = id; progress = nil
        defer { sending = false; activeID = nil }
        var params = extras; params["kind"] = kind; params["to"] = peer.id; params["transfer_id"] = id
        do {
            let result = try await Engine.decode(Transfer.self,"transfers.send_discovered",params)
            progress = result; notice = result.status
            if result.state == "completed" {
                if kind == "files" { if photoTransfer { clearPhotos() } else { clearFiles() } } else { message = "" }
            }
            await refreshActivity()
        } catch { self.error = error.localizedDescription; await refreshActivity() }
    }
    func cancel() async {
        guard let id = activeID else { return }
        do { _ = try await Engine.call("transfers.cancel",["transfer_id":id]) }
        catch { self.error = error.localizedDescription }
    }
    func importFiles(_ urls: [URL]) async {
        guard !sending, !importing else { return }
        importing = true; defer { importing = false }
        do {
            // Coordinate reads under the provider's security scope; copy on a worker before releasing grants.
            let copied = try await Task.detached(priority:.userInitiated) {
                try Sandbox.prepare()
                let directory = Sandbox.outgoing.appendingPathComponent(UUID().uuidString, isDirectory:true)
                try FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true)
                var copied: [URL] = []
                do {
                    guard !urls.isEmpty, urls.count <= 100 else { throw EngineFailure(code:"TOO_LARGE",message:"파일은 한 번에 100개까지 선택하세요") }
                    for url in urls {
                        let scoped = url.startAccessingSecurityScopedResource()
                        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
                        var coordinatorError: NSError?
                        var copyError: Error?
                        let target = directory.appendingPathComponent(UUID().uuidString, isDirectory:true)
                        try FileManager.default.createDirectory(at:target,withIntermediateDirectories:true)
                        let destination = target.appendingPathComponent(url.lastPathComponent)
                        NSFileCoordinator().coordinate(readingItemAt:url,options:[],error:&coordinatorError) { source in
                            do {
                                let values = try source.resourceValues(forKeys:[.isRegularFileKey,.isSymbolicLinkKey])
                                guard values.isRegularFile == true, values.isSymbolicLink != true else { throw EngineFailure(code:"INVALID_PATH",message:"일반 파일만 보낼 수 있습니다") }
                                try FileManager.default.copyItem(at:source,to:destination)
                            } catch { copyError = error }
                        }
                        if let error = coordinatorError { throw error }; if let error = copyError { throw error }
                        copied.append(destination)
                    }
                    return copied
                } catch { try? FileManager.default.removeItem(at:directory); throw error }
            }.value
            clearFiles(); files = copied
        } catch { self.error = error.localizedDescription }
    }
    // Gallery and Files have independent drafts; successful transmission clears only its own selection.
    func importPhotos(_ providers: [NSItemProvider]) async {
        guard !sending, !importing, !providers.isEmpty else { return }
        importing = true; defer { importing = false }
        var directory: URL?
        do {
            guard providers.count <= PhotoStaging.maximumSelection else { throw EngineFailure(code:"TOO_LARGE",message:"사진은 한 번에 100개까지 선택하세요") }
            let session = try PhotoStaging.session(); directory = session
            var selected: [URL] = []
            for provider in providers { selected.append(try await PhotoStaging.copy(provider,into:session)) }
            clearPhotos(); photos = selected
        } catch {
            if let directory { try? FileManager.default.removeItem(at:directory) }
            self.error = error.localizedDescription
        }
    }
    func importCapturedPhoto(_ image: UIImage) async {
        guard !sending, !importing else { return }
        importing = true; defer { importing = false }
        do {
            // JPEG encoding and disk I/O must not freeze the native camera dismissal.
            let photo = try await Task.detached(priority:.userInitiated) {
                let directory = try PhotoStaging.session()
                do { return try PhotoStaging.capture(image,into:directory) }
                catch { try? FileManager.default.removeItem(at:directory); throw error }
            }.value
            clearPhotos(); photos = [photo]
        } catch { self.error = error.localizedDescription }
    }
    func clearFiles() { removeSelection(files); files = [] }
    func clearPhotos() { removeSelection(photos); photos = [] }
    private func removeSelection(_ urls: [URL]) {
        // Remove only owned selection sessions; never delete a source in Photos or an external provider.
        for directory in Set(urls.map { $0.deletingLastPathComponent().deletingLastPathComponent() }) {
            if directory.deletingLastPathComponent().standardizedFileURL.path == Sandbox.outgoing.standardizedFileURL.path {
                try? FileManager.default.removeItem(at:directory)
            }
        }
    }
    func openRecord(_ id: String) async {
        await lifecycle?.value
        do { detail = try await Engine.decode(Transfer.self,"inbox.get",["transfer_id":id]) }
        catch { self.error = error.localizedDescription }
    }
    func reconcile(_ record: Transfer) async {
        do { detail = try await Engine.decode(Transfer.self,"transfers.get",["transfer_id":record.id]); await refreshActivity() }
        catch { self.error = error.localizedDescription }
    }
    func deleteRecord(_ record: Transfer) async {
        do { _ = try await Engine.call("inbox.delete",["transfer_id":record.id]); detail = nil; await refreshActivity() }
        catch { self.error = error.localizedDescription }
    }
    func unpair(_ peer: Peer) async {
        do { _ = try await Engine.call("devices.unpair",["device_id":peer.id]); await refreshDevices() }
        catch { self.error = error.localizedDescription }
    }
    func saveName() async {
        do {
            guard !name.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty, name.utf8.count <= 128 else { throw EngineFailure(code:"INVALID_REQUEST",message:"이름을 1~128바이트로 입력하세요") }
            if running { _ = try await Engine.call("settings.update",["values":["display_name":name]]) }
            UserDefaults.standard.set(name,forKey:"deviceName")
            // Republish with new TXT metadata instead of leaving a stale discovery name.
            if running { bonjour.start(); await refreshActivity() }
            notice = "기기 이름을 저장했습니다"
        } catch { self.error = error.localizedDescription }
    }
    func requestNotifications() async {
        do { _ = try await UNUserNotificationCenter.current().requestAuthorization(options:[.alert,.sound,.badge]); await refreshNotificationStatus() }
        catch { self.error = error.localizedDescription }
    }
    func refreshNotificationStatus() async {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        notificationStatus = [.authorized,.provisional,.ephemeral].contains(settings.authorizationStatus) ? "허용됨" : (settings.authorizationStatus == .notDetermined ? "설정 전" : "차단됨")
    }
}
