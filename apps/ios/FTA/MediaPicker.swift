import SwiftUI
import PhotosUI
import UniformTypeIdentifiers
import AVFoundation

// The system photo picker grants only the selected images; FTA never enumerates the photo library.
struct PhotoLibraryPicker: UIViewControllerRepresentable {
    let selected: ([NSItemProvider]) -> Void
    func makeUIViewController(context: Context) -> PHPickerViewController {
        var configuration = PHPickerConfiguration()
        configuration.filter = .images
        configuration.selectionLimit = PhotoStaging.maximumSelection
        configuration.preferredAssetRepresentationMode = .current
        let picker = PHPickerViewController(configuration: configuration)
        picker.delegate = context.coordinator
        return picker
    }
    func updateUIViewController(_ controller: PHPickerViewController, context: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator(selected) }
    final class Coordinator: NSObject, PHPickerViewControllerDelegate {
        let selected: ([NSItemProvider]) -> Void
        init(_ selected: @escaping ([NSItemProvider]) -> Void) { self.selected = selected }
        func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
            selected(results.map(\.itemProvider))
        }
    }
}

// Capture is explicit and full-screen. It does not save to Photos or request broad library access.
struct CameraPicker: UIViewControllerRepresentable {
    let captured: (UIImage?) -> Void
    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.mediaTypes = [UTType.image.identifier]
        picker.cameraCaptureMode = .photo
        picker.delegate = context.coordinator
        return picker
    }
    func updateUIViewController(_ controller: UIImagePickerController, context: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator(captured) }
    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let captured: (UIImage?) -> Void
        init(_ captured: @escaping (UIImage?) -> Void) { self.captured = captured }
        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) { captured(nil) }
        func imagePickerController(_ picker: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            captured(info[.originalImage] as? UIImage)
        }
    }
}

// Copy inside the provider callback: its temporary file is not guaranteed to survive callback return.
// All image bytes stay in native storage and subsequently use the same Rust streaming/grant checks.
enum PhotoStaging {
    static let maximumSelection = 100
    static let captureJPEGQuality: CGFloat = 0.95
    static func session() throws -> URL {
        try Sandbox.prepare()
        let directory = Sandbox.outgoing.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }
    static func copy(_ provider: NSItemProvider, into directory: URL) async throws -> URL {
        guard let identifier = provider.registeredTypeIdentifiers.first(where: { UTType($0)?.conforms(to: .image) == true }) else {
            throw EngineFailure(code:"INVALID_MEDIA", message:"선택한 사진을 파일로 읽을 수 없습니다")
        }
        // Capture immutable metadata rather than the non-Sendable provider in its background callback.
        let suggestedName = provider.suggestedName ?? "사진"
        return try await withCheckedThrowingContinuation { continuation in
            provider.loadFileRepresentation(forTypeIdentifier: identifier) { source, error in
                do {
                    if let error { throw error }
                    guard let source else { throw EngineFailure(code:"NOT_FOUND", message:"사진 원본을 준비하지 못했습니다") }
                    let values = try source.resourceValues(forKeys:[.isRegularFileKey, .isSymbolicLinkKey])
                    guard values.isRegularFile == true, values.isSymbolicLink != true else { throw EngineFailure(code:"INVALID_MEDIA",message:"일반 사진 파일만 보낼 수 있습니다") }
                    let slot = directory.appendingPathComponent(UUID().uuidString, isDirectory: true)
                    try FileManager.default.createDirectory(at:slot,withIntermediateDirectories:true)
                    let suggested = suggestedName as NSString
                    var name = suggested.lastPathComponent
                    if name.isEmpty || name == "." || name == ".." { name = "사진" }
                    if (name as NSString).pathExtension.isEmpty {
                        name += "." + (UTType(identifier)?.preferredFilenameExtension ?? source.pathExtension)
                    }
                    let destination = slot.appendingPathComponent(name)
                    try FileManager.default.copyItem(at:source,to:destination)
                    continuation.resume(returning:destination)
                } catch { continuation.resume(throwing:error) }
            }
        }
    }
    static func capture(_ image: UIImage, into directory: URL) throws -> URL {
        guard let data = image.jpegData(compressionQuality:captureJPEGQuality) else { throw EngineFailure(code:"INVALID_MEDIA",message:"촬영한 사진을 준비하지 못했습니다") }
        let slot = directory.appendingPathComponent(UUID().uuidString,isDirectory:true)
        try FileManager.default.createDirectory(at:slot,withIntermediateDirectories:true)
        let destination = slot.appendingPathComponent("촬영한 사진.jpg")
        try data.write(to:destination,options:[.atomic,.completeFileProtection])
        return destination
    }
}
