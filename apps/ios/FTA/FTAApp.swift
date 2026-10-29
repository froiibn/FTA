// System controls preserve native accessibility, keyboard behavior and OS appearance across all three tabs.
import SwiftUI
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
            NavigationStack { SendView() }.tabItem { Label("보내기",systemImage:"paperplane") }.tag(0)
            NavigationStack { HistoryView() }.tabItem { Label("기록",systemImage:"clock") }.tag(1)
            NavigationStack { SettingsView() }.tabItem { Label("설정",systemImage:"gearshape") }.tag(2)
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
        .alert("작업을 완료하지 못했습니다",isPresented:Binding(get:{model.error != nil},set:{if !$0 { model.error = nil }})) {
            Button("확인",role:.cancel) { model.error = nil }
        } message: { Text(model.error ?? "") }
    }
}

// The computer outline copies desktop main.ts's two SVG paths exactly, rather than substituting an SF icon.
struct PeerIcon: View {
    let mobile: Bool
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
        }.frame(width:30,height:30).accessibilityHidden(true)
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
struct SendView: View {
    @EnvironmentObject var model: AppModel
    @State private var picking = false
    @State private var pickingPhotos = false
    @State private var takingPhoto = false
    @State private var choosingPhotoSource = false
    var body: some View {
        ScrollView {
            VStack(alignment:.leading,spacing:24) {
                HStack {
                    Label(model.serviceText,systemImage:model.running ? "dot.radiowaves.left.and.right" : "pause.circle")
                        .font(.footnote).foregroundStyle(.secondary)
                    Spacer(minLength:4)
                    if !model.running { Button("연결 설정") { model.tab = 2 }.font(.footnote) }
                }
                VStack(alignment:.leading,spacing:14) {
                    Text("연결 가능한 기기").font(.subheadline.weight(.medium)).foregroundStyle(.secondary)
                    if model.targets.isEmpty {
                        ContentUnavailableView(model.running ? "발견된 기기가 없습니다" : "송수신을 시작하세요",systemImage:"network",description:Text(model.running ? "같은 LAN에서 상대 FTA를 실행하세요." : "설정에서 집 네트워크를 확인하세요."))
                            .frame(maxWidth:.infinity).padding(.vertical,8)
                    } else {
                        // Native horizontal scrolling responds to a finger drag without a visible scrollbar.
                        ScrollView(.horizontal) {
                            HStack(alignment:.top,spacing:16) {
                                ForEach(model.targets) { peer in
                                    Button { model.selectedPeer = peer.id } label: {
                                        VStack(spacing:10) {
                                            PeerIcon(mobile:peer.mobile).foregroundStyle(.white)
                                                .frame(width:70,height:66)
                                                .background(Color(white:0.12).opacity(0.8),in:RoundedRectangle(cornerRadius:20))
                                                .modifier(GlassSurface(radius:20))
                                                .overlay(RoundedRectangle(cornerRadius:20).stroke(.white.opacity(model.selectedPeer == peer.id ? 0.95 : 0),lineWidth:2))
                                                .overlay(alignment:.bottomTrailing) {
                                                    Circle().fill(peer.paired && peer.online ? Color.green : Color.orange)
                                                        .frame(width:9,height:9).overlay(Circle().stroke(Color(white:0.15),lineWidth:2)).offset(x:1,y:1)
                                                }
                                            Text(peer.display_name).font(.caption).foregroundStyle(.primary).lineLimit(2)
                                                .multilineTextAlignment(.center).frame(width:86)
                                        }
                                    }.buttonStyle(.plain).disabled(!peer.selectable || model.sending)
                                        .accessibilityLabel("\(peer.display_name), \(peer.status)")
                                        .accessibilityAddTraits(model.selectedPeer == peer.id ? .isSelected : [])
                                }
                            }.padding(4)
                        }.scrollIndicators(.hidden).accessibilityIdentifier("available-devices")
                    }
                }
                // Photos and Files are two freestanding actions, with no surrounding section box.
                HStack(spacing:14) {
                    mediaButton("사진",symbol:"photo.on.rectangle") { choosingPhotoSource = true }
                    mediaButton("파일",symbol:"folder") { picking = true }
                }.disabled(model.sending || model.importing)
                if model.importing { ProgressView("선택한 항목 준비 중").font(.footnote) }
                if !(model.photos + model.files).isEmpty {
                    VStack(alignment:.leading,spacing:12) {
                        ForEach(model.photos + model.files,id:\.self) { url in
                            Label(url.lastPathComponent,systemImage:model.photos.contains(url) ? "photo" : "doc").font(.subheadline).lineLimit(2)
                        }
                        HStack {
                            Button("선택 해제",role:.destructive) { model.clearPhotos(); model.clearFiles() }
                            Spacer()
                            Button { Task { await model.sendAttachments() } } label: { Label("전송",systemImage:"arrow.up.circle.fill") }
                                .accessibilityIdentifier("send-attachments").disabled(!model.canSend)
                        }.disabled(model.sending || model.importing)
                    }.padding(16).modifier(GlassSurface())
                }
                if model.sending || model.progress != nil {
                    VStack(alignment:.leading,spacing:12) {
                        if let progress = model.progress {
                            Text(progress.status)
                            if !progress.terminal { ProgressView(value:progress.fraction) }
                        } else { ProgressView("연결 및 등록 확인 중") }
                        if model.sending { Button("전송 취소",role:.destructive) { Task { await model.cancel() } } }
                    }.padding(16).modifier(GlassSurface())
                }
                if let notice = model.notice {
                    HStack { Text(notice).font(.footnote).foregroundStyle(.secondary); Spacer(); Button("닫기") { model.notice = nil } }
                }
            }.padding(20)
        }.scrollDismissesKeyboard(.interactively).scrollIndicators(.hidden)
        // The safe-area inset keeps all four lines and the send control above the system keyboard.
        .safeAreaInset(edge:.bottom,spacing:0) { composer.padding(.horizontal,20).padding(.vertical,12) }
        .background(GlassBackground())
        .navigationTitle("보내기")
        .toolbar { ToolbarItem(placement:.topBarTrailing) { Button { Task { await model.refreshDevices() } } label:{ Image(systemName:"arrow.clockwise") }.disabled(!model.running).accessibilityLabel("기기 새로고침") } }
        .refreshable { await model.refreshDevices() }
        .confirmationDialog("사진",isPresented:$choosingPhotoSource,titleVisibility:.visible) {
            Button("갤러리에서 선택") { pickingPhotos = true }
            Button("촬영") { Task { await openCamera() } }
            Button("취소",role:.cancel) {}
        }
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
    private var composer: some View {
        VStack(alignment:.leading,spacing:10) {
                    HStack(alignment:.bottom,spacing:10) {
                        // Native vertical TextField grows from one to four lines, then scrolls internally.
                        TextField("텍스트 또는 URL",text:$model.message,axis:.vertical)
                            .lineLimit(1...4).textInputAutocapitalization(.never).autocorrectionDisabled()
                            .accessibilityLabel("전송할 텍스트 또는 URL").accessibilityIdentifier("message-input")
                            .padding(.vertical,10)
                        Button { Task { await model.sendMessage() } } label: {
                            Image(systemName:"arrow.up").font(.system(size:17,weight:.semibold))
                                .foregroundStyle(.white).frame(width:36,height:36)
                                .background(Color(white:0.2),in:Circle())
                                .overlay(Circle().stroke(.white.opacity(0.3),lineWidth:0.7))
                                .frame(width:44,height:44)
                        }.buttonStyle(.plain).accessibilityLabel("메시지 전송").accessibilityIdentifier("send-message")
                            .disabled(!model.canSend || model.message.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty)
                            .opacity(model.canSend && !model.message.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty ? 1 : 0.4)
                    }.padding(.leading,16).padding(.trailing,6).padding(.vertical,4).modifier(GlassSurface())
                        .disabled(model.sending)
                }
    }
    private func mediaButton(_ title: String, symbol: String, action: @escaping () -> Void) -> some View {
        Button(action:action) { Label(title,systemImage:symbol).font(.body.weight(.medium)).foregroundStyle(.white).frame(maxWidth:.infinity).padding(.vertical,16) }
            .buttonStyle(.plain).background(Color(white:0.12).opacity(0.83),in:Capsule()).modifier(GlassSurface(radius:30))
    }
    private func openCamera() async {
        guard UIImagePickerController.isSourceTypeAvailable(.camera) else {
            model.error = "이 기기에서는 카메라를 사용할 수 없습니다"; return
        }
        let permitted: Bool
        switch AVCaptureDevice.authorizationStatus(for:.video) {
        case .authorized: permitted = true
        case .notDetermined: permitted = await AVCaptureDevice.requestAccess(for:.video)
        default: permitted = false
        }
        if permitted { takingPhoto = true }
        else { model.error = "카메라 접근이 허용되지 않았습니다. iOS 설정에서 FTA의 카메라 권한을 확인하세요." }
    }
}

struct HistoryView: View {
    @EnvironmentObject var model: AppModel
    @State private var filter = "all"
    var body: some View {
        List {
            Picker("표시",selection:$filter) { Text("전체").tag("all");Text("받음").tag("receive");Text("보냄").tag("send") }.pickerStyle(.segmented)
            if model.history.isEmpty { ContentUnavailableView("아직 전송 기록이 없습니다",systemImage:"tray",description:Text("파일·텍스트·URL의 송수신 결과가 여기에 표시됩니다.")) }
            ForEach(model.history.filter { filter == "all" || $0.direction == filter }) { record in
                Button { Task { await model.openRecord(record.id) } } label: {
                    HStack(spacing:12) {
                        Image(systemName:record.direction == "receive" ? "arrow.down.circle" : "arrow.up.circle").foregroundStyle(Color.accentColor)
                        VStack(alignment:.leading,spacing:4) {
                            Text(record.title).foregroundStyle(.primary).lineLimit(1)
                            Text("\(record.direction == "receive" ? "받음" : "보냄") · \(record.status)").font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(Date(timeIntervalSince1970:record.created_at ?? 0),style:.date).font(.caption2).foregroundStyle(.secondary)
                    }
                }
            }
        }.scrollContentBackground(.hidden).background(GlassBackground()).navigationTitle("기록").refreshable { await model.refreshActivity() }
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
            Section("전송 결과") {
                LabeledContent("상태",value:record.status)
                LabeledContent("방향",value:record.direction == "receive" ? "받음" : "보냄")
                LabeledContent("날짜") { Text(Date(timeIntervalSince1970:record.created_at ?? 0),format:.dateTime) }
                if let error = record.error { Text(error).foregroundStyle(.red).textSelection(.enabled) }
                if record.state == "unknown" { Button("수신 측 결과 다시 확인") { Task { await model.reconcile(record) } } }
            }
            if let body = record.body {
                Section(record.kind == "url" ? "URL" : "텍스트") {
                    Text(body).textSelection(.enabled)
                    Button("복사",systemImage:"doc.on.doc") { UIPasteboard.general.string = body }
                    // Only an explicit tap opens HTTP(S); stored strings never execute automatically.
                    if record.kind == "url", let url = URL(string:body), ["http","https"].contains(url.scheme?.lowercased() ?? ""), url.host != nil {
                        Link(destination:url) { Label("링크 열기",systemImage:"safari") }
                    }
                }
            }
            ForEach(Array(record.items.enumerated()),id:\.offset) { _,item in
                Section(item.saved_name ?? item.name) {
                    LabeledContent("크기",value:ByteCountFormatter.string(fromByteCount:Int64(item.size) ?? 0,countStyle:.file))
                    LabeledContent("상태",value:item.state == "completed" ? "완료" : item.state)
                    if let error = item.error { Text(error).foregroundStyle(.red) }
                    if record.direction == "receive", item.state == "completed" {
                        if let url = Sandbox.receivedURL(item.path) {
                            Button("미리보기",systemImage:"doc.text.magnifyingglass") { preview = url }
                            ShareLink(item:url) { Label("공유 · 파일에 저장",systemImage:"square.and.arrow.up") }
                        } else { Text("파일이 이동되었거나 삭제되었습니다.").foregroundStyle(.secondary) }
                    }
                }
            }
            Section { Button("기록 삭제",role:.destructive) { deleting = true } } footer:{ Text("기록을 삭제해도 이미 받은 파일은 유지됩니다.") }
        }.navigationTitle(record.title).navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement:.confirmationAction) { Button("닫기") { dismiss() } } }
            .quickLookPreview($preview)
            .confirmationDialog("이 기록을 삭제할까요?",isPresented:$deleting,titleVisibility:.visible) {
                Button("기록 삭제",role:.destructive) { Task { await model.deleteRecord(record) } }
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
                    Text("이 기기를 등록하면 같은 LAN에서 온라인 상태일 때 파일·텍스트·URL을 추가 승인 없이 받습니다.")
                    Text("등록 승인 전에는 전송 내용을 받지 않습니다.").foregroundStyle(.secondary)
                    LabeledContent("전송 종류",value:request.kind == "files" ? "파일 \(request.item_count)개" : (request.kind == "url" ? "URL" : "텍스트"))
                    LabeledContent("요청 만료") { Text(Date(timeIntervalSince1970:request.expires_at),style:.timer) }
                }
                Section {
                    Button("기기 등록 허용") { Task { await model.decide(request,accept:true) } }.disabled(model.deciding)
                    Button("거절",role:.destructive) { Task { await model.decide(request,accept:false) } }.disabled(model.deciding)
                }
            }.navigationTitle("기기 등록 요청").navigationBarTitleDisplayMode(.inline)
        }.presentationDetents([.medium,.large])
    }
}

struct SettingsView: View {
    @EnvironmentObject var model: AppModel
    @State private var removing: Peer?
    var body: some View {
        Form {
            Section {
                TextField("기기 이름",text:$model.name).textContentType(.name)
                Button("이름 저장") { Task { await model.saveName() } }
            } header:{ Text("이 iPhone") } footer:{ Text("FTA · File Transfer Anything") }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section {
                Picker("사용할 연결",selection:Binding(get:{model.selectedLAN},set:{model.selectLAN($0)})) {
                    if model.lans.isEmpty { Text("Wi-Fi 연결 없음").tag("") }
                    ForEach(model.lans) { lan in Text(lan.address).tag(lan.id) }
                }.disabled(model.running || model.starting)
                if let lan = model.lan { LabeledContent("허용 대역",value:lan.network).font(.footnote) }
                Toggle("신뢰하는 집 네트워크입니다",isOn:Binding(get:{model.confirmed},set:{model.setNetworkTrusted($0)}))
                    .disabled(model.starting || model.lan == nil)
                Toggle("송수신 사용",isOn:Binding(get:{model.receiveEnabled},set:{model.setReceiveEnabled($0)}))
                Label(model.serviceText,systemImage:model.running ? "checkmark.circle" : "pause.circle").foregroundStyle(.secondary)
            } header:{ Text("LAN 연결") } footer:{ Text("앱을 열어둔 동안 자동 수신합니다. 백그라운드·화면 잠금·연결 단절 시 수신을 중지하며, 신뢰한 연결로 돌아오면 복원합니다. 신뢰 설정은 표시된 허용 대역을 기준으로 기억합니다. 등록된 기기만 내용을 보낼 수 있습니다.") }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section {
                LabeledContent("저장 위치",value:"나의 iPhone / FTA / 받은 파일")
                Text("파일 앱에서 받은 파일을 확인하거나 이동할 수 있습니다. 앱을 삭제하면 앱 안의 파일도 삭제됩니다.").font(.footnote).foregroundStyle(.secondary)
            } header:{ Text("받은 파일") }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section("알림") {
                LabeledContent("권한",value:model.notificationStatus)
                Button("알림 허용 요청") { Task { await model.requestNotifications() } }
                Button("iOS 설정 열기") { if let url = URL(string:UIApplication.openSettingsURLString) { UIApplication.shared.open(url) } }
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
            Section("등록된 기기") {
                if !model.peers.contains(where:{$0.paired}) { Text("등록된 기기가 없습니다").foregroundStyle(.secondary) }
                ForEach(model.peers.filter(\.paired)) { peer in
                    HStack { PeerIcon(mobile:peer.mobile); Text(peer.display_name); Spacer(); Button("해제",role:.destructive) { removing = peer } }
                }
            }.listRowBackground(Rectangle().fill(.ultraThinMaterial))
        }.scrollContentBackground(.hidden).background(GlassBackground()).navigationTitle("설정")
            .confirmationDialog("기기 등록을 해제할까요?",isPresented:Binding(get:{removing != nil},set:{if !$0 {removing = nil}}),titleVisibility:.visible) {
                if let peer = removing { Button("등록 해제",role:.destructive) { Task { await model.unpair(peer); removing = nil } } }
            } message:{ Text("이 기기의 새 전송을 차단합니다. 다시 보내려면 수신자의 등록 승인이 필요합니다.") }
    }
}
