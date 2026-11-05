// Localize visible copy while preserving identifiers, user data, and existing storage folder names.
// System controls preserve native accessibility, keyboard behavior and OS appearance across all three tabs.
import SwiftUI
import ImageIO
import UniformTypeIdentifiers
import QuickLook
import AVFoundation

@main struct FTAApp: App {
    @StateObject private var model = AppModel()
    @Environment(\.scenePhase) private var phase
    var body: some Scene {
        WindowGroup {
            RootView().environmentObject(model)
                .onChange(of: phase) { _, phase in
                    if phase == .background { model.setForeground(false) }
                    else if phase == .active { model.setForeground(true); Task { await model.refreshNotificationStatus() } }
                }
                .onOpenURL { url in
                    // File-provider imports require a user action; deep links never auto-send or auto-open content.
                    if url.isFileURL { Task { await model.importFiles([url]); model.tab = 0 } }
                }
        }
    }
}

// One presentation route lets an incoming approval replace a history sheet without competing sheets.
private enum AppSheet: Identifiable {
    case record(Transfer), registration(Registration)
    var id: String { switch self { case .record(let value): return "record:" + value.id; case .registration(let value): return "registration:" + value.id } }
}
struct RootView: View {
    @EnvironmentObject var model: AppModel
    var body: some View {
        TabView(selection:$model.tab) {
            NavigationStack { SendView() }.tabItem { Label(String(localized:"보내기"),systemImage:"paperplane") }.tag(0)
            NavigationStack { HistoryView() }.tabItem { Label(String(localized:"기록"),systemImage:"clock") }.badge(model.unreadCount).tag(1)
            NavigationStack { SettingsView() }.tabItem { Label(String(localized:"설정"),systemImage:"gearshape") }.tag(2)
        }
        // A transient material banner does not interrupt typing or require notification permission.
        .overlay(alignment:.top) {
            if let event = model.receiptBanner {
                HStack(spacing:12) {
                    Button { Task { model.tab = 1; await model.openRecord(event.transfer_id) } } label: {
                        HStack(spacing:12) {
                            Image(systemName:event.record.state == "completed" ? "tray.and.arrow.down.fill" : "exclamationmark.triangle")
                            VStack(alignment:.leading,spacing:4) {
                                Text(model.receiptTitle(event)).font(.subheadline.weight(.semibold)).lineLimit(2)
                                Text(model.receiptSubtitle(event)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                            }
                            Spacer(minLength:0)
                        }.padding(.vertical,12).contentShape(Rectangle())
                    }.buttonStyle(.plain)
                    Button(String(localized:"닫기"),systemImage:"xmark") { model.dismissReceiptBanner() }.modifier(IconActionStyle())
                }.padding(.horizontal,16).background(.regularMaterial,in:RoundedRectangle(cornerRadius:18))
                    .overlay(RoundedRectangle(cornerRadius:18).stroke(.primary.opacity(0.12)))
                    .shadow(color:.black.opacity(0.12),radius:12,y:4).padding(.horizontal,16).padding(.top,8)
                    .accessibilityElement(children:.contain).accessibilityIdentifier("receipt-banner")
            }
        }
        .sheet(item:Binding<AppSheet?>(get:{
            if let request = model.requests.first { return .registration(request) }
            if let detail = model.detail { return .record(detail) }
            return nil
        },set:{ if $0 == nil { model.detail = nil } })) { sheet in
            switch sheet {
            case .registration(let request): RegistrationView(request:request).environmentObject(model).interactiveDismissDisabled()
            case .record(let record): NavigationStack { DetailView(record:record) }.environmentObject(model)
            }
        }
        .alert(String(localized:"작업을 완료하지 못했습니다"),isPresented:Binding(get:{model.error != nil},set:{if !$0 { model.error = nil }})) {
            Button(String(localized:"확인"),role:.cancel) { model.error = nil }
        } message: { Text(model.error ?? "") }
    }
}

// The computer outline copies desktop main.ts's two SVG paths exactly, rather than substituting an SF icon.
struct PeerIcon: View {
    let mobile: Bool
    var side: CGFloat = 30
    var body: some View {
        Group {
            if mobile { Image(systemName:"iphone").resizable().scaledToFit().padding(3) }
            else {
                Canvas { context, size in
                    var p = Path()
                    func point(_ x:CGFloat,_ y:CGFloat)->CGPoint { CGPoint(x:x/24*size.width,y:y/24*size.height) }
                    p.move(to:point(3,4)); p.addLine(to:point(21,4)); p.addLine(to:point(21,16)); p.addLine(to:point(3,16)); p.closeSubpath()
                    p.move(to:point(8,21)); p.addLine(to:point(16,21)); p.move(to:point(12,16)); p.addLine(to:point(12,21))
                    context.stroke(p,with:.foreground,lineWidth:1.7)
                }
            }
        }.frame(width:side,height:side).accessibilityHidden(true)
    }
}
// Materials retain native blur/accessibility while white actions echo the desktop transfer panel.
private struct GlassSurface: ViewModifier {
    var radius: CGFloat = 24
    func body(content: Content) -> some View {
        content.background(.ultraThinMaterial,in:RoundedRectangle(cornerRadius:radius))
            .overlay(RoundedRectangle(cornerRadius:radius).stroke(.white.opacity(0.2),lineWidth:0.7))
            .shadow(color:.black.opacity(0.08),radius:16,y:6)
    }
}
private struct GlassBackground: View {
    @Environment(\.colorScheme) private var scheme
    var body: some View {
        ZStack {
            Color(uiColor:.systemGroupedBackground)
            // The desktop icon's blue tint gives the translucent layers a shared FTA identity.
            LinearGradient(colors:[Color(red:36/255,green:70/255,blue:107/255).opacity(scheme == .dark ? 0.6 : 0.17),.clear],startPoint:.topLeading,endPoint:.bottomTrailing)
        }.ignoresSafeArea(.container)
    }
}
// Unknown progress stays empty: only measured bytes may fill the green device border.
private struct TransferBorder: View {
    let fraction: Double?
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius:20).stroke(.white.opacity(0.2),lineWidth:3)
            if let fraction {
                RoundedRectangle(cornerRadius:20).trim(from:0,to:fraction).stroke(.green,style:StrokeStyle(lineWidth:3,lineCap:.round))
            }
        }.accessibilityLabel(String(localized:"전송 진행률")).accessibilityValue(fraction.map { "\(Int($0*100))%" } ?? String(localized:"진행률 확인 중"))
    }
}
// Receipt time drives two finite blinks; a new send cannot inherit a repeating animation or stale timer.
private struct TransferCompletionBlink: ViewModifier {
    let deadline: Date?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    func body(content: Content) -> some View {
        TimelineView(.animation(minimumInterval:1.0/30,paused:deadline == nil || reduceMotion)) { context in
            content.opacity(TransferFeedback.completionIconOpacity(deadline:deadline,now:context.date,reduceMotion:reduceMotion))
        }
    }
}
struct SendView: View {
    @EnvironmentObject var model: AppModel
    @State private var picking = false
    @State private var pickingPhotos = false
    @State private var takingPhoto = false
    @State private var choosingPhotoSource = false
    @FocusState private var messageFocused: Bool
    var body: some View {
        GeometryReader { geometry in
        ScrollView {
            VStack(alignment:.leading,spacing:24) {
                // Branding is ordinary scroll content, so no collapsed navigation strip remains above it.
                HStack(spacing:12) {
                    Image("AppMark").resizable().scaledToFit().frame(width:52,height:52).accessibilityHidden(true)
                    VStack(alignment:.leading,spacing:3) {
                        Text("FTA").font(.largeTitle.weight(.bold))
                        Text("File Transfer Anything").font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    refreshButton
                }.accessibilityIdentifier("app-header")
                HStack {
                    Label(model.serviceText,systemImage:model.running ? "dot.radiowaves.left.and.right" : "pause.circle")
                        .font(.footnote).foregroundStyle(.secondary)
                    Spacer(minLength:4)
                    if !model.running { Button(String(localized:"연결 설정"),systemImage:"gearshape") { model.tab = 2 }.modifier(IconActionStyle()).font(.footnote) }
                }
                VStack(alignment:.leading,spacing:14) {
                    Text(String(localized:"연결 가능한 기기")).font(.subheadline.weight(.medium)).foregroundStyle(.secondary)
                    if model.targets.isEmpty {
                        ContentUnavailableView(model.running ? String(localized:"발견된 기기가 없습니다") : String(localized:"송수신을 시작하세요"),systemImage:"network",description:Text(model.running ? String(localized:"같은 LAN에서 상대 FTA를 실행하세요.") : String(localized:"설정에서 집 네트워크를 확인하세요.")))
                            .frame(maxWidth:.infinity).padding(.vertical,8)
                    } else {
                        // Native horizontal scrolling responds to a finger drag without a visible scrollbar.
                        ScrollView(.horizontal) {
                            HStack(alignment:.top,spacing:16) {
                                ForEach(model.targets) { peer in
                                    Button { model.selectedPeer = peer.id } label: {
                                        VStack(spacing:10) {
                                            ZStack(alignment:.bottomTrailing) {
                                                PeerIcon(mobile:peer.mobile,side:34).foregroundStyle(.white)
                                                    .frame(width:76,height:72)
                                                    .background(Color(white:0.12).opacity(0.8),in:RoundedRectangle(cornerRadius:20))
                                                    .modifier(GlassSurface(radius:20))
                                                    .overlay(RoundedRectangle(cornerRadius:20).stroke(.white.opacity(model.selectedPeer == peer.id && !(model.transferFeedback.peerID == peer.id && model.transferFeedback.isActive) ? 0.95 : 0),lineWidth:2))
                                                    .overlay {
                                                        if model.transferFeedback.peerID == peer.id && model.transferFeedback.isActive {
                                                            TransferBorder(fraction:model.transferFeedback.fraction)
                                                        }
                                                    }
                                                    .modifier(TransferCompletionBlink(deadline:model.transferFeedback.completionDeadline(for:peer.id)))
                                                // Attach presence to the silhouette, rather than the far corner of its tile.
                                                Circle().fill(peer.paired && peer.online ? Color.green : Color.orange)
                                                    .frame(width:9,height:9).overlay(Circle().stroke(Color(white:0.15),lineWidth:2))
                                                    .offset(x:peer.mobile ? -26 : -20,y:peer.mobile ? -22 : -26)
                                                if let deadline = model.transferFeedback.completionDeadline(for:peer.id), Date() < deadline {
                                                    Image(systemName:"checkmark.circle.fill").symbolRenderingMode(.palette)
                                                        .foregroundStyle(.white,Color.green).font(.system(size:24))
                                                        .padding(3).accessibilityLabel(String(localized:"전송 완료")).accessibilityIdentifier("transfer-completed")
                                                }
                                            }
                                            Text(peer.display_name).font(.caption).foregroundStyle(.primary).lineLimit(2)
                                                .multilineTextAlignment(.center).frame(width:86)
                                        }
                                    }.buttonStyle(.plain).disabled(!peer.selectable || model.sending)
                                        .accessibilityLabel("\(peer.display_name), \(peer.status)")
                                        .accessibilityValue(transferStatus(for:peer.id))
                                        .accessibilityAddTraits(model.selectedPeer == peer.id ? .isSelected : [])
                                }
                            }.padding(4)
                        }.scrollIndicators(.hidden).accessibilityIdentifier("available-devices")
                    }
                }
                // Photos and Files are two freestanding actions, with no surrounding section box.
                HStack(spacing:14) {
                    mediaButton(String(localized:"사진"),symbol:"photo.on.rectangle") { messageFocused = false; choosingPhotoSource = true }
                        // Anchor the native popover to the actual source button, with its arrow above the menu.
                        .popover(isPresented:$choosingPhotoSource,attachmentAnchor:.rect(.bounds),arrowEdge:.top) {
                            VStack(alignment:.leading,spacing:0) {
                                Button { choosingPhotoSource = false; pickingPhotos = true } label: { Label(String(localized:"갤러리에서 선택"),systemImage:"photo.on.rectangle").frame(maxWidth:.infinity,alignment:.leading).padding(14) }
                                Divider()
                                Button { choosingPhotoSource = false; Task { await openCamera() } } label: { Label(String(localized:"촬영"),systemImage:"camera").frame(maxWidth:.infinity,alignment:.leading).padding(14) }
                            }.buttonStyle(.plain).frame(minWidth:200).padding(4).presentationCompactAdaptation(.popover)
                                .accessibilityIdentifier("photo-source-menu")
                        }
                    mediaButton(String(localized:"파일"),symbol:"folder") { messageFocused = false; picking = true }
                }.disabled(model.sending || model.importing)
                if model.importing { ProgressView(String(localized:"선택한 항목 준비 중")).font(.footnote) }
                if !(model.photos + model.files).isEmpty {
                    VStack(alignment:.leading,spacing:0) {
                        ForEach(model.photos + model.files,id:\.self) { url in
                            AttachmentPreviewRow(url:url) { model.removeAttachment(url) }
                                .disabled(model.sending || model.importing)
                            if url != (model.photos + model.files).last { Divider().padding(.leading,72) }
                        }
                        Divider().padding(.vertical,12)
                        HStack(spacing:12) {
                            VStack(alignment:.leading,spacing:4) {
                                Label("\((model.photos.count + model.files.count).formatted())",systemImage:"paperclip").font(.subheadline.weight(.semibold))
                                Text(ByteCountFormatter.string(fromByteCount:(model.photos + model.files).reduce(Int64(0)) { $0 + Int64((try? $1.resourceValues(forKeys:[.fileSizeKey]).fileSize) ?? 0) },countStyle:.file)).font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button(String(localized:"선택 해제"),systemImage:"trash",role:.destructive) { model.clearPhotos(); model.clearFiles() }.modifier(IconActionStyle()).disabled(model.sending || model.importing)
                            Button { Task { await model.sendAttachments() } } label: { Label(String(localized:"전송"),systemImage:"arrow.up") }
                                .labelStyle(.iconOnly).frame(minWidth:44,minHeight:44)
                                .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                                .accessibilityIdentifier("send-attachments").disabled(!model.canSend)
                        }
                    }.padding(16).modifier(GlassSurface())
                }
                if model.sending {
                    Button(String(localized:"전송 취소"),systemImage:"stop.circle",role:.destructive) { Task { await model.cancel() } }.modifier(IconActionStyle())
                }
                if let notice = model.notice {
                    HStack { Text(notice).font(.footnote).foregroundStyle(.secondary); Spacer(); Button(String(localized:"닫기"),systemImage:"xmark") { model.notice = nil }.modifier(IconActionStyle()) }
                }
            }.padding(20).frame(maxWidth:.infinity,minHeight:geometry.size.height,alignment:.top)
                // Only unoccupied content receives this gesture; TextField and buttons retain native taps.
                .background { Color.clear.contentShape(Rectangle()).onTapGesture { messageFocused = false } }
        }.accessibilityIdentifier("send-scroll").scrollDismissesKeyboard(.interactively).scrollIndicators(.hidden)
        .refreshable { await model.refreshDevices() }
        }
        // Restore the original bottom material composer. The system inset keeps it above the keyboard.
        .safeAreaInset(edge:.bottom,spacing:0) { composer.padding(.horizontal,20).padding(.vertical,12) }
        .background(GlassBackground())
        .toolbar(.hidden,for:.navigationBar)
        .sheet(isPresented:$pickingPhotos) {
            PhotoLibraryPicker { providers in
                pickingPhotos = false
                Task { await model.importPhotos(providers) }
            }.ignoresSafeArea()
        }
        .fullScreenCover(isPresented:$takingPhoto) {
            CameraPicker { image in
                takingPhoto = false
                if let image { Task { await model.importCapturedPhoto(image) } }
            }.ignoresSafeArea()
        }
        .fileImporter(isPresented:$picking,allowedContentTypes:[.item],allowsMultipleSelection:true) { result in
            switch result { case .success(let urls): Task { await model.importFiles(urls) }; case .failure(let error): model.error = error.localizedDescription }
        }
    }
    // The old toolbar supplied a native circular glass surface; retain it without restoring a fixed bar.
    @ViewBuilder private var refreshButton: some View {
        if #available(iOS 26.0, *) {
            refreshAction.glassEffect(.regular.interactive(),in:Circle())
        } else {
            refreshAction.background(.ultraThinMaterial,in:Circle())
                .overlay(Circle().stroke(.white.opacity(0.2),lineWidth:0.7))
        }
    }
    private var refreshAction: some View {
        Button { Task { await model.refreshDevices() } } label: {
            Image(systemName:"arrow.clockwise").frame(width:44,height:44)
        }.buttonStyle(.plain).disabled(!model.running).accessibilityLabel(String(localized:"기기 새로고침"))
    }
    private var composer: some View {
        VStack(alignment:.leading,spacing:10) {
                    HStack(alignment:.bottom,spacing:10) {
                        // Native vertical TextField grows from one to four lines, then scrolls internally.
                        TextField(String(localized:"텍스트 또는 URL"),text:$model.message,axis:.vertical)
                            .focused($messageFocused).lineLimit(1...4).textInputAutocapitalization(.never).autocorrectionDisabled()
                            .accessibilityLabel(String(localized:"전송할 텍스트 또는 URL")).accessibilityIdentifier("message-input")
                            .padding(.vertical,10)
                        Button { Task { await model.sendMessage() } } label: {
                            Image(systemName:"arrow.up").font(.system(size:17,weight:.semibold))
                                .foregroundStyle(.white).frame(width:36,height:36)
                                .background(Color(white:0.2),in:Circle())
                                .overlay(Circle().stroke(.white.opacity(0.3),lineWidth:0.7))
                                .frame(width:44,height:44)
                        }.buttonStyle(.plain).accessibilityLabel(String(localized:"메시지 전송")).accessibilityIdentifier("send-message")
                            .disabled(!model.canSend || model.message.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty)
                            .opacity(model.canSend && !model.message.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty ? 1 : 0.4)
                    }.padding(.leading,16).padding(.trailing,6).padding(.vertical,4).modifier(GlassSurface())
                        .disabled(model.sending)
                }
    }
    // The icon replaces visible status text, so its enclosing native button must retain a spoken result.
    private func transferStatus(for peerID: String) -> String {
        let feedback = model.transferFeedback
        if let end = feedback.completionDeadline(for:peerID), Date() < end { return String(localized:"전송 완료") }
        guard feedback.peerID == peerID, feedback.isActive else { return "" }
        return feedback.fraction.map { String(localized:"전송 중 \(Int($0*100))%") } ?? String(localized:"전송 중")
    }
    private func mediaButton(_ title: String, symbol: String, action: @escaping () -> Void) -> some View {
        Button(action:action) { Label(title,systemImage:symbol).labelStyle(.iconOnly).font(.body.weight(.medium)).foregroundStyle(.white).frame(maxWidth:.infinity).padding(.vertical,16) }
            .buttonStyle(.plain).background(Color(white:0.12).opacity(0.83),in:Capsule()).modifier(GlassSurface(radius:30))
    }
    private func openCamera() async {
        guard UIImagePickerController.isSourceTypeAvailable(.camera) else {
            model.error = String(localized:"이 기기에서는 카메라를 사용할 수 없습니다"); return
        }
        let permitted: Bool
        switch AVCaptureDevice.authorizationStatus(for:.video) {
        case .authorized: permitted = true
        case .notDetermined: permitted = await AVCaptureDevice.requestAccess(for:.video)
        default: permitted = false
        }
        if permitted { takingPhoto = true }
        else { model.error = String(localized:"카메라 접근이 허용되지 않았습니다. iOS 설정에서 FTA의 카메라 권한을 확인하세요.") }
    }
}

// Keep the existing history layout/background; only the explicitly requested text color changes.
struct HistoryView: View {
    @EnvironmentObject var model: AppModel
    @State private var filter = "all"
    var body: some View {
        List {
            Picker(String(localized:"표시"),selection:$filter) { Text(String(localized:"전체")).tag("all");Text(String(localized:"받음")).tag("receive");Text(String(localized:"보냄")).tag("send") }.pickerStyle(.segmented)
            if model.history.isEmpty { ContentUnavailableView { Label(String(localized:"아직 전송 기록이 없습니다"),systemImage:"tray").foregroundStyle(.white) } description: { Text(String(localized:"파일·텍스트·URL의 송수신 결과가 여기에 표시됩니다.")).foregroundStyle(.white.opacity(0.7)) } }
            ForEach(model.history.filter { filter == "all" || $0.direction == filter }) { record in
                Button { Task { await model.openRecord(record.id) } } label: {
                    HStack(spacing:12) {
                        Image(systemName:record.direction == "receive" ? "arrow.down.circle" : "arrow.up.circle").foregroundStyle(Color.accentColor)
                        VStack(alignment:.leading,spacing:4) {
                            Text(record.title).foregroundStyle(.white).lineLimit(1)
                            Text(String(localized:"\(record.direction == "receive" ? String(localized:"받음") : String(localized:"보냄")) · \(record.status)")).font(.caption).foregroundStyle(.white.opacity(0.7))
                        }
                        Spacer()
                        Text(Date(timeIntervalSince1970:record.created_at ?? 0),style:.date).font(.caption2).foregroundStyle(.white.opacity(0.7))
                    }
                }
            }
        }.scrollContentBackground(.hidden).background(GlassBackground()).navigationTitle(String(localized:"기록")).refreshable { await model.refreshActivity() }
    }
}

struct DetailView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.dismiss) var dismiss
    let record: Transfer
    @State private var preview: URL?
    @State private var deleting = false
    var body: some View {
        List {
            Section(String(localized:"전송 결과")) {
                LabeledContent(String(localized:"상태"),value:record.status)
                LabeledContent(String(localized:"방향"),value:record.direction == "receive" ? String(localized:"받음") : String(localized:"보냄"))
                LabeledContent(String(localized:"날짜")) { Text(Date(timeIntervalSince1970:record.created_at ?? 0),format:.dateTime) }
                if let error = record.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
                if record.state == "unknown" { Button(String(localized:"수신 측 결과 다시 확인"),systemImage:"arrow.clockwise") { Task { await model.reconcile(record) } }.modifier(IconActionStyle()) }
            }
            if let body = record.body {
                Section(record.kind == "url" ? "URL" : String(localized:"텍스트")) {
                    Text(body).textSelection(.enabled)
                    Button(String(localized:"복사"),systemImage:"doc.on.doc") { UIPasteboard.general.string = body }.modifier(IconActionStyle())
                    // Only an explicit tap opens HTTP(S); stored strings never execute automatically.
                    if record.kind == "url", let url = URL(string:body), ["http","https"].contains(url.scheme?.lowercased() ?? ""), url.host != nil {
                        Link(destination:url) { Label(String(localized:"링크 열기"),systemImage:"safari") }.modifier(IconActionStyle())
                    }
                }
            }
            ForEach(Array(record.items.enumerated()),id:\.offset) { _,item in
                Section(item.saved_name ?? item.name) {
                    LabeledContent(String(localized:"크기"),value:ByteCountFormatter.string(fromByteCount:Int64(item.size) ?? 0,countStyle:.file))
                    LabeledContent(String(localized:"상태"),value:item.state == "completed" ? String(localized:"완료") : item.state)
                    if let error = item.error { Text(error).foregroundStyle(.red) }
                    if record.direction == "receive", item.state == "completed" {
                        if let url = Sandbox.receivedURL(item.path) {
                            Button(String(localized:"미리보기"),systemImage:"doc.text.magnifyingglass") { preview = url }.modifier(IconActionStyle())
                            ShareLink(item:url) { Label(String(localized:"공유 · 파일에 저장"),systemImage:"square.and.arrow.up") }.modifier(IconActionStyle())
                        } else { Text(String(localized:"파일이 이동되었거나 삭제되었습니다.")).foregroundStyle(.secondary) }
                    }
                }
            }
            Section { Button(String(localized:"기록 삭제"),systemImage:"trash",role:.destructive) { deleting = true }.modifier(IconActionStyle()) } footer:{ Text(String(localized:"기록을 삭제해도 이미 받은 파일은 유지됩니다.")) }
        }.navigationTitle(record.title).navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement:.confirmationAction) { Button(String(localized:"닫기"),systemImage:"xmark") { dismiss() }.modifier(IconActionStyle()) } }
            .quickLookPreview($preview)
            .confirmationDialog(String(localized:"이 기록을 삭제할까요?"),isPresented:$deleting,titleVisibility:.visible) {
                Button(String(localized:"기록 삭제"),role:.destructive) { Task { await model.deleteRecord(record) } }
            }
    }
}

struct RegistrationView: View {
    @EnvironmentObject var model: AppModel
    let request: Registration
    var body: some View {
        NavigationStack {
            Form {
                Section {
                    HStack { PeerIcon(mobile:["ios","android"].contains(request.os)); Text(request.display_name).font(.headline) }
                    Text(String(localized:"이 기기를 등록하면 같은 LAN에서 온라인 상태일 때 파일·텍스트·URL을 추가 승인 없이 받습니다."))
                    Text(String(localized:"등록 승인 전에는 전송 내용을 받지 않습니다.")).foregroundStyle(.secondary)
                    LabeledContent(String(localized:"전송 종류"),value:request.kind == "files" ? String(localized:"파일 \(request.item_count)개") : (request.kind == "url" ? "URL" : String(localized:"텍스트")))
                    LabeledContent(String(localized:"요청 만료")) { Text(Date(timeIntervalSince1970:request.expires_at),style:.timer) }
                }
                Section {
                    Button(String(localized:"기기 등록 허용")) { Task { await model.decide(request,accept:true) } }.disabled(model.deciding)
                    Button(String(localized:"거절"),role:.destructive) { Task { await model.decide(request,accept:false) } }.disabled(model.deciding)
                }
            }.navigationTitle(String(localized:"기기 등록 요청")).navigationBarTitleDisplayMode(.inline)
        }.presentationDetents([.medium,.large])
    }
}

struct SettingsView: View {
    @EnvironmentObject var model: AppModel
    @State private var removing: Peer?
    var body: some View {
        Form {
            Section {
                TextField(String(localized:"기기 이름"),text:$model.name).textContentType(.name)
                Button(String(localized:"이름 저장"),systemImage:"checkmark.square") { Task { await model.saveName() } }.modifier(IconActionStyle())
            } header:{ Text(String(localized:"이 iPhone")) } footer:{ Text("FTA · File Transfer Anything") }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section {
                Picker(String(localized:"사용할 연결"),selection:Binding(get:{model.selectedLAN},set:{model.selectLAN($0)})) {
                    if model.lans.isEmpty { Text(String(localized:"Wi-Fi 연결 없음")).tag("") }
                    ForEach(model.lans) { lan in Text(lan.address).tag(lan.id) }
                }.disabled(model.running || model.starting)
                if let lan = model.lan { LabeledContent(String(localized:"허용 대역"),value:lan.network).font(.footnote) }
                Toggle(String(localized:"신뢰하는 집 네트워크입니다"),isOn:Binding(get:{model.confirmed},set:{model.setNetworkTrusted($0)}))
                    .disabled(model.starting || model.lan == nil)
                Toggle(String(localized:"송수신 사용"),isOn:Binding(get:{model.receiveEnabled},set:{model.setReceiveEnabled($0)}))
                Label(model.serviceText,systemImage:model.running ? "checkmark.circle" : "pause.circle").foregroundStyle(.secondary)
            } header:{ Text(String(localized:"LAN 연결")) } footer:{ Text(String(localized:"앱을 열어둔 동안 자동 수신합니다. 백그라운드·화면 잠금·연결 단절 시 수신을 중지하며, 신뢰한 연결로 돌아오면 복원합니다. 신뢰 설정은 표시된 허용 대역을 기준으로 기억합니다. 등록된 기기만 내용을 보낼 수 있습니다.")) }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section {
                LabeledContent(String(localized:"저장 위치"),value:String(localized:"나의 iPhone / FTA / 받은 파일"))
                Text(String(localized:"파일 앱에서 받은 파일을 확인하거나 이동할 수 있습니다. 앱을 삭제하면 앱 안의 파일도 삭제됩니다.")).font(.footnote).foregroundStyle(.secondary)
            } header:{ Text(String(localized:"받은 파일")) }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section(String(localized:"알림")) {
                LabeledContent(String(localized:"권한"),value:model.notificationStatus)
                if let issue = model.notificationDeliveryIssue { Text(issue).font(.footnote).foregroundStyle(.secondary) }
                Button(String(localized:"알림 허용 요청"),systemImage:"bell.badge") { Task { await model.requestNotifications() } }.modifier(IconActionStyle())
                Button(String(localized:"iOS 설정 열기"),systemImage:"gearshape") { if let url = URL(string:UIApplication.openSettingsURLString) { UIApplication.shared.open(url) } }.modifier(IconActionStyle())
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section(String(localized:"등록된 기기")) {
                if !model.peers.contains(where:{$0.paired}) { Text(String(localized:"등록된 기기가 없습니다")).foregroundStyle(.secondary) }
                ForEach(model.peers.filter(\.paired)) { peer in
                    HStack { PeerIcon(mobile:peer.mobile); Text(peer.display_name); Spacer(); Button(String(localized:"해제"),systemImage:"minus.circle",role:.destructive) { removing = peer }.modifier(IconActionStyle()) }
                }
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            // Submission requires accessible policy/support links; preserve the existing settings controls.
            Section(String(localized:"개인정보 및 지원")) {
                // Keep desktop onboarding alongside help without changing network or trust settings.
                NavigationLink(String(localized:"컴퓨터에 FTA 설치하기")) { DesktopInstallView() }
                NavigationLink(String(localized:"개인정보 처리방침")) { LegalDocumentView(title:String(localized:"개인정보 처리방침"),resource:AppLegal.privacyResource,publicURL:AppLegal.privacyURL) }
                if let url = AppLegal.supportURL { Link(String(localized:"지원 및 문의"),destination:url) }
                NavigationLink(String(localized:"오픈소스 고지")) { ThirdPartyNoticesView() }
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
        }.scrollContentBackground(.hidden).background(GlassBackground()).navigationTitle(String(localized:"설정"))
            .confirmationDialog(String(localized:"기기 등록을 해제할까요?"),isPresented:Binding(get:{removing != nil},set:{if !$0 {removing = nil}}),titleVisibility:.visible) {
                if let peer = removing { Button(String(localized:"등록 해제"),role:.destructive) { Task { await model.unpair(peer); removing = nil } } }
            } message:{ Text(String(localized:"이 기기의 새 전송을 차단합니다. 다시 보내려면 수신자의 등록 승인이 필요합니다.")) }
    }
}

// Pin the verified desktop-only release rather than offering mobile packages from a changing latest release.
// Update this address when the next verified Windows/macOS release is published.
private enum DesktopDistribution {
    static let releaseAddress = "https://github.com/froiibn/FTA/releases/tag/v0.1.2"
}

struct DesktopInstallView: View {
    @State private var copied = false
    var body: some View {
        Form {
            Section { Text(String(localized:"무료 PC 버전입니다. 컴퓨터의 브라우저에서 아래 주소를 열어 설치하세요.")) }
                .listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section("Windows · macOS") {
                Text(String(localized:"Windows: x64-setup.exe 설치 파일을 실행하세요."))
                Text(String(localized:"macOS: Apple Silicon용 aarch64.dmg를 열고 FTA를 응용 프로그램 폴더로 드래그하세요. Intel Mac 설치 파일은 아직 제공하지 않습니다."))
                address(DesktopDistribution.releaseAddress)
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section("Linux") {
                // Linux has not been tested on a real machine; do not offer an installation path yet.
                Text(String(localized:"지원 준비 중"))
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section {
                Text(String(localized:"컴퓨터에서 FTA를 실행하고 모바일과 같은 LAN에 연결하세요. 양쪽에서 사용할 네트워크를 신뢰하도록 설정한 뒤 기기를 선택해 전송하세요. 최초 전송은 수신 기기에서 한 번 등록을 승인해야 합니다."))
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
        }.scrollContentBackground(.hidden).background(GlassBackground())
            .navigationTitle(String(localized:"컴퓨터에 FTA 설치하기")).navigationBarTitleDisplayMode(.inline)
            .alert(String(localized:"주소를 복사했습니다"),isPresented:$copied) { Button(String(localized:"확인"),role:.cancel) {} }
    }
    // Copy only on explicit action; installers are opened by the user on their computer.
    private func address(_ value: String) -> some View {
        VStack(alignment:.leading,spacing:12) {
            Text(verbatim:value).font(.footnote).textSelection(.enabled)
            Button(String(localized:"주소 복사"),systemImage:"doc.on.doc") { UIPasteboard.general.string = value; copied = true }.modifier(IconActionStyle())
                .accessibilityLabel(String(localized:"주소 복사") + " " + value)
        }
    }
}

// Native labels remain the accessibility names while icon actions retain a minimum touch target.
struct IconActionStyle: ViewModifier {
    func body(content: Content) -> some View {
        content.labelStyle(.iconOnly).frame(minWidth:44,minHeight:44).contentShape(Rectangle())
    }
}

// Downsample only private staged images off the main actor; original full-size assets never enter the UI.
private struct AttachmentPreviewRow: View {
    let url: URL
    let remove: () -> Void
    @State private var thumbnail: CGImage?
    private static let thumbnailPixels = 144
    var body: some View {
        HStack(spacing:12) {
            ZStack {
                RoundedRectangle(cornerRadius:12).fill(.secondary.opacity(0.12))
                if let thumbnail { Image(decorative:thumbnail,scale:1).resizable().scaledToFill() }
                else { Image(systemName:"doc.fill").font(.title2).foregroundStyle(.secondary) }
            }.frame(width:56,height:56).clipShape(RoundedRectangle(cornerRadius:12)).accessibilityHidden(true)
            VStack(alignment:.leading,spacing:5) {
                Text(url.lastPathComponent).font(.subheadline.weight(.medium)).lineLimit(2).truncationMode(.middle)
                Text(ByteCountFormatter.string(fromByteCount:Int64((try? url.resourceValues(forKeys:[.fileSizeKey]).fileSize) ?? 0),countStyle:.file) + (url.pathExtension.isEmpty ? "" : " · " + url.pathExtension.uppercased()))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }.frame(maxWidth:.infinity,alignment:.leading)
            Button(String(localized:"선택 해제"),systemImage:"xmark.circle") { remove() }
                .modifier(IconActionStyle()).foregroundStyle(.secondary)
                .accessibilityLabel(String(localized:"선택 해제") + " " + url.lastPathComponent)
        }.padding(.vertical,8)
        .task(id:url) {
            guard UTType(filenameExtension:url.pathExtension)?.conforms(to:.image) == true else { return }
            // Capture this Sendable budget before leaving the UI actor.
            let maximumPixels = Self.thumbnailPixels
            thumbnail = await Task.detached(priority:.utility) {
                guard let source = CGImageSourceCreateWithURL(url as CFURL,nil) else { return nil as CGImage? }
                return CGImageSourceCreateThumbnailAtIndex(source,0,[kCGImageSourceCreateThumbnailFromImageAlways:true,kCGImageSourceCreateThumbnailWithTransform:true,kCGImageSourceThumbnailMaxPixelSize:maximumPixels] as CFDictionary)
            }.value
        }
    }
}
