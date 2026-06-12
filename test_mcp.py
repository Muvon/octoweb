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
    result = rpc("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "octoweb-e2e", "version": "1.0.0"},
    })
    rpc("notifications/initialized", notify=True)
    return result


def call_tool(name, arguments=None, timeout=DEFAULT_TIMEOUT):
    """Returns (is_error, content_list, joined_text)."""
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


def get_tabs():
    return parse_json_text(tool_text("browser_get_tabs"), "browser_get_tabs")


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
        expect(entry.get("is_active") is False,
               f"new background tab must have is_active false, got {entry}")
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
        header = lines[0].lower()
        expect("scroll" in header or "height" in header,
               f"snapshot header lacks scroll/height info: {lines[0]!r}")


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
        for fn in selected:
            start = time.monotonic()
            try:
                fn()
                print(f"PASS {fn.__name__} ({time.monotonic() - start:.2f}s)")
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
        print(f"{total - len(failed)}/{total} tests passed")
        if failed:
            print("failed: " + ", ".join(failed))
            return 1
        return 0
    finally:
        httpd.shutdown()


if __name__ == "__main__":
    sys.exit(main())
