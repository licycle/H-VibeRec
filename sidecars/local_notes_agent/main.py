#!/usr/bin/env python3
from __future__ import annotations

import asyncio
import json
import os
import sys
from pathlib import Path
from typing import Any, Dict, List

MODULE_DIR = Path(__file__).resolve().parent
TOOLS_SCRIPT = MODULE_DIR / "notes_mcp_server.py"
WEB_TOOLS_SCRIPT = MODULE_DIR / "web_mcp_server.py"
MAX_HISTORY_MESSAGES = 20
DEFAULT_MAX_TURNS = 16
AGENT_SESSIONS_TABLE = "assistant_agent_sessions"
AGENT_ITEMS_TABLE = "assistant_agent_items"


class SidecarError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


def read_request() -> Dict[str, Any]:
    raw = sys.stdin.readline()
    if not raw:
        raise SidecarError("BAD_REQUEST", "No request received")
    try:
        return json.loads(raw)
    except json.JSONDecodeError as exc:
        raise SidecarError("BAD_REQUEST", f"Invalid JSON request: {exc}") from exc


def validate_payload(payload: Dict[str, Any]) -> Dict[str, Any]:
    scope = str(payload.get("scope") or "").strip()
    if scope not in {"current", "global"}:
        raise SidecarError("INVALID_SCOPE", "scope must be current or global")
    workspace_folder = payload.get("workspace_folder")
    if scope == "current" and not str(workspace_folder or "").strip():
        raise SidecarError("INVALID_WORKSPACE", "workspace_folder is required for current scope")
    question = str(payload.get("question") or "").strip()
    if not question:
        raise SidecarError("INVALID_QUESTION", "question is required")
    history = payload.get("history") or []
    if not isinstance(history, list):
        raise SidecarError("INVALID_HISTORY", "history must be a list")
    for item in history:
        if not isinstance(item, dict):
            raise SidecarError("INVALID_HISTORY", "history entries must be objects")
        if item.get("role") not in {"user", "assistant"}:
            raise SidecarError("INVALID_HISTORY", "history roles must be user or assistant")
        if not isinstance(item.get("content"), str) or not item["content"].strip():
            raise SidecarError("INVALID_HISTORY", "history content must be non-empty text")
        blocked = {"tool_calls", "tool_results", "sources", "scope", "workspace_roots"}
        if any(key in item for key in blocked):
            raise SidecarError("INVALID_HISTORY", "history may only contain final user/assistant content")
    roots = payload.get("workspace_roots") or []
    if not isinstance(roots, list):
        raise SidecarError("INVALID_WORKSPACE", "workspace_roots must be a list")
    llm = payload.get("llm") or {}
    if not isinstance(llm, dict):
        raise SidecarError("INVALID_LLM", "llm settings must be an object")
    prompt = str(payload.get("prompt") or "").strip()
    if not prompt:
        raise SidecarError("INVALID_PROMPT", "prompt is required")
    web_enabled = payload.get("web_enabled", False)
    if not isinstance(web_enabled, bool):
        raise SidecarError("INVALID_WEB_ENABLED", "web_enabled must be a boolean")
    session_id = str(payload.get("session_id") or "").strip()
    if session_id and (len(session_id) > 120 or any(ch in session_id for ch in ("/", "\\", ".."))):
        raise SidecarError("INVALID_SESSION", "session_id is invalid")
    session_db_path = str(payload.get("session_db_path") or "").strip()
    if session_db_path and Path(session_db_path).expanduser().suffix != ".sqlite":
        raise SidecarError("INVALID_SESSION", "session_db_path must point to a SQLite database")
    session_reset_to_history = payload.get("session_reset_to_history", False)
    if not isinstance(session_reset_to_history, bool):
        raise SidecarError("INVALID_SESSION", "session_reset_to_history must be a boolean")
    max_turns = payload.get("max_turns", DEFAULT_MAX_TURNS)
    try:
        max_turns = int(max_turns)
    except (TypeError, ValueError) as exc:
        raise SidecarError("INVALID_MAX_TURNS", "max_turns must be a positive integer") from exc
    if max_turns < 1:
        raise SidecarError("INVALID_MAX_TURNS", "max_turns must be a positive integer")
    payload["max_turns"] = max_turns
    return payload


def validate_roots(roots: List[Dict[str, str]]) -> List[Dict[str, str]]:
    validated: List[Dict[str, str]] = []
    for root in roots:
        if not isinstance(root, dict):
            raise SidecarError("INVALID_WORKSPACE", "workspace root entries must be objects")
        workspace_folder = str(root.get("workspace_folder") or "").strip()
        notes_dir = Path(str(root.get("notes_dir") or "")).expanduser()
        if not workspace_folder:
            raise SidecarError("INVALID_WORKSPACE", "workspace_folder is required")
        if any(sep in workspace_folder for sep in ("/", "\\")) or ".." in workspace_folder:
            raise SidecarError("INVALID_WORKSPACE", "workspace_folder is invalid")
        resolved = notes_dir.resolve()
        if resolved.name != "notes":
            raise SidecarError("INVALID_WORKSPACE", "notes_dir must point to a notes directory")
        if resolved.parent.name != workspace_folder:
            raise SidecarError("INVALID_WORKSPACE", "notes_dir does not match workspace_folder")
        validated.append({"workspace_folder": workspace_folder, "notes_dir": str(resolved)})
    return validated


def build_roots_env(roots: List[Dict[str, str]]) -> Dict[str, str]:
    return {"VOICE_VIBE_NOTES_ROOTS": json.dumps(roots, ensure_ascii=False)}


def build_path_env() -> Dict[str, str]:
    current_path = os.environ.get("PATH", "")
    extra_paths = [str(Path("/usr/bin")), str(Path("/bin"))]
    merged = current_path.split(os.pathsep) if current_path else []
    for extra in extra_paths:
        if extra not in merged:
            merged.insert(0, extra)
    return {"PATH": os.pathsep.join([p for p in merged if p])}


def build_web_env() -> Dict[str, str]:
    env = dict(os.environ)
    env.update(build_path_env())
    passthrough_keys = (
        "DDGS_PROXY",
        "VOICE_VIBE_SEARXNG_URL",
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
        "no_proxy",
        "NO_PROXY",
    )
    for key in passthrough_keys:
        value = os.environ.get(key)
        if value:
            env[key] = value
    if "DDGS_PROXY" not in env:
        for key in ("https_proxy", "HTTPS_PROXY", "http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"):
            value = env.get(key)
            if value:
                env["DDGS_PROXY"] = value
                break
    return env


def build_mcp_server(roots: List[Dict[str, str]], server_class: Any):
    env = {}
    env.update(build_roots_env(roots))
    env.update(build_path_env())
    return server_class(
        {
            "command": sys.executable,
            "args": [str(TOOLS_SCRIPT)],
            "env": env,
            "cwd": str(MODULE_DIR),
        },
        cache_tools_list=True,
        name="local-notes-tools",
        use_structured_content=True,
    )


def build_web_mcp_server(server_class: Any):
    if not WEB_TOOLS_SCRIPT.exists():
        raise SidecarError("WEB_TOOLS_MISSING", f"Web tools sidecar missing: {WEB_TOOLS_SCRIPT}")
    return server_class(
        {
            "command": sys.executable,
            "args": [str(WEB_TOOLS_SCRIPT)],
            "env": build_web_env(),
            "cwd": str(MODULE_DIR),
        },
        cache_tools_list=True,
        name="web-tools",
        use_structured_content=True,
    )


def build_history(history: List[Dict[str, Any]]) -> List[Dict[str, str]]:
    return [
        {"role": item["role"], "content": item["content"]}
        for item in history[-MAX_HISTORY_MESSAGES:]
    ]


async def prepare_agent_session(payload: Dict[str, Any], session_class: Any) -> Any | None:
    session_id = str(payload.get("session_id") or "").strip()
    db_path = str(payload.get("session_db_path") or "").strip()
    if not session_id or not db_path:
        return None

    resolved_db_path = Path(db_path).expanduser()
    if resolved_db_path.parent:
        resolved_db_path.parent.mkdir(parents=True, exist_ok=True)
    session = session_class(
        session_id=session_id,
        db_path=str(resolved_db_path),
        sessions_table=AGENT_SESSIONS_TABLE,
        messages_table=AGENT_ITEMS_TABLE,
    )

    history_items = build_history(payload.get("history", []))
    if payload.get("session_reset_to_history") is True:
        await session.clear_session()
        if history_items:
            await session.add_items(history_items)
        return session

    existing_items = await session.get_items(limit=1)
    if not existing_items and history_items:
        await session.add_items(history_items)
    return session


async def reset_agent_session_to_history(session: Any | None, payload: Dict[str, Any]) -> None:
    if session is None:
        return
    await session.clear_session()
    history_items = build_history(payload.get("history", []))
    if history_items:
        await session.add_items(history_items)


def assistant_sources_from_items(result: Any) -> List[Dict[str, str]]:
    sources: Dict[str, Dict[str, str]] = {}
    for item in getattr(result, "new_items", []) or []:
        if getattr(item, "type", None) != "tool_call_output_item":
            continue
        for candidate in extract_sources(getattr(item, "output", None)):
            sources[candidate["id"]] = candidate
    if sources:
        return list(sources.values())
    return []


def extract_sources(output: Any) -> List[Dict[str, str]]:
    if isinstance(output, str):
        try:
            output = json.loads(output)
        except json.JSONDecodeError:
            return []
    sources: List[Dict[str, str]] = []
    if isinstance(output, dict):
        tool_name = str(output.get("tool") or "").strip()
        candidate = normalize_source(output)
        source_type = str(output.get("source_type") or output.get("type") or "").strip()
        if candidate is not None and (tool_name == "read_note_file" or source_type == "web"):
            sources.append(candidate)
        for value in output.values():
            sources.extend(extract_sources(value))
    elif isinstance(output, list):
        for value in output:
            sources.extend(extract_sources(value))
    return sources


def normalize_source(payload: Dict[str, Any]) -> Dict[str, str] | None:
    tool_name = str(payload.get("tool") or "").strip()
    source_type = str(payload.get("source_type") or payload.get("type") or "").strip()
    if tool_name == "read_web_page" or source_type == "web":
        return normalize_web_source(payload)
    path = str(payload.get("path") or payload.get("id") or "").strip()
    note_id = str(payload.get("note_id") or "").strip()
    title = str(payload.get("title") or "").strip()
    workspace_folder = str(payload.get("workspace_folder") or "").strip()
    if not path or not note_id or not title:
        return None
    source = {
        "type": "note",
        "id": path,
        "note_id": note_id,
        "title": title,
        "workspace_folder": workspace_folder,
    }
    snippet = str(payload.get("snippet") or payload.get("content") or "").strip()
    if snippet:
        source["snippet"] = snippet[:500]
    for key in ("start_line", "end_line"):
        value = int_value(payload.get(key))
        if value > 0:
            source[key] = value
    return source


def normalize_web_source(payload: Dict[str, Any]) -> Dict[str, str] | None:
    url = str(payload.get("url") or payload.get("id") or "").strip()
    title = str(payload.get("title") or "").strip()
    snippet = str(payload.get("snippet") or payload.get("content") or "").strip()
    if not url or not title:
        return None
    source = {
        "type": "web",
        "id": url,
        "note_id": "",
        "title": title,
        "workspace_folder": "",
        "url": url,
        "snippet": snippet[:500],
    }
    return source


def int_value(value: Any) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def workspace_names_from_payload(payload: Dict[str, Any]) -> List[str]:
    names: List[str] = []
    for root in payload.get("workspace_roots") or []:
        if not isinstance(root, dict):
            continue
        name = str(root.get("workspace_folder") or "").strip()
        if name and name not in names:
            names.append(name)
    return names


def build_answer_prompt(
    scope: str,
    question: str,
    web_enabled: bool = False,
    workspace_folder: str = "",
    allowed_workspaces: List[str] | None = None,
) -> str:
    scope_label = "当前空间" if scope == "current" else "全部空间"
    prompt = (
        f"范围：{scope_label}。问题：{question}\n"
    )
    if scope == "current" and workspace_folder:
        prompt += (
            f"当前空间的实际 workspace_folder：{workspace_folder}。"
            "工具调用时优先省略 workspace_folder；如果必须填写，只能填写这个实际名称，"
            "不要把 current 当成 workspace_folder。\n"
        )
    elif allowed_workspaces:
        prompt += (
            "允许访问的 workspace_folder："
            f"{', '.join(allowed_workspaces)}。工具调用只能使用这些实际名称。\n"
        )
    prompt += (
        "请先使用 list_notes 或 grep_notes 找到当前允许范围内的相关笔记，再使用 read_note_file 读取必要片段。\n"
        "只能基于当前请求允许范围内的笔记回答。不要编造来源。\n"
        "如果没有找到依据，明确说明未在当前范围的笔记中找到相关信息。\n"
        "回答末尾用“引用来源”列出你实际读取过的 note id 和标题。"
    )
    if web_enabled:
        prompt += (
            "\n联网已开启。需要最新公开网页信息时，可以使用 web_search 搜索网页，"
            "再使用 read_web_page 读取网页内容后回答。"
            "网页依据必须来自 read_web_page 的结果，回答中要区分本地笔记依据和网页依据，"
            "并在“引用来源”中列出网页标题和 URL。"
        )
    return prompt


def build_agent_input(payload: Dict[str, Any]) -> str:
    return build_answer_prompt(
        payload["scope"],
        payload["question"],
        web_enabled=bool(payload.get("web_enabled", False)),
        workspace_folder=str(payload.get("workspace_folder") or "").strip(),
        allowed_workspaces=workspace_names_from_payload(payload),
    )


def build_conversation(payload: Dict[str, Any]) -> List[Dict[str, str]]:
    conversation = build_history(payload.get("history", []))
    conversation.append(
        {
            "role": "user",
            "content": build_agent_input(payload),
        }
    )
    return conversation


async def run_assistant(payload: Dict[str, Any]) -> Dict[str, Any]:
    try:
        from agents import Agent, ModelSettings, Runner, SQLiteSession
        from agents.models.openai_chatcompletions import OpenAIChatCompletionsModel
        from agents.mcp import MCPServerManager, MCPServerStdio
        from openai import AsyncOpenAI
    except Exception as exc:
        raise SidecarError("AGENTS_SDK_MISSING", f"openai-agents-python is not installed: {exc}") from exc

    roots = validate_roots(payload["workspace_roots"])
    llm = payload["llm"]
    servers = [build_mcp_server(roots, MCPServerStdio)]
    if payload.get("web_enabled") is True:
        servers.append(build_web_mcp_server(MCPServerStdio))
    async with MCPServerManager(servers, connect_in_parallel=False) as manager:
        session = await prepare_agent_session(payload, SQLiteSession)
        client = AsyncOpenAI(
            api_key=str(llm.get("api_key") or ""),
            base_url=str(llm.get("base_url") or "").rstrip("/"),
            timeout=float(llm.get("timeout_seconds") or 120),
        )
        model = OpenAIChatCompletionsModel(
            model=str(llm.get("model") or ""),
            openai_client=client,
        )
        agent = Agent(
            name="Local Notes QA",
            instructions=str(payload.get("prompt") or "").strip(),
            model=model,
            mcp_servers=manager.active_servers,
            model_settings=ModelSettings(
                temperature=float(llm.get("temperature") or 0.1),
                max_tokens=int(llm.get("max_tokens") or 2048),
            ),
        )
        try:
            result = await Runner.run(
                agent,
                build_agent_input(payload) if session is not None else build_conversation(payload),
                max_turns=int(payload.get("max_turns") or DEFAULT_MAX_TURNS),
                session=session,
            )
        except Exception:
            await reset_agent_session_to_history(session, payload)
            raise
        answer = output_text(result)
        if not answer:
            raise SidecarError("EMPTY_ANSWER", "Agent returned empty answer")
        sources = assistant_sources_from_items(result)
        if not sources:
            sources = fallback_sources(payload, roots)
        return {"answer": answer, "sources": sources}


async def run_assistant_stream(payload: Dict[str, Any], req_id: str) -> Dict[str, Any]:
    try:
        from agents import Agent, ModelSettings, Runner, SQLiteSession
        from agents.models.openai_chatcompletions import OpenAIChatCompletionsModel
        from agents.mcp import MCPServerManager, MCPServerStdio
        from openai import AsyncOpenAI
    except Exception as exc:
        raise SidecarError("AGENTS_SDK_MISSING", f"openai-agents-python is not installed: {exc}") from exc

    roots = validate_roots(payload["workspace_roots"])
    llm = payload["llm"]
    servers = [build_mcp_server(roots, MCPServerStdio)]
    if payload.get("web_enabled") is True:
        servers.append(build_web_mcp_server(MCPServerStdio))
    async with MCPServerManager(servers, connect_in_parallel=False) as manager:
        session = await prepare_agent_session(payload, SQLiteSession)
        client = AsyncOpenAI(
            api_key=str(llm.get("api_key") or ""),
            base_url=str(llm.get("base_url") or "").rstrip("/"),
            timeout=float(llm.get("timeout_seconds") or 120),
        )
        model = OpenAIChatCompletionsModel(
            model=str(llm.get("model") or ""),
            openai_client=client,
        )
        agent = Agent(
            name="Local Notes QA",
            instructions=str(payload.get("prompt") or "").strip(),
            model=model,
            mcp_servers=manager.active_servers,
            model_settings=ModelSettings(
                temperature=float(llm.get("temperature") or 0.1),
                max_tokens=int(llm.get("max_tokens") or 2048),
            ),
        )
        result = Runner.run_streamed(
            agent,
            build_agent_input(payload) if session is not None else build_conversation(payload),
            max_turns=int(payload.get("max_turns") or DEFAULT_MAX_TURNS),
            session=session,
        )
        emitted_any_delta = False
        last_emitted_turn: int | None = None

        def emit_turn_if_changed() -> None:
            nonlocal last_emitted_turn
            payload = turn_status_payload(result)
            current_turn = payload["current_turn"]
            if current_turn == last_emitted_turn:
                return
            last_emitted_turn = current_turn
            emit_stream(req_id, "turn", **payload)

        try:
            emit_turn_if_changed()
            async for event in result.stream_events():
                emit_turn_if_changed()
                delta = stream_delta_from_event(event)
                if delta:
                    emitted_any_delta = True
                    emit_stream(req_id, "delta", text=delta)
                    continue
                tool_name = tool_name_from_event(event)
                if tool_name:
                    emit_stream(req_id, "tool", name=tool_name)
            emit_turn_if_changed()
            answer = output_text(result)
            if not answer:
                raise SidecarError("EMPTY_ANSWER", "Agent returned empty answer")
            if not emitted_any_delta:
                emit_chunked_delta(req_id, answer)
            sources = assistant_sources_from_items(result)
            if not sources:
                sources = fallback_sources(payload, roots)
            return {"answer": answer, "sources": sources}
        except Exception:
            await reset_agent_session_to_history(session, payload)
            raise


def fallback_sources(payload: Dict[str, Any], roots: List[Dict[str, str]]) -> List[Dict[str, str]]:
    try:
        from notes_mcp_server import NotesToolStore
    except Exception:
        return []
    store = NotesToolStore(roots)
    return store.fallback_sources_for_query(str(payload.get("question") or ""), max_results=5)


def emit(payload: Dict[str, Any]) -> None:
    print(json.dumps(payload, ensure_ascii=False))
    sys.stdout.flush()


def emit_stream(req_id: str, event: str, **payload: Any) -> None:
    emit({"id": req_id, "ok": True, "event": event, **payload})


def turn_status_payload(result: Any) -> Dict[str, Any]:
    try:
        current_turn = int(getattr(result, "current_turn", 0) or 0)
    except (TypeError, ValueError):
        current_turn = 0
    return {
        "current_turn": max(current_turn, 0),
        "max_turns": str(getattr(result, "max_turns", "") or ""),
    }


def response_ok(req_id: str, result: Dict[str, Any]) -> Dict[str, Any]:
    return {"id": req_id, "ok": True, "result": result}


def response_error(req_id: str, code: str, message: str) -> Dict[str, Any]:
    return {"id": req_id, "ok": False, "error": {"code": code, "message": message}}


def stream_delta_from_event(event: Any) -> str:
    if getattr(event, "type", None) != "raw_response_event":
        return ""
    data = getattr(event, "data", None)
    event_type = str(getattr(data, "type", "") or "")
    if "delta" not in event_type and not hasattr(data, "delta"):
        return ""
    return str(getattr(data, "delta", "") or "")


def tool_name_from_event(event: Any) -> str:
    if getattr(event, "type", None) != "run_item_stream_event":
        return ""
    item = getattr(event, "item", None)
    raw = getattr(item, "raw_item", None) or getattr(item, "item", None) or item
    for attr in ("name", "tool_name"):
        value = getattr(raw, attr, None)
        if isinstance(value, str) and value.strip():
            return value.strip()
    if isinstance(raw, dict):
        for key in ("name", "tool_name"):
            value = raw.get(key)
            if isinstance(value, str) and value.strip():
                return value.strip()
    return str(getattr(item, "type", "") or "")


def emit_chunked_delta(req_id: str, text: str, chunk_size: int = 24) -> None:
    for index in range(0, len(text), chunk_size):
        emit_stream(req_id, "delta", text=text[index : index + chunk_size])


def is_max_turns_error(exc: Exception) -> bool:
    name = exc.__class__.__name__
    message = str(exc)
    return name == "MaxTurnsExceeded" or "Max turns" in message


def output_text(result: Any) -> str:
    final_output = getattr(result, "final_output", result)
    if isinstance(final_output, str):
        return final_output.strip()
    if isinstance(final_output, dict):
        value = final_output.get("answer") or final_output.get("content")
        if isinstance(value, str):
            return value.strip()
    return str(final_output or "").strip()


async def handle(req: Dict[str, Any]) -> Dict[str, Any]:
    req_id = str(req.get("id") or "")
    if req.get("type") not in {"ask_local_notes", "ask_local_notes_stream"}:
        raise SidecarError("UNKNOWN_REQUEST", f"Unsupported request type: {req.get('type')}")
    payload = validate_payload(req.get("payload") or {})
    try:
        if req.get("type") == "ask_local_notes_stream":
            result = await run_assistant_stream(payload, req_id)
        else:
            result = await run_assistant(payload)
    except SidecarError as exc:
        if exc.code != "AGENTS_SDK_MISSING":
            raise
        result = run_fallback(payload)
        if req.get("type") == "ask_local_notes_stream":
            emit_chunked_delta(req_id, result["answer"])
    except Exception as exc:
        if is_max_turns_error(exc):
            max_turns = int(payload.get("max_turns") or DEFAULT_MAX_TURNS)
            raise SidecarError(
                "MAX_TURNS_EXCEEDED",
                f"AI 工具调用轮次已用完（当前 {max_turns}）。请调高轮次或缩小问题范围。",
            ) from exc
        raise SidecarError("UNHANDLED", str(exc)) from exc
    return response_ok(req_id, result)


def run_fallback(payload: Dict[str, Any]) -> Dict[str, Any]:
    try:
        from notes_mcp_server import NotesToolStore
    except Exception as exc:
        raise SidecarError("AGENTS_SDK_MISSING", f"Local notes tools are unavailable: {exc}") from exc
    roots = validate_roots(payload["workspace_roots"])
    store = NotesToolStore(roots)
    matches = store.grep_notes(str(payload.get("question") or ""), max_results=3)
    if not matches:
        return {
            "answer": "未在当前范围的笔记中找到相关依据，无法基于本地笔记回答这个问题。",
            "sources": [],
        }
    sources: Dict[str, Dict[str, str]] = {}
    parts = ["我在当前范围的本地笔记中找到以下相关内容："]
    for match in matches:
        path = str(match.get("path") or "")
        if not path:
            continue
        source = store.source_for_path(path)
        sources[source["id"]] = source
        excerpt = str(match.get("excerpt") or "").strip()
        parts.append(f"- {source['title']}：{excerpt}")
    parts.append("\n引用来源：")
    for source in sources.values():
        parts.append(f"- {source['note_id']} {source['title']}")
    return {"answer": "\n".join(parts), "sources": list(sources.values())}


def main() -> None:
    req_id = ""
    try:
        req = read_request()
        req_id = str(req.get("id") or "")
        emit(asyncio.run(handle(req)))
    except SidecarError as exc:
        emit(response_error(req_id, exc.code, exc.message))
        sys.exit(1)
    except Exception as exc:
        emit(response_error(req_id, "UNHANDLED", str(exc)))
        sys.exit(1)


if __name__ == "__main__":
    main()
