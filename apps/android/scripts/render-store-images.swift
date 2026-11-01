// Branded Play assets compose only complete verified Android captures; no device, result or app control is redrawn.
import AppKit
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
struct Copy:Decodable {let headlines:[String];let captions:[String]}
func color(_ value:UInt32)->NSColor {NSColor(red:CGFloat((value>>16)&255)/255,green:CGFloat((value>>8)&255)/255,blue:CGFloat(value&255)/255,alpha:1)}
@main struct Render {
 @MainActor static func main()throws {
  let root=URL(fileURLWithPath:FileManager.default.currentDirectoryPath),out=root.appendingPathComponent("output/android-play-v7/assets")
  let metadata=try JSONDecoder().decode([String:Copy].self,from:Data(contentsOf:root.appendingPathComponent("apps/android/Store/metadata.json")))
  let mark=NSImage(contentsOf:root.appendingPathComponent("apps/android/app/src/main/res/drawable-nodpi/app_mark.png"))!
  func image(_ width:Int,_ height:Int,_ path:URL,_ draw:()->Void)throws {
   let ctx=CGContext(data:nil,width:width,height:height,bitsPerComponent:8,bytesPerRow:0,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.noneSkipLast.rawValue)!
   NSGraphicsContext.saveGraphicsState();NSGraphicsContext.current=NSGraphicsContext(cgContext:ctx,flipped:false);draw();NSGraphicsContext.restoreGraphicsState()
   try FileManager.default.createDirectory(at:path.deletingLastPathComponent(),withIntermediateDirectories:true)
   guard let output=CGImageDestinationCreateWithURL(path as CFURL,UTType.png.identifier as CFString,1,nil) else {throw NSError(domain:"PNGOutput",code:1)}
   CGImageDestinationAddImage(output,ctx.makeImage()!,nil);guard CGImageDestinationFinalize(output) else {throw NSError(domain:"PNGFinalize",code:1)}
  }
  func text(_ value:String,_ rect:NSRect,_ size:CGFloat,_ weight:NSFont.Weight,_ tint:NSColor){
   let paragraph=NSMutableParagraphStyle();paragraph.lineSpacing=size*0.08;paragraph.lineBreakMode = .byWordWrapping
   var point=size
   while point>size*0.62 {let measured=(value as NSString).boundingRect(with:NSSize(width:rect.width,height:.greatestFiniteMagnitude),options:[.usesLineFragmentOrigin,.usesFontLeading],attributes:[.font:NSFont.systemFont(ofSize:point,weight:weight),.paragraphStyle:paragraph]);if measured.height<=rect.height{break};point-=1}
   (value as NSString).draw(in:rect,withAttributes:[.font:NSFont.systemFont(ofSize:point,weight:weight),.foregroundColor:tint,.paragraphStyle:paragraph])
  }
  // The Play icon is an opaque square; Google supplies its own external mask instead of receiving premasked corners.
  try image(512,512,out.appendingPathComponent("icon-512.png")){color(0x24466B).setFill();NSRect(x:0,y:0,width:512,height:512).fill();mark.draw(in:NSRect(x:0,y:0,width:512,height:512))}
  let families=Array(CommandLine.arguments.dropFirst()).isEmpty ? ["phone","tablet10"]:Array(CommandLine.arguments.dropFirst())
  let scenes=["send","photos","history","settings"]
  for(locale,copy) in metadata.sorted(by:{$0.key<$1.key}){
   try image(1024,500,out.appendingPathComponent("\(locale)/feature-1024x500.png")){
    color(0x14283D).setFill();NSRect(x:0,y:0,width:1024,height:500).fill();mark.draw(in:NSRect(x:56,y:306,width:128,height:128));text("FTA",NSRect(x:216,y:354,width:720,height:80),56,.bold,.white);text("File Transfer Anything",NSRect(x:218,y:312,width:720,height:40),26,.medium,color(0xC9DCEC));text(copy.headlines[0],NSRect(x:56,y:100,width:912,height:180),62,.bold,.white);text(copy.captions[0],NSRect(x:56,y:40,width:912,height:42),26,.medium,color(0xC9DCEC))
   }
   for family in families {
    for(index,scene) in scenes.enumerated(){
     let source=root.appendingPathComponent("output/android-play-v7/raw/\(family)/\(locale)-\(scene).png")
     guard let screen=NSImage(contentsOf:source),let original=screen.cgImage(forProposedRect:nil,context:nil,hints:nil) else {throw NSError(domain:"MissingActualCapture",code:1,userInfo:[NSLocalizedDescriptionKey:source.path])}
     let width=family=="phone" ? 1080:1440,height=family=="phone" ? 1920:2560,w=CGFloat(width),h=CGFloat(height)
     try image(width,height,out.appendingPathComponent("\(locale)/\(family)-\(index+1)-\(scene).png")){
      color(0x14283D).setFill();NSRect(x:0,y:0,width:w,height:h).fill()
      let logo=w*0.055;mark.draw(in:NSRect(x:w*0.065,y:h-w*0.055-logo,width:logo,height:logo));text("FTA",NSRect(x:w*0.065+logo*1.3,y:h-w*0.055-logo,width:w*0.65,height:logo*1.1),w*0.05,.bold,.white)
      text(copy.headlines[index],NSRect(x:w*0.065,y:h*0.825,width:w*0.87,height:h*0.105),w*0.061,.bold,.white);text(copy.captions[index],NSRect(x:w*0.065,y:h*0.787,width:w*0.87,height:h*0.032),w*0.026,.medium,color(0xC9DCEC))
      let availableHeight=h*0.74,availableWidth=w*0.91,scale=min(availableWidth/CGFloat(original.width),availableHeight/CGFloat(original.height));let sw=CGFloat(original.width)*scale,sh=CGFloat(original.height)*scale
      let rect=NSRect(x:(w-sw)/2,y:h*0.025+(availableHeight-sh)/2,width:sw,height:sh)
      // Fit the entire native buffer including system bars. Cropping or changing its aspect would misrepresent the UI.
      screen.draw(in:rect);color(0x60758B).setStroke();NSBezierPath(rect:rect).stroke()
     }
    }
   }
  }
  print("Play icon +10 feature graphics +40 screenshots per requested family rendered from actual Android buffers")
 }
}
