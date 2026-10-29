import Foundation
import Network
import Darwin

struct LAN: Identifiable, Equatable {
    let name: String
    let address: String
    let network: String
    var id: String { "\(name):\(address):\(network)" }
    static func available() -> [LAN] {
        var head: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&head) == 0 else { return [] }
        defer { freeifaddrs(head) }
        var result: [LAN] = []
        var entry = head
        while let current = entry {
            defer { entry = current.pointee.ifa_next }
            let item = current.pointee
            guard let addr = item.ifa_addr, let mask = item.ifa_netmask,
                  addr.pointee.sa_family == UInt8(AF_INET), (item.ifa_flags & UInt32(IFF_UP)) != 0,
                  String(cString: item.ifa_name).hasPrefix("en") else { continue }
            let ip = UnsafeRawPointer(addr).assumingMemoryBound(to: sockaddr_in.self).pointee.sin_addr.s_addr
            let netmask = UnsafeRawPointer(mask).assumingMemoryBound(to: sockaddr_in.self).pointee.sin_addr.s_addr
            let number = UInt32(bigEndian: ip)
            // Only directly attached private IPv4 LANs are offered; VPN/cellular never inherit trust.
            guard number >> 24 == 10 || number >> 20 == 0xAC1 || number >> 16 == 0xC0A8 else { continue }
            func text(_ value: UInt32) -> String {
                var input = in_addr(s_addr: value)
                var buffer = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
                inet_ntop(AF_INET, &input, &buffer, socklen_t(buffer.count))
                return String(cString: buffer)
            }
            result.append(LAN(name: String(cString: item.ifa_name), address: text(ip), network: "\(text(ip & netmask))/\(netmask.nonzeroBitCount)"))
        }
        return result.sorted { $0.id < $1.id }
    }
}

// System Bonjour performs mDNS; Rust receives untrusted address hints and still authenticates every peer.
final class Bonjour: NSObject, NetServiceBrowserDelegate, NetServiceDelegate {
    private static let maximumServices = 1000
    static let transferType = "_fta._tcp."
    static let pairingType = "_fta-pair._tcp."
    private var browsers: [NetServiceBrowser] = []
    private var services: [String: NetService] = [:]
    private var publishers: [String: NetService] = [:]
    private var resolvedIDs: [String: String] = [:]
    var failure: ((String) -> Void)?
    func start() {
        stop()
        for type in [Self.transferType, Self.pairingType] {
            let browser = NetServiceBrowser(); browser.delegate = self
            browsers.append(browser); browser.searchForServices(ofType: type, inDomain: "local.")
        }
    }
    func stop() {
        browsers.forEach { $0.stop() }; browsers.removeAll()
        services.values.forEach { $0.stop() }; services.removeAll(); resolvedIDs.removeAll()
        publishers.values.forEach { $0.stop() }; publishers.removeAll()
    }
    func advertise(_ data: [String: Any], name: String) {
        guard let id = data["device_id"] as? String else { return }
        for (type, key) in [(Self.transferType, "address"), (Self.pairingType, "pairing_address")] {
            guard let address = data[key] as? String, let port = Int32(address.split(separator: ":").last ?? "") else {
                publishers.removeValue(forKey: type)?.stop(); continue
            }
            if let old = publishers[type], old.port == Int(port), old.name == id { continue }
            publishers.removeValue(forKey: type)?.stop()
            let service = NetService(domain: "local.", type: type, name: id, port: port)
            service.delegate = self
            service.setTXTRecord(NetService.data(fromTXTRecord: ["device_id":Data(id.utf8), "display_name":Data(name.utf8), "os":Data("ios".utf8), "version":Data("1".utf8)]))
            publishers[type] = service; service.publish()
        }
    }
    private func key(_ service: NetService) -> String { service.type + service.name }
    func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
        guard services.count < Self.maximumServices || services[key(service)] != nil else { return }
        services[key(service)] = service; service.delegate = self; service.resolve(withTimeout: 5)
    }
    func netServiceBrowser(_ browser: NetServiceBrowser, didRemove service: NetService, moreComing: Bool) {
        let k = key(service); services.removeValue(forKey: k)?.stop()
        guard let id = resolvedIDs.removeValue(forKey: k) else { return }
        Task { _ = try? await Engine.call("host.removed", ["device_id":id, "pairing":service.type == Self.pairingType]) }
    }
    func netServiceDidResolveAddress(_ sender: NetService) {
        guard let txt = sender.txtRecordData() else { return }
        let values = NetService.dictionary(fromTXTRecord: txt)
        func value(_ key: String) -> String { String(data: values[key] ?? Data(), encoding: .utf8) ?? "" }
        guard value("version") == "1", UUID(uuidString: value("device_id")) != nil, value("display_name").utf8.count <= 128, value("os").utf8.count <= 32 else { return }
        for data in sender.addresses ?? [] {
            let address: String? = data.withUnsafeBytes { bytes in
                guard let base = bytes.baseAddress, bytes.count >= MemoryLayout<sockaddr_in>.size,
                      base.assumingMemoryBound(to: sockaddr.self).pointee.sa_family == UInt8(AF_INET) else { return nil }
                var ip = base.assumingMemoryBound(to: sockaddr_in.self).pointee.sin_addr
                var buffer = [CChar](repeating: 0, count: Int(INET_ADDRSTRLEN))
                inet_ntop(AF_INET, &ip, &buffer, socklen_t(buffer.count))
                return String(cString: buffer)
            }
            guard let address else { continue }
            resolvedIDs[key(sender)] = value("device_id")
            let device: [String: Any] = ["device_id":value("device_id"),"display_name":value("display_name"),"os":value("os"),"address":"\(address):\(sender.port)"]
            Task { _ = try? await Engine.call("host.discovered", ["device":device, "pairing":sender.type == Self.pairingType]) }
        }
    }
    func netServiceBrowser(_ browser: NetServiceBrowser, didNotSearch errorDict: [String: NSNumber]) {
        failure?("기기를 검색할 수 없습니다. 설정에서 FTA의 로컬 네트워크 권한과 Wi-Fi 연결을 확인하세요.")
    }
    func netService(_ sender: NetService, didNotPublish errorDict: [String: NSNumber]) {
        failure?("수신 서비스를 알리지 못했습니다. 로컬 네트워크 권한을 확인하세요.")
    }
}
