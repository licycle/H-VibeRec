"""Real persistent AX helper + disposable native/WebKit fixtures, no user data.

Checks delayed delivery, hidden/minimized windows, duplicate window titles and
closed-window rejection through the production JSONL protocol. No ASR is used.
"""
import json
import select
import subprocess
import sys
import time
from pathlib import Path
from uuid import uuid4


class Peer:
    def __init__(self, args):
        self.child = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      text=True, bufsize=1)

    def read(self):
        if not select.select([self.child.stdout], [], [], 15)[0]:
            raise RuntimeError("Isolated test peer timed out")
        return json.loads(self.child.stdout.readline())

    def request(self, **request):
        self.child.stdin.write(json.dumps(request) + "\n")
        self.child.stdin.flush()
        return self.read()

    def stop(self):
        self.child.terminate()
        self.child.wait(timeout=5)


def main():
    executable = Path(sys.argv[1]).resolve()
    if executable.name != "AXFixture" or not executable.is_file():
        raise ValueError("Expected an explicitly supplied AXFixture executable")
    root = Path(__file__).resolve().parents[2]
    peers = []
    results = []
    try:
        helper = Peer([sys.executable, "-u", "-B", "-s", str(root / "sidecars/pastebox_ax/main.py")])
        peers.append(helper)
        def call(**request):
            response = helper.request(request_id=str(uuid4()), **request)
            if "error" in response:
                raise ValueError(response["error"])
            return response["result"]
        assert call(op="status")["accessibility_trusted"]
        for web in (False, True):
            fixture = Peer([str(executable), "HVR window identity " + str(uuid4()), "--audit"] +
                           (["--web"] if web else []))
            peers.append(fixture)
            ready = fixture.read()
            assert ready["ready"] and ready["pid"] == fixture.child.pid
            identity = fixture.request(op="identity")
            time.sleep(0.4)
            identifier = "web" if web else "native"
            target = call(op="capture", id=identifier, expected_pid=fixture.child.pid)
            assert target["window_id"] == identity["window_id"] > 0, target
            assert target["selection_location"] == 3 and target["selection_length"] == 0
            fixture.request(op="select", location=0)
            fixture.request(op="move")
            fixture.request(op="other_window", same_title=True)
            # The production helper receives no requests while we wait. Its AX
            # run loop must keep processing notifications during this interval.
            time.sleep(5)
            outcome = call(op="paste", target_id=identifier, text="【延迟】")
            assert outcome["verified"], outcome
            assert fixture.request(op="read")["text"] == "甲😀【延迟】乙"
            assert fixture.request(op="read_other")["text"] == "另一个窗口"
            results.append({"case": identifier + "_idle_duplicate_title", "window_id": target["window_id"]})
            for operation in ("hide", "minimize"):
                fixture.request(op=operation)
                time.sleep(0.4)
                assert call(op="paste", target_id=identifier, text="【恢复】")["verified"]
                results.append({"case": identifier + "_" + operation, "window_id": target["window_id"]})
            before_other = fixture.request(op="read_other")["text"]
            fixture.request(op="close")
            time.sleep(0.4)
            try:
                call(op="paste", target_id=identifier, text="禁止写入")
            except ValueError as error:
                assert "窗口" in str(error), str(error)
            else:
                raise AssertionError("Closed target accepted")
            assert fixture.request(op="read_other")["text"] == before_other
            results.append({"case": identifier + "_closed_no_wrong_window"})
            print(json.dumps({"passed": True, "completed": results}, ensure_ascii=False), flush=True)
    finally:
        for peer in reversed(peers):
            peer.stop()


if __name__ == "__main__":
    main()
