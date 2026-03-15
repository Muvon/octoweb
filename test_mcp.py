#!/usr/bin/env python3
"""
MCP Server Test Script for Octoweb Browser

Tests the MCP server running on http://localhost:3434/mcp.

MCP Protocol Handshake:
1. Client sends 'initialize' request → server responds with capabilities
2. Client sends 'notifications/initialized' notification (no response)
3. Client can now call tools/list, tools/call, etc.

Usage:
    python3 test_mcp.py
    
The octoweb browser must be running with MCP server enabled.
"""

import requests
import json
import time
import sys

MCP_URL = "http://127.0.0.1:3434/mcp"
TIMEOUT = 10.0

def log(msg):
    """Print with timestamp."""
    print(f"[{time.strftime('%H:%M:%S')}] {msg}")

def send_request(method, params=None, request_id=None):
    """Send a JSON-RPC request via HTTP POST."""
    payload = {
        "jsonrpc": "2.0",
        "method": method,
    }
    if request_id is not None:
        payload["id"] = request_id
    if params is not None:
        payload["params"] = params
    
    log(f"SENT: {json.dumps(payload)}")
    
    response = requests.post(
        MCP_URL,
        json=payload,
        headers={"Content-Type": "application/json"},
        timeout=TIMEOUT
    )
    
    if response.status_code != 200:
        log(f"HTTP ERROR: {response.status_code}")
        return None
    
    result = response.json()
    log(f"RECV: {json.dumps(result)}")
    return result

def send_notification(method, params=None):
    """Send a JSON-RPC notification (no id, no response expected)."""
    payload = {
        "jsonrpc": "2.0",
        "method": method,
    }
    if params is not None:
        payload["params"] = params
    
    log(f"SENT (notification): {json.dumps(payload)}")
    
    response = requests.post(
        MCP_URL,
        json=payload,
        headers={"Content-Type": "application/json"},
        timeout=TIMEOUT
    )
    
    # Notifications don't get a response, but HTTP still returns
    log(f"HTTP status: {response.status_code}")

def test_mcp_server():
    """Run full MCP protocol test suite."""
    log(f"Testing MCP server at {MCP_URL}...")
    
    all_passed = True
    
    # ═══════════════════════════════════════════════════════════════
    # TEST 1: Initialize handshake
    # ═══════════════════════════════════════════════════════════════
    log("\n" + "="*60)
    log("TEST 1: Initialize handshake")
    log("="*60)
    
    init_response = send_request(
        "initialize",
        params={
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        },
        request_id=1
    )
    
    if not init_response:
        log("FAILED: No initialize response received")
        return False
    
    if "result" not in init_response:
        log(f"FAILED: Expected result in initialize response, got: {init_response}")
        all_passed = False
    else:
        result = init_response["result"]
        log(f"Server: {result.get('serverInfo', {})}")
        log(f"Protocol: {result.get('protocolVersion', 'unknown')}")
        log(f"Capabilities: {result.get('capabilities', {})}")
        log("PASSED: Initialize handshake")
    
    # Step 2: Send initialized notification (REQUIRED before any other requests)
    send_notification("notifications/initialized")
    time.sleep(0.2)  # Give server time to process
    
    # ═══════════════════════════════════════════════════════════════
    # TEST 2: List tools
    # ═══════════════════════════════════════════════════════════════
    log("\n" + "="*60)
    log("TEST 2: List available tools")
    log("="*60)
    
    tools_response = send_request("tools/list", request_id=2)
    
    if not tools_response:
        log("FAILED: No tools/list response received")
        return False
    
    if "result" not in tools_response:
        log(f"FAILED: Expected result in tools/list response, got: {tools_response}")
        all_passed = False
    else:
        tools = tools_response["result"].get("tools", [])
        log(f"Available tools ({len(tools)}):")
        for tool in tools:
            log(f"  - {tool['name']}: {tool.get('description', 'no description')[:60]}...")
        log("PASSED: Tools listed successfully")
    
    # ═══════════════════════════════════════════════════════════════
    # TEST 3: Call browser_get_tabs
    # ═══════════════════════════════════════════════════════════════
    log("\n" + "="*60)
    log("TEST 3: Call browser_get_tabs")
    log("="*60)
    
    tabs_response = send_request(
        "tools/call",
        params={
            "name": "browser_get_tabs",
            "arguments": {}
        },
        request_id=3
    )
    
    if not tabs_response:
        log("FAILED: No browser_get_tabs response received")
        all_passed = False
    elif "result" not in tabs_response:
        log(f"FAILED: Expected result in browser_get_tabs response, got: {tabs_response}")
        all_passed = False
    else:
        content = tabs_response["result"].get("content", [])
        for item in content:
            if item.get("type") == "text":
                log(f"Tabs: {item.get('text', 'no data')}")
        log("PASSED: browser_get_tabs executed")
    
    # ═══════════════════════════════════════════════════════════════
    # TEST 4: Call browser_get_page_info
    # ═══════════════════════════════════════════════════════════════
    log("\n" + "="*60)
    log("TEST 4: Call browser_get_page_info")
    log("="*60)
    
    page_info_response = send_request(
        "tools/call",
        params={
            "name": "browser_get_page_info",
            "arguments": {}  # No tab_id = use active tab
        },
        request_id=4
    )
    
    if not page_info_response:
        log("FAILED: No browser_get_page_info response received")
        all_passed = False
    elif "result" not in page_info_response:
        log(f"FAILED: Expected result in browser_get_page_info response, got: {page_info_response}")
        all_passed = False
    else:
        content = page_info_response["result"].get("content", [])
        for item in content:
            if item.get("type") == "text":
                log(f"Page info: {item.get('text', 'no data')}")
        log("PASSED: browser_get_page_info executed")
    
    # ═══════════════════════════════════════════════════════════════
    # TEST 5: Call browser_navigate (to example.com)
    # ═══════════════════════════════════════════════════════════════
    log("\n" + "="*60)
    log("TEST 5: Call browser_navigate to example.com")
    log("="*60)
    
    nav_response = send_request(
        "tools/call",
        params={
            "name": "browser_navigate",
            "arguments": {
                "url": "https://example.com"
            }
        },
        request_id=5
    )
    
    if not nav_response:
        log("FAILED: No browser_navigate response received")
        all_passed = False
    elif "result" not in nav_response:
        log(f"FAILED: Expected result in browser_navigate response, got: {nav_response}")
        all_passed = False
    else:
        content = nav_response["result"].get("content", [])
        for item in content:
            if item.get("type") == "text":
                log(f"Result: {item.get('text', 'no data')}")
        log("PASSED: browser_navigate executed")
    
    # Wait a moment for navigation to complete
    time.sleep(1.0)
    
    # ═══════════════════════════════════════════════════════════════
    # TEST 6: Execute JavaScript
    # ═══════════════════════════════════════════════════════════════
    log("\n" + "="*60)
    log("TEST 6: Call browser_execute_js")
    log("="*60)
    
    js_response = send_request(
        "tools/call",
        params={
            "name": "browser_execute_js",
            "arguments": {
                "script": "document.title"
            }
        },
        request_id=6
    )
    
    if not js_response:
        log("FAILED: No browser_execute_js response received")
        all_passed = False
    elif "result" not in js_response:
        log(f"FAILED: Expected result in browser_execute_js response, got: {js_response}")
        all_passed = False
    else:
        content = js_response["result"].get("content", [])
        for item in content:
            if item.get("type") == "text":
                log(f"JS result: {item.get('text', 'no data')}")
        log("PASSED: browser_execute_js executed")
    
    # ═══════════════════════════════════════════════════════════════
    # SUMMARY
    # ═══════════════════════════════════════════════════════════════
    log("\n" + "="*60)
    if all_passed:
        log("ALL TESTS PASSED ✓")
    else:
        log("SOME TESTS FAILED ✗")
    log("="*60)
    
    return all_passed

if __name__ == "__main__":
    success = test_mcp_server()
    sys.exit(0 if success else 1)