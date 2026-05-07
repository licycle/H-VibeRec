#!/usr/bin/env python3
from __future__ import annotations

import html
import ipaddress
import json
import os
import re
import socket
import ssl
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, List, Optional


MAX_SEARCH_RESULTS = 8
MAX_DDGS_FETCH_RESULTS = 24
MAX_PAGE_CHARS = 20_000
MAX_SNIPPET_CHARS = 500
DEFAULT_TIMEOUT_SECONDS = 20
USER_AGENT = "VoiceVibeLocal/0.1 (+https://local.app)"
DDGS_PROXY_ENV_KEYS = (
    "DDGS_PROXY",
    "https_proxy",
    "HTTPS_PROXY",
    "http_proxy",
    "HTTP_PROXY",
    "all_proxy",
    "ALL_PROXY",
)


class WebToolError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


@dataclass(frozen=True)
class WebSource:
    url: str
    title: str
    snippet: str


def validate_public_http_url(url: str) -> str:
    raw = str(url or "").strip()
    if not raw:
        raise WebToolError("INVALID_URL", "url is required")
    parsed = urllib.parse.urlparse(raw)
    if parsed.scheme not in {"http", "https"}:
        raise WebToolError("INVALID_URL", "only http and https URLs are allowed")
    if not parsed.hostname:
        raise WebToolError("INVALID_URL", "URL host is required")
    host = parsed.hostname.strip().lower()
    if host in {"localhost"} or host.endswith(".localhost"):
        raise WebToolError("PRIVATE_URL", "localhost URLs are not allowed")
    if is_private_host(host):
        raise WebToolError("PRIVATE_URL", "private network URLs are not allowed")
    return urllib.parse.urlunparse(parsed)


def is_private_host(host: str) -> bool:
    try:
        return is_blocked_ip(ipaddress.ip_address(host))
    except ValueError:
        pass
    if host.endswith(".local"):
        return True
    try:
        infos = socket.getaddrinfo(host, None, type=socket.SOCK_STREAM)
    except OSError:
        return False
    for info in infos:
        address = info[4][0]
        try:
            if is_blocked_ip(ipaddress.ip_address(address)):
                return True
        except ValueError:
            continue
    return False


def is_blocked_ip(address: ipaddress._BaseAddress) -> bool:
    return (
        address.is_private
        or address.is_loopback
        or address.is_link_local
        or address.is_multicast
        or address.is_reserved
        or address.is_unspecified
    )


def clamp_int(value: Any, minimum: int, maximum: int, fallback: int) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return fallback
    return max(minimum, min(maximum, parsed))


def compact_text(value: str) -> str:
    return re.sub(r"\s+", " ", html.unescape(value or "")).strip()


def trim(value: str, limit: int) -> str:
    value = compact_text(value)
    if len(value) <= limit:
        return value
    return value[:limit].rstrip()


def run_web_search(query: str, max_results: int = 5) -> List[Dict[str, Any]]:
    query = str(query or "").strip()
    if not query:
        raise WebToolError("INVALID_QUERY", "query is required")
    limit = clamp_int(max_results, 1, MAX_SEARCH_RESULTS, 5)
    searxng = os.environ.get("VOICE_VIBE_SEARXNG_URL", "").strip()
    if searxng:
        return search_with_searxng(searxng, query, limit)
    return search_with_ddgs(query, limit)


def search_with_ddgs(query: str, limit: int) -> List[Dict[str, Any]]:
    try:
        from ddgs import DDGS
    except Exception as exc:
        raise WebToolError("SEARCH_UNAVAILABLE", f"ddgs is not installed: {exc}") from exc
    results: List[Dict[str, Any]] = []
    fetch_limit = min(MAX_DDGS_FETCH_RESULTS, max(limit * 3, limit + 4))
    try:
        with DDGS(proxy=ddgs_proxy_from_env(), timeout=DEFAULT_TIMEOUT_SECONDS) as ddgs:
            for item in ddgs.text(query, max_results=fetch_limit):
                source = source_from_search_item(item)
                if source is not None:
                    results.append(source)
                if len(results) >= limit:
                    break
    except Exception as exc:
        raise WebToolError("SEARCH_FAILED", str(exc)) from exc
    return results


def ddgs_proxy_from_env() -> Optional[str]:
    for key in DDGS_PROXY_ENV_KEYS:
        value = os.environ.get(key, "").strip()
        if value:
            return value
    return None


def search_with_searxng(base_url: str, query: str, limit: int) -> List[Dict[str, Any]]:
    endpoint = base_url.rstrip("/") + "/search"
    params = urllib.parse.urlencode({"q": query, "format": "json", "language": "auto"})
    request = urllib.request.Request(
        f"{endpoint}?{params}",
        headers={"User-Agent": USER_AGENT, "Accept": "application/json"},
    )
    try:
        with open_url(request) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
    except Exception as exc:
        raise WebToolError("SEARCH_FAILED", str(exc)) from exc
    results: List[Dict[str, Any]] = []
    for item in payload.get("results") or []:
        source = source_from_search_item(item)
        if source is not None:
            results.append(source)
        if len(results) >= limit:
            break
    return results


def source_from_search_item(item: Any) -> Optional[Dict[str, Any]]:
    if not isinstance(item, dict):
        return None
    url = str(item.get("href") or item.get("url") or "").strip()
    if not url:
        return None
    try:
        url = validate_public_http_url(url)
    except WebToolError:
        return None
    title = trim(str(item.get("title") or url), 180)
    snippet = trim(str(item.get("body") or item.get("content") or item.get("snippet") or ""), MAX_SNIPPET_CHARS)
    return {
        "tool": "web_search",
        "source_type": "web",
        "type": "web",
        "id": url,
        "url": url,
        "title": title,
        "snippet": snippet,
    }


def read_public_web_page(url: str, max_chars: int = 12_000) -> Dict[str, Any]:
    url = validate_public_http_url(url)
    limit = clamp_int(max_chars, 1_000, MAX_PAGE_CHARS, 12_000)
    source = read_with_urllib(url, limit)
    return {
        "tool": "read_web_page",
        "source_type": "web",
        "type": "web",
        "id": source.url,
        "url": source.url,
        "title": source.title,
        "snippet": source.snippet,
        "content": source.snippet,
    }


def read_with_urllib(url: str, limit: int) -> WebSource:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "text/html, text/plain"})
    try:
        with open_url(request) as response:
            content_type = response.headers.get("content-type", "")
            final_url = validate_public_http_url(response.geturl())
            raw = response.read(limit * 4)
    except Exception as exc:
        raise WebToolError("PAGE_READ_FAILED", str(exc)) from exc
    text = raw.decode("utf-8", errors="replace")
    title = extract_title(text) or final_url
    if "html" in content_type.lower() or "<html" in text.lower():
        text = html_to_text(text)
    return WebSource(url=final_url, title=trim(title, 180), snippet=trim(text, limit))


def open_url(request: urllib.request.Request):
    context = ssl._create_unverified_context() if ddgs_proxy_from_env() else None
    return urllib.request.urlopen(request, timeout=DEFAULT_TIMEOUT_SECONDS, context=context)


def extract_title(text: str) -> str:
    match = re.search(r"<title[^>]*>(.*?)</title>", text, flags=re.IGNORECASE | re.DOTALL)
    return compact_text(match.group(1)) if match else ""


def html_to_text(text: str) -> str:
    text = re.sub(r"(?is)<(script|style|noscript).*?>.*?</\1>", " ", text)
    text = re.sub(r"(?i)<br\s*/?>", "\n", text)
    text = re.sub(r"(?i)</(p|div|section|article|h[1-6]|li)>", "\n", text)
    return re.sub(r"<[^>]+>", " ", text)


def build_mcp():
    from mcp.server.fastmcp import FastMCP

    mcp = FastMCP(
        "voice-vibe-web",
        instructions=(
            "Read-only public web tools. Search public web pages and read bounded page text. "
            "Do not use private network URLs or non-http(s) URLs."
        ),
        log_level="ERROR",
    )

    @mcp.tool()
    def web_search(query: str, max_results: int = 5) -> List[Dict[str, Any]]:
        """Search public web pages without an API key."""
        return run_web_search(query=query, max_results=max_results)

    @mcp.tool()
    def read_web_page(url: str, max_chars: int = 12_000) -> Dict[str, Any]:
        """Read bounded text from one public web page."""
        return read_public_web_page(url=url, max_chars=max_chars)

    return mcp


def main() -> None:
    build_mcp().run(transport="stdio")


if __name__ == "__main__":
    main()
