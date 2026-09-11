"""Native window/process identities, isolated from AX target and text handling.

CGWindowListCopyWindowInfo and proc_pidinfo are public APIs. The AX -> window
number bridge is the optional, undocumented symbol used by Hammerspoon; load it
dynamically so its absence never prevents the helper from starting. A missing
bridge falls back to retained AX references, never title/position matching.
"""
import ctypes
from time import monotonic

import Foundation as NS
import Quartz as Q
import objc


class BSDProcessInfo(ctypes.Structure):
    # sys/proc_info.h: proc_bsdinfo (MAXCOMLEN = 16), including padding.
    _fields_ = [(name, ctypes.c_uint32) for name in (
        "flags", "status", "xstatus", "pid", "ppid", "uid", "gid", "ruid",
        "rgid", "svuid", "svgid", "reserved",
    )] + [("comm", ctypes.c_char * 16), ("name", ctypes.c_char * 32)] + [
        (name, ctypes.c_uint32) for name in (
            "nfiles", "pgid", "pjobc", "tdev", "tpgid",
        )
    ] + [("nice", ctypes.c_int32), ("start_seconds", ctypes.c_uint64),
         ("start_microseconds", ctypes.c_uint64)]


class WindowIdentity:
    def __init__(self):
        functions = {}
        try:
            bundle = NS.NSBundle.bundleWithPath_("/System/Library/Frameworks/ApplicationServices.framework")
            objc.loadBundleFunctions(bundle, functions, [
                ("_AXUIElementGetWindow", b"i^{__AXUIElement=}o^I", ""),
            ])
        except (objc.error, ValueError):
            pass
        self._number = functions.get("_AXUIElementGetWindow")
        self._proc = ctypes.CDLL("/usr/lib/libproc.dylib").proc_pidinfo
        self._proc.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                               ctypes.c_void_p, ctypes.c_int]
        self._proc.restype = ctypes.c_int
        self._snapshot = None
        self._snapshot_at = 0.0

    @property
    def supported(self):
        return self._number is not None

    def number(self, element):
        if self._number is None or element is None:
            return None
        code, number = self._number(element, None)
        return int(number) if code == 0 and number else None

    def process_token(self, pid):
        info = BSDProcessInfo()
        size = ctypes.sizeof(info)
        if self._proc(pid, 3, 0, ctypes.byref(info), size) != size or info.pid != pid:
            return None
        return info.start_seconds, info.start_microseconds

    def invalidate(self):
        self._snapshot_at = 0.0

    def windows(self, refresh=False):
        # One listing serves a panel's many queued targets. Never restrict this
        # to on-screen windows: covered, minimized and other-Space windows count.
        now = monotonic()
        if refresh or now - self._snapshot_at > 0.05:
            values = Q.CGWindowListCopyWindowInfo(Q.kCGWindowListOptionAll, Q.kCGNullWindowID)
            self._snapshot = None if values is None else {
                int(value[Q.kCGWindowNumber]): int(value[Q.kCGWindowOwnerPID]) for value in values
            }
            self._snapshot_at = now
        return self._snapshot
