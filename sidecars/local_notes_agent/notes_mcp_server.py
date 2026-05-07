#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

MAX_NOTE_FILES = 500
MAX_GREP_RESULTS = 80
MAX_EXCERPT_CHARS = 360
MAX_READ_LINES = 160
MAX_READ_CHARS = 16_000
MAX_FILE_BYTES = 5 * 1024 * 1024
RG_TIMEOUT_SECONDS = 8
NOTE_FILE_PATTERN = re.compile(r"^[^/\\]+__.+\.md$")


class ToolError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


@dataclass(frozen=True)
class NoteFile:
    path: str
    note_id: str
    title: str
    workspace_folder: str
    absolute_path: Path


def load_roots_from_env() -> List[Dict[str, str]]:
    raw = os.environ.get("VOICE_VIBE_NOTES_ROOTS", "")
    if not raw.strip():
        raise ToolError("NO_WORKSPACES", "VOICE_VIBE_NOTES_ROOTS is required")
    try:
        roots = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ToolError("INVALID_WORKSPACE", f"Invalid workspace roots JSON: {exc}") from exc
    return validate_roots(roots)


def validate_roots(roots: Any) -> List[Dict[str, str]]:
    if not isinstance(roots, list) or not roots:
        raise ToolError("NO_WORKSPACES", "No workspace note roots are available")
    validated: List[Dict[str, str]] = []
    seen = set()
    for root in roots:
        if not isinstance(root, dict):
            raise ToolError("INVALID_WORKSPACE", "workspace root entries must be objects")
        workspace_folder = str(root.get("workspace_folder") or "").strip()
        notes_dir = Path(str(root.get("notes_dir") or "")).expanduser()
        if not workspace_folder:
            raise ToolError("INVALID_WORKSPACE", "workspace_folder is required")
        if any(sep in workspace_folder for sep in ("/", "\\")) or ".." in workspace_folder:
            raise ToolError("INVALID_WORKSPACE", "workspace_folder is invalid")
        resolved = notes_dir.resolve()
        if resolved.name != "notes":
            raise ToolError("INVALID_WORKSPACE", "notes_dir must point to a notes directory")
        if resolved.parent.name != workspace_folder:
            raise ToolError("INVALID_WORKSPACE", "notes_dir does not match workspace_folder")
        if workspace_folder in seen:
            raise ToolError("INVALID_WORKSPACE", f"Duplicate workspace_folder: {workspace_folder}")
        seen.add(workspace_folder)
        validated.append({"workspace_folder": workspace_folder, "notes_dir": str(resolved)})
    return validated


class NotesToolStore:
    def __init__(self, roots: List[Dict[str, str]]) -> None:
        self._roots = validate_roots(roots)
        self._root_by_workspace = {
            root["workspace_folder"]: Path(root["notes_dir"]).resolve()
            for root in self._roots
        }

    @property
    def roots(self) -> List[Dict[str, str]]:
        return self._roots

    def list_notes(self, workspace_folder: Optional[str] = None, max_results: int = 200) -> List[Dict[str, Any]]:
        workspaces = self._selected_workspaces(workspace_folder)
        limit = clamp_int(max_results, 1, MAX_NOTE_FILES, 200)
        notes: List[NoteFile] = []
        for workspace in workspaces:
            notes.extend(self._scan_workspace(workspace, max_files=limit))
            if len(notes) >= limit:
                break
        notes = notes[:limit]
        return [
            {
                "tool": "list_notes",
                "path": note.path,
                "id": note.path,
                "note_id": note.note_id,
                "title": note.title,
                "workspace_folder": note.workspace_folder,
            }
            for note in notes
        ]

    def grep_notes(
        self,
        query: str,
        workspace_folder: Optional[str] = None,
        max_results: int = 24,
        fixed_strings: bool = True,
    ) -> List[Dict[str, Any]]:
        query = str(query or "").strip()
        if not query:
            raise ToolError("INVALID_QUERY", "query is required")
        workspaces = self._selected_workspaces(workspace_folder)
        limit = clamp_int(max_results, 1, MAX_GREP_RESULTS, 24)
        results: List[Dict[str, Any]] = []
        for workspace in workspaces:
            if len(results) >= limit:
                break
            notes_dir = self._root_by_workspace[workspace]
            if not notes_dir.exists():
                continue
            remaining = limit - len(results)
            matches = self._grep_with_rg(notes_dir, workspace, query, remaining, fixed_strings)
            if not matches:
                matches = self._grep_stream(notes_dir, workspace, query, remaining, fixed_strings)
            results.extend(matches[:remaining])
        return results[:limit]

    def read_note_file(
        self,
        path: str,
        start_line: int = 1,
        line_count: int = 120,
        workspace_folder: Optional[str] = None,
    ) -> Dict[str, Any]:
        note = self.resolve_note_path(path, workspace_folder)
        start = clamp_int(start_line, 1, 1_000_000, 1)
        count = clamp_int(line_count, 1, MAX_READ_LINES, 120)
        lines: List[str] = []
        total_lines = 0
        truncated = False
        char_count = 0
        with note.absolute_path.open("r", encoding="utf-8", errors="replace") as handle:
            for total_lines, line in enumerate(handle, 1):
                if total_lines < start:
                    continue
                if total_lines >= start + count:
                    truncated = True
                    break
                if char_count + len(line) > MAX_READ_CHARS:
                    remaining = max(0, MAX_READ_CHARS - char_count)
                    if remaining > 0:
                        lines.append(line[:remaining])
                    truncated = True
                    break
                lines.append(line)
                char_count += len(line)
        end_line = start + len(lines) - 1 if lines else start - 1
        return {
            "tool": "read_note_file",
            "path": note.path,
            "id": note.path,
            "note_id": note.note_id,
            "title": note.title,
            "workspace_folder": note.workspace_folder,
            "start_line": start,
            "end_line": end_line,
            "line_count": len(lines),
            "truncated": truncated,
            "content": "".join(lines),
            "total_lines_seen": total_lines,
        }

    def source_for_path(self, path: str, workspace_folder: Optional[str] = None) -> Dict[str, str]:
        note = self.resolve_note_path(path, workspace_folder)
        return {
            "id": note.path,
            "note_id": note.note_id,
            "title": note.title,
            "workspace_folder": note.workspace_folder,
        }

    def fallback_sources_for_query(self, query: str, max_results: int = 5) -> List[Dict[str, str]]:
        sources: Dict[str, Dict[str, str]] = {}
        for match in self.grep_notes(query, max_results=max_results):
            path = str(match.get("path") or "")
            if not path or path in sources:
                continue
            try:
                sources[path] = self.source_for_path(path)
            except ToolError:
                continue
        return list(sources.values())[:max_results]

    def _selected_workspaces(self, workspace_folder: Optional[str]) -> List[str]:
        workspace = self._normalized_workspace_folder(workspace_folder)
        if workspace:
            if workspace not in self._root_by_workspace:
                raise ToolError("WORKSPACE_NOT_ALLOWED", f"Workspace is not allowed: {workspace}")
            return [workspace]
        return [root["workspace_folder"] for root in self._roots]

    def _normalized_workspace_folder(self, workspace_folder: Optional[str]) -> str:
        workspace = str(workspace_folder or "").strip()
        if workspace == "current" and len(self._root_by_workspace) == 1:
            return next(iter(self._root_by_workspace))
        return workspace

    def _scan_workspace(self, workspace: str, max_files: int = MAX_NOTE_FILES) -> List[NoteFile]:
        notes_dir = self._root_by_workspace[workspace]
        if not notes_dir.exists():
            return []
        notes: List[NoteFile] = []
        for path in sorted(notes_dir.glob("*.md")):
            if len(notes) >= max_files:
                break
            try:
                note = self._note_from_absolute_path(workspace, path)
            except ToolError:
                continue
            notes.append(note)
        return notes

    def resolve_note_path(self, requested_path: str, workspace_folder: Optional[str] = None) -> NoteFile:
        raw = str(requested_path or "").strip()
        if not raw:
            raise ToolError("INVALID_PATH", "path is required")
        normalized = raw.replace("\\", "/")
        if normalized.startswith("/") or re.match(r"^[A-Za-z]:/", normalized):
            raise ToolError("INVALID_PATH", "absolute paths are not allowed")
        parts = [part for part in normalized.split("/") if part not in {"", "."}]
        if any(part == ".." for part in parts):
            raise ToolError("PATH_TRAVERSAL", "path must not contain '..'")

        explicit_workspace = self._normalized_workspace_folder(workspace_folder)
        if explicit_workspace:
            workspace = explicit_workspace
            rel_parts = drop_optional_notes_prefix(parts)
        elif len(parts) >= 3 and parts[1] == "notes":
            workspace = parts[0]
            rel_parts = parts[2:]
        elif len(parts) >= 2 and parts[0] in self._root_by_workspace:
            workspace = parts[0]
            rel_parts = parts[1:]
        elif len(self._root_by_workspace) == 1:
            workspace = next(iter(self._root_by_workspace))
            rel_parts = drop_optional_notes_prefix(parts)
        else:
            raise ToolError("AMBIGUOUS_PATH", "global scope paths must include workspace/notes/file.md")

        if workspace not in self._root_by_workspace:
            raise ToolError("WORKSPACE_NOT_ALLOWED", f"Workspace is not allowed: {workspace}")
        if not rel_parts:
            raise ToolError("INVALID_PATH", "note file path is required")
        if len(rel_parts) != 1:
            raise ToolError("INVALID_PATH", "only files directly inside notes are supported")
        file_name = rel_parts[0]
        if "/" in file_name or "\\" in file_name or not file_name.endswith(".md"):
            raise ToolError("INVALID_PATH", "only Markdown note files are allowed")
        notes_dir = self._root_by_workspace[workspace]
        candidate = (notes_dir / file_name).resolve()
        assert_child(notes_dir, candidate)
        return self._note_from_absolute_path(workspace, candidate)

    def _note_from_absolute_path(self, workspace: str, path: Path) -> NoteFile:
        notes_dir = self._root_by_workspace[workspace]
        resolved = path.resolve()
        assert_child(notes_dir, resolved)
        if not resolved.is_file():
            raise ToolError("NOTE_NOT_FOUND", f"Note file not found: {path.name}")
        if resolved.suffix != ".md":
            raise ToolError("INVALID_PATH", "only Markdown note files are allowed")
        if not NOTE_FILE_PATTERN.match(resolved.name):
            raise ToolError("INVALID_PATH", f"Invalid note file name: {resolved.name}")
        stat = resolved.stat()
        if stat.st_size > MAX_FILE_BYTES:
            raise ToolError("FILE_TOO_LARGE", f"Note exceeds {MAX_FILE_BYTES} bytes: {resolved.name}")
        raw_id, title = resolved.stem.split("__", 1)
        if not raw_id.strip() or not title.strip():
            raise ToolError("INVALID_PATH", f"Invalid note file name: {resolved.name}")
        return NoteFile(
            path=f"{workspace}/notes/{resolved.name}",
            note_id=raw_id,
            title=title,
            workspace_folder=workspace,
            absolute_path=resolved,
        )

    def _grep_with_rg(
        self,
        notes_dir: Path,
        workspace: str,
        query: str,
        max_results: int,
        fixed_strings: bool,
    ) -> List[Dict[str, Any]]:
        rg_path = shutil.which("rg")
        if not rg_path:
            return []
        args = [
            rg_path,
            "--json",
            "--line-number",
            "--column",
            "--max-count",
            str(max_results),
            "--max-filesize",
            str(MAX_FILE_BYTES),
            "--glob",
            "*.md",
            "--no-config",
            "--no-messages",
        ]
        if fixed_strings:
            args.append("--fixed-strings")
        args.extend(["--", query, str(notes_dir)])
        try:
            proc = subprocess.run(
                args,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=RG_TIMEOUT_SECONDS,
            )
        except (OSError, subprocess.TimeoutExpired):
            return []
        if proc.returncode not in {0, 1}:
            return []
        results: List[Dict[str, Any]] = []
        for line in proc.stdout.splitlines():
            if len(results) >= max_results:
                break
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") != "match":
                continue
            data = event.get("data") or {}
            raw_path = ((data.get("path") or {}).get("text") or "").strip()
            if not raw_path:
                continue
            try:
                note = self._note_from_absolute_path(workspace, Path(raw_path))
            except ToolError:
                continue
            line_text = ((data.get("lines") or {}).get("text") or "").rstrip("\n")
            results.append(
                {
                    "path": note.path,
                    "tool": "grep_notes",
                    "id": note.path,
                    "note_id": note.note_id,
                    "title": note.title,
                    "workspace_folder": note.workspace_folder,
                    "line": int(data.get("line_number") or 0),
                    "column": first_submatch_column(data),
                    "excerpt": trim_excerpt(line_text),
                }
            )
        return results

    def _grep_stream(
        self,
        notes_dir: Path,
        workspace: str,
        query: str,
        max_results: int,
        fixed_strings: bool,
    ) -> List[Dict[str, Any]]:
        results: List[Dict[str, Any]] = []
        pattern = None if fixed_strings else re.compile(query, re.IGNORECASE)
        needle = query.lower()
        for note in self._scan_workspace(workspace, max_files=MAX_NOTE_FILES):
            if len(results) >= max_results:
                break
            with note.absolute_path.open("r", encoding="utf-8", errors="replace") as handle:
                for line_number, line in enumerate(handle, 1):
                    if len(results) >= max_results:
                        break
                    haystack = line.lower()
                    if fixed_strings:
                        column = haystack.find(needle)
                        if column < 0:
                            continue
                        column += 1
                    else:
                        match = pattern.search(line) if pattern else None
                        if not match:
                            continue
                        column = match.start() + 1
                    results.append(
                        {
                            "path": note.path,
                            "tool": "grep_notes",
                            "id": note.path,
                            "note_id": note.note_id,
                            "title": note.title,
                            "workspace_folder": note.workspace_folder,
                            "line": line_number,
                            "column": column,
                            "excerpt": trim_excerpt(line.rstrip("\n")),
                        }
                    )
        return results


def drop_optional_notes_prefix(parts: List[str]) -> List[str]:
    if parts and parts[0] == "notes":
        return parts[1:]
    return parts


def assert_child(root: Path, path: Path) -> None:
    try:
        path.relative_to(root)
    except ValueError as exc:
        raise ToolError("PATH_TRAVERSAL", f"Path escapes notes directory: {path}") from exc


def clamp_int(value: Any, minimum: int, maximum: int, fallback: int) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return fallback
    return max(minimum, min(maximum, parsed))


def trim_excerpt(text: str) -> str:
    compact = re.sub(r"\s+", " ", text).strip()
    if len(compact) <= MAX_EXCERPT_CHARS:
        return compact
    return compact[:MAX_EXCERPT_CHARS].rstrip()


def first_submatch_column(data: Dict[str, Any]) -> int:
    submatches = data.get("submatches") or []
    if not submatches:
        return int(data.get("absolute_offset") or 0) + 1
    first = submatches[0] or {}
    return int(first.get("start") or 0) + 1


def build_mcp(store: NotesToolStore):
    from mcp.server.fastmcp import FastMCP

    mcp = FastMCP(
        "voice-vibe-local-notes",
        instructions=(
            "Read-only local notes tools. All paths are scoped to the current request's "
            "allowed workspaces and must be relative paths returned by list_notes or grep_notes."
        ),
        log_level="ERROR",
    )

    @mcp.tool()
    def list_notes(workspace_folder: Optional[str] = None, max_results: int = 200) -> List[Dict[str, Any]]:
        """List Markdown notes in the allowed local workspace scope."""
        return store.list_notes(workspace_folder=workspace_folder, max_results=max_results)

    @mcp.tool()
    def grep_notes(
        query: str,
        workspace_folder: Optional[str] = None,
        max_results: int = 24,
        fixed_strings: bool = True,
    ) -> List[Dict[str, Any]]:
        """Search allowed Markdown notes with ripgrep-style line results."""
        return store.grep_notes(
            query=query,
            workspace_folder=workspace_folder,
            max_results=max_results,
            fixed_strings=fixed_strings,
        )

    @mcp.tool()
    def read_note_file(
        path: str,
        start_line: int = 1,
        line_count: int = 120,
        workspace_folder: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Read a bounded line range from one allowed Markdown note file."""
        return store.read_note_file(
            path=path,
            start_line=start_line,
            line_count=line_count,
            workspace_folder=workspace_folder,
        )

    return mcp


def main() -> None:
    store = NotesToolStore(load_roots_from_env())
    build_mcp(store).run(transport="stdio")


if __name__ == "__main__":
    main()
