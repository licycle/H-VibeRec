"""Run real AX capture/restore against two disposable fixture processes only.

Usage: runtime/asr/bin/python -B scripts/debug/check-ax-fixture.py /absolute/path/AXFixture
No transcripts, user documents, or stored settings are used.
"""
import subprocess
import sys
import time
from pathlib import Path
from uuid import uuid4

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sidecars/pastebox_ax"))
from macos import AX, MacAX, attr, pause
from service import TargetService


def wait_front(backend, pid):
    until = time.monotonic() + 8
    while backend.front_pid() != pid:
        if time.monotonic() >= until:
            raise RuntimeError(f"Fixture did not become frontmost: expected={pid}, actual={backend.front_pid()}")
        pause(0.03)


def main():
    executable = Path(sys.argv[1]).resolve()
    if executable.name != "AXFixture" or not executable.is_file():
        raise ValueError("Expected the compiled AXFixture test executable")
    fixtures = []
    token = "HVR AX Fixture " + str(uuid4())
    backend = MacAX()
    service = TargetService(backend)
    try:
        first = subprocess.Popen([str(executable), token + " A"])
        fixtures.append(first)
        wait_front(backend, first.pid)
        pause(0.15)
        try:
            captured = service.handle({"op": "capture", "id": "fixture-only", "expected_pid": first.pid})
        except Exception:
            element = attr(AX.AXUIElementCreateApplication(first.pid), AX.kAXFocusedUIElementAttribute)
            print("Fixture supported attributes:", AX.AXUIElementCopyAttributeNames(element, None), flush=True)
            raise
        control = service.targets["fixture-only"].control
        if control.window_title != token + " A" or backend.read(control) != "甲😀乙":
            raise RuntimeError("Fixture identity mismatch; refusing to paste")
        assert (captured["selection_location"], captured["selection_length"]) == (3, 0), captured
        print("PASS: native text control captured at UTF-16 offset 3", flush=True)

        second = subprocess.Popen([str(executable), token + " B"])
        fixtures.append(second)
        wait_front(backend, second.pid)
        pause(0.15)
        result = service.handle({"op": "paste", "target_id": "fixture-only", "text": "【AX测试】"})
        assert result["verified"], {**result, "fixture_text": backend.read(control), "front_pid": backend.front_pid()}
        assert backend.read(control) == "甲😀【AX测试】乙"
        assert backend.front_pid() == first.pid
        print("PASS: restored the original app and exact caret after switching apps; paste read back", flush=True)
    finally:
        for fixture in fixtures:
            fixture.terminate()
            fixture.wait(timeout=5)


if __name__ == "__main__":
    main()
