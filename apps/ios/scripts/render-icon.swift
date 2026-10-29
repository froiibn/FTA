import AppKit
// Reuse the desktop transfer arrows, with an opaque edge-to-edge canvas for iOS's own icon mask.
let side = 1024
let image = NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:side,pixelsHigh:side,bitsPerSample:8,samplesPerPixel:3,hasAlpha:false,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep:image)
NSColor(calibratedRed:36/255,green:70/255,blue:107/255,alpha:1).setFill()
NSBezierPath(rect:NSRect(x:0,y:0,width:side,height:side)).fill()
NSColor.white.setStroke()
let path = NSBezierPath(); path.lineWidth = 58; path.lineCapStyle = .round; path.lineJoinStyle = .round
func move(_ x:Double,_ y:Double) { path.move(to:NSPoint(x:x,y:1024-y)) }
func line(_ x:Double,_ y:Double) { path.line(to:NSPoint(x:x,y:1024-y)) }
move(264,352); line(760,352); move(632,224); line(760,352); line(632,480)
move(760,672); line(264,672); move(392,544); line(264,672); line(392,800)
path.stroke(); NSGraphicsContext.restoreGraphicsState()
try image.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:CommandLine.arguments[1]))
