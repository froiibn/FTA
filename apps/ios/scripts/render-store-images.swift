// Compose verified native captures with branded copy; never redraw device lists or transfer state.
import AppKit
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

struct StoreCopy: Decodable { let headlines: [String]; let captions: [String] }
func color(_ hex: UInt32) -> NSColor { NSColor(red:CGFloat((hex>>16)&255)/255,green:CGFloat((hex>>8)&255)/255,blue:CGFloat(hex&255)/255,alpha:1) }

@main struct RenderStoreImages {
    @MainActor static func main() throws {
        let root=URL(fileURLWithPath:FileManager.default.currentDirectoryPath)
        let metadata=try JSONDecoder().decode([String:StoreCopy].self,from:Data(contentsOf:root.appendingPathComponent("apps/ios/Store/metadata.json")))
        let assetRoot=root.appendingPathComponent("output/ios-store-assets")
        let mark=NSImage(contentsOf:root.appendingPathComponent("apps/ios/FTA/Assets.xcassets/AppMark.imageset/AppMark.png"))!
        let scenes=["send","photos","history"]
        for family in ["phone","ipad"] {
            for (locale,copy) in metadata.sorted(by:{$0.key<$1.key}) {
                guard copy.headlines.count == scenes.count, copy.captions.count == scenes.count else { throw NSError(domain:"InvalidStoreCopy",code:1) }
                for (index,scene) in scenes.enumerated() {
                    let source=assetRoot.appendingPathComponent("raw/\(family)/store-\(locale)-\(scene).png")
                    guard let screen=NSImage(contentsOf:source), let image=screen.cgImage(forProposedRect:nil,context:nil,hints:nil) else { throw NSError(domain:"MissingVerifiedCapture",code:1,userInfo:[NSLocalizedDescriptionKey:source.path]) }
                    let width=image.width, height=image.height, w=CGFloat(width), h=CGFloat(height)
                    // Opaque RGB prevents App Store alpha-channel rejection; the underlying UI stays unmodified.
                    let context=CGContext(data:nil,width:width,height:height,bitsPerComponent:8,bytesPerRow:0,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.noneSkipLast.rawValue)!
                    NSGraphicsContext.saveGraphicsState(); NSGraphicsContext.current=NSGraphicsContext(cgContext:context,flipped:false)
                    NSGradient(colors:[color(0x081523),color(0x12364A),color(0x0A1C2A)])!.draw(in:NSRect(x:0,y:0,width:w,height:h),angle:-55)
                    func rect(_ x:CGFloat,_ top:CGFloat,_ width:CGFloat,_ height:CGFloat)->NSRect { NSRect(x:x,y:h-top-height,width:width,height:height) }
                    func text(_ value:String,top:CGFloat,size:CGFloat,weight:NSFont.Weight,tint:NSColor,maxHeight:CGFloat) {
                        let paragraph=NSMutableParagraphStyle(); paragraph.lineSpacing=size*0.1
                        var fontSize=size
                        while fontSize>size*0.7 {
                            let attributed=NSAttributedString(string:value,attributes:[.font:NSFont.systemFont(ofSize:fontSize,weight:weight),.paragraphStyle:paragraph])
                            if attributed.boundingRect(with:NSSize(width:w*0.85,height:.greatestFiniteMagnitude),options:[.usesLineFragmentOrigin,.usesFontLeading]).height<=maxHeight { break }
                            fontSize-=1
                        }
                        (value as NSString).draw(in:rect(w*0.075,top,w*0.85,maxHeight),withAttributes:[.font:NSFont.systemFont(ofSize:fontSize,weight:weight),.foregroundColor:tint,.paragraphStyle:paragraph])
                    }
                    let brandSize=w*0.05
                    mark.draw(in:rect(w*0.075,h*0.035,brandSize,brandSize))
                    ("FTA" as NSString).draw(in:rect(w*0.075+brandSize*1.25,h*0.035,w*0.4,brandSize*1.2),withAttributes:[.font:NSFont.systemFont(ofSize:w*0.045,weight:.bold),.foregroundColor:NSColor.white])
                    text(copy.headlines[index],top:h*0.095,size:w*(family.hasPrefix("phone") ? 0.069 : 0.057),weight:.bold,tint:.white,maxHeight:h*0.15)
                    text(copy.captions[index],top:h*0.25,size:w*0.029,weight:.medium,tint:color(0xC8DDE8),maxHeight:h*0.045)
                    let top=h*0.31, availableHeight=h*0.645, availableWidth=w*(family.hasPrefix("phone") ? 0.78 : 0.87)
                    let scale=min(availableWidth/CGFloat(image.width),availableHeight/CGFloat(image.height))
                    let deviceW=CGFloat(image.width)*scale,deviceH=CGFloat(image.height)*scale
                    let deviceRect=rect((w-deviceW)/2,top,deviceW,deviceH)
                    let outline=NSBezierPath(roundedRect:deviceRect,xRadius:w*0.035,yRadius:w*0.035)
                    NSGraphicsContext.saveGraphicsState()
                    let shadow=NSShadow();shadow.shadowColor=NSColor.black.withAlphaComponent(0.38);shadow.shadowBlurRadius=w*0.035;shadow.shadowOffset=NSSize(width:0,height:-w*0.015);shadow.set()
                    NSColor.black.setFill();outline.fill();NSGraphicsContext.restoreGraphicsState()
                    NSGraphicsContext.saveGraphicsState();outline.addClip();screen.draw(in:deviceRect);NSGraphicsContext.restoreGraphicsState()
                    NSColor.white.withAlphaComponent(0.25).setStroke();outline.lineWidth=max(1,w*0.0015);outline.stroke()
                    let footer="File Transfer Anything" as NSString
                    footer.draw(in:rect(w*0.075,h*0.972,w*0.85,h*0.025),withAttributes:[.font:NSFont.systemFont(ofSize:w*0.022,weight:.medium),.foregroundColor:color(0xA8C4D2)])
                    NSGraphicsContext.restoreGraphicsState()
                    let destination=assetRoot.appendingPathComponent("promotional/\(family)/\(locale)/\(index+1)-\(scene).png")
                    try FileManager.default.createDirectory(at:destination.deletingLastPathComponent(),withIntermediateDirectories:true)
                    let writer=CGImageDestinationCreateWithURL(destination as CFURL,UTType.png.identifier as CFString,1,nil)!
                    CGImageDestinationAddImage(writer,context.makeImage()!,nil)
                    guard CGImageDestinationFinalize(writer) else { throw NSError(domain:"StoreImage",code:1) }
                    print("\(family)/\(locale)/\(index+1)-\(scene).png \(width)x\(height)")
                }
            }
        }
    }
}
