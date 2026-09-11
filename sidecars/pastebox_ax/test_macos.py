"""AX binding contract tests using controlled app/clipboard doubles, not a live desktop."""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
try:
    import macos as mac
except ImportError:
    mac = None


@unittest.skipIf(mac is None, "macOS + PyObjC required")
class AXAttributeTests(unittest.TestCase):
    def test_missing_native_identifier_does_not_invalidate_live_text_control(self):
        with patch.object(mac.AX, "AXUIElementCopyAttributeValue", return_value=(mac.AX.kAXErrorFailure, None)):
            self.assertIsNone(mac.attr(object(), mac.AX.kAXIdentifierAttribute))
            with self.assertRaises(mac.AXFailure):
                mac.attr(object(), mac.AX.kAXSelectedTextRangeAttribute)

    def test_identifier_permission_transport_and_stale_reference_errors_remain_errors(self):
        for code in (mac.AX.kAXErrorCannotComplete, mac.AX.kAXErrorInvalidUIElement, mac.AX.kAXErrorAPIDisabled):
            with self.subTest(code=code), patch.object(mac.AX, "AXUIElementCopyAttributeValue", return_value=(code, None)):
                with self.assertRaises(mac.AXFailure):
                    mac.attr(object(), mac.AX.kAXIdentifierAttribute)


@unittest.skipIf(mac is None, "macOS + PyObjC required")
class AXIdentityTests(unittest.TestCase):
    def setUp(self):
        self.ax = mac.MacAX.__new__(mac.MacAX)
        self.ax.require_permission = lambda: None
        self.ax.alive = lambda c: True
        self.window, self.old, self.new = "window", "old", "new"
        self.data = {
            self.window: {"AXRole": "AXWindow", "AXChildren": [self.new]},
            self.old: {"AXRole": None},
            self.new: {"AXRole": "AXTextArea", "AXIdentifier": "editor-id", "AXWindow": self.window},
        }
        self.target = mac.Control(None, self.window, self.old, "editor-id", "AXTextArea", None,
                                  self.window, None, None, None)
        self.ax.document = lambda element, window: (window, None)
        self.patches = [patch.object(mac, "attr", side_effect=lambda el, name: self.data[el].get(name)),
                        patch.object(mac, "equal", side_effect=lambda a, b: a == b),
                        patch.object(mac, "is_element", side_effect=lambda x: x in self.data),
                        patch.object(mac, "settable", return_value=True)]
        for p in self.patches:
            p.start()
            self.addCleanup(p.stop)

    def test_unique_stable_identifier_can_rebind_within_same_document(self):
        self.ax.resolve(self.target)
        self.assertEqual(self.target.element, self.new)

    def test_duplicate_identifier_never_picks_first_match(self):
        self.data["duplicate"] = dict(self.data[self.new])
        self.data[self.window]["AXChildren"].append("duplicate")
        with self.assertRaisesRegex(ValueError, "唯一"):
            self.ax.resolve(self.target)
        self.assertEqual(self.target.element, self.old)

    def test_no_identifier_does_not_rebind_by_title_or_tree_index(self):
        self.target.identifier = None
        with self.assertRaisesRegex(ValueError, "稳定标识"):
            self.ax.resolve(self.target)

    def test_reused_live_control_with_changed_identity_rejected(self):
        self.target.element = self.new
        self.data[self.new]["AXIdentifier"] = "different-editor"
        with self.assertRaisesRegex(ValueError, "身份"):
            self.ax.resolve(self.target)

    def test_navigation_and_readonly_selection_are_rejected(self):
        self.target.element = self.new
        self.ax.document = lambda e, w: (w, "https://different-document.test")
        with self.assertRaisesRegex(ValueError, "文档"):
            self.ax.resolve(self.target)
        self.ax.document = lambda e, w: (w, None)
        with patch.object(mac, "settable", return_value=False):
            with self.assertRaisesRegex(ValueError, "不可写"):
                self.ax.resolve(self.target)

    def test_api_write_errors_are_not_silently_ignored(self):
        with patch.object(mac.AX, "AXUIElementSetAttributeValue", return_value=mac.AX.kAXErrorCannotComplete):
            with self.assertRaises(mac.AXFailure):
                mac.set_attr(self.new, "AXFocused", True)

    def test_real_pyobjc_utf16_range_bridge(self):
        value = mac.AX.AXValueCreate(mac.AX.kAXValueCFRangeType, (4, 2))
        self.data[self.new]["AXSelectedTextRange"] = value
        self.target.element = self.new
        self.assertEqual(self.ax.selection(self.target), (4, 2))
        self.data[self.new]["AXSelectedTextRanges"] = [value, value]
        with self.assertRaisesRegex(ValueError, "多个光标"):
            self.ax.selection(self.target)


class Clipboard:
    def __init__(self):
        self.change, self.text, self.restored = 1, "original", False
    def changeCount(self): return self.change
    def pasteboardItems(self): return [SimpleNamespace(types=lambda: ["text"], dataForType_=lambda k: b"original")]
    def clearContents(self): self.change += 1; self.text = ""
    def setString_forType_(self, text, kind): self.change += 1; self.text = text; return True
    def writeObjects_(self, items): self.change += 1; self.text = "original"; self.restored = True; return True


@unittest.skipIf(mac is None, "macOS + PyObjC required")
class ClipboardDeliveryTests(unittest.TestCase):
    def setUp(self):
        self.ax = mac.MacAX.__new__(mac.MacAX)
        self.board = Clipboard()
        item = SimpleNamespace(setData_forType_=lambda data, kind: None)
        patches = [patch.object(mac.AK, "NSPasteboard", SimpleNamespace(generalPasteboard=lambda: self.board)),
                   patch.object(mac.AK, "NSPasteboardItem", SimpleNamespace(alloc=lambda: SimpleNamespace(init=lambda: item))),
                   patch.object(mac.Q, "CGEventCreateKeyboardEvent", return_value=object()),
                   patch.object(mac.Q, "CGEventSetFlags"), patch.object(mac, "pause")]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)
        self.ax.read = lambda _: "expected"

    def test_verified_delivery_restores_clipboard_representations(self):
        with patch.object(mac.Q, "CGEventPost") as post:
            self.assertTrue(self.ax.paste(None, "insert", "expected", lambda: None))
            self.assertEqual(post.call_count, 2)
        self.assertTrue(self.board.restored)

    def test_new_user_copy_survives_cleanup(self):
        def post(*args): self.board.setString_forType_("new user copy", "text")
        with patch.object(mac.Q, "CGEventPost", side_effect=post):
            self.assertTrue(self.ax.paste(None, "insert", "expected", lambda: None))
        self.assertEqual(self.board.text, "new user copy")
        self.assertFalse(self.board.restored)

    def test_dispatch_failure_is_uncertain_and_restores_clipboard(self):
        with patch.object(mac.Q, "CGEventPost", side_effect=RuntimeError("transport")):
            self.assertFalse(self.ax.paste(None, "insert", "expected", lambda: None))
        self.assertTrue(self.board.restored)

    def test_focus_change_after_clipboard_write_stops_input(self):
        def validate():
            if self.board.text == "insert": raise ValueError("focus moved")
        with patch.object(mac.Q, "CGEventPost") as post:
            with self.assertRaisesRegex(ValueError, "focus moved"):
                self.ax.paste(None, "insert", "expected", validate)
            post.assert_not_called()
        self.assertTrue(self.board.restored)


if __name__ == "__main__":
    unittest.main()
