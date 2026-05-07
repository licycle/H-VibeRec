import importlib.util
import asyncio
import json
import os
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
MODULE_DIR = REPO_ROOT / "sidecars" / "local_notes_agent"
MAIN_PATH = MODULE_DIR / "main.py"
TOOLS_PATH = MODULE_DIR / "notes_mcp_server.py"
WEB_TOOLS_PATH = MODULE_DIR / "web_mcp_server.py"
RUNTIME_PYTHON = REPO_ROOT / "runtime" / "asr" / "bin" / "python"
SYSTEM_PYTHON = Path(shutil.which("python3") or sys.executable)


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


agent = load_module("local_notes_agent", MAIN_PATH)
tools = load_module("local_notes_tools", TOOLS_PATH)


def make_note(root: Path, workspace: str, name: str, content: str) -> Path:
    notes = root / workspace / "notes"
    notes.mkdir(parents=True, exist_ok=True)
    path = notes / name
    path.write_text(content, encoding="utf-8")
    return path


def root_entry(root: Path, workspace: str) -> dict:
    return {
        "workspace_folder": workspace,
        "notes_dir": str((root / workspace / "notes").resolve()),
    }


class LocalNotesAgentTests(unittest.TestCase):
    def test_prepare_agent_session_uses_persistent_sqlite_and_seeds_empty_history(self):
        created = []

        class FakeSession:
            def __init__(self, session_id, db_path, sessions_table, messages_table):
                self.session_id = session_id
                self.db_path = db_path
                self.sessions_table = sessions_table
                self.messages_table = messages_table
                self.items = []
                self.cleared = False
                created.append(self)

            async def get_items(self, limit=None):
                return list(self.items)

            async def add_items(self, items):
                self.items.extend(items)

            async def clear_session(self):
                self.cleared = True
                self.items.clear()

        with tempfile.TemporaryDirectory() as tmp:
            payload = {
                "session_id": "session-1",
                "session_db_path": str(Path(tmp) / "app.sqlite"),
                "session_reset_to_history": False,
                "history": [{"role": "user", "content": "之前的问题"}],
            }

            session = asyncio.run(agent.prepare_agent_session(payload, FakeSession))

        self.assertIs(session, created[0])
        self.assertEqual(session.db_path, payload["session_db_path"])
        self.assertEqual(session.sessions_table, "assistant_agent_sessions")
        self.assertEqual(session.messages_table, "assistant_agent_items")
        self.assertFalse(session.cleared)
        self.assertEqual(session.items, [{"role": "user", "content": "之前的问题"}])

    def test_prepare_agent_session_resets_to_history_for_reused_message(self):
        class FakeSession:
            def __init__(self, *args, **kwargs):
                self.items = [{"role": "user", "content": "stale"}]
                self.cleared = False

            async def get_items(self, limit=None):
                return list(self.items)

            async def add_items(self, items):
                self.items.extend(items)

            async def clear_session(self):
                self.cleared = True
                self.items.clear()

        with tempfile.TemporaryDirectory() as tmp:
            payload = {
                "session_id": "session-1",
                "session_db_path": str(Path(tmp) / "app.sqlite"),
                "session_reset_to_history": True,
                "history": [{"role": "assistant", "content": "保留的回答"}],
            }

            session = asyncio.run(agent.prepare_agent_session(payload, FakeSession))

        self.assertTrue(session.cleared)
        self.assertEqual(session.items, [{"role": "assistant", "content": "保留的回答"}])

    def test_build_agent_input_uses_only_current_question_when_sdk_session_exists(self):
        payload = {
            "scope": "current",
            "workspace_folder": "space-a",
            "question": "当前问题",
            "web_enabled": False,
        }

        current_input = agent.build_agent_input(payload)

        self.assertIsInstance(current_input, str)
        self.assertIn("当前问题", current_input)
        self.assertIn("实际 workspace_folder：space-a", current_input)
        self.assertIn("不要把 current 当成 workspace_folder", current_input)
        self.assertNotIn("之前的问题", current_input)

    def test_run_assistant_does_not_request_sdk_output_type(self):
        captured = {}

        class FakeAgent:
            def __init__(self, **kwargs):
                captured["agent_kwargs"] = kwargs
                if "output_type" in kwargs:
                    raise AssertionError("output_type should not be passed")

        class FakeModelSettings:
            def __init__(self, **kwargs):
                self.kwargs = kwargs

        class FakeRunner:
            @staticmethod
            async def run(agent_instance, input_value, max_turns, session=None):
                captured["input_value"] = input_value
                captured["session"] = session

                class Item:
                    type = "tool_call_output_item"

                    def __init__(self):
                        self.output = {
                            "tool": "read_note_file",
                            "path": "space-a/notes/n1__Alpha.md",
                            "note_id": "n1",
                            "title": "Alpha",
                            "workspace_folder": "space-a",
                        }

                class Result:
                    final_output = "测试回答"
                    new_items = [Item()]

                return Result()

        class FakeSQLiteSession:
            pass

        class FakeChatCompletionsModel:
            def __init__(self, **kwargs):
                self.kwargs = kwargs

        class FakeMCPServerStdio:
            def __init__(self, *args, **kwargs):
                self.args = args
                self.kwargs = kwargs

        class FakeMCPServerManager:
            def __init__(self, servers, connect_in_parallel=False):
                self.active_servers = servers

            async def __aenter__(self):
                return self

            async def __aexit__(self, exc_type, exc, traceback):
                return False

        class FakeAsyncOpenAI:
            def __init__(self, **kwargs):
                self.kwargs = kwargs

        module_names = [
            "agents",
            "agents.models",
            "agents.models.openai_chatcompletions",
            "agents.mcp",
            "openai",
        ]
        originals = {name: sys.modules.get(name) for name in module_names}
        try:
            agents_module = types.ModuleType("agents")
            agents_module.Agent = FakeAgent
            agents_module.ModelSettings = FakeModelSettings
            agents_module.Runner = FakeRunner
            agents_module.SQLiteSession = FakeSQLiteSession
            sys.modules["agents"] = agents_module

            sys.modules["agents.models"] = types.ModuleType("agents.models")
            chat_module = types.ModuleType("agents.models.openai_chatcompletions")
            chat_module.OpenAIChatCompletionsModel = FakeChatCompletionsModel
            sys.modules["agents.models.openai_chatcompletions"] = chat_module

            mcp_module = types.ModuleType("agents.mcp")
            mcp_module.MCPServerManager = FakeMCPServerManager
            mcp_module.MCPServerStdio = FakeMCPServerStdio
            sys.modules["agents.mcp"] = mcp_module

            openai_module = types.ModuleType("openai")
            openai_module.AsyncOpenAI = FakeAsyncOpenAI
            sys.modules["openai"] = openai_module

            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                make_note(root, "space-a", "n1__Alpha.md", "苹果 项目 预算")
                payload = {
                    "scope": "current",
                    "workspace_folder": "space-a",
                    "question": "苹果",
                    "workspace_roots": [root_entry(root, "space-a")],
                    "history": [],
                    "llm": {},
                    "prompt": "请基于笔记回答",
                    "max_turns": 4,
                }

                result = asyncio.run(agent.run_assistant(payload))

            self.assertEqual(result["answer"], "测试回答")
            self.assertNotIn("output_type", captured["agent_kwargs"])
            self.assertIsNone(captured["session"])
        finally:
            for name, original in originals.items():
                if original is None:
                    sys.modules.pop(name, None)
                else:
                    sys.modules[name] = original

    def test_validate_payload_rejects_non_boolean_web_enabled(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = {
                "scope": "current",
                "workspace_folder": "space-a",
                "question": "问题",
                "workspace_roots": [root_entry(root, "space-a")],
                "history": [],
                "llm": {},
                "prompt": "请回答",
                "web_enabled": "yes",
            }

            with self.assertRaises(agent.SidecarError) as raised:
                agent.validate_payload(payload)
            self.assertEqual(raised.exception.code, "INVALID_WEB_ENABLED")

    def test_validate_payload_defaults_and_accepts_unbounded_positive_max_turns(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = {
                "scope": "current",
                "workspace_folder": "space-a",
                "question": "问题",
                "workspace_roots": [root_entry(root, "space-a")],
                "history": [],
                "llm": {},
                "prompt": "请回答",
            }

            validated = agent.validate_payload(payload)
            self.assertEqual(validated["max_turns"], agent.DEFAULT_MAX_TURNS)

            payload["max_turns"] = 128
            validated = agent.validate_payload(payload)
            self.assertEqual(validated["max_turns"], 128)

    def test_validate_payload_rejects_non_positive_max_turns(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = {
                "scope": "current",
                "workspace_folder": "space-a",
                "question": "问题",
                "workspace_roots": [root_entry(root, "space-a")],
                "history": [],
                "llm": {},
                "prompt": "请回答",
                "max_turns": 0,
            }

            with self.assertRaises(agent.SidecarError) as raised:
                agent.validate_payload(payload)
            self.assertEqual(raised.exception.code, "INVALID_MAX_TURNS")

    def test_detects_agents_max_turns_error(self):
        error = RuntimeError("Max turns (10) exceeded")

        self.assertTrue(agent.is_max_turns_error(error))

    def test_turn_status_payload_uses_string_max_turns(self):
        class Result:
            current_turn = 3
            max_turns = 123456789012345678901234567890

        payload = agent.turn_status_payload(Result())

        self.assertEqual(payload["current_turn"], 3)
        self.assertEqual(payload["max_turns"], "123456789012345678901234567890")

    def test_build_answer_prompt_mentions_web_tools_when_enabled(self):
        prompt = agent.build_answer_prompt("global", "网页问题", web_enabled=True)

        self.assertIn("web_search", prompt)
        self.assertIn("read_web_page", prompt)
        self.assertIn("网页", prompt)
        self.assertIn("引用来源", prompt)
        self.assertNotIn("结构化结果", prompt)

    def test_assistant_sources_from_items_extracts_web_sources(self):
        class Item:
            type = "tool_call_output_item"

            def __init__(self, output):
                self.output = output

        class Result:
            new_items = [Item({
                "tool": "read_web_page",
                "url": "https://example.com",
                "title": "Example",
                "snippet": "摘要",
                "source_type": "web",
            })]

        sources = agent.assistant_sources_from_items(Result())

        self.assertEqual(len(sources), 1)
        self.assertEqual(sources[0]["type"], "web")
        self.assertEqual(sources[0]["url"], "https://example.com")

    def test_build_web_mcp_server_includes_search_and_proxy_env(self):
        captured = {}

        class FakeServerClass:
            def __init__(self, config, cache_tools_list, name, use_structured_content):
                captured["config"] = config
                captured["cache_tools_list"] = cache_tools_list
                captured["name"] = name
                captured["use_structured_content"] = use_structured_content

        original_search = os.environ.get("VOICE_VIBE_SEARXNG_URL")
        original_proxy = os.environ.get("https_proxy")
        original_ddgs_proxy = os.environ.get("DDGS_PROXY")
        try:
            os.environ["VOICE_VIBE_SEARXNG_URL"] = "https://searx.example"
            os.environ["https_proxy"] = "http://127.0.0.1:1087"
            os.environ.pop("DDGS_PROXY", None)

            server = agent.build_web_mcp_server(FakeServerClass)

            self.assertIsNotNone(server)
            env = captured["config"]["env"]
            self.assertEqual(env["VOICE_VIBE_SEARXNG_URL"], "https://searx.example")
            self.assertEqual(env["https_proxy"], "http://127.0.0.1:1087")
            self.assertEqual(env["DDGS_PROXY"], "http://127.0.0.1:1087")
            self.assertEqual(env.get("HOME"), os.environ.get("HOME"))
            self.assertEqual(captured["name"], "web-tools")
        finally:
            if original_search is None:
                os.environ.pop("VOICE_VIBE_SEARXNG_URL", None)
            else:
                os.environ["VOICE_VIBE_SEARXNG_URL"] = original_search
            if original_proxy is None:
                os.environ.pop("https_proxy", None)
            else:
                os.environ["https_proxy"] = original_proxy
            if original_ddgs_proxy is None:
                os.environ.pop("DDGS_PROXY", None)
            else:
                os.environ["DDGS_PROXY"] = original_ddgs_proxy

    def test_grep_notes_searches_only_current_workspace(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "a1__Alpha.md", "苹果 项目 预算")
            make_note(root, "space-b", "b1__Beta.md", "苹果 私密 信息")
            store = tools.NotesToolStore([root_entry(root, "space-a")])

            matches = store.grep_notes("苹果")

            self.assertEqual(len(matches), 1)
            self.assertEqual(matches[0]["workspace_folder"], "space-a")
            self.assertEqual(matches[0]["note_id"], "a1")
            self.assertEqual(matches[0]["path"], "space-a/notes/a1__Alpha.md")

    def test_current_alias_maps_to_single_allowed_workspace(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "a1__Alpha.md", "苹果 项目 预算")
            store = tools.NotesToolStore([root_entry(root, "space-a")])

            listed = store.list_notes(workspace_folder="current")
            matches = store.grep_notes("苹果", workspace_folder="current")
            note = store.read_note_file(
                "notes/a1__Alpha.md",
                workspace_folder="current",
                start_line=1,
                line_count=1,
            )

            self.assertEqual(listed[0]["workspace_folder"], "space-a")
            self.assertEqual(matches[0]["workspace_folder"], "space-a")
            self.assertEqual(note["workspace_folder"], "space-a")

    def test_grep_notes_global_scope_searches_all_workspaces(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "a1__Alpha.md", "共同 主题")
            make_note(root, "space-b", "b1__Beta.md", "共同 主题")
            store = tools.NotesToolStore([root_entry(root, "space-a"), root_entry(root, "space-b")])

            matches = store.grep_notes("共同")

            self.assertEqual({item["workspace_folder"] for item in matches}, {"space-a", "space-b"})

    def test_read_note_file_accepts_safe_relative_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "a1__Alpha.md", "第一行\n第二行 苹果\n第三行")
            store = tools.NotesToolStore([root_entry(root, "space-a")])

            note = store.read_note_file("space-a/notes/a1__Alpha.md", start_line=2, line_count=1)

            self.assertEqual(note["note_id"], "a1")
            self.assertEqual(note["start_line"], 2)
            self.assertEqual(note["end_line"], 2)
            self.assertEqual(note["content"], "第二行 苹果\n")

    def test_read_note_file_rejects_path_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "a1__Alpha.md", "内容")
            store = tools.NotesToolStore([root_entry(root, "space-a")])

            with self.assertRaises(tools.ToolError) as raised:
                store.read_note_file("../secret.md")
            self.assertEqual(raised.exception.code, "PATH_TRAVERSAL")

    def test_global_read_requires_workspace_prefix(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "a1__Alpha.md", "内容")
            make_note(root, "space-b", "b1__Beta.md", "内容")
            store = tools.NotesToolStore([root_entry(root, "space-a"), root_entry(root, "space-b")])

            with self.assertRaises(tools.ToolError) as raised:
                store.read_note_file("notes/a1__Alpha.md")
            self.assertEqual(raised.exception.code, "AMBIGUOUS_PATH")

    def test_history_only_allows_final_user_and_assistant_content(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = {
                "scope": "global",
                "question": "问题",
                "workspace_roots": [root_entry(root, "space-a")],
                "history": [{"role": "assistant", "content": "回答", "sources": []}],
                "llm": {},
                "prompt": "请回答",
            }

            with self.assertRaises(agent.SidecarError) as raised:
                agent.validate_payload(payload)
            self.assertEqual(raised.exception.code, "INVALID_HISTORY")

    def test_sidecar_empty_directory_returns_no_evidence_answer(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            notes = root / "space-a" / "notes"
            notes.mkdir(parents=True)
            req = {
                "id": "test",
                "type": "ask_local_notes",
                "payload": {
                    "scope": "current",
                    "workspace_folder": "space-a",
                    "question": "不存在的问题",
                    "workspace_roots": [root_entry(root, "space-a")],
                    "history": [{"role": "user", "content": "之前的问题"}],
                    "llm": {},
                    "prompt": "请基于笔记回答",
                },
            }

            proc = subprocess.run(
                [str(SYSTEM_PYTHON), str(MAIN_PATH)],
                input=json.dumps(req, ensure_ascii=False) + "\n",
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            payload = json.loads(proc.stdout.strip().splitlines()[-1])
            self.assertIs(payload["ok"], True)
            self.assertEqual(payload["result"]["sources"], [])
            self.assertIn("未在当前范围", payload["result"]["answer"])

    def test_stream_request_emits_delta_events_before_final_response(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "n1__Alpha.md", "苹果 项目 预算")
            req = {
                "id": "stream-test",
                "type": "ask_local_notes_stream",
                "payload": {
                    "scope": "current",
                    "workspace_folder": "space-a",
                    "question": "苹果",
                    "workspace_roots": [root_entry(root, "space-a")],
                    "history": [],
                    "llm": {},
                    "prompt": "请基于笔记回答",
                },
            }

            proc = subprocess.run(
                [str(SYSTEM_PYTHON), str(MAIN_PATH)],
                input=json.dumps(req, ensure_ascii=False) + "\n",
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(proc.returncode, 0, proc.stderr)
            lines = [json.loads(line) for line in proc.stdout.strip().splitlines()]
            self.assertTrue(any(line.get("event") == "delta" for line in lines))
            self.assertIs(lines[-1]["ok"], True)
            self.assertEqual(lines[-1]["result"]["sources"][0]["note_id"], "n1")

    def test_notes_mcp_server_lists_tools_over_stdio(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            make_note(root, "space-a", "n1__Alpha.md", "苹果 项目 预算")
            env = os.environ.copy()
            env["VOICE_VIBE_NOTES_ROOTS"] = json.dumps([root_entry(root, "space-a")], ensure_ascii=False)
            message = {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "test", "version": "0"},
                },
            }
            proc = subprocess.Popen(
                [str(RUNTIME_PYTHON), str(TOOLS_PATH)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                env=env,
            )
            assert proc.stdin is not None
            assert proc.stdout is not None
            proc.stdin.write(json.dumps(message) + "\n")
            proc.stdin.flush()
            line = proc.stdout.readline()
            proc.kill()
            _, stderr = proc.communicate(timeout=5)

            self.assertTrue(line.strip(), stderr)
            response = json.loads(line)
            self.assertEqual(response["id"], 1)
            self.assertIn("capabilities", response["result"])


if __name__ == "__main__":
    unittest.main()
