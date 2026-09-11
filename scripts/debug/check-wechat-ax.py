"""Read-only diagnostic for macOS WeChat Accessibility exposure.

This does not click, type, paste, or read chat content. It activates the
WeChat app only to query its AX application/window metadata and runs the same
capture discovery used by the pastebox helper.
"""
from __future__ import annotations

import sys
import time
from pathlib import Path

import AppKit as AK
import ApplicationServices as AX

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "sidecars/pastebox_ax"))
from macos import MacAX  # noqa: E402


def value(element, attribute):
    if element is None:
        return None
    try:
        code, result = AX.AXUIElementCopyAttributeValue(element, attribute, None)
        return result if code == AX.kAXErrorSuccess else f"AX_ERROR_{code}"
    except Exception as error:  # pragma: no cover - depends on installed AX bridge
        return f"{type(error).__name__}: {error}"


def main() -> int:
    apps = [
        app
        for app in AK.NSWorkspace.sharedWorkspace().runningApplications()
        if str(app.bundleIdentifier() or "")
        in ("com.tencent.xinWeChat", "com.tencent.flue.WeChatAppEx")
    ]
    if not apps:
        print("WeChat is not running")
        return 2
    backend = MacAX()
    print("accessibility_trusted:", AX.AXIsProcessTrusted())
    print("frontmost_before:", AK.NSWorkspace.sharedWorkspace().frontmostApplication().bundleIdentifier())
    for app in apps:
        app.activateWithOptions_(AK.NSApplicationActivateIgnoringOtherApps)
        time.sleep(0.25)
        element = AX.AXUIElementCreateApplication(app.processIdentifier())
        focused_window = value(element, AX.kAXFocusedWindowAttribute)
        windows = value(element, AX.kAXWindowsAttribute)
        focused = value(element, AX.kAXFocusedUIElementAttribute)
        print({
            "pid": app.processIdentifier(),
            "bundle_id": app.bundleIdentifier(),
            "active": app.isActive(),
            "focused_window": str(focused_window)[:160],
            "window_count": len(windows) if isinstance(windows, (list, tuple)) else str(windows),
            "focused_ui_element": str(focused)[:160],
        })
    root_app = next((app for app in apps if app.bundleIdentifier() == "com.tencent.xinWeChat"), apps[0])
    try:
        backend.capture(root_app.processIdentifier())
    except Exception as error:
        print("capture_result:", type(error).__name__, str(error))
        return 1
    print("capture_result: editable AX target found")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
