"""Exercise missing focus attributes, lazy AX trees and ambiguous input fields."""
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
class FocusDiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.ax = mac.MacAX.__new__(mac.MacAX)
        self.ax.owner_pid = 99
        self.ax.front_pid = lambda: 11
        self.ax.require_permission = lambda: None
        self.app = SimpleNamespace(launchDate=lambda: 1, processIdentifier=lambda: 11, isTerminated=lambda: False)
        self.data = {
            "app": {"AXFocusedWindow": "window", "AXFocusedUIElement": "editor"},
            "system": {},
            "window": {"AXRole": "AXWindow", "AXTitle": "fixture", "AXChildren": ["editor"]},
            "editor": {"AXRole": "AXTextArea", "AXWindow": "window", "AXParent": "window",
                       "AXFocused": True, "AXValue": "甲😀乙",
                       "AXSelectedTextRange": mac.AX.AXValueCreate(mac.AX.kAXValueCFRangeType, (3, 0))},
        }
        patches = [
            patch.object(mac, "attr", side_effect=lambda e, a: self.data[e].get(a)),
            patch.object(mac, "equal", side_effect=lambda a, b: a is not None and a == b),
            patch.object(mac, "is_element", side_effect=lambda e: isinstance(e, str) and e in self.data),
            patch.object(mac, "settable", side_effect=lambda e, a: a in self.data[e]),
            patch.object(mac.AX, "AXUIElementCreateSystemWide", return_value="system"),
            patch.object(mac.AX, "AXUIElementCreateApplication", return_value="app"),
            patch.object(mac.AX, "AXUIElementGetPid", return_value=(0, 11)),
            patch.object(mac.AX, "AXUIElementCopyAttributeNames", side_effect=lambda e, _: (0, list(self.data[e]))),
            patch.object(mac.AK, "NSRunningApplication", SimpleNamespace(
                runningApplicationWithProcessIdentifier_=lambda _: self.app)),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def capture(self):
        control, text, selection = self.ax.capture(11)
        self.assertEqual((text, selection), ("甲😀乙", (3, 0)))
        self.assertTrue(self.ax.focus_matches(control))
        return control

    def test_application_focus_captures_the_actual_utf16_caret(self):
        self.assertEqual(self.capture().capture_method, "application")

    def test_trigger_capture_accepts_unchanged_input_counters(self):
        with patch.object(mac.Q, "CGEventSourceCounterForEventType", side_effect=[10, 2, 0, 0] * 2):
            _, text, selection = self.ax.capture(11, [10, 2, 0, 0])
        self.assertEqual((text, selection), ("甲😀乙", (3, 0)))

    def test_click_after_shortcut_cannot_retarget_to_another_caret_in_the_same_app(self):
        self.data["editor"]["AXSelectedTextRange"] = mac.AX.AXValueCreate(mac.AX.kAXValueCFRangeType, (0, 0))
        with patch.object(mac.Q, "CGEventSourceCounterForEventType", side_effect=[10, 3, 0, 0]):
            with self.assertRaisesRegex(ValueError, "后来的位置"):
                self.ax.capture(11, [10, 2, 0, 0])

    def test_input_during_snapshot_cannot_be_accepted_as_the_shortcut_position(self):
        with patch.object(mac.Q, "CGEventSourceCounterForEventType", side_effect=[10, 2, 0, 0, 11, 2, 0, 0]):
            with self.assertRaisesRegex(ValueError, "后来的位置"):
                self.ax.capture(11, [10, 2, 0, 0])

    def test_own_main_window_is_a_valid_target(self):
        self.ax.owner_pid = 11
        self.data["window"]["AXIdentifier"] = "hvr.pastebox.main"
        self.assertEqual(self.capture().capture_method, "application")

    def test_webkit_window_placeholder_falls_back_to_the_real_parent_window(self):
        self.ax.owner_pid = 11
        self.data["window"]["AXIdentifier"] = "hvr.pastebox.main"
        self.data["window-placeholder"] = {}
        self.data["editor"].update(AXWindow="window-placeholder", AXParent="web")
        self.data["web"] = {"AXRole": "AXWebArea", "AXWindow": "window-placeholder", "AXParent": "window"}
        self.assertEqual(self.capture().window, "window")

    def test_own_tool_windows_cannot_become_targets(self):
        self.ax.owner_pid = 11
        for identifier in (None, "voice-input-overlay", "pastebox"):
            with self.subTest(identifier=identifier):
                self.data["window"]["AXIdentifier"] = identifier
                with self.assertRaisesRegex(ValueError, "主窗口的编辑区"):
                    self.ax.capture(11)

    def test_temporarily_busy_window_is_not_mistaken_for_a_closed_window(self):
        control = self.capture()
        self.data["app"]["AXWindows"] = ["window"]
        attempts = []
        def busy_once(element, attribute):
            if element == "window" and attribute == "AXRole" and not attempts:
                attempts.append(True)
                raise mac.AXFailure(attribute, mac.AX.kAXErrorCannotComplete)
            return self.data[element].get(attribute)
        with patch.object(mac, "attr", side_effect=busy_once):
            self.assertTrue(self.ax.alive(control))

    def test_closed_window_is_rejected_even_if_its_reference_still_has_a_role(self):
        control = self.capture()
        self.data["app"]["AXWindows"] = []
        self.assertFalse(self.ax.alive(control))

    def test_same_title_and_no_document_cannot_identify_a_replacement_window(self):
        control = self.capture()
        replacement = "window-replacement"
        self.data[replacement] = {
            "AXRole": "AXWindow", "AXTitle": "fixture", "AXDocument": None,
            "AXChildren": ["editor"],
        }
        self.data["window"]["AXDocument"] = None
        self.data["app"]["AXWindows"] = [replacement]
        self.assertFalse(self.ax.alive(control))
        self.assertEqual(control.window, "window")

    def test_same_title_different_document_is_not_rebound(self):
        control = self.capture()
        replacement = "other-document-window"
        self.data[replacement] = {
            "AXRole": "AXWindow", "AXTitle": "fixture", "AXDocument": "different",
            "AXChildren": [],
        }
        self.data["window"]["AXDocument"] = "original"
        self.data["app"]["AXWindows"] = [replacement]
        self.assertFalse(self.ax.alive(control))

    def test_system_focus_works_when_application_attribute_is_missing(self):
        self.data["app"].pop("AXFocusedUIElement")
        self.data["system"]["AXFocusedUIElement"] = "editor"
        self.assertEqual(self.capture().capture_method, "system")

    def test_window_focus_can_supply_the_text_control(self):
        self.data["app"].pop("AXFocusedUIElement")
        self.data["window"]["AXFocusedUIElement"] = "editor"
        self.assertEqual(self.capture().capture_method, "window")

    def test_explicit_focused_descendant_works_through_a_proxy(self):
        self.data["app"]["AXFocusedUIElement"] = "window"
        self.assertEqual(self.capture().capture_method, "focused_descendant")

    def test_editable_web_wrapper_uses_capabilities_not_only_native_role_names(self):
        self.data["editor"].update(AXRole="AXGroup", AXEditable=True)
        self.assertEqual(self.capture().role, "AXGroup")

    def test_never_assume_a_single_editable_field_is_focused(self):
        self.data["app"].pop("AXFocusedUIElement")
        self.data["editor"]["AXFocused"] = False
        with self.assertRaisesRegex(ValueError, "已找到窗口"):
            self.ax.capture(11)

    def test_ambiguous_focused_fields_are_rejected(self):
        self.data["app"].pop("AXFocusedUIElement")
        self.data["other"] = dict(self.data["editor"])
        self.data["window"]["AXChildren"].append("other")
        with self.assertRaisesRegex(ValueError, "已找到窗口"):
            self.ax.capture(11)

    def test_lazy_accessibility_is_enabled_without_pressing_or_moving_focus(self):
        self.data["app"].pop("AXFocusedUIElement")
        self.data["app"]["AXEnhancedUserInterface"] = False
        self.data["editor"]["AXFocused"] = False
        def enable(e, name, value):
            self.assertEqual((e, name, value), ("app", "AXEnhancedUserInterface", True))
            self.data[e][name] = value
            self.data["editor"]["AXFocused"] = True
            return 0
        with patch.object(mac.AX, "AXUIElementSetAttributeValue", side_effect=enable), \
                patch.object(mac.AX, "AXUIElementPerformAction") as action:
            self.assertEqual(self.capture().capture_method, "enhanced_focused_descendant")
            action.assert_not_called()

    def test_focus_from_another_application_cannot_supply_a_target(self):
        self.data["app"].pop("AXFocusedUIElement")
        self.data["system"]["AXFocusedUIElement"] = "editor"
        self.data["editor"]["AXFocused"] = False
        with patch.object(mac.AX, "AXUIElementGetPid", return_value=(0, 22)):
            with self.assertRaisesRegex(ValueError, "已找到窗口"):
                self.ax.capture(11)

    def test_selection_change_during_capture_is_rejected(self):
        with patch.object(self.ax, "selection", side_effect=[(3, 0), (0, 0)]):
            with self.assertRaisesRegex(ValueError, "记录期间焦点"):
                self.ax.capture(11)


if __name__ == "__main__":
    unittest.main()
