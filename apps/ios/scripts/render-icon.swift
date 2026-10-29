import CoreGraphics
import ImageIO
import Foundation

// Reuse the desktop's actual pixels; a standalone bitmap context avoids AppKit's absent display context.
// iOS icons must be opaque, so only the desktop's transparent outer canvas is flattened onto white.
let side = 1024
let source = CGImageSourceCreateWithURL(URL(fileURLWithPath:CommandLine.arguments[1]) as CFURL,nil)!
let image = CGImageSourceCreateImageAtIndex(source,0,nil)!
let context = CGContext(data:nil,width:side,height:side,bitsPerComponent:8,bytesPerRow:side*4,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.noneSkipLast.rawValue)!
context.setFillColor(CGColor(gray:1,alpha:1))
context.fill(CGRect(x:0,y:0,width:side,height:side))
context.interpolationQuality = .high
context.draw(image,in:CGRect(x:0,y:0,width:side,height:side))
let output = CGImageDestinationCreateWithURL(URL(fileURLWithPath:CommandLine.arguments[2]) as CFURL,"public.png" as CFString,1,nil)!
CGImageDestinationAddImage(output,context.makeImage()!,nil)
precondition(CGImageDestinationFinalize(output),"Could not write the iOS icon")
