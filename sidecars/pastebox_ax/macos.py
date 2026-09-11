"""macOS AX client through maintained PyObjC bindings. No coordinate targeting."""
from __future__ import annotations
import os
from dataclasses import dataclass
from time import monotonic

import AppKit as AK
import ApplicationServices as AX
import CoreFoundation as CF
import Foundation as NS
import Quartz as Q

from lifecycle import WindowLifecycle
from window_identity import WindowIdentity


class AXFailure(ValueError):
    def __init__(self, operation, code):
        super().__init__(f"辅助功能操作失败：{operation}（AX {code}）")
        self.code = code


def checked(code, operation):
    if code != AX.kAXErrorSuccess:
        raise AXFailure(operation, code)


def attr(element, name):
    code, value = AX.AXUIElementCopyAttributeValue(element, name, None)
    if code in (AX.kAXErrorAttributeUnsupported, AX.kAXErrorNoValue):
        return None
    # AppKit advertises AXIdentifier even for NSTextView instances with no identifier;
    # macOS can return generic failure for this nil, optional attribute. Keep the live
    # AX reference; resolve() still refuses rebinding a rebuilt control without an ID.
    if name == AX.kAXIdentifierAttribute and code == AX.kAXErrorFailure:
        return None
    checked(code, name)
    return value


def parameter(element, name, value):
    code, result = AX.AXUIElementCopyParameterizedAttributeValue(element, name, value, None)
    if code in (AX.kAXErrorParameterizedAttributeUnsupported, AX.kAXErrorNoValue):
        return None
    checked(code, name)
    return result


def settable(element, name):
    code, writable = AX.AXUIElementIsAttributeSettable(element, name, None)
    if code in (AX.kAXErrorAttributeUnsupported, AX.kAXErrorNoValue):
        return False
    checked(code, name)
    return bool(writable)


def set_attr(element, name, value):
    if not settable(element, name):
        raise ValueError(f"目标不支持设置 {name}，请重新记录可编辑位置")
    checked(AX.AXUIElementSetAttributeValue(element, name, value), name)


def is_element(value):
    return value is not None and CF.CFGetTypeID(value) == AX.AXUIElementGetTypeID()


def equal(a, b):
    return a is not None and b is not None and bool(CF.CFEqual(a, b))


def pause(seconds=0.02):
    # Keep NSRunningApplication and AX notifications fresh without blocking Tauri's main thread.
    pool = NS.NSAutoreleasePool.alloc().init()
    try:
        NS.NSRunLoop.currentRunLoop().runUntilDate_(NS.NSDate.dateWithTimeIntervalSinceNow_(seconds))
    finally:
        del pool


@dataclass
class Control:
    app: object
    window: object
    element: object
    identifier: str | None
    role: str
    subrole: str | None
    document_element: object
    document_url: str | None
    window_document: str | None
    launch_date: object
    window_title: str = ""
    marker: object = None
    capture_method: str = "application"
    unavailable_reason: str | None = None
    window_id: int | None = None
    process_token: tuple | None = None
    window_watch: object = None
    availability: str = "available"
    # `exact_ax` retains the original editable AX control. `foreground_paste`
    # deliberately has no control element; the WindowServer identity is the
    # only safe destination exposed by apps such as WeChat.
    capability: str = "exact_ax"


class MacAX:
    def __init__(self):
        self.owner_pid = int(os.environ.get("HVR_OWNER_PID", "0"))
        # The helper is a background AX client, never a focusable application.
        AK.NSApplication.sharedApplication().setActivationPolicy_(AK.NSApplicationActivationPolicyProhibited)
        system = AX.AXUIElementCreateSystemWide()
        checked(AX.AXUIElementSetMessagingTimeout(system, 0.2), "设置 AX 超时")
        self.identity = WindowIdentity()
        self.lifecycle = WindowLifecycle(self.identity.invalidate)

    def retain(self, controls):
        self.lifecycle.retain(control.window_watch for control in controls)

    def close(self):
        self.lifecycle.close()

    def trusted(self):
        return bool(AX.AXIsProcessTrusted())

    def request_permission(self):
        AX.AXIsProcessTrustedWithOptions({AX.kAXTrustedCheckOptionPrompt: True})

    def require_permission(self):
        if not self.trusted():
            raise ValueError("辅助功能服务未获授权，请在系统设置允许 H-VibeRec（开发时可能显示 Python 或终端）")

    def alive(self, control):
        """False means delivery cannot recover this identity. Busy is retryable.

        WindowServer identity is independent of AX visibility/proxy lifetime.
        In particular, querying a background AX tree is not a prerequisite for
        bringing its original window back. No document or selection is rebound here.
        """
        control.unavailable_reason = None
        control.availability = "available"
        def unavailable(reason, state="closed"):
            control.unavailable_reason, control.availability = reason, state
            return state == "unreachable"

        app = AK.NSRunningApplication.runningApplicationWithProcessIdentifier_(control.app.processIdentifier())
        if not app or app.isTerminated():
            return unavailable("原应用进程已退出")
        if control.process_token is not None:
            token = self.identity.process_token(app.processIdentifier())
            if token is None:
                return unavailable("暂时无法核实原应用进程身份", "unreachable")
            if token != control.process_token:
                return unavailable("原应用进程已重新启动")
        elif (control.launch_date is not None and app.launchDate() is not None and
              app.launchDate() != control.launch_date):
            return unavailable("原应用进程启动时间已变化")
        if control.window_id is not None:
            destroyed = control.window_watch is not None and control.window_watch.destroyed
            windows = self.identity.windows(refresh=destroyed)
            owner = windows.get(control.window_id) if windows is not None else None
            if owner == app.processIdentifier():
                return True
            if owner is not None:
                return unavailable("原系统窗口身份已失效")
            if windows is not None and destroyed:
                return unavailable("原窗口已关闭")
            # A missed/unsupported AX notification or a transient enumeration
            # gap must not permanently discard a recorded target.
            return unavailable("原系统窗口暂时不可访问，恢复时将重新核实", "unreachable")
        deadline = monotonic() + 1.0
        while True:
            try:
                if attr(control.window, AX.kAXRoleAttribute) != "AXWindow":
                    control.unavailable_reason = "保存的窗口引用不再返回 AXWindow"
                    control.availability = "unreachable"
                    return False
                windows = attr(AX.AXUIElementCreateApplication(app.processIdentifier()), AX.kAXWindowsAttribute)
                if windows is None:
                    return True
                if any(equal(window, control.window) for window in windows):
                    return True
                control.unavailable_reason = f"原窗口引用未出现在应用窗口列表中（当前 {len(windows)} 个窗口）"
                control.availability = "unreachable"
                return False
            except AXFailure as error:
                # Miniaturization can make even AXRole/AXWindows temporarily busy.
                # An RPC timeout does not establish that the original window closed.
                if error.code != AX.kAXErrorCannotComplete or monotonic() >= deadline:
                    control.unavailable_reason = str(error)
                    control.availability = "unreachable"
                    return False
                pause(0.04)

    def describe(self, control):
        return {"app_name": str(control.app.localizedName() or "应用"),
                "bundle_id": str(control.app.bundleIdentifier() or ""),
                "window_title": control.window_title,
                "process_id": int(control.app.processIdentifier()),
                "window_id": control.window_id,
                "identity_method": "window_server" if control.window_id is not None else "ax_reference",
                "availability": control.availability,
                "unavailable_reason": control.unavailable_reason,
                "control_role": control.role, "capture_method": control.capture_method,
                "capability": control.capability}
                

    def same_window(self, a, b, window_id=None):
        if window_id is not None:
            return (is_element(a) and is_element(b) and
                    self.identity.number(a) == window_id and self.identity.number(b) == window_id)
        return equal(a, b)

    def bind_window(self, control, window):
        if equal(window, control.window):
            return
        # Native text documents use their window as the document root. Web areas
        # retain their independent identity and must still pass resolve().
        if equal(control.document_element, control.window):
            control.document_element = window
        control.window = window
        control.window_watch = self.lifecycle.watch(control.app.processIdentifier(), window)

    def find_window(self, control):
        if control.window_id is not None:
            windows = self.identity.windows(refresh=True)
            if windows is not None and windows.get(control.window_id) != control.app.processIdentifier():
                return None
        candidates = [control.window]
        application = AX.AXUIElementCreateApplication(control.app.processIdentifier())
        candidates.extend(self.focus_attr(application, AX.kAXWindowsAttribute) or [])
        for window in candidates:
            if self.focus_attr(window, AX.kAXRoleAttribute) != "AXWindow":
                continue
            if (self.identity.number(window) == control.window_id if control.window_id is not None
                    else equal(window, control.window)):
                return window
        return None

    def restore_window(self, control, check_time):
        if not self.alive(control):
            raise ValueError(control.unavailable_reason)
        if not control.app.activateWithOptions_(AK.NSApplicationActivateIgnoringOtherApps):
            raise ValueError("无法激活原应用")
        until = monotonic() + 2.5
        while monotonic() < until:
            check_time()
            if not self.alive(control):
                raise ValueError(control.unavailable_reason)
            try:
                window = self.find_window(control)
                if window is not None:
                    self.bind_window(control, window)
                    if attr(window, AX.kAXMinimizedAttribute):
                        set_attr(window, AX.kAXMinimizedAttribute, False)
                    if settable(window, AX.kAXMainAttribute):
                        set_attr(window, AX.kAXMainAttribute, True)
                    checked(AX.AXUIElementPerformAction(window, AX.kAXRaiseAction), "恢复原窗口")
                    application = AX.AXUIElementCreateApplication(control.app.processIdentifier())
                    focused = self.focus_attr(application, AX.kAXFocusedWindowAttribute)
                    if (self.front_pid() == control.app.processIdentifier() and
                            self.same_window(focused, window, control.window_id)):
                        control.availability, control.unavailable_reason = "available", None
                        return
            except AXFailure as error:
                if error.code not in (AX.kAXErrorCannotComplete, AX.kAXErrorInvalidUIElement):
                    raise
            pause(0.04)
        raise ValueError("无法恢复原系统窗口：窗口已关闭或暂时无法访问；内容已保留")

    def front_pid(self):
        front = AK.NSWorkspace.sharedWorkspace().frontmostApplication()
        return front.processIdentifier() if front else 0

    def focus_attr(self, element, name):
        # Some apps expose focus on the window/tree but return CannotComplete on
        # the system-wide or application focus query. Only discovery may fall back.
        if not is_element(element):
            return None
        try:
            return attr(element, name)
        except AXFailure as error:
            if error.code in (AX.kAXErrorCannotComplete, AX.kAXErrorInvalidUIElement):
                return None
            raise

    def element_window(self, element):
        current = element
        for _ in range(32):
            if not is_element(current):
                break
            if self.focus_attr(current, AX.kAXRoleAttribute) == "AXWindow":
                return current
            window = self.focus_attr(current, AX.kAXWindowAttribute)
            # WKWebView can return an AX object with no role for AXWindow.
            # Only accept a real window; the parent chain still reaches NSWindow.
            if is_element(window) and self.focus_attr(window, AX.kAXRoleAttribute) == "AXWindow":
                return window
            current = self.focus_attr(current, AX.kAXParentAttribute)
        return None

    def editable(self, element):
        role = self.focus_attr(element, AX.kAXRoleAttribute)
        if role not in ("AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"):
            if not self.focus_attr(element, "AXEditable"):
                return False
        return settable(element, AX.kAXSelectedTextRangeAttribute)

    def focused_descendant(self, window, deadline):
        # Search only the captured window and require explicit AXFocused. Never
        # infer focus from the first editable field, its title or screen geometry.
        queue, visited, matches = [window], [], []
        while queue and len(visited) < 300 and monotonic() < deadline:
            element = queue.pop()
            if not is_element(element) or any(equal(element, old) for old in visited):
                continue
            visited.append(element)
            if self.focus_attr(element, AX.kAXFocusedAttribute) and self.editable(element):
                matches.append(element)
                if len(matches) > 1:
                    return None
            children = self.focus_attr(element, AX.kAXChildrenAttribute) or []
            if len(children) + len(queue) + len(visited) > 300:
                return None
            queue.extend(child for child in children if is_element(child))
        # A truncated search cannot establish uniqueness.
        return matches[0] if not queue and len(matches) == 1 else None

    def locate_focus(self, application, pid, deadline):
        window = self.focus_attr(application, AX.kAXFocusedWindowAttribute)
        if not is_element(window):
            window = self.focus_attr(application, AX.kAXMainWindowAttribute)
        candidates = [(self.focus_attr(application, AX.kAXFocusedUIElementAttribute), "application")]
        system = AX.AXUIElementCreateSystemWide()
        focused = self.focus_attr(system, AX.kAXFocusedUIElementAttribute)
        if is_element(focused):
            code, focused_pid = AX.AXUIElementGetPid(focused, None)
            if code == AX.kAXErrorSuccess and focused_pid == pid:
                candidates.append((focused, "system"))
        if is_element(window):
            candidates.append((self.focus_attr(window, AX.kAXFocusedUIElementAttribute), "window"))
        for element, source in candidates:
            if not is_element(element) or not self.editable(element):
                continue
            actual_window = self.element_window(element)
            if is_element(actual_window) and (not is_element(window) or equal(actual_window, window)):
                return element, actual_window, source
        if is_element(window) and monotonic() < deadline:
            element = self.focused_descendant(window, deadline)
            if element is not None and equal(self.element_window(element), window):
                return element, window, "focused_descendant"
        return None, window, None

    def frontmost_window(self, application, pid):
        """Find the frontmost WindowServer window for a process.

        A few applications (notably WeChat's Chromium shell) expose an
        AXWindow but omit AXFocusedWindow/AXFocusedUIElement. WindowServer
        still gives us a stable window number, which is enough to guard a
        compatibility Cmd+V delivery. We only choose a single matching
        window; with several windows and no WindowServer bridge we fail safe.
        """
        windows = [w for w in (self.focus_attr(application, AX.kAXWindowsAttribute) or [])
                   if is_element(w) and self.focus_attr(w, AX.kAXRoleAttribute) == "AXWindow"]
        if not windows:
            return None
        numbers = {self.identity.number(w): w for w in windows if self.identity.number(w)}
        try:
            infos = Q.CGWindowListCopyWindowInfo(
                Q.kCGWindowListOptionOnScreenOnly | Q.kCGWindowListExcludeDesktopElements,
                Q.kCGNullWindowID) or []
            for info in infos:
                if int(info.get(Q.kCGWindowOwnerPID, 0)) != pid:
                    continue
                number = int(info.get(Q.kCGWindowNumber, 0))
                if number in numbers:
                    return numbers[number]
        except Exception:
            pass
        return windows[0] if len(windows) == 1 else None

    def frontmost_window_id(self, pid):
        try:
            infos = Q.CGWindowListCopyWindowInfo(
                Q.kCGWindowListOptionOnScreenOnly | Q.kCGWindowListExcludeDesktopElements,
                Q.kCGNullWindowID) or []
            for info in infos:
                if int(info.get(Q.kCGWindowOwnerPID, 0)) == pid and int(info.get(Q.kCGWindowLayer, 0)) == 0:
                    return int(info.get(Q.kCGWindowNumber, 0)) or None
        except Exception:
            return None
        return None

    def enable_accessibility(self, application):
        # Request the richer AX tree only when the target advertises the switch.
        # Do not press a web area or otherwise change focus to discover an editor.
        code, names = AX.AXUIElementCopyAttributeNames(application, None)
        if code != AX.kAXErrorSuccess:
            return False
        changed = False
        for name in ("AXManualAccessibility", "AXEnhancedUserInterface"):
            if name not in (names or []):
                continue
            try:
                if not attr(application, name) and settable(application, name):
                    checked(AX.AXUIElementSetAttributeValue(application, name, True), name)
                    changed = True
            except AXFailure as error:
                if error.code == AX.kAXErrorAPIDisabled:
                    raise
        return changed

    def document(self, element, window):
        current = element
        for _ in range(32):
            if not is_element(current) or equal(current, window):
                break
            if attr(current, AX.kAXRoleAttribute) == "AXWebArea":
                url = attr(current, "AXURL")
                return current, str(url) if url is not None else None
            current = attr(current, AX.kAXParentAttribute)
        return window, None

    def selection(self, control):
        value = attr(control.element, AX.kAXSelectedTextRangeAttribute)
        if value is None or CF.CFGetTypeID(value) != AX.AXValueGetTypeID():
            raise ValueError("控件未提供可校验的文本选区")
        if AX.AXValueGetType(value) != AX.kAXValueCFRangeType:
            raise ValueError("控件的文本选区类型不受支持")
        ok, result = AX.AXValueGetValue(value, AX.kAXValueCFRangeType, None)
        if not ok:
            raise ValueError("无法解析文本选区")
        ranges = attr(control.element, AX.kAXSelectedTextRangesAttribute)
        if ranges is not None and len(ranges) > 1:
            raise ValueError("多个光标或不连续选区暂不支持自动恢复")
        return int(result[0]), int(result[1])

    def read(self, control):
        value = attr(control.element, AX.kAXValueAttribute)
        if not isinstance(value, str):
            count = attr(control.element, AX.kAXNumberOfCharactersAttribute)
            if count is not None and 0 <= int(count) <= 1_000_000:
                value = parameter(control.element, AX.kAXStringForRangeParameterizedAttribute,
                                  AX.AXValueCreate(AX.kAXValueCFRangeType, (0, int(count))))
        if not isinstance(value, str) or len(value) > 1_000_000:
            raise ValueError("无法读取可校验的输入内容，或内容超出支持长度")
        return value

    def check_trigger_input(self, expected_input):
        if expected_input is None:
            return
        current = [int(Q.CGEventSourceCounterForEventType(
            Q.kCGEventSourceStateCombinedSessionState, kind)) for kind in
            (Q.kCGEventKeyDown, Q.kCGEventLeftMouseDown, Q.kCGEventRightMouseDown, Q.kCGEventOtherMouseDown)]
        if current != expected_input:
            raise ValueError("快捷键触发后发生了新的输入或点击，未将后来的位置作为原目标；内容将保留到粘贴箱")

    def capture(self, expected_pid, expected_input=None):
        self.require_permission()
        self.check_trigger_input(expected_input)
        if expected_pid in (0, os.getpid()):
            raise ValueError("前台应用已改变，请在输入框重新记录位置")
        app = AK.NSRunningApplication.runningApplicationWithProcessIdentifier_(expected_pid)
        application = AX.AXUIElementCreateApplication(expected_pid)
        deadline = monotonic() + 1.2
        element, window, method = self.locate_focus(application, expected_pid, deadline)
        enhanced = False
        if not is_element(element) and monotonic() < deadline:
            enhanced = self.enable_accessibility(application)
            while enhanced and monotonic() < deadline:
                pause(0.04)
                if self.front_pid() != expected_pid:
                    raise ValueError("记录期间前台应用已改变，请在原输入框重新记录位置")
                element, window, method = self.locate_focus(application, expected_pid, deadline)
                if is_element(element):
                    break
        if not is_element(window):
            window = self.frontmost_window(application, expected_pid)
        if not is_element(window):
            raise ValueError("当前应用未提供可恢复的窗口")
        if (expected_pid == self.owner_pid and
                attr(window, AX.kAXIdentifierAttribute) != "hvr.pastebox.main"):
            raise ValueError("请在 H-VibeRec 主窗口的编辑区记录位置；录音提示和粘贴箱不能作为目标")
        if not is_element(element):
            # Some cross-platform shells intentionally expose no editable AX
            # node. Preserve the exact window identity and use the same
            # clipboard+Cmd+V behavior as 0.2.0, guarded by frontmost checks.
            identity = getattr(self, "identity", None)
            if identity is None or not getattr(identity, "supported", False):
                raise ValueError("已找到窗口，但未找到有明确焦点且支持文本选区的输入框；内容将保留到粘贴箱")
            control = Control(app, window, None, None, "AXWindow", None,
                              window, None, attr(window, AX.kAXDocumentAttribute),
                              app.launchDate(), capability="foreground_paste")
            control.window_title = str(attr(window, AX.kAXTitleAttribute) or "")
            control.capture_method = "foreground_window"
            control.window_id = identity.number(window)
            control.process_token = identity.process_token(expected_pid)
            if control.window_id is None:
                raise ValueError("当前应用窗口没有可确认的 WindowServer 身份；内容将保留到粘贴箱")
            if getattr(self, "lifecycle", None) is not None:
                control.window_watch = self.lifecycle.watch(expected_pid, window)
            return control, "", (0, 0)
        role = attr(element, AX.kAXRoleAttribute)
        subrole = attr(element, AX.kAXSubroleAttribute)
        if subrole == AX.kAXSecureTextFieldSubrole:
            raise ValueError("密码输入框不支持记录粘贴位置")
        if not settable(element, AX.kAXSelectedTextRangeAttribute):
            raise ValueError("该编辑器未提供可写的文本选区，请使用仅复制")
        document_element, document_url = self.document(element, window)
        control = Control(app, window, element, attr(element, AX.kAXIdentifierAttribute), role,
                          subrole, document_element, document_url,
                          attr(window, AX.kAXDocumentAttribute), app.launchDate())
        control.window_title = str(attr(window, AX.kAXTitleAttribute) or "")
        control.capture_method = ("enhanced_" if enhanced else "") + method
        identity = getattr(self, "identity", None)
        if identity is not None:
            control.window_id = identity.number(window)
            control.process_token = identity.process_token(expected_pid)
        if settable(element, "AXSelectedTextMarkerRange"):
            control.marker = attr(element, "AXSelectedTextMarkerRange")
        value, selection = self.read(control), self.selection(control)
        if (self.front_pid() != expected_pid or not self.focus_matches(control) or
                self.read(control) != value or self.selection(control) != selection):
            raise ValueError("记录期间焦点已改变，请重新记录位置")
        self.check_trigger_input(expected_input)
        if getattr(self, "lifecycle", None) is not None:
            control.window_watch = self.lifecycle.watch(expected_pid, window)
        return control, value, selection

    def same_control(self, a, b):
        if (a.app.processIdentifier() != b.app.processIdentifier() or
                (a.process_token is None and a.launch_date != b.launch_date) or
                a.process_token != b.process_token or
                not self.same_window(a.window, b.window, a.window_id) or
                not (equal(a.document_element, b.document_element) or
                     (equal(a.document_element, a.window) and equal(b.document_element, b.window))) or
                a.document_url != b.document_url or a.window_document != b.window_document):
            return False
        return equal(a.element, b.element)

    def clear_marker(self, control):
        control.marker = None

    def resolve(self, control):
        self.require_permission()
        if not self.alive(control):
            raise ValueError(control.unavailable_reason or "原应用或窗口暂时无法访问")
        # A closed/replaced window is never matched by title alone.
        if attr(control.window, AX.kAXRoleAttribute) != "AXWindow":
            raise ValueError("原窗口已关闭或失效")
        if attr(control.window, AX.kAXDocumentAttribute) != control.window_document:
            raise ValueError("目标窗口的文档已切换")
        try:
            role = attr(control.element, AX.kAXRoleAttribute)
        except AXFailure as error:
            if error.code != AX.kAXErrorInvalidUIElement:
                raise
            role = None
        if role is None:
            self.rebind(control)
        if (attr(control.element, AX.kAXRoleAttribute) != control.role or
                attr(control.element, AX.kAXSubroleAttribute) != control.subrole or
                attr(control.element, AX.kAXIdentifierAttribute) != control.identifier or
                not self.same_window(self.element_window(control.element), control.window, control.window_id)):
            raise ValueError("目标控件身份已变化，请重新记录位置")
        document, url = self.document(control.element, control.window)
        if not equal(document, control.document_element) or url != control.document_url:
            raise ValueError("目标网页或文档已切换，请重新记录位置")
        if not settable(control.element, AX.kAXSelectedTextRangeAttribute):
            raise ValueError("目标选区已不可写")

    def rebind(self, control):
        if not control.identifier:
            raise ValueError("控件已重建且没有稳定标识，请重新记录位置")
        queue, matches, visited = [control.window], [], 0
        deadline = monotonic() + 1.5
        while queue and visited < 400 and monotonic() < deadline:
            element = queue.pop()
            visited += 1
            if (attr(element, AX.kAXIdentifierAttribute) == control.identifier and
                    attr(element, AX.kAXRoleAttribute) == control.role and
                    attr(element, AX.kAXSubroleAttribute) == control.subrole):
                document, url = self.document(element, control.window)
                if equal(document, control.document_element) and url == control.document_url:
                    matches.append(element)
            children = attr(element, AX.kAXChildrenAttribute) or []
            if len(children) + len(queue) + visited > 400:
                raise ValueError("控件树过大，无法唯一确认原目标")
            queue.extend(child for child in children if is_element(child))
        if queue or len(matches) != 1:
            raise ValueError("无法唯一找回原输入控件，请重新记录位置")
        control.element, control.marker = matches[0], None

    def window_matches(self, control):
        pid = control.app.processIdentifier()
        if self.front_pid() != pid:
            return False
        if control.window_id is not None:
            # Prefer WindowServer ordering because AXFocusedWindow is absent
            # in otherwise usable shells such as WeChat.
            if self.frontmost_window_id(pid) == control.window_id:
                return True
            windows = self.identity.windows(refresh=True)
            if windows is not None and windows.get(control.window_id) != pid:
                return False
        application = AX.AXUIElementCreateApplication(pid)
        window = self.focus_attr(application, AX.kAXFocusedWindowAttribute)
        return self.same_window(window, control.window, control.window_id)

    def focus_matches(self, control):
        pid = control.app.processIdentifier()
        if self.front_pid() != pid:
            return False
        app = AX.AXUIElementCreateApplication(pid)
        element, window, _ = self.locate_focus(app, pid, monotonic() + 0.25)
        return equal(element, control.element) and self.same_window(window, control.window, control.window_id)

    def focus(self, control, check_time):
        modifiers = (Q.kCGEventFlagMaskCommand | Q.kCGEventFlagMaskControl |
                     Q.kCGEventFlagMaskAlternate | Q.kCGEventFlagMaskShift)
        until = monotonic() + 1.0
        while Q.CGEventSourceFlagsState(Q.kCGEventSourceStateCombinedSessionState) & modifiers:
            check_time()
            if monotonic() >= until:
                raise ValueError("请松开修饰键后再粘贴")
            pause()
        self.restore_window(control, check_time)
        until = monotonic() + 1.0
        while monotonic() < until:
            check_time()
            try:
                # Document identity is checked before focusing any input field.
                self.resolve(control)
                if not self.focus_matches(control):
                    set_attr(control.element, AX.kAXFocusedAttribute, True)
                if self.focus_matches(control):
                    return
            except AXFailure as error:
                if error.code != AX.kAXErrorCannotComplete:
                    raise
            pause(0.04)
        raise ValueError("无法将键盘焦点恢复到原窗口的输入控件")

    def select(self, control, selection, check_time):
        # Markers are an optional fast path. Always verify the canonical UTF-16 selection.
        if control.marker is not None and settable(control.element, "AXSelectedTextMarkerRange"):
            try:
                set_attr(control.element, "AXSelectedTextMarkerRange", control.marker)
                pause()
            except AXFailure:
                control.marker = None
        if self.selection(control) != selection:
            value = AX.AXValueCreate(AX.kAXValueCFRangeType, selection)
            set_attr(control.element, AX.kAXSelectedTextRangeAttribute, value)
        until = monotonic() + 0.8
        while self.selection(control) != selection or not self.focus_matches(control):
            check_time()
            if monotonic() >= until:
                raise ValueError("目标应用未恢复准确选区")
            pause()

    def paste(self, control, text, expected, validate):
        down = Q.CGEventCreateKeyboardEvent(None, 9, True)
        up = Q.CGEventCreateKeyboardEvent(None, 9, False)
        if down is None or up is None:
            raise ValueError("无法创建粘贴按键")
        Q.CGEventSetFlags(down, Q.kCGEventFlagMaskCommand)
        Q.CGEventSetFlags(up, 0)
        board = AK.NSPasteboard.generalPasteboard()
        previous = []
        initial_change = board.changeCount()
        for item in board.pasteboardItems() or []:
            saved = AK.NSPasteboardItem.alloc().init()
            for kind in item.types():
                data = item.dataForType_(kind)
                if data is not None:
                    saved.setData_forType_(data, kind)
            previous.append(saved)
        validate()
        if board.changeCount() != initial_change:
            raise ValueError("剪贴板正在变化，请稍后重试")
        board.clearContents()
        our_change = board.changeCount()
        dispatched, verified = False, False
        try:
            if not board.setString_forType_(text, AK.NSPasteboardTypeString):
                raise ValueError("无法写入剪贴板")
            our_change = board.changeCount()
            validate()
            # From this point failures must be reported as unverified, never safe-to-retry.
            dispatched = True
            Q.CGEventPost(Q.kCGSessionEventTap, down)
            Q.CGEventPost(Q.kCGSessionEventTap, up)
            until = monotonic() + 1.0
            while monotonic() < until:
                pause()
                if self.read(control) == expected:
                    verified = True
                    break
        except Exception:
            if not dispatched:
                raise
        finally:
            try:
                if board.changeCount() == our_change:
                    board.clearContents()
                    if previous:
                        board.writeObjects_(previous)
            except Exception:
                if not dispatched:
                    raise
                verified = False
        return verified

    def foreground_paste(self, control, text, check_time):
        """Send Cmd+V to a still-frontmost WindowServer window.

        This mirrors the 0.2.0 fallback. Clipboard restoration is best effort;
        once the event is dispatched the receipt is deliberately unverified.
        """
        if not self.window_matches(control):
            raise ValueError("原窗口已不在前台，内容已保留到粘贴箱")
        board = AK.NSPasteboard.generalPasteboard()
        previous = []
        for item in board.pasteboardItems() or []:
            saved = AK.NSPasteboardItem.alloc().init()
            for kind in item.types():
                data = item.dataForType_(kind)
                if data is not None:
                    saved.setData_forType_(data, kind)
            previous.append(saved)
        board.clearContents()
        if not board.setString_forType_(text, AK.NSPasteboardTypeString):
            if previous:
                board.writeObjects_(previous)
            raise ValueError("无法写入剪贴板")
        our_change = board.changeCount()
        check_time()
        if not self.window_matches(control):
            board.clearContents()
            if previous:
                board.writeObjects_(previous)
            raise ValueError("原窗口在粘贴前已失去前台，内容已保留到粘贴箱")
        down = Q.CGEventCreateKeyboardEvent(None, 9, True)
        up = Q.CGEventCreateKeyboardEvent(None, 9, False)
        if down is None or up is None:
            raise ValueError("无法创建粘贴按键")
        Q.CGEventSetFlags(down, Q.kCGEventFlagMaskCommand)
        Q.CGEventPost(Q.kCGSessionEventTap, down)
        Q.CGEventPost(Q.kCGSessionEventTap, up)
        pause(0.5)
        try:
            if board.changeCount() == our_change:
                board.clearContents()
                if previous:
                    board.writeObjects_(previous)
        except Exception:
            pass
        return False
