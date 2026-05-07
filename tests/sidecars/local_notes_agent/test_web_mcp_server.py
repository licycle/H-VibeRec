import importlib.util
import os
import sys
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]
WEB_TOOLS_PATH = REPO_ROOT / "sidecars" / "local_notes_agent" / "web_mcp_server.py"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


web_tools = load_module("voice_vibe_web_tools", WEB_TOOLS_PATH)


class WebMcpServerTests(unittest.TestCase):
    def test_search_with_ddgs_passes_https_proxy_from_env(self):
        captured = {}

        class FakeDDGS:
            def __init__(self, **kwargs):
                captured.update(kwargs)

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def text(self, query, max_results):
                return [{
                    "href": "https://93.184.216.34/",
                    "title": "Example",
                    "body": "Search result.",
                }]

        original_module = sys.modules.get("ddgs")
        original_proxy = os.environ.get("https_proxy")
        try:
            sys.modules["ddgs"] = type("FakeDdgsModule", (), {"DDGS": FakeDDGS})
            os.environ["https_proxy"] = "http://127.0.0.1:1087"

            web_tools.search_with_ddgs("example search", 1)

            self.assertEqual(captured.get("proxy"), "http://127.0.0.1:1087")
        finally:
            if original_module is None:
                sys.modules.pop("ddgs", None)
            else:
                sys.modules["ddgs"] = original_module
            if original_proxy is None:
                os.environ.pop("https_proxy", None)
            else:
                os.environ["https_proxy"] = original_proxy

    def test_search_with_ddgs_prefers_ddgs_proxy_env(self):
        captured = {}

        class FakeDDGS:
            def __init__(self, **kwargs):
                captured.update(kwargs)

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def text(self, query, max_results):
                return [{
                    "href": "https://93.184.216.34/",
                    "title": "Example",
                    "body": "Search result.",
                }]

        original_module = sys.modules.get("ddgs")
        original_ddgs_proxy = os.environ.get("DDGS_PROXY")
        original_https_proxy = os.environ.get("https_proxy")
        try:
            sys.modules["ddgs"] = type("FakeDdgsModule", (), {"DDGS": FakeDDGS})
            os.environ["DDGS_PROXY"] = "http://127.0.0.1:1087"
            os.environ["https_proxy"] = "http://127.0.0.1:9999"

            web_tools.search_with_ddgs("example search", 1)

            self.assertEqual(captured.get("proxy"), "http://127.0.0.1:1087")
        finally:
            if original_module is None:
                sys.modules.pop("ddgs", None)
            else:
                sys.modules["ddgs"] = original_module
            if original_ddgs_proxy is None:
                os.environ.pop("DDGS_PROXY", None)
            else:
                os.environ["DDGS_PROXY"] = original_ddgs_proxy
            if original_https_proxy is None:
                os.environ.pop("https_proxy", None)
            else:
                os.environ["https_proxy"] = original_https_proxy

    def test_search_with_ddgs_overfetches_when_filtered_results_precede_valid_results(self):
        captured = {}

        class FakeDDGS:
            def __init__(self, **kwargs):
                captured.update(kwargs)

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def text(self, query, max_results):
                captured["max_results"] = max_results
                return [
                    {
                        "href": "http://127.0.0.1/private",
                        "title": "Private",
                        "body": "Filtered.",
                    },
                    {
                        "href": "https://93.184.216.34/",
                        "title": "Example",
                        "body": "Search result.",
                    },
                ]

        original_module = sys.modules.get("ddgs")
        try:
            sys.modules["ddgs"] = type("FakeDdgsModule", (), {"DDGS": FakeDDGS})

            results = web_tools.search_with_ddgs("example search", 1)

            self.assertGreater(captured["max_results"], 1)
            self.assertEqual(len(results), 1)
            self.assertEqual(results[0]["url"], "https://93.184.216.34/")
        finally:
            if original_module is None:
                sys.modules.pop("ddgs", None)
            else:
                sys.modules["ddgs"] = original_module

    def test_read_with_urllib_uses_unverified_ssl_context_when_proxy_configured(self):
        calls = []

        class FakeResponse:
            headers = {"content-type": "text/html"}

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def geturl(self):
                return "https://example.com/"

            def read(self, limit):
                return b"<html><head><title>Example</title></head><body>Hello</body></html>"

        def fake_urlopen(request, timeout, context=None):
            calls.append(context)
            self.assertIsNotNone(context)
            self.assertFalse(context.check_hostname)
            return FakeResponse()

        original_urlopen = web_tools.urllib.request.urlopen
        original_proxy = os.environ.get("DDGS_PROXY")
        try:
            web_tools.urllib.request.urlopen = fake_urlopen
            os.environ["DDGS_PROXY"] = "http://127.0.0.1:1087"

            source = web_tools.read_with_urllib("https://example.com/", 1000)

            self.assertEqual(len(calls), 1)
            self.assertEqual(source.title, "Example")
            self.assertIn("Hello", source.snippet)
        finally:
            web_tools.urllib.request.urlopen = original_urlopen
            if original_proxy is None:
                os.environ.pop("DDGS_PROXY", None)
            else:
                os.environ["DDGS_PROXY"] = original_proxy


    def test_validate_public_http_url_rejects_private_targets(self):
        blocked = [
            "file:///tmp/a.html",
            "http://localhost:8000",
            "http://127.0.0.1:8000",
            "http://10.0.0.1",
            "http://192.168.1.1",
        ]

        for url in blocked:
            with self.subTest(url=url):
                with self.assertRaises(web_tools.WebToolError):
                    web_tools.validate_public_http_url(url)

    def test_source_from_search_item_returns_web_source(self):
        source = web_tools.source_from_search_item({
            "href": "https://example.com/page",
            "title": "Example Page",
            "body": "A short search result.",
        })

        self.assertEqual(source["type"], "web")
        self.assertEqual(source["source_type"], "web")
        self.assertEqual(source["id"], "https://example.com/page")
        self.assertEqual(source["url"], "https://example.com/page")
        self.assertEqual(source["title"], "Example Page")

    def test_html_to_text_removes_scripts_and_tags(self):
        text = web_tools.html_to_text(
            "<html><head><script>bad()</script></head><body><h1>标题</h1><p>正文</p></body></html>"
        )

        self.assertIn("标题", text)
        self.assertIn("正文", text)
        self.assertNotIn("bad()", text)


if __name__ == "__main__":
    unittest.main()
