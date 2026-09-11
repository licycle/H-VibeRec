"""Window identity/transport regressions, independent of any named application."""
import io
import os
import sys
import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).parent))
from main import input_lines
from test_focus import FocusDiscoveryTests
from test_service import FakeAX
from service import TargetService
import macos as mac
from lifecycle import WindowLifecycle, WindowWatch
from window_identity import WindowIdentity


class NativeWindowTests(FocusDiscoveryTests):
    # Reuse fixture setup, not its test methods (see load_tests below).
    def setUp(self):
        super().setUp()
        self.numbers = {"window": 100}
        self.owners = {100: 11}
        self.ax.identity = SimpleNamespace(number=lambda w: self.numbers.get(w),
            process_token=lambda pid: (1000, 20), windows=lambda **kwargs: self.owners)
        self.ax.lifecycle = SimpleNamespace(watch=lambda pid, window: WindowWatch(pid, window))

    def test_background_ax_failure_does_not_discard_native_window(self):
        control = self.capture()
        with patch.object(mac, "attr", side_effect=mac.AXFailure("AXRole", mac.AX.kAXErrorCannotComplete)):
            self.assertTrue(self.ax.alive(control))
        self.assertEqual(control.window_id, 100)
        self.assertEqual(control.availability, "available")

    def test_same_native_window_rebinds_proxy_without_changing_document_identity(self):
        control = self.capture()
        self.data["window"]["AXRole"] = None
        self.data["replacement"] = {"AXRole": "AXWindow", "AXTitle": "new title"}
        self.numbers["replacement"] = 100
        self.data["app"]["AXWindows"] = ["replacement"]
        window = self.ax.find_window(control)
        self.assertEqual(window, "replacement")
        self.ax.bind_window(control, window)
        self.assertEqual(control.document_element, "replacement")
        self.assertEqual(control.window_id, 100)

    def test_rebinding_window_does_not_adopt_a_new_web_page(self):
        control = self.capture()
        control.document_element = "original-web-area"
        self.ax.bind_window(control, "new-window-proxy")
        self.assertEqual(control.document_element, "original-web-area")

    def test_same_title_other_native_window_is_never_a_match(self):
        control = self.capture()
        self.data["window"]["AXRole"] = None
        self.data["replacement"] = {"AXRole": "AXWindow", "AXTitle": "fixture"}
        self.numbers["replacement"] = 200
        self.data["app"]["AXWindows"] = ["replacement"]
        self.assertIsNone(self.ax.find_window(control))
        self.assertEqual(control.window, "window")

    def test_equal_ax_reference_cannot_override_a_different_native_window_id(self):
        control = self.capture()
        self.numbers["window"] = 200
        self.assertFalse(self.ax.same_window(control.window, control.window, control.window_id))

    def test_reused_pid_with_new_start_time_is_rejected(self):
        control = self.capture()
        self.ax.identity.process_token = lambda pid: (2000, 20)
        self.assertFalse(self.ax.alive(control))
        self.assertEqual(control.availability, "closed")

    def test_late_launch_date_metadata_does_not_replace_native_process_identity(self):
        control = self.capture()
        self.app.launchDate = lambda: 999
        self.assertTrue(self.ax.alive(control))

    def test_missing_window_without_destroy_event_is_retryable(self):
        control = self.capture()
        self.owners.clear()
        self.assertTrue(self.ax.alive(control))
        self.assertEqual(control.availability, "unreachable")
        self.assertIsNone(self.ax.find_window(control))
        self.owners[100] = 11
        self.assertTrue(self.ax.alive(control))
        self.assertEqual(control.availability, "available")

    def test_destroyed_proxy_does_not_mean_native_window_closed(self):
        control = self.capture()
        control.window_watch.destroyed = True
        self.assertTrue(self.ax.alive(control))
        self.owners.clear()
        self.assertFalse(self.ax.alive(control))
        self.assertEqual(control.availability, "closed")


class NativeBridgeTests(unittest.TestCase):
    def test_native_process_identity_is_stable_and_invalid_pid_is_not_an_identity(self):
        identity = WindowIdentity()
        token = identity.process_token(os.getpid())
        self.assertIsNotNone(token)
        self.assertEqual(identity.process_token(os.getpid()), token)
        self.assertIsNone(identity.process_token(0))

    def test_missing_optional_bridge_keeps_native_queries_available(self):
        with patch("window_identity.objc.loadBundleFunctions", side_effect=ValueError("missing")):
            identity = WindowIdentity()
        self.assertFalse(identity.supported)
        self.assertIsNone(identity.number(object()))
        self.assertIsNotNone(identity.process_token(os.getpid()))

    def test_window_query_includes_offscreen_windows(self):
        identity = WindowIdentity()
        with patch("window_identity.Q.CGWindowListCopyWindowInfo", return_value=[
                {mac.Q.kCGWindowNumber: 100, mac.Q.kCGWindowOwnerPID: 11}]) as query:
            self.assertEqual(identity.windows(refresh=True), {100: 11})
            query.assert_called_once_with(mac.Q.kCGWindowListOptionAll, mac.Q.kCGNullWindowID)


class LifecycleTests(unittest.TestCase):
    def test_real_pyobjc_observer_callback_bridge(self):
        life = WindowLifecycle(lambda: None)
        code, observer = mac.AX.AXObserverCreate(os.getpid(), life.callback, None)
        self.assertEqual(code, mac.AX.kAXErrorSuccess)
        self.assertIsNotNone(observer)

    def test_notification_only_marks_its_original_window_and_invalidates_queries(self):
        invalidate = Mock()
        life = WindowLifecycle(invalidate)
        a, b = WindowWatch(1, "a"), WindowWatch(1, "b")
        life.watches = [a, b]
        with patch("lifecycle.CF.CFEqual", side_effect=lambda x, y: x == y):
            life._changed(None, "a", mac.AX.kAXWindowMovedNotification, None)
            self.assertFalse(a.destroyed)
            life._changed(None, "a", mac.AX.kAXUIElementDestroyedNotification, None)
        self.assertTrue(a.destroyed)
        self.assertFalse(b.destroyed)
        self.assertEqual(invalidate.call_count, 2)

    def test_idle_input_keeps_pumping_notifications_and_preserves_request_order(self):
        ready = threading.Event()
        class DelayedInput:
            def __iter__(self):
                if not ready.wait(2):
                    raise RuntimeError("AX loop was blocked by stdin")
                return iter(["A\n", "B\n"])
        pumps = []
        def pump(seconds):
            pumps.append(seconds)
            ready.set()
        self.assertEqual(list(input_lines(DelayedInput(), pump)), ["A\n", "B\n"])
        self.assertTrue(pumps)
        self.assertEqual(list(input_lines(io.StringIO(""), pump)), [])


class RestoreOrderTests(unittest.TestCase):
    def test_background_document_is_read_only_after_restoring_original_window(self):
        backend = FakeAX("原文", (1, 0))
        service = TargetService(backend)
        service.capture("job", 42)
        backend.front = "other"
        original_read = backend.read
        def read(control):
            if backend.front != control.id:
                raise ValueError("background AX unavailable")
            return original_read(control)
        backend.read = read
        service.clone("job", "pinned-job")
        self.assertTrue(service.handle({"op": "paste", "target_id": "pinned-job", "text": "插入"})["verified"])
        self.assertEqual(original_read(service.targets["job"].control), "原插入文")


def load_tests(loader, tests, pattern):
    # The base fixture already runs in test_focus.py.
    suite = unittest.TestSuite()
    for cls in (NativeWindowTests, NativeBridgeTests, LifecycleTests, RestoreOrderTests):
        for name in cls.__dict__:
            if name.startswith("test_"):
                suite.addTest(cls(name))
    return suite
