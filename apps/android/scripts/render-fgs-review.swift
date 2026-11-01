// Keep both real device recordings visible: a registration dialog alone cannot
// demonstrate the peer interaction required by Play's connectedDevice review.
import AppKit
import AVFoundation
import Foundation

guard CommandLine.arguments.count == 4 else {
    fatalError("Usage: render-fgs-review.swift phone.mp4 tablet.mp4 output.mp4")
}
let canvas = CGSize(width: 1920, height: 1440)
let inputURLs = CommandLine.arguments[1...2].map { URL(fileURLWithPath: $0) }
let destination = URL(fileURLWithPath: CommandLine.arguments[3])
let assets = inputURLs.map { AVURLAsset(url: $0) }
let duration = assets.map(\.duration).min(by: { CMTimeCompare($0, $1) < 0 })!
guard duration.seconds > 20 else { fatalError("Missing full interaction recordings") }
let composition = AVMutableComposition()
let instruction = AVMutableVideoCompositionInstruction()
instruction.timeRange = CMTimeRange(start: .zero, duration: duration)
var instructions: [AVVideoCompositionLayerInstruction] = []
for (index, asset) in assets.enumerated() {
    guard let source = asset.tracks(withMediaType: .video).first,
          let track = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid) else {
        fatalError("The recording has no actual video track")
    }
    try track.insertTimeRange(instruction.timeRange, of: source, at: .zero)
    let transformed = source.naturalSize.applying(source.preferredTransform)
    let size = CGSize(width: abs(transformed.width), height: abs(transformed.height))
    let panel = CGRect(x: CGFloat(index) * 960 + 40, y: 160, width: 880, height: 1220)
    let scale = min(panel.width / size.width, panel.height / size.height)
    let x = panel.midX - size.width * scale / 2
    let y = panel.midY - size.height * scale / 2
    let layer = AVMutableVideoCompositionLayerInstruction(assetTrack: track)
    layer.setTransform(source.preferredTransform.concatenating(CGAffineTransform(scaleX: scale, y: scale)).concatenating(CGAffineTransform(translationX: x, y: y)), at: .zero)
    instructions.append(layer)
}
instruction.layerInstructions = instructions
let video = AVMutableVideoComposition()
video.instructions = [instruction]
video.renderSize = canvas
video.frameDuration = CMTime(value: 1, timescale: 30)
let parent = CALayer()
parent.frame = CGRect(origin: .zero, size: canvas)
parent.backgroundColor = NSColor(calibratedWhite: 0.96, alpha: 1).cgColor
let imageLayer = CALayer()
imageLayer.frame = parent.frame
parent.addSublayer(imageLayer)
// Draw labels into a native bitmap rather than relying on lazy CATextLayer
// display in a headless export process; frame inspection exposed empty labels.
let overlayBitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(canvas.width), pixelsHigh: Int(canvas.height), bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
let graphics = NSGraphicsContext(bitmapImageRep: overlayBitmap)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = graphics
NSColor(calibratedWhite: 0.96, alpha: 1).setFill()
CGRect(x: 0, y: 1290, width: canvas.width, height: 150).fill()
CGRect(x: 0, y: 0, width: canvas.width, height: 50).fill()
func title(_ value: String, frame: CGRect, size: CGFloat) {
    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = .center
    (value as NSString).draw(in: frame, withAttributes: [
        .font: NSFont.systemFont(ofSize: size, weight: .semibold),
        .foregroundColor: NSColor(calibratedRed: 0.13, green: 0.26, blue: 0.39, alpha: 1),
        .paragraphStyle: paragraph,
    ])
}
title("FTA · Connected-device foreground service", frame: CGRect(x: 0, y: 1365, width: 1920, height: 55), size: 42)
title("Receiving phone", frame: CGRect(x: 0, y: 1300, width: 960, height: 45), size: 32)
title("Sending tablet", frame: CGRect(x: 960, y: 1300, width: 960, height: 45), size: 32)
// Only labels are composed. Native app pixels and system screens remain complete;
// these are independent real recordings, not a reconstructed or simulated UI.
title("Actual Android emulator LAN interaction · No injected peers or results", frame: CGRect(x: 0, y: 8, width: 1920, height: 40), size: 24)
NSGraphicsContext.restoreGraphicsState()
let overlay = CALayer()
overlay.frame = parent.frame
overlay.contents = overlayBitmap.cgImage
parent.addSublayer(overlay)
video.animationTool = AVVideoCompositionCoreAnimationTool(postProcessingAsVideoLayer: imageLayer, in: parent)
if FileManager.default.fileExists(atPath: destination.path) { try FileManager.default.removeItem(at: destination) }
guard let exporter = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetHighestQuality) else { fatalError("No native video exporter") }
exporter.videoComposition = video
exporter.outputURL = destination
exporter.outputFileType = .mp4
exporter.shouldOptimizeForNetworkUse = true
exporter.exportAsynchronously {}
while exporter.status == .waiting || exporter.status == .exporting || exporter.status == .unknown {
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
}
guard exporter.status == .completed else { fatalError("Video export failed: \(String(describing: exporter.error))") }
let verified = AVURLAsset(url: destination)
guard verified.duration.seconds > 20, verified.tracks(withMediaType: .video).count == 1 else { fatalError("Invalid exported recording") }
// Inspect actual exported frames so a valid MP4 alone cannot hide cropped UI or
// a blank Core Animation composition. These PNGs are review evidence, not UI art.
let generator = AVAssetImageGenerator(asset: verified)
generator.appliesPreferredTrackTransform = true
for (index, fraction) in [0.1, 0.45, 0.7, 0.92].enumerated() {
    let frame = try generator.copyCGImage(at: CMTime(seconds: verified.duration.seconds * fraction, preferredTimescale: 600), actualTime: nil)
    let bitmap = NSBitmapImageRep(cgImage: frame)
    let path = destination.deletingLastPathComponent().appendingPathComponent("review-frame-\(index + 1).png")
    try bitmap.representation(using: .png, properties: [:])!.write(to: path)
}
print("Created actual FGS demonstration: \(destination.path) (\(Int(verified.duration.seconds)) seconds)")
