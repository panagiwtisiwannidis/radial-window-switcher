import AppKit

let size: CGFloat = 1024
let output = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon_1024.png"

let image = NSImage(size: NSSize(width: size, height: size), flipped: false) { _ in
    guard let context = NSGraphicsContext.current?.cgContext else { return false }
    let center = CGPoint(x: size / 2, y: size / 2)

    let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
    let tilePath = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)
    context.saveGState()
    context.addPath(tilePath)
    context.clip()
    let background = CGGradient(
        colorsSpace: CGColorSpaceCreateDeviceRGB(),
        colors: [CGColor(red: 0.12, green: 0.14, blue: 0.28, alpha: 1), CGColor(red: 0.03, green: 0.04, blue: 0.10, alpha: 1)] as CFArray,
        locations: [0, 1]
    )!
    context.drawLinearGradient(background, start: CGPoint(x: 0, y: tile.maxY), end: CGPoint(x: 0, y: tile.minY), options: [])
    context.restoreGState()

    func sector(start: CGFloat, end: CGFloat, inner: CGFloat, outer: CGFloat, gap: CGFloat) -> CGPath {
        let path = CGMutablePath()
        let a0 = .pi / 2 - start - gap / outer
        let a1 = .pi / 2 - end + gap / outer
        path.addArc(center: center, radius: outer, startAngle: a0, endAngle: a1, clockwise: true)
        path.addArc(center: center, radius: inner, startAngle: .pi / 2 - end + gap / inner,
                    endAngle: .pi / 2 - start - gap / inner, clockwise: false)
        path.closeSubpath()
        return path
    }

    func ring(count: Int, inner: CGFloat, outer: CGFloat, highlighted: Int?) {
        let step = 2 * CGFloat.pi / CGFloat(count)
        for i in 0..<count {
            let start = CGFloat(i) * step - step / 2
            let path = sector(start: start, end: start + step, inner: inner, outer: outer, gap: 7)
            context.addPath(path)
            if i == highlighted {
                context.setFillColor(CGColor(red: 0.30, green: 0.56, blue: 1.0, alpha: 1))
            } else {
                context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 0.16))
            }
            context.fillPath()
            context.addPath(path)
            context.setStrokeColor(CGColor(red: 1, green: 1, blue: 1, alpha: 0.35))
            context.setLineWidth(2)
            context.strokePath()
        }
    }

    ring(count: 8, inner: 232, outer: 340, highlighted: 1)
    ring(count: 4, inner: 118, outer: 220, highlighted: nil)

    context.addEllipse(in: CGRect(x: center.x - 82, y: center.y - 82, width: 164, height: 164))
    context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 0.9))
    context.fillPath()
    return true
}

guard let tiff = image.tiffRepresentation,
      let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:])
else { fatalError("Couldn't render the icon") }
try png.write(to: URL(fileURLWithPath: output))
