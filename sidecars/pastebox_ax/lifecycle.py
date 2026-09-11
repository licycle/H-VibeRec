"""Bounded, event-driven AX window lifetimes. Call only on the AX run-loop thread.

Notifications invalidate window queries; they never change a recorded selection
or bind a job to the newly focused app. Some applications do not implement every
notification, so restoration also checks native window/process identities.
"""
from dataclasses import dataclass, field

import ApplicationServices as AX
import CoreFoundation as CF
import objc


@dataclass(eq=False)
class WindowWatch:
    pid: int
    element: object
    destroyed: bool = False
    notifications: list = field(default_factory=list)


class WindowLifecycle:
    def __init__(self, invalidate):
        self.invalidate = invalidate
        self.observers = {}
        self.watches = []
        # Keep the Python callback alive for as long as its C observers.
        @objc.callbackFor(AX.AXObserverCreate)
        def callback(observer, element, notification, context):
            self._changed(observer, element, notification, context)
        self.callback = callback

    def _changed(self, observer, element, notification, context):
        self.invalidate()
        if notification == AX.kAXUIElementDestroyedNotification:
            for watch in self.watches:
                if CF.CFEqual(watch.element, element):
                    watch.destroyed = True

    def watch(self, pid, element):
        for watch in self.watches:
            if watch.pid == pid and not watch.destroyed and CF.CFEqual(watch.element, element):
                return watch
        watch = WindowWatch(pid, element)
        self.watches.append(watch)
        observer = self.observers.get(pid)
        if observer is None:
            code, observer = AX.AXObserverCreate(pid, self.callback, None)
            if code != AX.kAXErrorSuccess:
                return watch
            self.observers[pid] = observer
            CF.CFRunLoopAddSource(CF.CFRunLoopGetCurrent(), AX.AXObserverGetRunLoopSource(observer),
                                 CF.kCFRunLoopDefaultMode)
        for event in (AX.kAXUIElementDestroyedNotification, AX.kAXWindowMovedNotification,
                      AX.kAXWindowMiniaturizedNotification, AX.kAXWindowDeminiaturizedNotification):
            code = AX.AXObserverAddNotification(observer, element, event, None)
            if code == AX.kAXErrorSuccess:
                watch.notifications.append(event)
        return watch

    def retain(self, watches):
        keep = {id(watch) for watch in watches if watch is not None}
        for watch in self.watches:
            if id(watch) not in keep:
                observer = self.observers.get(watch.pid)
                if observer is not None:
                    for event in watch.notifications:
                        AX.AXObserverRemoveNotification(observer, watch.element, event)
        self.watches = [watch for watch in self.watches if id(watch) in keep]
        active_pids = {watch.pid for watch in self.watches}
        for pid in list(self.observers):
            if pid not in active_pids:
                observer = self.observers.pop(pid)
                CF.CFRunLoopRemoveSource(CF.CFRunLoopGetCurrent(), AX.AXObserverGetRunLoopSource(observer),
                                        CF.kCFRunLoopDefaultMode)

    def close(self):
        self.retain([])
