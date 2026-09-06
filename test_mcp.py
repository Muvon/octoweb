#!/usr/bin/env python3
"""
End-to-end test suite for the octoweb MCP browser-control server.

Prerequisites:
    The octoweb browser must be running with the MCP server enabled on
    http://127.0.0.1:3434/mcp (stateless streamable-http, plain JSON).

The suite starts its own fixture HTTP server on http://127.0.0.1:8765
serving tests/fixtures/ plus two dynamic endpoints:
    /api/data   -> JSON payload (network-capture tests)
    /submitted  -> echoes query params in the body (form-submit tests)

Each test opens a fresh background tab pointed at a fixture page, drives
tools against that tab_id, asserts via browser_execute_js, and closes the
tab afterwards.

Usage:
    python3 test_mcp.py                    # run everything
    python3 test_mcp.py --filter nav       # run tests whose name contains "nav"
    python3 test_mcp.py --list             # list test names
    python3 test_mcp.py --mcp-url URL      # non-default MCP endpoint

Python 3 stdlib only. Exit code 1 if any test fails.
"""

import argparse
import html
import json
import os
import re
import sys
import tempfile
import threading
import time
import traceback
import urllib.error
import urllib.request
from contextlib import contextmanager
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qsl, urlparse

MCP_URL = "http://127.0.0.1:3434/mcp"
FIXTURE_HOST = "127.0.0.1"
FIXTURE_PORT = 8765
FIXTURE_BASE = f"http://{FIXTURE_HOST}:{FIXTURE_PORT}"
FIXTURES_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "tests", "fixtures")

DEFAULT_TIMEOUT = 10.0   # seconds, per HTTP call
NAVIGATE_TIMEOUT = 30.0  # navigation blocks until page load


class MCPError(Exception):
    """Transport / protocol level failure (aborts the run)."""


class TestFailure(Exception):
    """A single test's assertion failure."""


# ──────────────────────────────────────────────────────────────────────
# Fixture HTTP server
# ──────────────────────────────────────────────────────────────────────

class FixtureHandler(SimpleHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path == "/api/data":
            body = json.dumps({"ok": True, "value": 42}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if parsed.path == "/api/attachment":
            # Showable MIME type served as an attachment — the Google Drive
            # download shape. Must be saved, never rendered inline.
            name = dict(parse_qsl(parsed.query)).get("name", "file.txt")
            body = b"octoweb attachment fixture\n"
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Disposition", f'attachment; filename="{name}"')
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if parsed.path == "/submitted":
            params = parse_qsl(parsed.query)
            echoed = " ".join(f"{k}={v}" for k, v in params) or "(no params)"
            body = (
                "<!DOCTYPE html><html><head><meta charset=\"utf-8\">"
                "<title>OW Fixture: Submitted</title></head>"
                "<body><h1>Form Submitted</h1>"
                f"<p id=\"echo\">submitted: {html.escape(echoed)}</p>"
                "</body></html>"
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        super().do_GET()

    def end_headers(self):
        # Never let the browser cache fixtures between runs / re-navigations.
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *args):
        pass  # keep test output clean


def start_fixture_server():
    handler = partial(FixtureHandler, directory=FIXTURES_DIR)
    httpd = ThreadingHTTPServer((FIXTURE_HOST, FIXTURE_PORT), handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    # Sanity check it actually serves a fixture.
    with urllib.request.urlopen(f"{FIXTURE_BASE}/basic.html", timeout=5) as resp:
        if resp.status != 200:
            raise MCPError(f"fixture server self-check failed: HTTP {resp.status}")
    return httpd


def fixture_url(page):
    return f"{FIXTURE_BASE}/{page}"


# ──────────────────────────────────────────────────────────────────────
# MCP client (JSON-RPC 2.0 over HTTP POST, plain JSON responses)
# ──────────────────────────────────────────────────────────────────────

_next_id = 1


def _http_post_json(payload, timeout):
    data = json.dumps(payload).encode()
    req = urllib.request.Request(
        MCP_URL,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
            ctype = resp.headers.get("Content-Type", "")
    except urllib.error.HTTPError as exc:
        raise MCPError(f"HTTP {exc.code} from MCP server: {exc.read()[:200]!r}")
    except (urllib.error.URLError, OSError) as exc:
        raise MCPError(f"cannot reach MCP server at {MCP_URL}: {exc}")
    if not body.strip():
        return None  # e.g. accepted notification
    if "text/event-stream" in ctype:
        # Defensive: server normally replies plain JSON, but tolerate SSE framing.
        for line in body.splitlines():
            if line.startswith("data:"):
                chunk = line[5:].strip()
                if chunk:
                    return json.loads(chunk)
        raise MCPError(f"SSE response without data payload: {body[:200]!r}")
    return json.loads(body)


def rpc(method, params=None, notify=False, timeout=DEFAULT_TIMEOUT):
    global _next_id
    payload = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        payload["params"] = params
    if not notify:
        payload["id"] = _next_id
        _next_id += 1
    resp = _http_post_json(payload, timeout)
    if notify:
        return None
    if resp is None:
        raise MCPError(f"empty response for request {method}")
    if "error" in resp:
        raise MCPError(f"{method} JSON-RPC error: {resp['error']}")
    return resp.get("result", {})


def mcp_initialize():
    # rmcp negotiates DOWN to whatever the client asks for, so a client stuck on
    # 2024-11-05 never sees the spec revision that defines tool annotations.
    result = rpc("initialize", {
        "protocolVersion": "2025-11-25",
        "capabilities": {},
        "clientInfo": {"name": "octoweb-e2e", "version": "1.0.0"},
    })
    rpc("notifications/initialized", notify=True)
    return result


# Calls-to-success benchmark: total MCP tool calls per test. Fewer calls to
# reach the same verified outcome = a more efficient (less "stupid") agent.
CALL_COUNTER = {"n": 0}


def call_tool(name, arguments=None, timeout=DEFAULT_TIMEOUT):
    """Returns (is_error, content_list, joined_text)."""
    CALL_COUNTER["n"] += 1
    result = rpc("tools/call", {"name": name, "arguments": arguments or {}}, timeout=timeout)
    content = result.get("content", [])
    text = "\n".join(c.get("text", "") for c in content if c.get("type") == "text")
    return bool(result.get("isError")), content, text


def tool_text(name, arguments=None, timeout=DEFAULT_TIMEOUT):
    """Call a tool, fail the test if it reports isError, return its text."""
    is_err, _, text = call_tool(name, arguments, timeout)
    if is_err:
        raise TestFailure(f"{name}({arguments}) returned error: {text}")
    return text


# ──────────────────────────────────────────────────────────────────────
# Test helpers
# ──────────────────────────────────────────────────────────────────────

def expect(cond, msg):
    if not cond:
        raise TestFailure(msg)


def parse_json_text(text, what):
    try:
        return json.loads(text)
    except ValueError:
        match = re.search(r"[\[{].*[\]}]", text, re.S)
        if match:
            try:
                return json.loads(match.group(0))
            except ValueError:
                pass
        raise TestFailure(f"{what}: expected JSON text, got: {text[:300]!r}")


def navigate(url, tab_id=None):
    args = {"url": url}
    if tab_id is not None:
        args["tab_id"] = tab_id
    text = tool_text("browser_navigate", args, timeout=NAVIGATE_TIMEOUT)
    info = parse_json_text(text, "browser_navigate")
    expect("tab_id" in info, f"browser_navigate response missing tab_id: {text[:300]!r}")
    return info


def get_tabs(**args):
    """Returns the tab list; pass limit/query to exercise paging."""
    args.setdefault("limit", 200)
    page = parse_json_text(tool_text("browser_get_tabs", args), "browser_get_tabs")
    expect(isinstance(page, dict) and "tabs" in page and "total" in page,
           f"browser_get_tabs must return {{total, tabs}}, got: {str(page)[:200]!r}")
    return page["tabs"]


def tab_entry(tabs, tab_id):
    for tab in tabs:
        if tab.get("id") == tab_id:
            return tab
    return None


def close_tab(tab_id):
    """Best-effort close; ignores errors (tab may already be gone)."""
    try:
        call_tool("browser_close_tab", {"tab_id": tab_id})
    except MCPError:
        pass


@contextmanager
def fixture_tab(page):
    """Open a fixture in a new background tab; always close it afterwards."""
    info = navigate(fixture_url(page))
    tab_id = info["tab_id"]
    try:
        yield tab_id
    finally:
        close_tab(tab_id)


def exec_js(tab_id, script):
    return tool_text("browser_execute_js", {"tab_id": tab_id, "script": script})


def js_text(tab_id, element_id):
    return exec_js(tab_id, f"document.getElementById('{element_id}').textContent")


def snapshot(tab_id):
    return tool_text("browser_snapshot", {"tab_id": tab_id}, timeout=15)


def find_ref(snapshot_text, needle):
    """Find the @ref of the snapshot line containing needle. Returns (ref, line)."""
    for line in snapshot_text.splitlines():
        if needle in line:
            match = re.search(r"@\d+", line)
            if match:
                return match.group(0), line
    raise TestFailure(f"snapshot has no @ref line containing {needle!r}; snapshot:\n{snapshot_text[:1500]}")


def poll(fn, predicate, attempts=10, delay=0.3):
    """Re-evaluate fn() until predicate(result) or attempts exhausted. Returns last result."""
    result = None
    for i in range(attempts):
        result = fn()
        if predicate(result):
            break
        if i < attempts - 1:
            time.sleep(delay)
    return result


# ──────────────────────────────────────────────────────────────────────
# Tests
# ──────────────────────────────────────────────────────────────────────

TESTS = []


def test(fn):
    TESTS.append(fn)
    return fn


# ──────────────────────────────────────────────────────────────────────
# Protocol surface — the tool list, its annotations, and the negotiated
# spec revision. Nothing pinned these, so a refactor that dropped a tool,
# flipped a readOnlyHint back to the destructive default, or renamed a
# response field shipped green.
# ──────────────────────────────────────────────────────────────────────

EXPECTED_TOOLS = {
    "browser_navigate", "browser_go_back", "browser_go_forward", "browser_reload",
    "browser_wait", "browser_get_tabs", "browser_get_current_tab", "browser_switch_tab",
    "browser_close_tab", "browser_snapshot", "browser_get_page_info",
    "browser_get_page_content", "browser_execute_js", "browser_click", "browser_type",
    "browser_fill_form", "browser_dismiss_overlay", "browser_hover", "browser_scroll",
    "browser_press_key", "browser_select_option", "browser_screenshot",
    "browser_console_messages", "browser_network_requests", "browser_handle_dialog",
    "browser_upload_file", "browser_get_history", "browser_get_playing_tabs", "render_ui",
}

# Tools a host must be able to auto-approve. If any of these loses
# readOnlyHint, cautious clients start prompting on every page read and
# unattended runs stall at step 2.
READ_ONLY_TOOLS = {
    "browser_snapshot", "browser_get_page_content", "browser_get_page_info",
    "browser_get_tabs", "browser_get_current_tab", "browser_get_history",
    "browser_get_playing_tabs", "browser_screenshot", "browser_console_messages",
    "browser_network_requests", "browser_wait",
}


def _tools_by_name():
    listing = rpc("tools/list", {})
    return {t["name"]: t for t in listing.get("tools", [])}


@test
def tools_list_surface_is_pinned():
    """Every documented tool is present and nothing extra crept in."""
    tools = _tools_by_name()
    missing = EXPECTED_TOOLS - set(tools)
    extra = set(tools) - EXPECTED_TOOLS
    expect(not missing, f"tools/list is missing: {sorted(missing)}")
    expect(not extra, f"tools/list has undocumented tools: {sorted(extra)}")


@test
def read_tools_are_annotated_read_only():
    """readOnlyHint is what lets a host stop prompting on every page read."""
    tools = _tools_by_name()
    for name in sorted(READ_ONLY_TOOLS):
        ann = tools.get(name, {}).get("annotations") or {}
        expect(ann.get("readOnlyHint") is True,
               f"{name} must be annotated readOnlyHint=true, got {ann!r}")
    # Acting tools must NOT claim to be read-only.
    for name in ("browser_click", "browser_type", "browser_execute_js", "browser_close_tab"):
        ann = tools.get(name, {}).get("annotations") or {}
        expect(ann.get("readOnlyHint") is not True,
               f"{name} must not be annotated read-only, got {ann!r}")


@test
def initialize_negotiates_a_spec_that_defines_annotations():
    """2024-11-05 predates tool annotations — the server must offer newer."""
    result = mcp_initialize()
    version = result.get("protocolVersion", "")
    expect(version >= "2025-03-26",
           f"server negotiated {version!r}; annotations need 2025-03-26 or later")


# ──────────────────────────────────────────────────────────────────────
# Agent-safety and context-budget behaviour
# ──────────────────────────────────────────────────────────────────────

@test
def navigate_refuses_javascript_and_data_urls():
    """An agent takes URLs from pages it just read — these are code, not pages."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        for bad in ("javascript:window.__pwn=1",
                    "data:text/html,<h1>pwn</h1>",
                    "JavaScript:window.__pwn=1"):
            is_err, _, text = call_tool("browser_navigate", {"tab_id": tab_id, "url": bad})
            expect(is_err, f"browser_navigate accepted {bad!r}: {text[:200]!r}")
        # The page was never touched.
        val = tool_text("browser_execute_js",
                        {"tab_id": tab_id, "script": "String(window.__pwn)"})
        expect("undefined" in val, f"javascript: URL executed in the page: {val!r}")
    finally:
        close_tab(tab_id)


@test
def navigate_reports_where_it_landed_and_how_settled():
    """A redirect into a login wall must not look like a clean load."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        expect("url" in info, f"browser_navigate response has no url: {info!r}")
        expect(info["url"].endswith("basic.html"),
               f"navigate reported the wrong landing url: {info['url']!r}")
        expect(info.get("readiness") in ("ready", "live", "partial", "shell")
               or str(info.get("readiness", "")).startswith("probe-error"),
               f"unexpected readiness verdict: {info.get('readiness')!r}")
    finally:
        close_tab(tab_id)


@test
def page_content_pages_instead_of_dumping_everything():
    """One call must not be able to eat the caller's whole context window."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        head = tool_text("browser_get_page_content", {"tab_id": tab_id, "limit": 20})
        first_line = head.split("\n", 1)[0]
        expect(first_line.startswith("[showing characters "),
               f"truncated content must lead with a paging note: {head[:200]!r}")
        expect("offset:" in first_line,
               f"paging note must carry the resume offset: {first_line!r}")
        expect(head.index("<untrusted>") > head.index(first_line),
               "the paging note must sit outside the untrusted fence")
        full = tool_text("browser_get_page_content", {"tab_id": tab_id, "limit": 0})
        expect(not full.startswith("[showing characters "),
               f"limit:0 must not be truncated: {full[:120]!r}")
        # Compare the page text itself, not the replies: only the truncated one
        # carries a paging note, which on a short fixture is longer than the
        # content it withheld.
        def body(reply):
            start = reply.index("<untrusted>") + len("<untrusted>")
            return reply[start:reply.index("</untrusted>")].strip()
        expect(len(body(full)) > len(body(head)),
               f"limit:0 should return more page text than limit:20 "
               f"({len(body(full))} vs {len(body(head))})")
    finally:
        close_tab(tab_id)


@test
def snapshot_find_and_limit_narrow_the_map():
    """find/limit are how an agent avoids paying for the whole page."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        capped = tool_text("browser_snapshot", {"tab_id": tab_id, "limit": 1})
        expect(capped.count("@") >= 1, f"limit:1 returned nothing usable: {capped[:200]!r}")
        nomatch = tool_text("browser_snapshot",
                            {"tab_id": tab_id, "find": "zzz-no-such-control-zzz"})
        expect("no elements matching" in nomatch,
               f"find with no matches should say so: {nomatch[:200]!r}")
    finally:
        close_tab(tab_id)


@test
def page_derived_text_is_fenced_as_untrusted():
    """The agent can click and type — it must know which bytes are the page's."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        for tool in ("browser_get_page_content", "browser_snapshot"):
            text = tool_text(tool, {"tab_id": tab_id})
            expect("<untrusted>" in text and "</untrusted>" in text,
                   f"{tool} output is not fenced: {text[:200]!r}")
    finally:
        close_tab(tab_id)


@test
def scroll_reports_the_resulting_position():
    """Without this an agent paging a feed has no termination condition."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        text = tool_text("browser_scroll", {"tab_id": tab_id, "direction": "bottom"})
        expect("scrollTop" in text, f"scroll must report position, got {text!r}")
    finally:
        close_tab(tab_id)


@test
def unknown_workspace_token_is_refused_not_silently_defaulted():
    """A bad token must never fall through to the user's real workspace."""
    payload = {
        "jsonrpc": "2.0", "id": 999999, "method": "tools/call",
        "params": {"name": "browser_get_tabs", "arguments": {}},
    }
    req = urllib.request.Request(
        MCP_URL, data=json.dumps(payload).encode(),
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "X-Octoweb-Workspace": "deadbeef" * 4,
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=DEFAULT_TIMEOUT) as resp:
            body = json.loads(resp.read().decode("utf-8", "replace"))
    except urllib.error.HTTPError:
        return  # rejected at the transport — also correct
    result = body.get("result", {})
    text = "\n".join(c.get("text", "") for c in result.get("content", []))
    expect(body.get("error") or result.get("isError"),
           f"unknown workspace token was accepted: {text[:200]!r}")


@test
def cross_origin_pages_cannot_drive_the_browser():
    """Any site the user visits could otherwise fetch() this endpoint."""
    payload = {
        "jsonrpc": "2.0", "id": 999998, "method": "tools/call",
        "params": {"name": "browser_get_tabs", "arguments": {}},
    }
    req = urllib.request.Request(
        MCP_URL, data=json.dumps(payload).encode(),
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "Origin": "https://evil.example",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=DEFAULT_TIMEOUT) as resp:
            body = resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as exc:
        expect(exc.code in (400, 403),
               f"expected the Origin to be rejected, got HTTP {exc.code}")
        return
    raise TestFailure(f"cross-origin request was served: {body[:200]!r}")


@test
def type_reports_what_the_field_actually_holds():
    """Setting a value is an assertion; reading it back is the observation."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        # A field that accepts the value cleanly must NOT be flagged.
        ok = tool_text("browser_execute_js", {
            "tab_id": tab_id,
            "script": "(function(){var i=document.createElement('input');i.id='octoweb_probe';"
                      "document.body.appendChild(i);return 'ok';})()",
        })
        expect("ok" in ok, f"could not create probe input: {ok!r}")
        clean = tool_text("browser_type", {"tab_id": tab_id, "selector": "#octoweb_probe",
                                           "value": "hello world"})
        expect("TEXT DID NOT STICK" not in clean,
               f"a field that accepted the value was flagged: {clean!r}")

        # A field whose value the page reverts must be flagged, not reported as typed.
        tool_text("browser_execute_js", {
            "tab_id": tab_id,
            "script": "(function(){var i=document.getElementById('octoweb_probe');"
                      "i.addEventListener('input',function(){i.value='';});return 'armed';})()",
        })
        reverted = tool_text("browser_type", {"tab_id": tab_id, "selector": "#octoweb_probe",
                                              "value": "this will be thrown away"})
        expect("TEXT DID NOT STICK" in reverted,
               f"a reverted value was reported as typed: {reverted!r}")
    finally:
        close_tab(tab_id)


@test
def snapshot_shows_contenteditable_content():
    """Without val= a rich editor reads identically full or empty."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        tool_text("browser_execute_js", {
            "tab_id": tab_id,
            "script": "(function(){var d=document.createElement('div');d.contentEditable='true';"
                      "d.id='octoweb_ce';d.textContent='DRAFTBODY';document.body.appendChild(d);"
                      "return 'ok';})()",
        })
        snap = tool_text("browser_snapshot", {"tab_id": tab_id, "find": "DRAFTBODY"})
        expect("DRAFTBODY" in snap,
               f"contenteditable content missing from snapshot: {snap[:300]!r}")
    finally:
        close_tab(tab_id)


@test
def fill_form_reports_per_field_effects_not_just_ticks():
    """A bare tick is the same claim-instead-of-observation shape as browser_type's."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        tool_text("browser_execute_js", {
            "tab_id": tab_id,
            "script": "(function(){var i=document.createElement('input');i.id='octoweb_ff';"
                      "i.addEventListener('input',function(){i.value='';});"
                      "document.body.appendChild(i);return 'ok';})()",
        })
        text = tool_text("browser_fill_form", {
            "tab_id": tab_id,
            "fields": [{"selector": "#octoweb_ff", "value": "value the page rejects"}],
        })
        expect("TEXT DID NOT STICK" in text,
               f"fill_form hid a rejected value behind a tick: {text!r}")
        expect("Filled 0/1" in text,
               f"a rejected field must not count as filled: {text!r}")
    finally:
        close_tab(tab_id)


@test
def nav_default_is_background():
    """navigate without tab_id opens a NEW tab that is NOT active."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        expect(info.get("mode") == "new_background_tab",
               f"expected mode 'new_background_tab', got {info.get('mode')!r}")
        entry = tab_entry(get_tabs(), tab_id)
        expect(entry is not None, f"new tab {tab_id} missing from browser_get_tabs")
        expect(not entry.get("is_active"),
               f"new background tab must not be active, got {entry}")
    finally:
        close_tab(tab_id)


@test
def nav_in_place():
    """navigate with tab_id reuses the tab; no extra tab appears."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        count_before = len(get_tabs())
        info2 = navigate(fixture_url("textarea.html"), tab_id=tab_id)
        expect(info2.get("mode") == "in_place",
               f"expected mode 'in_place', got {info2.get('mode')!r}")
        expect(info2["tab_id"] == tab_id,
               f"in-place navigation changed tab id: {tab_id} -> {info2['tab_id']}")
        tabs = get_tabs()
        expect(len(tabs) == count_before,
               f"tab count changed on in-place navigation: {count_before} -> {len(tabs)}")
        entry = tab_entry(tabs, tab_id)
        expect(entry is not None and "textarea.html" in entry.get("url", ""),
               f"tab {tab_id} url not updated after in-place navigation: {entry}")
    finally:
        close_tab(tab_id)


@test
def nav_dead_tab():
    """navigate with a nonexistent tab_id errors and suggests omitting tab_id."""
    is_err, _, text = call_tool(
        "browser_navigate",
        {"url": fixture_url("basic.html"), "tab_id": 99999},
        timeout=NAVIGATE_TIMEOUT,
    )
    expect(is_err, f"navigate to dead tab 99999 must be isError, got success: {text[:200]!r}")
    expect("omit" in text.lower(),
           f"dead-tab error should mention omitting tab_id, got: {text[:300]!r}")


@test
def switch_and_close_tab():
    """switch_tab makes a tab active; close_tab removes it from the list."""
    info = navigate(fixture_url("basic.html"))
    tab_id = info["tab_id"]
    try:
        tool_text("browser_switch_tab", {"tab_id": tab_id})
        entry = tab_entry(get_tabs(), tab_id)
        expect(entry is not None, f"tab {tab_id} missing after switch")
        expect(entry.get("is_active") is True,
               f"tab {tab_id} not active after browser_switch_tab: {entry}")
        tool_text("browser_close_tab", {"tab_id": tab_id})
        expect(tab_entry(get_tabs(), tab_id) is None,
               f"tab {tab_id} still listed after browser_close_tab")
    finally:
        close_tab(tab_id)


@test
def type_textarea():
    """Typing into a <textarea> works (regression: used to throw)."""
    with fixture_tab("textarea.html") as tab:
        tool_text("browser_type", {"tab_id": tab, "selector": "#ta", "text": "hello textarea"})
        value = exec_js(tab, "document.getElementById('ta').value")
        expect("hello textarea" in value, f"textarea value mismatch: {value!r}")


@test
def type_react_controlled():
    """Typed text must survive a controlled input that re-renders from state."""
    with fixture_tab("controlled_input.html") as tab:
        tool_text("browser_type", {"tab_id": tab, "selector": "#ctl", "text": "controlled text"})
        mirror = js_text(tab, "mirror")
        expect("controlled text" in mirror,
               f"state not updated -- 'input' event was not dispatched (mirror={mirror!r})")
        time.sleep(0.3)  # let the 100ms re-render interval wipe non-event typing
        value = exec_js(tab, "document.getElementById('ctl').value")
        expect("controlled text" in value,
               f"value wiped by controlled re-render -- native value setter + input event required (value={value!r})")


@test
def click_pointer_only():
    """Button listening only to pointerdown/pointerup must react to click tool."""
    with fixture_tab("pointer_only.html") as tab:
        tool_text("browser_click", {"tab_id": tab, "selector": "#ptr-btn"}, timeout=15)
        status = js_text(tab, "status")
        expect("pointer-activated" in status,
               f"pointer-only button not activated, status={status!r}")


@test
def click_listener_div():
    """div interactive only via addEventListener('click') is in snapshot and clickable via @ref."""
    with fixture_tab("listener_div.html") as tab:
        ref, line = find_ref(snapshot(tab), "Listener Div Target")
        expect("clickable" in line.lower(),
               f"listener div should have role 'clickable' in snapshot line: {line!r}")
        tool_text("browser_click", {"tab_id": tab, "selector": ref}, timeout=15)
        status = js_text(tab, "status")
        expect("div-clicked" in status, f"listener div not clicked, status={status!r}")


@test
def shadow_dom():
    """Button inside an open shadow root is in snapshot and clickable via @ref."""
    with fixture_tab("shadow_dom.html") as tab:
        ref, _ = find_ref(snapshot(tab), "Shadow Button")
        tool_text("browser_click", {"tab_id": tab, "selector": ref}, timeout=15)
        status = js_text(tab, "status")
        expect("shadow-clicked" in status, f"shadow button not clicked, status={status!r}")
        attr = exec_js(tab, "document.getElementById('host').getAttribute('data-clicked')")
        expect("1" in attr, f"host data-clicked attribute not set: {attr!r}")


@test
def iframe_click():
    """Button inside a same-origin iframe is in snapshot and clickable via @ref."""
    with fixture_tab("iframe_outer.html") as tab:
        ref, _ = find_ref(snapshot(tab), "Iframe Button")
        tool_text("browser_click", {"tab_id": tab, "selector": ref}, timeout=15)
        status = js_text(tab, "iframe-status")
        expect("iframe-clicked" in status, f"iframe button not clicked, status={status!r}")


@test
def enter_submits_form():
    """Type + Enter submits the form; /submitted echoes the value."""
    with fixture_tab("form_submit.html") as tab:
        tool_text("browser_type", {"tab_id": tab, "selector": "#q", "text": "octotest123"})
        tool_text("browser_press_key", {"tab_id": tab, "key": "Enter", "selector": "#q"})
        tool_text("browser_wait", {"tab_id": tab, "event": "load", "timeout_ms": 10000}, timeout=15)
        content = poll(
            lambda: tool_text("browser_get_page_content", {"tab_id": tab}),
            lambda c: "octotest123" in c,
        )
        expect("octotest123" in content,
               f"submitted page does not echo the typed value: {content[:300]!r}")


@test
def occluded_click_blocked():
    """Click on a permanently covered button errors mentioning 'covered'."""
    with fixture_tab("occluded.html") as tab:
        is_err, _, text = call_tool("browser_click", {"tab_id": tab, "selector": "#target"}, timeout=15)
        expect(is_err, f"click on occluded button must error, got success: {text[:200]!r}")
        expect("covered" in text.lower(),
               f"occlusion error should mention 'covered', got: {text[:300]!r}")
        status = js_text(tab, "status")
        expect("unclicked" in status, f"occluded button must not have been clicked: {status!r}")


@test
def occluded_click_retry():
    """Overlay removes itself after 800ms; click auto-retry succeeds."""
    with fixture_tab("occluded_retry.html") as tab:
        tool_text("browser_click", {"tab_id": tab, "selector": "#target"}, timeout=15)
        status = js_text(tab, "status")
        expect("clicked" in status, f"retry click did not land, status={status!r}")


@test
def late_element_click():
    """Button added 750ms after load is clickable by CSS selector (resolve retry)."""
    with fixture_tab("late.html") as tab:
        tool_text("browser_click", {"tab_id": tab, "selector": "#late"}, timeout=15)
        status = js_text(tab, "status")
        expect("late-clicked" in status, f"late button not clicked, status={status!r}")


@test
def select_option():
    """browser_select_option fires the change handler with the right value."""
    with fixture_tab("select_option.html") as tab:
        tool_text("browser_select_option", {"tab_id": tab, "selector": "#sel", "value": "beta"})
        status = js_text(tab, "status")
        expect("beta" in status, f"select change handler saw wrong value: {status!r}")


@test
def contenteditable_type():
    """Typing into contenteditable updates its textContent."""
    with fixture_tab("contenteditable.html") as tab:
        tool_text("browser_type", {"tab_id": tab, "selector": "#editor", "text": "editable text here"})
        content = js_text(tab, "editor")
        expect("editable text here" in content,
               f"contenteditable textContent mismatch: {content!r}")


@test
def controlled_editor_type():
    """Typing into a model-backed editor updates its internal model, not just
    the DOM — a submit button gated on that model must enable. Guards the
    rich-editor (Lexical/DraftJS/ProseMirror) regression where synthetic DOM
    writes left the model empty and the button disabled."""
    with fixture_tab("controlled_editor.html") as tab:
        tool_text("browser_type", {"tab_id": tab, "selector": "#editor", "text": "model gets this"})
        content = js_text(tab, "editor").strip('"')  # js_text returns JSON-encoded value
        expect(content == "model gets this",
               f"controlled editor textContent mismatch (expected exact replace, no doubling): {content!r}")
        disabled = exec_js(tab, "document.getElementById('post').disabled")
        expect("false" in disabled.lower(),
               f"submit button gated on the editor model did not enable: disabled={disabled!r}")


@test
def paragraph_editor_type():
    """Multi-paragraph text into a Medium-shaped editor (one host, title +
    body blocks, paste handler that splits lines into <p>): the paste must be
    recognised as landed (no double insert, no error) and the replace must be
    scoped to the targeted block — the title survives."""
    with fixture_tab("paragraph_editor.html") as tab:
        text = "First paragraph of the post.\n\nSecond paragraph, with \"quotes\" and more."
        result = tool_text("browser_type", {"tab_id": tab, "selector": "#body", "text": text})
        expect("keystrokes" not in result,
               f"paste handler was bypassed — fell through to native typing: {result!r}")
        host = exec_js(tab, "document.getElementById('host').textContent")
        expect("First paragraph" in host and "Second paragraph" in host,
               f"pasted paragraphs missing from editor: {host!r}")
        expect(host.count("First paragraph") == 1,
               f"text was inserted twice (paste + editing-command fallback): {host!r}")
        expect("Draft title" in js_text(tab, "title"),
               "replace was not scoped to the target block — the title was wiped")
        paragraphs = exec_js(tab, "document.querySelectorAll('#host p').length")
        expect("3" in paragraphs, f"expected 3 <p> blocks after paste, got {paragraphs!r}")
        pastes = exec_js(tab, "window.__pastes")
        expect("1" in pastes, f"expected exactly one paste event, got {pastes!r}")


@test
def type_keys_mode():
    """mode=\"keys\" types with trusted native keystrokes: the text lands in a
    plain contenteditable, and a newline becomes a real Enter (a new block)."""
    with fixture_tab("contenteditable.html") as tab:
        result = tool_text("browser_type", {"tab_id": tab, "selector": "#editor",
                                            "text": "typed by keys\nsecond line", "mode": "keys"})
        expect("keystrokes" in result, f"expected the native-keystroke path, got: {result!r}")
        content = js_text(tab, "editor")
        expect("typed by keys" in content and "second line" in content,
               f"keystroke typing did not land: {content!r}")
        blocks = exec_js(tab, "document.getElementById('editor').querySelectorAll('div,p,br').length")
        expect(blocks.strip('"') not in ("0", ""),
               f"Enter did not produce a line/block break: {blocks!r}")


@test
def type_rejects_non_editable():
    """browser_type must error (not falsely succeed) when the target is not a
    text field — the false 'success' that masks a wrong selector."""
    with fixture_tab("contenteditable.html") as tab:
        is_err, _, text = call_tool("browser_type", {"tab_id": tab, "selector": "h1", "text": "nope"})
        expect(is_err, f"typing into a non-editable <h1> should error, got success: {text!r}")
        expect("not a text field" in text,
               f"expected a clear 'not a text field' error, got: {text!r}")
        is_err, _, text = call_tool("browser_type", {"tab_id": tab, "selector": "#editor",
                                                     "text": "x", "mode": "paste"})
        expect(is_err and "mode must be" in text,
               f"unknown mode should be rejected, got: {text!r}")


@test
def console_messages():
    """console.log/warn/error and an uncaught error are captured with right levels."""
    with fixture_tab("console_log.html") as tab:
        needles = ["fixture-log-message", "fixture-warn-message",
                   "fixture-error-message", "fixture-uncaught-error"]

        def fetch():
            return parse_json_text(
                tool_text("browser_console_messages", {"tab_id": tab}),
                "browser_console_messages",
            )

        entries = poll(fetch, lambda es: all(
            any(n in e.get("text", "") for e in es) for n in needles))

        def entry_for(sub):
            for e in entries:
                if sub in e.get("text", ""):
                    return e
            return None

        log_e = entry_for("fixture-log-message")
        expect(log_e is not None, f"console.log entry missing: {entries}")
        expect(any(k in log_e.get("level", "").lower() for k in ("log", "info")),
               f"console.log entry has wrong level: {log_e}")
        warn_e = entry_for("fixture-warn-message")
        expect(warn_e is not None, f"console.warn entry missing: {entries}")
        expect("warn" in warn_e.get("level", "").lower(),
               f"console.warn entry has wrong level: {warn_e}")
        err_e = entry_for("fixture-error-message")
        expect(err_e is not None, f"console.error entry missing: {entries}")
        expect("error" in err_e.get("level", "").lower(),
               f"console.error entry has wrong level: {err_e}")
        uncaught = entry_for("fixture-uncaught-error")
        expect(uncaught is not None,
               f"uncaught window.onerror not captured: {entries}")
        expect("error" in uncaught.get("level", "").lower(),
               f"uncaught error entry has wrong level: {uncaught}")


@test
def network_requests():
    """fetch + XHR to /api/data both captured with status 200; filter param works."""
    with fixture_tab("network.html") as tab:
        text = tool_text("browser_wait", {"tab_id": tab, "event": "#done", "timeout_ms": 5000}, timeout=15)
        expect("ready" in text.lower(), f"fixture requests did not finish: {text!r}")
        entries = parse_json_text(
            tool_text("browser_network_requests", {"tab_id": tab}),
            "browser_network_requests",
        )

        def entry_for(sub):
            for e in entries:
                if sub in e.get("url", ""):
                    return e
            return None

        fetch_e = entry_for("src=fetch")
        xhr_e = entry_for("src=xhr")
        expect(fetch_e is not None, f"fetch request to /api/data not captured: {entries}")
        expect(xhr_e is not None, f"XHR request to /api/data not captured: {entries}")
        expect(int(fetch_e.get("status", 0)) == 200, f"fetch status != 200: {fetch_e}")
        expect(int(xhr_e.get("status", 0)) == 200, f"XHR status != 200: {xhr_e}")

        filtered = parse_json_text(
            tool_text("browser_network_requests", {"tab_id": tab, "filter": "api/data"}),
            "browser_network_requests(filter)",
        )
        expect(len(filtered) >= 2, f"filter 'api/data' should match both requests: {filtered}")
        expect(all("api/data" in e.get("url", "") for e in filtered),
               f"filter returned non-matching entries: {filtered}")


@test
def dialog_accept():
    """Armed accept makes confirm() return true. ALWAYS arm before triggering."""
    with fixture_tab("dialogs.html") as tab:
        text = tool_text("browser_handle_dialog", {"action": "accept"})
        expect(text.lower().startswith("armed"), f"handle_dialog should report Armed: {text!r}")
        tool_text("browser_click", {"tab_id": tab, "selector": "#confirm-btn"}, timeout=15)
        result = poll(lambda: js_text(tab, "confirm-result"), lambda r: "none" not in r)
        expect("accepted" in result, f"confirm() not auto-accepted: {result!r}")


@test
def dialog_dismiss():
    """Armed dismiss makes confirm() return false."""
    with fixture_tab("dialogs.html") as tab:
        text = tool_text("browser_handle_dialog", {"action": "dismiss"})
        expect(text.lower().startswith("armed"), f"handle_dialog should report Armed: {text!r}")
        tool_text("browser_click", {"tab_id": tab, "selector": "#confirm-btn"}, timeout=15)
        result = poll(lambda: js_text(tab, "confirm-result"), lambda r: "none" not in r)
        expect("dismissed" in result, f"confirm() not auto-dismissed: {result!r}")


@test
def prompt_text():
    """Armed accept with prompt_text answers prompt() with that text."""
    with fixture_tab("dialogs.html") as tab:
        text = tool_text("browser_handle_dialog", {"action": "accept", "prompt_text": "hello"})
        expect(text.lower().startswith("armed"), f"handle_dialog should report Armed: {text!r}")
        tool_text("browser_click", {"tab_id": tab, "selector": "#prompt-btn"}, timeout=15)
        result = poll(lambda: js_text(tab, "prompt-result"), lambda r: "none" not in r)
        expect("hello" in result, f"prompt() did not receive armed text: {result!r}")


@test
def upload_file():
    """Armed file-chooser answers with the temp file; change handler sees its name."""
    fd, path = tempfile.mkstemp(prefix="ow_upload_", suffix=".txt")
    os.write(fd, b"octoweb upload fixture")
    os.close(fd)
    try:
        with fixture_tab("upload.html") as tab:
            text = tool_text("browser_upload_file", {"paths": [path]})
            expect(text.lower().startswith("armed"),
                   f"upload_file should report Armed: {text!r}")
            tool_text("browser_click", {"tab_id": tab, "selector": "#file-input"}, timeout=15)
            name = os.path.basename(path)
            status = poll(lambda: js_text(tab, "file-status"), lambda s: name in s)
            expect(name in status, f"file input did not receive {name!r}: {status!r}")
    finally:
        os.unlink(path)


@test
def container_scroll():
    """browser_scroll with a selector scrolls the nearest scrollable container."""
    with fixture_tab("scroll_container.html") as tab:
        tool_text("browser_scroll",
                  {"tab_id": tab, "selector": "#inner-top", "direction": "down", "pixels": 300})
        result = exec_js(tab, "document.getElementById('box').scrollTop > 0")
        expect("true" in result.lower(),
               f"#box.scrollTop did not increase after container scroll: {result!r}")


@test
def wait_selector_ready():
    """browser_wait on a selector that appears at 750ms returns 'ready'."""
    with fixture_tab("late.html") as tab:
        text = tool_text("browser_wait", {"tab_id": tab, "event": "#late", "timeout_ms": 5000}, timeout=15)
        expect("ready" in text.lower(), f"wait for #late should be 'ready', got: {text!r}")


@test
def wait_selector_timeout():
    """browser_wait on a never-appearing selector times out AND actually waits."""
    with fixture_tab("basic.html") as tab:
        start = time.monotonic()
        text = tool_text("browser_wait", {"tab_id": tab, "event": "#never", "timeout_ms": 1500}, timeout=15)
        elapsed = time.monotonic() - start
        expect("timeout" in text.lower(), f"wait for #never should time out, got: {text!r}")
        expect(elapsed >= 1.4,
               f"LOUD FLAG: browser_wait returned in {elapsed:.2f}s (< 1.4s) -- "
               "the Promise-based wait is NOT actually awaiting, it resolved instantly")


@test
def snapshot_scroll_header():
    """Snapshot header line mentions scroll position / page height."""
    with fixture_tab("basic.html") as tab:
        snap = snapshot(tab)
        lines = [ln for ln in snap.splitlines() if ln.strip()]
        expect(lines, "snapshot is empty")
        # Line 0 is the <untrusted> fence; the meta line is the one naming the page.
        meta = next((ln for ln in lines if ln.lower().startswith("page: ")), "")
        expect(re.search(r"viewport \d+-\d+ of \d+px", meta.lower()) is not None,
               f"snapshot header lacks scroll/height info: {meta!r}")


@test
def screenshot_image():
    """browser_screenshot returns image content."""
    with fixture_tab("basic.html") as tab:
        is_err, content, text = call_tool("browser_screenshot", {"tab_id": tab}, timeout=20)
        expect(not is_err, f"browser_screenshot errored: {text!r}")
        images = [c for c in content if c.get("type") == "image"]
        expect(images, f"no image content in screenshot result: {content}")
        expect(images[0].get("data"), "screenshot image item has empty data")


@test
def page_content_and_info():
    """get_page_content includes fixture text; get_page_info identifies the page."""
    with fixture_tab("basic.html") as tab:
        content = tool_text("browser_get_page_content", {"tab_id": tab})
        expect("Basic Fixture Heading" in content,
               f"page content missing fixture heading: {content[:300]!r}")
        info = tool_text("browser_get_page_info", {"tab_id": tab})
        expect("OW Fixture: Basic" in info or "basic.html" in info,
               f"page info lacks fixture title/url: {info[:300]!r}")


@test
def trusted_click():
    """Native click: the page sees isTrusted=true and the effect line names the new text."""
    with fixture_tab("trusted_click.html") as tab:
        ref, _ = find_ref(snapshot(tab), "Request access")
        text = tool_text("browser_click", {"tab_id": tab, "selector": ref}, timeout=15)
        status = js_text(tab, "status").strip('"')
        expect(status == "trusted-click", f"click was not trusted: status={status!r}")
        activation = js_text(tab, "activation").strip('"')
        expect(activation in ("true", "unsupported"), f"no user activation after click: {activation!r}")
        expect("Request sent to owner" in text,
               f"effect summary should quote the new status text, got: {text!r}")
        snap = snapshot(tab)
        expect("Request sent to owner" in snap, f"snapshot header lacks role=status text: {snap[:500]!r}")


@test
def hover_fires_menu():
    """Hover fires the element's mouseover listener so JS hover menus open.
    On a background (hidden) tab WebKit can't paint CSS :hover, so hover is
    delivered via synthetic enter events there (trusted native move upgrades a
    visible tab); either way the JS listener must run and reveal the menu."""
    # Unique query defeats WKWebView's in-session memory cache (trusted_click.html
    # is shared with another test; no-store alone doesn't stop the memory cache).
    info = navigate(fixture_url("trusted_click.html") + "?hovtest=1")
    tab = info["tab_id"]
    try:
        tool_text("browser_hover", {"tab_id": tab, "selector": "#hoverzone"}, timeout=15)
        hovered = js_text(tab, "hovered").strip('"')
        expect(hovered in ("trusted-hover", "synthetic-hover"),
               f"hover listener did not fire: {hovered!r}")
        menu = exec_js(tab, "document.getElementById('menu-status') ? document.getElementById('menu-status').textContent : 'n/a'")
        # menu revealed via JS (see fixture) rather than CSS :hover, so it works on background tabs.
        expect("shown" in menu, f"hover menu did not open: {menu!r}")
    finally:
        close_tab(tab)


@test
def trusted_key_events():
    """Native keys: trusted keydown, Space activates a button, characters insert."""
    with fixture_tab("trusted_key.html") as tab:
        tool_text("browser_press_key", {"tab_id": tab, "key": "a", "selector": "#field"}, timeout=15)
        keys = js_text(tab, "keys").strip('"')
        expect(keys == "trusted:a", f"keydown not trusted / wrong key: {keys!r}")
        value = exec_js(tab, "document.getElementById('field').value")
        expect("a" in value, f"native key press did not insert the character: {value!r}")
        tool_text("browser_press_key", {"tab_id": tab, "key": "Space", "selector": "#btn"}, timeout=15)
        status = js_text(tab, "btnstatus").strip('"')
        expect(status == "activated-trusted", f"Space did not activate the button natively: {status!r}")


@test
def press_key_rejects_unknown():
    """A typo'd key name errors instead of pressing garbage."""
    with fixture_tab("basic.html") as tab:
        is_err, _, text = call_tool("browser_press_key", {"tab_id": tab, "key": " \""})
        expect(is_err, f"bogus key must error, got: {text[:200]!r}")
        expect("Unknown key" in text, f"error should say Unknown key: {text[:200]!r}")


@test
def snapshot_labels_and_state():
    """Snapshot shows <label> text for controls, headings, alerts and hidden-control count."""
    with fixture_tab("labels_state.html") as tab:
        snap = snapshot(tab)
        for needle in ('radio "Viewer"', 'radio "Commenter"', 'radio "Editor"'):
            expect(needle in snap, f"snapshot lacks labelled {needle}: {snap[:800]!r}")
        expect('"Editor"' in snap and "checked" in snap.split('"Editor"')[1].split("\n")[0],
               f"checked state missing on Editor radio: {snap[:800]!r}")
        expect('textbox "Message (optional)"' in snap, f"wrapping <label> not used: {snap[:800]!r}")
        expect('textbox "Search query"' in snap, f"aria-labelledby not resolved: {snap[:800]!r}")
        expect('h1 "You need access"' in snap, f"h1 missing from state header: {snap[:800]!r}")
        expect('alert: "Access denied for this account"' in snap, f"role=alert missing: {snap[:800]!r}")
        expect("present-but-hidden controls" in snap, f"hidden toolbar count missing: {snap[:800]!r}")
        expect("Download" not in snap.split("elements")[1], "hidden buttons must not get @refs")


@test
def effect_feedback_variants():
    """Action results describe the observable effect: text, route, dialog, requests, or silence."""
    with fixture_tab("effects.html") as tab:
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#toast"}, timeout=15)
        expect("Saved successfully" in text, f"toast text not in effect line: {text!r}")
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#route"}, timeout=15)
        expect("#/settings" in text, f"SPA route change not in effect line: {text!r}")
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#fetch"}, timeout=15)
        expect("api/data?src=effect" in text, f"fetch not in effect line: {text!r}")
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#dialog"}, timeout=15)
        expect("dialog opened" in text and "Confirm the thing" in text, f"dialog not in effect line: {text!r}")
        tool_text("browser_click", {"tab_id": tab, "selector": "#dlg-close"}, timeout=15)
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#silent"}, timeout=15)
        expect("no observable change" in text, f"silent click must say so: {text!r}")


@test
def click_reports_navigation():
    """A click that navigates is reported as success naming the new URL, not as a dropped call."""
    with fixture_tab("effects.html") as tab:
        start = time.monotonic()
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#navigate"}, timeout=20)
        elapsed = time.monotonic() - start
        expect("navigated to" in text and "basic.html" in text, f"navigation not reported: {text!r}")
        expect(elapsed < 6, f"navigation report took {elapsed:.1f}s — should not wait for the watchdog")


DOWNLOADS_DIR = os.path.expanduser("~/Downloads")


@contextmanager
def download_target(prefix):
    """Unique filename in ~/Downloads; removed afterwards."""
    import uuid
    name = f"{prefix}-{uuid.uuid4().hex[:8]}.txt"
    path = os.path.join(DOWNLOADS_DIR, name)
    try:
        yield name, path
    finally:
        try:
            os.remove(path)
        except OSError:
            pass


@test
def attachment_click_downloads():
    """Clicking a link to an attachment saves the file and the effect line says so; the tab stays put."""
    with fixture_tab("effects.html") as tab, download_target("octoweb-e2e-click") as (name, path):
        exec_js(tab, f"document.getElementById('download').href = '/api/attachment?name={name}'")
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#download"}, timeout=20)
        expect("download started" in text and name in text,
               f"effect line should report the download: {text!r}")
        found = poll(lambda: os.path.exists(path), lambda x: x, attempts=20, delay=0.25)
        expect(found, f"attachment was not saved to {path}")
        info = tool_text("browser_get_page_info", {"tab_id": tab})
        expect("effects.html" in info, f"tab must keep its page after a download, got: {info!r}")


@test
def attachment_navigate_downloads():
    """browser_navigate to an attachment URL returns promptly with a download note instead of hanging."""
    with fixture_tab("basic.html") as tab, download_target("octoweb-e2e-nav") as (name, path):
        start = time.monotonic()
        text = tool_text("browser_navigate",
                         {"tab_id": tab, "url": fixture_url(f"api/attachment?name={name}")},
                         timeout=NAVIGATE_TIMEOUT)
        elapsed = time.monotonic() - start
        expect("download" in text.lower() and name in text, f"navigate should report the download: {text!r}")
        expect(elapsed < 10, f"navigate-to-attachment took {elapsed:.1f}s (stale-pending fallback?)")
        found = poll(lambda: os.path.exists(path), lambda x: x, attempts=20, delay=0.25)
        expect(found, f"attachment was not saved to {path}")


@test
def network_sees_beacons_and_images():
    """Resource-timing capture: sendBeacon and <img> loads show up in browser_network_requests."""
    with fixture_tab("effects.html") as tab:
        tool_text("browser_click", {"tab_id": tab, "selector": "#beacon"}, timeout=15)

        def entries():
            return parse_json_text(
                tool_text("browser_network_requests", {"tab_id": tab, "filter": "api/data"}),
                "browser_network_requests",
            )

        got = poll(entries, lambda es: any("src=beacon" in e["url"] for e in es) and any("src=img" in e["url"] for e in es))
        expect(any("src=beacon" in e["url"] for e in got), f"beacon not captured: {got}")
        expect(any("src=img" in e["url"] for e in got), f"image load not captured: {got}")


@test
def execute_js_errors_and_timeouts():
    """JS exceptions carry their message; a slow script times out as 'still running', not 'navigated'."""
    with fixture_tab("basic.html") as tab:
        is_err, _, text = call_tool("browser_execute_js",
                                    {"tab_id": tab, "script": "(() => { throw new TypeError('boom from page'); })()"})
        expect(is_err and "boom from page" in text, f"exception message not surfaced: {text!r}")
        start = time.monotonic()
        is_err, _, text = call_tool("browser_execute_js",
                                    {"tab_id": tab, "script": "new Promise(r => setTimeout(r, 3000))", "timeout_ms": 1000},
                                    timeout=15)
        elapsed = time.monotonic() - start
        expect(is_err, f"slow script should time out, got: {text!r}")
        expect("did not finish" in text and "did NOT navigate" in text,
               f"timeout must be reported as a timeout, not a navigation: {text!r}")
        expect(0.9 <= elapsed < 3.0, f"timeout_ms not honoured: {elapsed:.1f}s")


@test
def wait_ready_on_static_page():
    """browser_wait event=ready resolves 'ready' on a static page (regression: phantom 'navigated')."""
    with fixture_tab("basic.html") as tab:
        text = tool_text("browser_wait", {"tab_id": tab, "event": "ready"}, timeout=15)
        expect(text.strip() in ("ready", "live"), f"wait ready on static page returned: {text!r}")


@test
def get_tabs_paging():
    """browser_get_tabs honours limit and query and always reports total."""
    with fixture_tab("basic.html") as tab:
        page = parse_json_text(tool_text("browser_get_tabs", {"limit": 1}), "browser_get_tabs")
        expect(page["total"] >= 1 and len(page["tabs"]) == 1, f"limit=1 not honoured: {page}")
        page = parse_json_text(tool_text("browser_get_tabs", {"query": "basic.html", "limit": 200}), "browser_get_tabs")
        ids = [t["id"] for t in page["tabs"]]
        expect(tab in ids, f"query did not match the fixture tab: {page}")
        expect(all("basic.html" in t["url"] for t in page["tabs"]), f"query returned non-matching tabs: {page}")
        expect(not any(t.get("is_playing_audio") is False for t in page["tabs"]),
               "false flags should be omitted from tab entries")


@test
def expect_met_and_unmet():
    """`expect` waits for the outcome and reports ✓/✗ in one call — no separate wait."""
    with fixture_tab("spa.html") as tab:
        tool_text("browser_type", {"tab_id": tab, "selector": "#name", "text": "Ada"})
        # Async success text appears ~600ms after click; expect should wait for it.
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#save", "expect": "text:Saved Ada"}, timeout=15)
        expect("✓ expected text:Saved Ada — met" in text, f"expect-met not reported: {text!r}")
        # An outcome that never happens is reported ✗, not as success.
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#save", "expect": "text:Nope404"}, timeout=15)
        expect("✗ expected text:Nope404 — NOT met" in text, f"expect-unmet not reported: {text!r}")


@test
def expect_url_on_route():
    """expect url:<frag> confirms an SPA route change from a click."""
    with fixture_tab("spa.html") as tab:
        text = tool_text("browser_click", {"tab_id": tab, "selector": "#to-settings", "expect": "url:#/settings"}, timeout=15)
        expect("✓ expected url:#/settings — met" in text, f"route expect not met: {text!r}")


@test
def wait_for_text():
    """browser_wait event=text:<phrase> resolves when the text appears."""
    with fixture_tab("spa.html") as tab:
        tool_text("browser_type", {"tab_id": tab, "selector": "#name", "text": "Bo"})
        tool_text("browser_click", {"tab_id": tab, "selector": "#save"})
        text = tool_text("browser_wait", {"tab_id": tab, "event": "text:Saved Bo", "timeout_ms": 4000}, timeout=15)
        expect("ready" in text.lower(), f"wait for text should be ready: {text!r}")
        gone = tool_text("browser_wait", {"tab_id": tab, "event": "text_gone:Saving", "timeout_ms": 4000}, timeout=15)
        expect("ready" in gone.lower(), f"wait text_gone should be ready: {gone!r}")


@test
def fill_form_batch():
    """browser_fill_form fills fields and submits with an expectation in one call."""
    with fixture_tab("spa.html") as tab:
        text = tool_text("browser_fill_form", {
            "tab_id": tab,
            "fields": [{"selector": "#name", "value": "Cy"}, {"selector": "#email", "value": "cy@x.test"}],
            "submit": "#save",
            "expect": "text:Saved Cy",
        }, timeout=20)
        expect("Filled 2/2" in text, f"fill_form field count wrong: {text!r}")
        expect("✓ expected text:Saved Cy — met" in text, f"fill_form submit/expect not verified: {text!r}")
        vals = exec_js(tab, "document.getElementById('name').value + '|' + document.getElementById('email').value")
        expect("Cy|cy@x.test" in vals, f"fields not actually filled: {vals!r}")


@test
def snapshot_stable_refs_and_diff():
    """Refs stay stable across snapshots; diff returns only what changed."""
    with fixture_tab("spa.html") as tab:
        snap1 = snapshot(tab)
        save_ref, _ = find_ref(snap1, "Save")
        # Re-snapshot: the same element keeps its @ref.
        snap2 = snapshot(tab)
        save_ref2, _ = find_ref(snap2, "Save")
        expect(save_ref == save_ref2, f"ref not stable: {save_ref} vs {save_ref2}")
        # Trigger a state change, then diff should surface the new status line, not the whole page.
        tool_text("browser_type", {"tab_id": tab, "selector": "#name", "text": "Di"})
        tool_text("browser_click", {"tab_id": tab, "selector": "#save", "expect": "text:Saved Di"}, timeout=15)
        diff = tool_text("browser_snapshot", {"tab_id": tab, "diff": True}, timeout=15)
        expect("Saved Di" in diff, f"diff should include the new status text: {diff[:400]!r}")
        # The diff must be much smaller than the full snapshot.
        expect(len(diff) < len(snap1), f"diff not smaller than full snapshot ({len(diff)} vs {len(snap1)})")


@test
def snapshot_within_scope():
    """within scopes the scan to one container."""
    with fixture_tab("spa.html") as tab:
        scoped = tool_text("browser_snapshot", {"tab_id": tab, "within": "#save-form"}, timeout=15)
        expect("within: #save-form" in scoped, f"within not reflected in header: {scoped[:200]!r}")
        expect("Save" in scoped, "scoped snapshot missing the form's button")
        expect("Settings" not in scoped, f"scoped snapshot leaked outside the container: {scoped[:400]!r}")


@test
def network_response_body():
    """include_body surfaces the JSON payload behind a fetch."""
    with fixture_tab("network.html") as tab:
        tool_text("browser_wait", {"tab_id": tab, "event": "#done", "timeout_ms": 5000}, timeout=15)
        entries = parse_json_text(
            tool_text("browser_network_requests", {"tab_id": tab, "filter": "api/data", "include_body": True}),
            "browser_network_requests(include_body)",
        )
        withbody = [e for e in entries if e.get("body")]
        expect(withbody, f"no response body captured: {entries}")
        expect(any('"value": 42' in e["body"] or '"value":42' in e["body"] for e in withbody),
               f"captured body missing payload: {[e.get('body') for e in withbody]}")
        # Default (no include_body) must NOT carry bodies.
        plain = parse_json_text(
            tool_text("browser_network_requests", {"tab_id": tab, "filter": "api/data"}),
            "browser_network_requests",
        )
        expect(all("body" not in e for e in plain), f"body leaked without include_body: {plain}")


@test
def dismiss_overlay_rejects():
    """browser_dismiss_overlay clears a consent wall via Reject, unblocking the page."""
    with fixture_tab("cookie_banner.html") as tab:
        # The CTA is covered by the overlay → a plain click reports occluded.
        is_err, _, text = call_tool("browser_click", {"tab_id": tab, "selector": "#cta"}, timeout=15)
        expect(is_err and "covered" in text.lower(), f"CTA should be occluded by the banner: {text[:200]!r}")
        # Dismiss chooses Reject (not Accept) and the banner leaves.
        out = tool_text("browser_dismiss_overlay", {"tab_id": tab}, timeout=15)
        expect("reject" in out.lower(), f"dismiss should use the reject control: {out!r}")
        gone = exec_js(tab, "!document.getElementById('cookie-consent')")
        expect("true" in gone.lower(), f"overlay not removed: {gone!r}")
        # Now the underlying CTA is clickable.
        tool_text("browser_click", {"tab_id": tab, "selector": "#cta", "expect": "text:bought"}, timeout=15)
        status = js_text(tab, "cta-status").strip('"')
        expect(status == "bought", f"CTA not reachable after dismiss: {status!r}")


@test
def dismiss_overlay_never_accepts():
    """When only an accept/agree control exists, dismiss reports it and does NOT click."""
    with fixture_tab("cookie_accept_only.html") as tab:
        out = tool_text("browser_dismiss_overlay", {"tab_id": tab}, timeout=15)
        expect("accept" in out.lower() and "not clicking" in out.lower(),
               f"should refuse to auto-accept: {out!r}")
        still = exec_js(tab, "!!document.getElementById('consent')")
        expect("true" in still.lower(), f"accept-only overlay must be left in place: {still!r}")


# ──────────────────────────────────────────────────────────────────────
# Runner
# ──────────────────────────────────────────────────────────────────────

def main():
    global MCP_URL
    parser = argparse.ArgumentParser(description="octoweb MCP end-to-end tests")
    parser.add_argument("--filter", default="", help="run only tests whose name contains this substring")
    parser.add_argument("--mcp-url", default=MCP_URL, help=f"MCP endpoint (default {MCP_URL})")
    parser.add_argument("--list", action="store_true", help="list test names and exit")
    args = parser.parse_args()
    MCP_URL = args.mcp_url

    if args.list:
        for fn in TESTS:
            print(fn.__name__)
        return 0

    selected = [fn for fn in TESTS if args.filter in fn.__name__]
    if not selected:
        print(f"no tests match filter {args.filter!r}")
        return 1

    if not os.path.isdir(FIXTURES_DIR):
        print(f"fixtures directory missing: {FIXTURES_DIR}")
        return 1

    try:
        httpd = start_fixture_server()
    except OSError as exc:
        print(f"cannot start fixture server on {FIXTURE_HOST}:{FIXTURE_PORT}: {exc}")
        return 1

    try:
        try:
            init = mcp_initialize()
            server_info = init.get("serverInfo", {})
            print(f"MCP server: {server_info.get('name', '?')} {server_info.get('version', '')} at {MCP_URL}")
        except MCPError as exc:
            print(f"MCP initialize failed: {exc}")
            print("Is the octoweb browser running with the MCP server enabled?")
            return 1

        failed = []
        call_counts = {}
        for fn in selected:
            start = time.monotonic()
            CALL_COUNTER["n"] = 0
            try:
                fn()
                call_counts[fn.__name__] = CALL_COUNTER["n"]
                print(f"PASS {fn.__name__} ({time.monotonic() - start:.2f}s, {CALL_COUNTER['n']} calls)")
            except TestFailure as exc:
                failed.append(fn.__name__)
                print(f"FAIL {fn.__name__} ({time.monotonic() - start:.2f}s)")
                print(f"     {exc}")
            except MCPError as exc:
                failed.append(fn.__name__)
                print(f"FAIL {fn.__name__} ({time.monotonic() - start:.2f}s)")
                print(f"     transport error: {exc}")
            except Exception:
                failed.append(fn.__name__)
                print(f"FAIL {fn.__name__} ({time.monotonic() - start:.2f}s)")
                print("     unexpected exception:")
                for line in traceback.format_exc().splitlines():
                    print(f"     {line}")

        total = len(selected)
        print()
        if call_counts:
            tot = sum(call_counts.values())
            worst = sorted(call_counts.items(), key=lambda kv: -kv[1])[:5]
            print(f"calls-to-success: {tot} MCP tool calls across {len(call_counts)} passing scenarios "
                  f"(avg {tot / len(call_counts):.1f}/scenario)")
            print("  heaviest: " + ", ".join(f"{n}={c}" for n, c in worst))
        print(f"{total - len(failed)}/{total} tests passed")
        if failed:
            print("failed: " + ", ".join(failed))
            return 1
        return 0
    finally:
        httpd.shutdown()


if __name__ == "__main__":
    sys.exit(main())
