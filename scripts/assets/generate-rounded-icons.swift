#!/usr/bin/env swift

import AppKit
import Foundation

struct IconTarget {
  let path: String
  let size: Int
}

let args = CommandLine.arguments

guard args.count >= 4 else {
  fputs("Usage: generate-rounded-icons.swift <source-png> <icons-dir> <rounded-source-out>\n", stderr)
  exit(2)
}

let sourcePath = args[1]
let iconsDir = args[2]
let roundedSourceOut = args[3]
let fileManager = FileManager.default
let cornerRatio = CGFloat(224.0 / 1024.0)
let dockContentScale = CGFloat(860.0 / 1024.0)

guard let sourceImage = NSImage(contentsOfFile: sourcePath) else {
  fputs("Failed to read source image: \(sourcePath)\n", stderr)
  exit(1)
}

func pngSize(at path: String) -> Int? {
  guard let image = NSImage(contentsOfFile: path),
        let rep = image.representations.first else {
    return nil
  }

  return min(rep.pixelsWide, rep.pixelsHigh)
}

func renderRoundedPng(size: Int) -> Data? {
  guard let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: size,
    pixelsHigh: size,
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bytesPerRow: 0,
    bitsPerPixel: 0
  ) else {
    return nil
  }

  rep.size = NSSize(width: size, height: size)

  guard let context = NSGraphicsContext(bitmapImageRep: rep) else {
    return nil
  }

  NSGraphicsContext.saveGraphicsState()
  NSGraphicsContext.current = context
  context.imageInterpolation = .high

  let rect = NSRect(x: 0, y: 0, width: size, height: size)
  NSColor.clear.setFill()
  rect.fill()

  let imageSize = CGFloat(size) * dockContentScale
  let imageOrigin = (CGFloat(size) - imageSize) / 2.0
  let imageRect = NSRect(x: imageOrigin, y: imageOrigin, width: imageSize, height: imageSize)
  let radius = imageSize * cornerRatio
  let clipPath = NSBezierPath(roundedRect: imageRect, xRadius: radius, yRadius: radius)
  clipPath.addClip()

  sourceImage.draw(
    in: imageRect,
    from: NSRect(origin: .zero, size: sourceImage.size),
    operation: .sourceOver,
    fraction: 1
  )

  NSGraphicsContext.restoreGraphicsState()

  return rep.representation(using: .png, properties: [.compressionFactor: 1.0])
}

func writeRoundedPng(size: Int, to path: String) throws {
  guard let data = renderRoundedPng(size: size) else {
    throw NSError(domain: "IconGeneration", code: 1, userInfo: [
      NSLocalizedDescriptionKey: "Failed to render \(path)"
    ])
  }

  try data.write(to: URL(fileURLWithPath: path), options: .atomic)
}

try writeRoundedPng(size: 1024, to: roundedSourceOut)

let iconsRoot = URL(fileURLWithPath: iconsDir)
guard let enumerator = fileManager.enumerator(
  at: iconsRoot,
  includingPropertiesForKeys: [.isRegularFileKey],
  options: [.skipsHiddenFiles]
) else {
  fputs("Failed to enumerate icon directory: \(iconsDir)\n", stderr)
  exit(1)
}

var targets: [IconTarget] = []

for case let fileURL as URL in enumerator {
  guard fileURL.pathExtension.lowercased() == "png" else {
    continue
  }

  let values = try fileURL.resourceValues(forKeys: [.isRegularFileKey])
  guard values.isRegularFile == true,
        let size = pngSize(at: fileURL.path) else {
    continue
  }

  targets.append(IconTarget(path: fileURL.path, size: size))
}

for target in targets {
  try writeRoundedPng(size: target.size, to: target.path)
}

print("Generated \(targets.count + 1) rounded PNG icons.")
