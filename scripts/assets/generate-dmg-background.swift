#!/usr/bin/env swift

import AppKit
import Foundation

let args = CommandLine.arguments

guard args.count >= 2 else {
  fputs("Usage: generate-dmg-background.swift <output-png>\n", stderr)
  exit(2)
}

let outputURL = URL(fileURLWithPath: args[1])
let width = CGFloat(660)
let height = CGFloat(400)
let canvasSize = NSSize(width: width, height: height)

try FileManager.default.createDirectory(
  at: outputURL.deletingLastPathComponent(),
  withIntermediateDirectories: true
)

guard let rep = NSBitmapImageRep(
  bitmapDataPlanes: nil,
  pixelsWide: Int(width),
  pixelsHigh: Int(height),
  bitsPerSample: 8,
  samplesPerPixel: 4,
  hasAlpha: true,
  isPlanar: false,
  colorSpaceName: .deviceRGB,
  bytesPerRow: 0,
  bitsPerPixel: 0
) else {
  fputs("Failed to allocate DMG background bitmap\n", stderr)
  exit(1)
}

rep.size = canvasSize

guard let context = NSGraphicsContext(bitmapImageRep: rep) else {
  fputs("Failed to create DMG background graphics context\n", stderr)
  exit(1)
}

func topLeftRect(x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat) -> NSRect {
  NSRect(x: x, y: canvasSize.height - y - height, width: width, height: height)
}

func topLeftPoint(x: CGFloat, y: CGFloat) -> NSPoint {
  NSPoint(x: x, y: canvasSize.height - y)
}

func drawCentered(_ text: String, top: CGFloat, attributes: [NSAttributedString.Key: Any]) {
  let value = text as NSString
  let textSize = value.size(withAttributes: attributes)
  value.draw(
    at: NSPoint(x: (canvasSize.width - textSize.width) / 2, y: canvasSize.height - top - textSize.height),
    withAttributes: attributes
  )
}

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = context
context.imageInterpolation = .high

NSColor(calibratedRed: 0.965, green: 0.973, blue: 0.984, alpha: 1).setFill()
NSRect(origin: .zero, size: canvasSize).fill()

let panelFill = NSColor(calibratedRed: 1, green: 1, blue: 1, alpha: 0.68)
let panelStroke = NSColor(calibratedRed: 0.81, green: 0.84, blue: 0.89, alpha: 1)

for rect in [
  topLeftRect(x: 82, y: 92, width: 196, height: 210),
  topLeftRect(x: 382, y: 92, width: 196, height: 210),
] {
  let panel = NSBezierPath(roundedRect: rect, xRadius: 22, yRadius: 22)
  panelFill.setFill()
  panel.fill()
  panelStroke.setStroke()
  panel.lineWidth = 1.5
  panel.stroke()
}

let titleAttributes: [NSAttributedString.Key: Any] = [
  .font: NSFont.systemFont(ofSize: 26, weight: .semibold),
  .foregroundColor: NSColor(calibratedRed: 0.10, green: 0.13, blue: 0.18, alpha: 1),
]
drawCentered("安装 H-VibeRec", top: 34, attributes: titleAttributes)

let arrowColor = NSColor(calibratedRed: 0.10, green: 0.42, blue: 0.78, alpha: 1)
arrowColor.setStroke()

let arrow = NSBezierPath()
arrow.move(to: topLeftPoint(x: 286, y: 198))
arrow.line(to: topLeftPoint(x: 382, y: 198))
arrow.lineWidth = 8
arrow.lineCapStyle = .round
arrow.stroke()

arrowColor.setFill()
let arrowHead = NSBezierPath()
arrowHead.move(to: topLeftPoint(x: 398, y: 198))
arrowHead.line(to: topLeftPoint(x: 372, y: 178))
arrowHead.line(to: topLeftPoint(x: 372, y: 218))
arrowHead.close()
arrowHead.fill()

let instructionAttributes: [NSAttributedString.Key: Any] = [
  .font: NSFont.systemFont(ofSize: 20, weight: .medium),
  .foregroundColor: NSColor(calibratedRed: 0.18, green: 0.22, blue: 0.29, alpha: 1),
]
drawCentered("将 H-VibeRec 拖到 Applications 文件夹", top: 322, attributes: instructionAttributes)

let captionAttributes: [NSAttributedString.Key: Any] = [
  .font: NSFont.systemFont(ofSize: 13, weight: .regular),
  .foregroundColor: NSColor(calibratedRed: 0.42, green: 0.46, blue: 0.53, alpha: 1),
]
drawCentered("Drag the app icon onto the system Applications shortcut", top: 354, attributes: captionAttributes)

NSGraphicsContext.restoreGraphicsState()

guard let png = rep.representation(using: .png, properties: [.compressionFactor: 1.0]) else {
  fputs("Failed to encode DMG background PNG\n", stderr)
  exit(1)
}

try png.write(to: outputURL, options: .atomic)
print("Generated DMG background: \(outputURL.path)")
