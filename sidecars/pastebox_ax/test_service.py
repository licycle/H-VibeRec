import sys
import unittest
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from anchors import Anchor, after_edit, index_at, replace, units
from service import Target, TargetService


@dataclass
class FakeControl:
    id: str
    capability: str = "exact_ax"


class FakeAX:
    def __init__(self, text="", selection=(0, 0)):
        self.controls = {"editor": {"text": text, "selection": selection, "alive": True}}
        self.front = "editor"
        self.has_permission = True
        self.sent = []
        self.before_dispatch = lambda: None
        self.verified = True
        self.focus_error = None

    def trusted(self): return self.has_permission
    def request_permission(self): pass
    def describe(self, control): return {"app_name": "Fixture", "bundle_id": "test.fixture", "window_title": control.id}
    def alive(self, control): return self.controls[control.id]["alive"]
    def clear_marker(self, control): pass
    def require_permission(self):
        if not self.has_permission:
            raise ValueError("permission denied")
    def capture(self, expected_pid):
        self.require_permission()
        if expected_pid != 42:
            raise ValueError("front application changed")
        control = FakeControl(self.front)
        return control, self.read(control), self.selection(control)
    def resolve(self, control):
        if not self.alive(control): raise ValueError("document closed")
    def read(self, control): return self.controls[control.id]["text"]
    def selection(self, control): return self.controls[control.id]["selection"]
    def focus(self, control, check):
        if self.focus_error:
            self.front = control.id
            raise ValueError(self.focus_error)
        self.front = control.id
    def restore_window(self, control, check): self.front = control.id
    def window_matches(self, control): return self.front == control.id
    def focus_matches(self, control): return self.front == control.id
    def select(self, control, selection, check): self.controls[control.id]["selection"] = selection
    def same_control(self, a, b): return a.id == b.id
    def paste(self, control, text, expected, validate):
        self.before_dispatch()
        validate()
        self.sent.append((control.id, text, self.selection(control)))
        if self.verified:
            self.controls[control.id]["text"] = expected
        return self.verified
    def foreground_paste(self, control, text, check):
        check()
        if not self.window_matches(control):
            raise ValueError("original window is not frontmost")
        self.sent.append((control.id, text, None))
        return False


class TargetBehaviorTests(unittest.TestCase):
    def setUp(self):
        self.ax = FakeAX("你好😀世界", (4, 0))
        self.service = TargetService(self.ax)

    def capture(self, key="A"):
        return self.service.handle({"op": "capture", "id": key, "expected_pid": 42})

    def paste(self, key, text):
        return self.service.handle({"op": "paste", "target_id": key, "text": text})

    def test_restore_after_switching_app_and_moving_selection(self):
        self.capture()
        self.ax.front = "another-app"
        self.ax.controls["editor"]["selection"] = (0, 2)
        self.assertTrue(self.paste("A", "插入")["verified"])
        self.assertEqual(self.ax.controls["editor"]["text"], "你好😀插入世界")

    def test_reverse_notification_order_at_same_pinned_caret(self):
        self.capture("pin")
        for key in ["A", "B"]:
            self.service.handle({"op": "clone", "source": "pin", "id": key})
        self.paste("B", "乙")
        self.paste("A", "甲")
        self.assertEqual(self.ax.controls["editor"]["text"], "你好😀乙甲世界")
        self.assertEqual(self.service.targets["pin"].anchor.location, 6)

    def test_different_positions_rebase_after_earlier_insert(self):
        self.capture("later")
        self.ax.controls["editor"]["selection"] = (0, 0)
        self.capture("earlier")
        self.paste("earlier", "前")
        self.paste("later", "后")
        self.assertEqual(self.ax.controls["editor"]["text"], "前你好😀后世界")

    def test_selected_emoji_replacement_and_overlap_invalidates(self):
        self.ax.controls["editor"]["selection"] = (2, 2)
        self.capture("replace")
        self.ax.controls["editor"]["selection"] = (0, 4)
        self.capture("overlap")
        self.paste("replace", "笑脸")
        self.assertEqual(self.ax.controls["editor"]["text"], "你好笑脸世界")
        with self.assertRaisesRegex(ValueError, "重叠"):
            self.paste("overlap", "不可写入")
        self.assertEqual(len(self.ax.sent), 1)

    def test_focus_or_text_changes_at_dispatch_never_send(self):
        self.capture()
        self.ax.before_dispatch = lambda: setattr(self.ax, "front", "wrong")
        with self.assertRaisesRegex(ValueError, "变化"):
            self.paste("A", "test")
        self.assertEqual(self.ax.sent, [])

    def test_permission_revoked_and_app_closed(self):
        self.capture()
        self.ax.has_permission = False
        with self.assertRaisesRegex(ValueError, "permission"):
            self.paste("A", "test")
        self.ax.has_permission = True
        self.ax.controls["editor"]["alive"] = False
        with self.assertRaisesRegex(ValueError, "关闭"):
            self.paste("A", "test")
        self.assertEqual(self.ax.sent, [])

    def test_unverified_input_invalidates_other_jobs_at_same_target(self):
        self.capture("A")
        self.capture("B")
        self.ax.verified = False
        self.assertFalse(self.paste("A", "test")["verified"])
        with self.assertRaisesRegex(ValueError, "未确认"):
            self.paste("B", "test")
        self.assertEqual(len(self.ax.sent), 1)

    def test_other_controls_are_not_rebased(self):
        self.capture("A")
        self.ax.controls["different"] = {"text": "", "selection": (0, 0), "alive": True}
        self.ax.front = "different"
        self.capture("B")
        self.paste("A", "test")
        self.assertEqual(self.service.targets["B"].anchor.location, 0)
        self.paste("B", "另一个输入框")
        self.assertEqual(self.ax.sent[-1][0], "different")

    def restore(self, key):
        return self.service.handle({"op": "restore", "target_id": key})

    def test_auto_notification_only_restores_after_emoji_replacement(self):
        self.ax.controls["editor"]["selection"] = (2, 2)
        self.capture()
        self.paste("A", "【结果】")
        sent = list(self.ax.sent)
        self.ax.front = "another-app"
        self.ax.controls["editor"]["selection"] = (0, 0)
        self.assertTrue(self.restore("A")["restored"])
        self.assertEqual(self.ax.front, "editor")
        self.assertEqual(self.ax.controls["editor"]["selection"], (6, 0))
        self.assertEqual(self.ax.controls["editor"]["text"], "你好【结果】世界")
        self.restore("A")
        self.assertEqual(self.ax.sent, sent)

    def test_auto_notifications_return_to_each_item_end_in_reverse_order(self):
        self.capture("A")
        self.capture("B")
        self.paste("A", "甲")
        self.paste("B", "乙")
        self.restore("B")
        self.assertEqual(self.ax.controls["editor"]["selection"], (6, 0))
        self.restore("A")
        self.assertEqual(self.ax.controls["editor"]["selection"], (5, 0))
        self.assertEqual(self.ax.controls["editor"]["text"], "你好😀甲乙世界")
        self.assertEqual(len(self.ax.sent), 2)

    def test_return_position_rebases_across_later_insert_before_it(self):
        self.capture("A")
        self.paste("A", "甲")
        self.ax.controls["editor"]["selection"] = (0, 0)
        self.capture("B")
        self.paste("B", "前缀")
        self.restore("A")
        self.assertEqual(self.ax.controls["editor"]["selection"], (7, 0))
        self.assertEqual(len(self.ax.sent), 2)

    def test_restore_after_unverified_paste_or_closed_window_never_sends_again(self):
        self.capture("A")
        self.ax.verified = False
        self.paste("A", "结果")
        self.ax.front = "another-app"
        self.assertFalse(self.restore("A")["caret_restored"])
        self.assertEqual(self.ax.front, "editor")
        self.assertEqual(len(self.ax.sent), 1)
        self.ax.verified = True
        self.capture("B")
        self.paste("B", "结果")
        self.ax.controls["editor"]["alive"] = False
        with self.assertRaisesRegex(ValueError, "关闭"):
            self.restore("B")
        self.assertEqual(len(self.ax.sent), 2)

    def test_successful_paste_with_late_readback_still_allows_exact_navigation(self):
        self.capture("A")
        self.ax.verified = False
        self.paste("A", "成功")
        # The app applied the input after the readback deadline.
        self.ax.controls["editor"].update(text="你好😀成功世界", selection=(0, 0))
        self.ax.front = "another-app"
        self.assertTrue(self.restore("A")["caret_restored"])
        self.assertEqual(self.ax.controls["editor"]["selection"], (6, 0))
        self.restore("A")
        with self.assertRaisesRegex(ValueError, "未确认"):
            self.paste("A", "不得重复")
        self.assertEqual(len(self.ax.sent), 1)

    def test_editor_normalization_keeps_original_window_without_claiming_exact_caret(self):
        self.capture("A")
        self.ax.verified = False
        self.paste("A", "成功")
        self.ax.controls["editor"].update(text="你好😀成功世界\n规范化", selection=(0, 0))
        self.ax.front = "another-app"
        result = self.restore("A")
        self.assertTrue(result["restored"])
        self.assertFalse(result["caret_restored"])
        self.assertEqual(self.ax.front, "editor")
        self.assertEqual(self.ax.controls["editor"]["selection"], (0, 0))
        self.assertEqual(len(self.ax.sent), 1)

    def test_document_replaced_in_original_window_only_restores_window(self):
        self.capture("A")
        self.paste("A", "成功")
        self.ax.front = "another-app"
        def changed_document(control): raise ValueError("目标网页或文档已切换")
        self.ax.resolve = changed_document
        self.assertFalse(self.restore("A")["caret_restored"])
        self.assertEqual(self.ax.front, "editor")
        self.assertEqual(len(self.ax.sent), 1)

    def test_navigation_does_not_report_success_after_user_switches_window(self):
        self.capture("A")
        def switched(control):
            self.ax.front = "user-chosen-window"
            raise ValueError("焦点已变化")
        self.ax.resolve = switched
        with self.assertRaisesRegex(ValueError, "前台窗口已改变"):
            self.restore("A")
        self.assertEqual(self.ax.front, "user-chosen-window")
        self.assertEqual(self.ax.sent, [])

    def test_front_application_race_rejects_capture(self):
        with self.assertRaises(ValueError):
            self.service.handle({"op": "capture", "id": "A", "expected_pid": 99})
        self.assertFalse(self.service.targets)

    def test_metadata_contains_no_external_text_or_marker(self):
        value = self.capture()
        self.assertNotIn("你好", str(value))
        self.assertNotIn("anchor", value)
        self.assertNotIn("marker", value)

    def test_ax_opaque_target_uses_guarded_foreground_paste(self):
        control = FakeControl("editor", "foreground_paste")
        self.service.remember(Target("wechat", control, Anchor.capture("", (0, 0)), "now"))
        result = self.service.handle({"op": "paste", "target_id": "wechat", "text": "微信兼容"})
        self.assertFalse(result["verified"])
        self.assertEqual(self.ax.sent[-1][0:2], ("editor", "微信兼容"))

    def test_ax_opaque_target_restores_original_window_after_switch(self):
        control = FakeControl("editor", "foreground_paste")
        self.service.remember(Target("wechat", control, Anchor.capture("", (0, 0)), "now"))
        self.ax.front = "another-app"
        result = self.service.handle({"op": "paste", "target_id": "wechat", "text": "恢复原窗口"})
        self.assertFalse(result["verified"])
        self.assertEqual(self.ax.front, "editor")

    def test_transient_exact_focus_failure_falls_back_on_original_window(self):
        self.capture("A")
        self.ax.focus_error = "无法将键盘焦点恢复到原窗口的输入控件"
        result = self.service.handle({"op": "paste", "target_id": "A", "text": "兼容"})
        self.assertFalse(result["verified"])
        self.assertEqual(self.ax.sent[-1][0:2], ("editor", "兼容"))


class AnchorTests(unittest.TestCase):
    def test_unicode_utf16_and_invalid_half_surrogate(self):
        self.assertEqual(units("a😀中"), 4)
        self.assertEqual(index_at("a😀中", 3), 2)
        self.assertEqual(replace("a😀中", (1, 2), "🙂"), "a🙂中")
        with self.assertRaises(ValueError): index_at("a😀中", 2)

    def test_edit_outside_context_preserves_unique_anchor(self):
        text = "a" * 80 + "left" + "b" * 80
        anchor = Anchor.capture(text, (84, 0))
        self.assertEqual(anchor.resolve("新的前缀" + text), (88, 0))
        self.assertEqual(anchor.resolve(text + "suffix"), (84, 0))

    def test_ambiguous_or_changed_context_rejects(self):
        text = "a" * 80 + "middle" + "b" * 80
        anchor = Anchor.capture(text, (86, 0))
        for changed in [text + text, text.replace("middle", "changed")]:
            with self.assertRaises(ValueError): anchor.resolve(changed)

    def test_empty_target_changed_is_not_a_new_insertion_target(self):
        anchor = Anchor.capture("", (0, 0))
        with self.assertRaises(ValueError): anchor.resolve("user input")

    def test_anchor_boundary_and_insert_affinity(self):
        self.assertEqual(after_edit((4, 2), (2, 0), 3), (7, 2))
        self.assertEqual(after_edit((0, 2), (2, 0), 3), (0, 2))
        self.assertEqual(after_edit((2, 0), (2, 0), 3), (5, 0))


if __name__ == "__main__":
    unittest.main()
