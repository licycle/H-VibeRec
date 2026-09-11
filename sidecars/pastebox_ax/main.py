#!/usr/bin/env python3
"""Private stdin/stdout JSONL protocol. Only opaque target IDs leave this process."""
import json
import os
import sys
from queue import Empty, Queue
from threading import Thread


def input_lines(stream, pump):
    """Wait for JSONL off-thread; keep AX/AppKit notifications running when idle."""
    pending = Queue(maxsize=16)

    def read():
        try:
            for line in stream:
                pending.put(line)
        finally:
            pending.put(None)

    Thread(target=read, name="pastebox-input", daemon=True).start()
    while True:
        pump(0.001)
        try:
            line = pending.get_nowait()
        except Empty:
            pump(0.02)
            continue
        if line is None:
            return
        yield line


def main():
    from macos import MacAX, NS, pause
    from service import TargetService

    service = TargetService(MacAX())
    if "--check" in sys.argv:
        # Exercises the real PyObjC CFRange bridge without needing AX permission or reading apps.
        import ApplicationServices as AX
        value = AX.AXValueCreate(AX.kAXValueCFRangeType, (2, 3))
        ok, restored = AX.AXValueGetValue(value, AX.kAXValueCFRangeType, None)
        if not ok or tuple(restored) != (2, 3):
            raise RuntimeError("PyObjC CFRange round trip failed")
        print(json.dumps({"engine": "pyobjc", "cf_range": "ok",
                          "window_id_bridge": service.backend.identity.supported,
                          "process_identity": service.backend.identity.process_token(os.getpid()) is not None,
                          "accessibility_trusted": service.backend.trusted()}))
        return
    for line in input_lines(sys.stdin, pause):
        request_id = None
        pool = NS.NSAutoreleasePool.alloc().init()
        try:
            pause(0.001)
            request = json.loads(line)
            request_id = request["request_id"]
            result = {"request_id": request_id, "result": service.handle(request)}
        except Exception as error:
            # Never include the request, target context or external text in errors/logs.
            result = {"request_id": request_id, "error": str(error)}
        finally:
            service.backend.retain(target.control for target in service.targets.values())
            del pool
        print(json.dumps(result, ensure_ascii=False), flush=True)
    service.backend.close()


if __name__ == "__main__":
    main()
