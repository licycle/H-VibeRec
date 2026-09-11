// A disposable native text document for real macOS AX integration checks.
import AppKit
import WebKit

func reply(_ value: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: value) {
        FileHandle.standardOutput.write(data + Data([10]))
    }
}

final class WebReady: NSObject, WKNavigationDelegate {
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        webView.evaluateJavaScript("document.querySelector('textarea').focus(); document.querySelector('textarea').setSelectionRange(3,3)") { _, error in
            reply(["ready": error == nil, "pid": ProcessInfo.processInfo.processIdentifier])
        }
    }
}

final class FixtureTextView: NSTextView {
    var decorateNextPaste = false
    override func paste(_ sender: Any?) {
        super.paste(sender)
        if decorateNextPaste {
            decorateNextPaste = false
            // Model an editor that accepts the paste and then normalizes its
            // exposed value. Full-string readback intentionally differs.
            textStorage?.append(NSAttributedString(string: "\nrendered"))
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let menu = NSMenu()
let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
let editMenu = NSMenu(title: "Edit")
editMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
editMenu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
editItem.submenu = editMenu
menu.addItem(editItem)
app.mainMenu = menu
let window = NSWindow(contentRect: NSRect(x: 160, y: 260, width: 460, height: 180),
                      styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
window.isReleasedWhenClosed = false
window.title = CommandLine.arguments.dropFirst().first ?? "HVR AX Fixture"
let text = FixtureTextView(frame: window.contentView!.bounds)
var otherWindow: NSWindow?
let otherText = NSTextView(frame: window.contentView!.bounds)
otherText.autoresizingMask = [.width, .height]
otherText.string = "另一个窗口"
text.autoresizingMask = [.width, .height]
text.font = .systemFont(ofSize: 24)
text.string = "甲😀乙"
text.setSelectedRange(NSRange(location: 3, length: 0))
let webMode = CommandLine.arguments.contains("--web")
let web = WKWebView(frame: window.contentView!.bounds)
let webReady = WebReady()
if webMode {
    web.autoresizingMask = [.width, .height]
    web.navigationDelegate = webReady
    window.contentView!.addSubview(web)
    web.loadHTMLString("<html><body><textarea aria-label='Fixture editor' style='width:400px;height:110px;font-size:24px'>甲😀乙</textarea></body></html>", baseURL: nil)
} else {
    window.contentView!.addSubview(text)
}
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(webMode ? web : text)
app.activate(ignoringOtherApps: true)
if !webMode { reply(["ready": true, "pid": ProcessInfo.processInfo.processIdentifier]) }
// Commands are private to the test's spawned fixture. No other app is inspected.
if CommandLine.arguments.contains("--audit") {
    DispatchQueue.global().async {
        while let line = readLine(), let data = line.data(using: .utf8),
              let command = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            DispatchQueue.main.async {
                switch command["op"] as? String {
                case "activate":
                    window.makeKeyAndOrderFront(nil)
                    app.activate(ignoringOtherApps: true)
                    reply(["ok": true])
                case "voice_shortcut":
                    // The isolated audit registers Control+Option+Shift+K before this
                    // command. Never send a key unless our own fixture is frontmost.
                    guard NSWorkspace.shared.frontmostApplication?.processIdentifier == ProcessInfo.processInfo.processIdentifier,
                          let down = CGEvent(keyboardEventSource: nil, virtualKey: 40, keyDown: true),
                          let up = CGEvent(keyboardEventSource: nil, virtualKey: 40, keyDown: false) else {
                        reply(["error": "Owned fixture must be frontmost for shortcut audit"])
                        return
                    }
                    down.flags = [.maskControl, .maskAlternate, .maskShift]
                    up.flags = []
                    down.post(tap: .cgSessionEventTap)
                    up.post(tap: .cgSessionEventTap)
                    reply(["ok": true])
                case "select":
                    let location = command["location"] as? Int ?? 0
                    let length = command["length"] as? Int ?? 0
                    if webMode {
                        web.evaluateJavaScript("document.querySelector('textarea').setSelectionRange(\(location),\(location + length))") { _, error in reply(["ok": error == nil]) }
                    } else {
                        text.setSelectedRange(NSRange(location: location, length: length))
                        reply(["ok": true])
                    }
                case "selection":
                    if webMode {
                        web.evaluateJavaScript("(() => {const t = document.querySelector('textarea'); return {location:t.selectionStart,length:t.selectionEnd-t.selectionStart};})()") { value, _ in reply(value as? [String: Any] ?? [:]) }
                    } else {
                        let selected = text.selectedRange()
                        reply(["location": selected.location, "length": selected.length])
                    }
                case "decorate_next_paste":
                    text.decorateNextPaste = true
                    reply(["ok": true])
                case "move":
                    window.setFrameOrigin(NSPoint(x: 620, y: 300))
                    reply(["ok": true])
                case "identity":
                    reply(["pid": ProcessInfo.processInfo.processIdentifier, "window_id": window.windowNumber])
                case "hide":
                    app.hide(nil)
                    reply(["ok": true])
                case "minimize":
                    window.miniaturize(nil)
                    reply(["ok": true])
                case "other_window":
                    if otherWindow == nil {
                        let second = NSWindow(contentRect: NSRect(x: 300, y: 420, width: 460, height: 180),
                                              styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
                        second.isReleasedWhenClosed = false
                        second.title = command["same_title"] as? Bool == true ? window.title : window.title + " second window"
                        second.contentView!.addSubview(otherText)
                        otherWindow = second
                    }
                    otherWindow!.makeKeyAndOrderFront(nil)
                    otherWindow!.makeFirstResponder(otherText)
                    app.activate(ignoringOtherApps: true)
                    reply(["ok": true])
                case "read_other":
                    reply(["text": otherText.string])
                case "close":
                    window.close()
                    reply(["ok": true])
                case "read":
                    if webMode {
                        web.evaluateJavaScript("document.querySelector('textarea').value") { value, _ in reply(["text": value as? String ?? ""]) }
                    } else { reply(["text": text.string]) }
                default: reply(["error": "Unknown fixture command"])
                }
            }
        }
    }
}
app.run()
