import CoreGraphics
import ImageIO
import Foundation

// Reuse the desktop's actual pixels; a standalone bitmap context avoids AppKit's absent display context.
// iOS icons must be opaque, so only the desktop's transparent outer canvas is filled with the shared black tile color so iOS can apply its own mask.
let side = 1024
let source = CGImageSourceCreateWithURL(URL(fileURLWithPath:CommandLine.arguments[1]) as CFURL,nil)!
let image = CGImageSourceCreateImageAtIndex(source,0,nil)!
let context = CGContext(data:nil,width:side,height:side,bitsPerComponent:8,bytesPerRow:side*4,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.noneSkipLast.rawValue)!
context.setFillColor(CGColor(red:8.0/255,green:9.0/255,blue:13.0/255,alpha:1))
context.fill(CGRect(x:0,y:0,width:side,height:side))
context.interpolationQuality = .high
context.draw(image,in:CGRect(x:0,y:0,width:side,height:side))
let output = CGImageDestinationCreateWithURL(URL(fileURLWithPath:CommandLine.arguments[2]) as CFURL,"public.png" as CFString,1,nil)!
CGImageDestinationAddImage(output,context.makeImage()!,nil)
precondition(CGImageDestinationFinalize(output),"Could not write the iOS icon")

// The scrolling brand header uses the same desktop source, retaining alpha for native material backgrounds.
if CommandLine.arguments.count > 3 {
    try Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1])).write(to:URL(fileURLWithPath:CommandLine.arguments[3]),options:.atomic)
}
