// Localize visible copy while preserving identifiers, user data, and existing storage folder names.
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
    @Published var transferFeedback = TransferFeedback()
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
    @Published var notificationStatus = String(localized:"확인 중")
    private let preferences = NetworkPreferences()
    private var serviceBlocked = false
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
        receiveEnabled = preferences.receiveEnabled
        restoreConnections(lans)
        UNUserNotificationCenter.current().delegate = notifications
        notifications.open = { [weak self] id in Task { @MainActor in self?.tab = 1; await self?.openRecord(id) } }
        bonjour.failure = { [weak self] text in
            self?.error = text; self?.serviceBlocked = true; self?.updateLifecycle()
        }
        monitor.pathUpdateHandler = { [weak self] path in
            let usableLAN = path.status == .satisfied && (path.usesInterfaceType(.wifi) || path.usesInterfaceType(.wiredEthernet))
            Task { @MainActor in
                guard let self else { return }
                self.restoreConnections(usableLAN ? LAN.available() : [])
                self.updateLifecycle()
            }
        }
        monitor.start(queue: DispatchQueue(label: "fta.ios.network"))
        // History must be available before LAN consent and while receiving is paused.
        lifecycle = Task {
            do {
                try Sandbox.prepare()
                try? FileManager.default.removeItem(at:Sandbox.outgoing)
                try Sandbox.prepare()
                _ = try await Engine.call("host.prepare",["container_dir":Sandbox.container.path,"state_dir":Sandbox.state.path,"receive_dir":Sandbox.received.path])
                await refreshLocalHistory()
            } catch { self.error = error.localizedDescription }
            await refreshNotificationStatus()
        }
    }
    // Preserve offline trust records for settings, but never offer them as send targets.
    var targets: [Peer] { foreground && running && confirmed && receiveEnabled && !serviceBlocked && activeLAN == lan ? peers.filter { !$0.paired || $0.online } : [] }
    var selected: Peer? { targets.first { $0.id == selectedPeer } }
    var canSend: Bool { running && !sending && !importing && selected?.selectable == true }
    var lan: LAN? { lans.first { $0.id == selectedLAN } }
    var serviceText: String { starting ? String(localized:"수신 준비 중") : (running ? String(localized:"이 화면이 열려 있는 동안 수신 가능") : String(localized:"송수신 중지됨")) }

    // Restore durable consent without revoking it on path changes; migrate only a recorded legacy approval.
    private func restoreConnections(_ available: [LAN]) {
        // The old engine config is evidence of a prior explicit start, not inferred LAN-wide consent.
        if let data = try? Data(contentsOf:Sandbox.state.appendingPathComponent("config.json")),
           let config = try? JSONSerialization.jsonObject(with:data) as? [String:Any],
           let networks = config["allowed_networks"] as? [String] {
            for connection in available {
                preferences.adoptLegacyNetworks(networks,interface:connection.name,network:connection.network)
            }
        }
        let old = lan
        let chosen = available.first { $0.id == selectedLAN }
            ?? available.first { $0.trustScope == preferences.selectedScope }
            ?? available.first { preferences.trusts($0.trustScope) }
            ?? available.first
        lans = available
        selectedLAN = chosen?.id ?? ""
        confirmed = chosen.map { preferences.trusts($0.trustScope) } ?? false
        if old != chosen {
            deviceEpoch += 1; selectedPeer = nil; serviceBlocked = false
        }
    }
    func selectLAN(_ id: String) {
        selectedLAN = id
        if let lan { preferences.selectedScope = lan.trustScope }
        confirmed = lan.map { preferences.trusts($0.trustScope) } ?? false
        deviceEpoch += 1; selectedPeer = nil; serviceBlocked = false
        updateLifecycle()
    }
    func setNetworkTrusted(_ trusted: Bool) {
        guard let lan else { return }
        preferences.setTrusted(trusted,scope:lan.trustScope)
        preferences.selectedScope = lan.trustScope
        confirmed = trusted; serviceBlocked = false; updateLifecycle()
    }
    func setReceiveEnabled(_ enabled: Bool) {
        receiveEnabled = enabled; preferences.receiveEnabled = enabled
        serviceBlocked = false; updateLifecycle()
    }
    // NWPath gates stale interface addresses so cellular/VPN paths never restore LAN trust.
    private var usableConnections: [LAN] {
        let path = monitor.currentPath
        return path.status == .satisfied && (path.usesInterfaceType(.wifi) || path.usesInterfaceType(.wiredEthernet)) ? LAN.available() : []
    }
    func setForeground(_ active: Bool) {
        foreground = active
        // Suspension pauses the listener, while returning restores previously approved LANs.
        if active { serviceBlocked = false; restoreConnections(usableConnections) }
        updateLifecycle()
    }
    func updateLifecycle() {
        let previous = lifecycle
        lifecycle = Task { [weak self] in
            await previous?.value
            guard let self else { return }
            if self.running && self.activeLAN != self.lan { await self.stop() }
            if self.foreground && self.confirmed && self.receiveEnabled && !self.serviceBlocked { await self.startIfNeeded() }
            else { await self.stop() }
        }
    }
    private func startIfNeeded() async {
        guard !running, let lan, usableConnections.contains(lan) else { return }
        starting = true; defer { starting = false }
        do {
            try Sandbox.prepare()
            let data = try await Engine.call("host.start", ["container_dir":Sandbox.container.path,"state_dir":Sandbox.state.path,"receive_dir":Sandbox.received.path,"display_name":name,"listen_addr":"\(lan.address):0","allowed_networks":[lan.network]]) as? [String:Any] ?? [:]
            // A path/scene change can arrive while Rust binds; never advertise a stale listener.
            guard foreground, confirmed, receiveEnabled, !serviceBlocked, self.lan == lan, usableConnections.contains(lan) else {
                _ = try await Engine.call("host.stop"); return
            }
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
        if sending { notice = String(localized:"앱이 백그라운드로 이동해 전송이 중단될 수 있습니다. 복귀 후 기록에서 결과를 확인하세요.") }
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
        if let activeLAN, !usableConnections.contains(activeLAN) {
            restoreConnections(usableConnections); updateLifecycle(); return
        }
        guard running else { await refreshLocalHistory(); return }
        do {
            let incoming = try await Engine.decode([Registration].self, "registration.requests")
            let records = try await Engine.decode([Transfer].self, "inbox.list", ["limit":100])
            let advert = try await Engine.call("host.advertisement") as? [String:Any] ?? [:]
            guard running else { return }
            requests = incoming; history = records; bonjour.advertise(advert, name:name)
            if let id = activeID {
                do {
                    let result = try await Engine.decode(Transfer.self,"transfers.get",["transfer_id":id])
                    guard activeID == id else { return }
                    transferFeedback.update(id:id,fraction:result.kind == "files" && result.items.contains(where:{ (Double($0.size) ?? 0) > 0 }) ? result.fraction : nil)
                }
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
                content.title = event.record.state == "completed" ? String(localized:"FTA 수신 완료") : String(localized:"FTA 수신 결과")
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
    // One attachment composer sends all selected sources through the same streaming engine.
    func sendAttachments() async { await send(kind:"files", extras:["paths":(photos + files).map(\.path)]) }
    // The composer has no mode switch: classify only to preserve link actions on the receiver.
    func sendMessage() async { await send(kind:MessageContent.kind(for:message),extras:["body":message]) }
    private func send(kind: String, extras: [String:Any]) async {
        guard canSend, let peer = selected else { return }
        guard kind == "files" ? !(photos + files).isEmpty : !message.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty else { return }
        sending = true; let id = UUID().uuidString.lowercased(); activeID = id
        transferFeedback.begin(id:id,peer:peer.id)
        // Terminal results stay in History, never as an indefinitely retained status card on Send.
        defer {
            sending = false; activeID = nil
            if transferFeedback.isActive { transferFeedback.finish(id:id,completed:false) }
        }
        var params = extras; params["kind"] = kind; params["to"] = peer.id; params["transfer_id"] = id
        do {
            let result = try await Engine.decode(Transfer.self,"transfers.send_discovered",params)
            transferFeedback.finish(id:id,completed:result.state == "completed")
            if result.state == "completed" {
                if let deadline = transferFeedback.completedUntil {
                    // Each saved receipt keeps its own deadline even if another recipient is sent to immediately.
                    Task { [weak self] in
                        do { try await Task.sleep(for:.seconds(max(0,deadline.timeIntervalSinceNow))) }
                        catch { return }
                        self?.transferFeedback.expire(id:id)
                    }
                }
                if kind == "files" { clearPhotos(); clearFiles() } else { message = "" }
            } else { error = result.error ?? result.status }
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
            let photoCount = photos.count
            let copied = try await Task.detached(priority:.userInitiated) {
                try Sandbox.prepare()
                let directory = Sandbox.outgoing.appendingPathComponent(UUID().uuidString, isDirectory:true)
                try FileManager.default.createDirectory(at:directory,withIntermediateDirectories:true)
                var copied: [URL] = []
                do {
                    guard !urls.isEmpty, urls.count + photoCount <= PhotoStaging.maximumSelection else { throw EngineFailure(code:"TOO_LARGE",message:String(localized:"사진과 파일은 합쳐서 100개까지 선택하세요")) }
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
                                guard values.isRegularFile == true, values.isSymbolicLink != true else { throw EngineFailure(code:"INVALID_PATH",message:String(localized:"일반 파일만 보낼 수 있습니다")) }
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
    // Picker cancellation keeps both drafts; only a completed combined transfer clears them.
    func importPhotos(_ providers: [NSItemProvider]) async {
        guard !sending, !importing, !providers.isEmpty else { return }
        importing = true; defer { importing = false }
        var directory: URL?
        do {
            guard providers.count + files.count <= PhotoStaging.maximumSelection else { throw EngineFailure(code:"TOO_LARGE",message:String(localized:"사진과 파일은 합쳐서 100개까지 선택하세요")) }
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
            guard files.count < PhotoStaging.maximumSelection else { throw EngineFailure(code:"TOO_LARGE",message:String(localized:"사진과 파일은 합쳐서 100개까지 선택하세요")) }
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
            guard !name.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty, name.utf8.count <= 128 else { throw EngineFailure(code:"INVALID_REQUEST",message:String(localized:"이름을 1~128바이트로 입력하세요")) }
            if running { _ = try await Engine.call("settings.update",["values":["display_name":name]]) }
            UserDefaults.standard.set(name,forKey:"deviceName")
            // Republish with new TXT metadata instead of leaving a stale discovery name.
            if running { bonjour.start(); await refreshActivity() }
            notice = String(localized:"기기 이름을 저장했습니다")
        } catch { self.error = error.localizedDescription }
    }
    func requestNotifications() async {
        do { _ = try await UNUserNotificationCenter.current().requestAuthorization(options:[.alert,.sound,.badge]); await refreshNotificationStatus() }
        catch { self.error = error.localizedDescription }
    }
    func refreshNotificationStatus() async {
        let settings = await UNUserNotificationCenter.current().notificationSettings()
        notificationStatus = [.authorized,.provisional,.ephemeral].contains(settings.authorizationStatus) ? String(localized:"허용됨") : (settings.authorizationStatus == .notDetermined ? String(localized:"설정 전") : String(localized:"차단됨"))
    }
}
