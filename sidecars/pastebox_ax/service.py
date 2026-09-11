"""Serialized target lifecycle, independent of the macOS binding for behavioral tests."""
from __future__ import annotations
from collections import OrderedDict
from copy import copy
from dataclasses import dataclass
from datetime import datetime, timezone
from time import monotonic

from anchors import Anchor, after_edit, replace, units


@dataclass
class Target:
    id: str
    control: object
    anchor: Anchor
    captured_at: str
    invalid_reason: str | None = None
    return_anchor: Anchor | None = None
    return_error: str | None = None


class TargetService:
    def __init__(self, backend):
        self.backend = backend
        self.targets = OrderedDict()
        self.deadline = 0.0

    def metadata(self, target):
        available = not target.invalid_reason and self.backend.alive(target.control)
        return {"id": target.id, **self.backend.describe(target.control),
                "captured_at": target.captured_at,
                "available": available,
                "selection_location": target.anchor.location,
                "selection_length": target.anchor.length,
                "caret_x": None, "caret_y": None, "window_bounds": None, "caret_offset": None}

    def remember(self, target):
        self.targets[target.id] = target
        while len(self.targets) > 500:
            self.targets.popitem(last=False)
        if hasattr(self.backend, "retain"):
            self.backend.retain(target.control for target in self.targets.values())
        return self.metadata(target)

    def get(self, identifier, returning=False):
        target = self.targets.get(identifier)
        if target is None:
            raise ValueError("目标已失效或辅助进程已重启，请重新记录位置")
        # A consumed/uncertain insertion may no longer be replayed, but its
        # live window is still a valid navigation destination.
        reason = None if returning else target.invalid_reason
        if reason:
            raise ValueError(reason)
        self.backend.require_permission()
        if not self.backend.alive(target.control):
            raise ValueError(getattr(target.control, "unavailable_reason", None) or
                             "原应用或窗口已关闭、失效或暂时无法访问，请重新记录位置")
        return target

    def ensure_time(self):
        if monotonic() >= self.deadline:
            raise ValueError("恢复目标超时，内容已保留")

    def capture(self, identifier, expected_pid, expected_input=None):
        if expected_input is None:
            control, value, selection = self.backend.capture(expected_pid)
        else:
            control, value, selection = self.backend.capture(expected_pid, expected_input)
        return self.remember(Target(identifier, control, Anchor.capture(value, selection),
                                    datetime.now(timezone.utc).isoformat()))

    def clone(self, source, identifier):
        original = self.get(source)
        # Copy the recorded anchor, including its original context.
        # Do not require the inactive application's AX tree to be readable, and
        # never adopt its later selection while cloning a recording job.
        anchor = copy(original.anchor)
        # Bindings are copied so resolving one job cannot change another job's identity.
        return self.remember(Target(identifier, copy(original.control), anchor, original.captured_at))

    def paste(self, identifier, text):
        target = self.get(identifier)
        self.backend.require_permission()
        if getattr(target.control, "capability", "exact_ax") == "foreground_paste":
            return self.foreground_paste(target, text)
        # Bring back the exact window before reading background-only AX objects.
        # focus() validates document/control identity before setting input focus.
        # Electron editors can expose a transient AX control that cannot regain
        # keyboard focus after a window switch. If the original WindowServer
        # window can still be identified, use the 0.2.0 clipboard fallback
        # instead of failing before Cmd+V is dispatched.
        try:
            self.backend.focus(target.control, self.ensure_time)
        except ValueError as error:
            if ("无法将键盘焦点恢复" in str(error) and
                    self.backend.window_matches(target.control) and
                    hasattr(self.backend, "foreground_paste")):
                return self.foreground_paste(target, text)
            raise
        self.backend.resolve(target.control)
        before = self.backend.read(target.control)
        selection = target.anchor.resolve(before)
        self.ensure_time()
        self.backend.select(target.control, selection, self.ensure_time)
        self.backend.resolve(target.control)
        expected = replace(before, selection, text)

        def validate():
            self.ensure_time()
            if (not self.backend.focus_matches(target.control) or
                    self.backend.selection(target.control) != selection or
                    self.backend.read(target.control) != before):
                raise ValueError("粘贴前目标或文字已变化，已停止粘贴")

        # Backend must not raise after it may have dispatched input: that outcome is uncertain.
        verified = self.backend.paste(target.control, text, expected, validate)
        for other in self.targets.values():
            if not self.backend.same_control(other.control, target.control):
                continue
            if other.return_anchor is not None and not other.return_error:
                try:
                    if not verified:
                        raise ValueError("后续粘贴未确认，无法确定原返回位置")
                    position = other.return_anchor.resolve(before)
                    # Returning to item A must stay at A's end when item B is
                    # subsequently appended at exactly that point.
                    updated = position if position == (selection[0], 0) and selection[1] == 0 else after_edit(position, selection, units(text))
                    other.return_anchor = Anchor.capture(expected, updated)
                except ValueError as error:
                    other.return_error = str(error)
            if other.invalid_reason:
                continue
            if not verified:
                other.invalid_reason = "上次粘贴结果未确认，请检查内容并重新记录位置"
                continue
            try:
                updated = after_edit(other.anchor.resolve(before), selection, units(text))
                other.anchor = Anchor.capture(expected, updated)
                self.backend.clear_marker(other.control)
            except ValueError as error:
                other.invalid_reason = str(error)
        # Keep the expected insertion end even when immediate readback times
        # out. It is only a candidate: restore must verify it against live text.
        # This never upgrades the delivery receipt or allows another paste.
        target.return_anchor = Anchor.capture(expected, (selection[0] + units(text), 0))
        target.return_error = None
        return {"verified": verified, "message": "已粘贴并校验文字" if verified else
                "已发送粘贴，结果未确认；请检查目标内容，不会自动重试"}

    def foreground_paste(self, target, text):
        """Compatibility delivery for AX-opaque apps (for example WeChat).

        The window identity is still checked immediately before dispatch. No
        AX control or caret claim is made, so the receipt remains unverified.
        """
        self.ensure_time()
        # Re-activate the exact WindowServer window before sending Cmd+V. This
        # is what makes the compatibility path useful when the user moved to
        # another app or VS Code window while transcription was running.
        self.backend.restore_window(target.control, self.ensure_time)
        if not self.backend.window_matches(target.control):
            raise ValueError("原窗口已不在前台，内容已保留到粘贴箱")
        verified = self.backend.foreground_paste(target.control, text, self.ensure_time)
        return {"verified": bool(verified),
                "message": "已发送到原前台窗口，系统未提供可读取的文本控件；请检查内容"}

    def restore(self, identifier):
        target = self.get(identifier, returning=True)
        if getattr(target.control, "capability", "exact_ax") == "foreground_paste":
            self.backend.restore_window(target.control, self.ensure_time)
            return {"restored": True, "caret_restored": False,
                    "message": "已返回原窗口；该应用未提供可恢复的光标"}
        self.backend.restore_window(target.control, self.ensure_time)
        try:
            self.backend.focus(target.control, self.ensure_time)
            self.backend.resolve(target.control)
            reason = target.return_error if target.return_anchor else target.invalid_reason
            if reason:
                raise ValueError(reason)
            before = self.backend.read(target.control)
            selection = (target.return_anchor or target.anchor).resolve(before)
            self.backend.select(target.control, selection, self.ensure_time)
            self.backend.resolve(target.control)
            if (not self.backend.focus_matches(target.control) or
                    self.backend.selection(target.control) != selection or self.backend.read(target.control) != before):
                raise ValueError("返回位置时目标或文字已变化")
        except ValueError as error:
            # Keep the exact window in front when only text/caret verification
            # fails. Never substitute the current window or issue input.
            if not self.backend.window_matches(target.control):
                raise ValueError("返回期间前台窗口已改变，未继续移动光标") from error
            return {"restored": True, "caret_restored": False,
                    "message": "已返回原窗口，光标位置未确认", "reason": str(error)}
        return {"restored": True, "caret_restored": True, "message": "已返回对应光标位置"}

    def handle(self, request):
        self.deadline = monotonic() + 6.0
        op = request["op"]
        if op == "status":
            trusted = self.backend.trusted()
            values = [self.metadata(t) for t in self.targets.values()]
            return {"engine": "pyobjc", "accessibility_trusted": trusted,
                    "targets": values, "available_ids": [t["id"] for t in values if trusted and t["available"]]}
        if op == "accessibility_permission":
            self.backend.request_permission()
            return {"accessibility_trusted": self.backend.trusted()}
        if op == "capture":
            return self.capture(request["id"], request["expected_pid"], request.get("expected_input"))
        if op == "clone":
            return self.clone(request["source"], request["id"])
        if op == "paste":
            if not isinstance(request.get("text"), str) or not request["text"]:
                raise ValueError("粘贴内容为空")
            return self.paste(request["target_id"], request["text"])
        if op == "restore":
            return self.restore(request["target_id"])
        raise ValueError("未知辅助功能操作")
