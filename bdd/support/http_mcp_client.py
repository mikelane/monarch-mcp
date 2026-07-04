"""
MCP streamable-HTTP client helper.

Launches the production Monarch MCP server binary in --http mode (issue #88)
and communicates via the MCP JSON-RPC protocol over HTTP + SSE, mirroring
McpClient's public interface (start/stop/call_tool) for the stdio transport.

Binary path: env MONARCH_MCP_BIN, defaulting to ../target/debug/monarch-mcp
Monarch base URL: env MONARCH_BASE (passed to the server process)
Goals file: env MONARCH_GOALS_FILE (passed to the server process)

Wire format (confirmed against the real server): every response to a POST
/mcp request that expects a body is SSE-framed (`Content-Type:
text/event-stream`) — a blank priming `data:` event followed by a
`data: {json-rpc payload}` line. The first response also carries an
`Mcp-Session-Id` header that must be echoed on every subsequent request.

Protocol flow:
  1. POST initialize, capture Mcp-Session-Id from the response headers
  2. POST notifications/initialized (no response body expected)
  3. POST tools/list / tools/call, parsing the JSON-RPC payload out of the
     SSE body
  4. on teardown, terminate the subprocess
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

import requests


_DEFAULT_BIN = str(
    Path(__file__).parent.parent.parent / "target" / "debug" / "monarch-mcp"
)

# Bound every HTTP call so a wedged server subprocess can't hang the BDD suite
# indefinitely (requests has no default timeout). Generous — the loopback mock
# responds in milliseconds; this only guards against a hung/crashed process.
_REQUEST_TIMEOUT_S = 30.0


def _free_port() -> int:
    """Finds an unused TCP port on loopback, matching mock_monarch's approach."""
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _parse_sse_json_rpc_payload(body: str) -> dict:
    """Extracts the JSON-RPC payload from an SSE-framed response body.

    The wire format is a blank priming `data:` event followed by a
    `data: {json}` line. This skips blank data lines and returns the first
    one that parses as JSON.
    """
    for line in body.splitlines():
        if not line.startswith("data:"):
            continue
        payload = line[len("data:") :].strip()
        if not payload:
            continue
        return json.loads(payload)
    raise RuntimeError(f"No JSON-RPC payload found in SSE body: {body!r}")


class HttpMcpClient:
    """Thin MCP streamable-HTTP client that drives the monarch-mcp --http binary."""

    def __init__(self, monarch_base: str, goals_file: str | None = None) -> None:
        self._bin = os.environ.get("MONARCH_MCP_BIN", _DEFAULT_BIN)
        self._monarch_base = monarch_base
        self._goals_file = goals_file
        self._process: subprocess.Popen | None = None
        self._next_id = 1
        self._session_id: str | None = None
        self._mcp_url: str | None = None
        self._session_tmpdir: str | None = None

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def start(self) -> None:
        """Launch the MCP server binary in --http mode. Raises if the binary is missing."""
        if not Path(self._bin).exists():
            raise FileNotFoundError(
                f"MCP binary not found: {self._bin!r}. "
                "Build it with `cargo build` or set MONARCH_MCP_BIN."
            )

        port = _free_port()
        self._mcp_url = f"http://127.0.0.1:{port}/mcp"

        env = os.environ.copy()
        env["MONARCH_BASE"] = self._monarch_base
        env.setdefault("MONARCH_TOKEN", "mock-test-token-abc123")
        if self._goals_file:
            env["MONARCH_GOALS_FILE"] = self._goals_file
        env["MONARCH_HTTP_ADDR"] = f"127.0.0.1:{port}"

        # Redirect session storage to a temp dir, same as McpClient (Phase 5 / B5).
        self._session_tmpdir = tempfile.mkdtemp(prefix="monarch_bdd_http_session_")
        env["MONARCH_CONFIG_DIR"] = self._session_tmpdir

        self._process = subprocess.Popen(
            [self._bin, "--http"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=True,
        )
        self._wait_until_listening(port)
        self._initialize()

    def stop(self) -> None:
        """Terminate the MCP server process and clean up session temp dir."""
        if self._process is not None:
            self._process.terminate()
            try:
                self._process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self._process.kill()
            self._process = None
        if self._session_tmpdir:
            shutil.rmtree(self._session_tmpdir, ignore_errors=True)
            self._session_tmpdir = None

    @property
    def mcp_url(self) -> str:
        """The /mcp endpoint URL this client is talking to."""
        assert self._mcp_url is not None, "HttpMcpClient.start() has not been called"
        return self._mcp_url

    # ------------------------------------------------------------------
    # MCP protocol
    # ------------------------------------------------------------------

    def list_tools(self) -> list[dict]:
        """Lists all tools the server advertises via tools/list."""
        request = {
            "jsonrpc": "2.0",
            "id": self._next_id,
            "method": "tools/list",
            "params": {},
        }
        self._next_id += 1
        response = self._send(request)
        if "error" in response:
            raise RuntimeError(f"MCP tools/list error: {response['error']}")
        return response.get("result", {}).get("tools", [])

    def call_tool(self, tool_name: str, arguments: dict[str, Any] | None = None) -> Any:
        """Call a tool and return its result content.

        Raises RuntimeError if the server returns an error response.
        """
        request = {
            "jsonrpc": "2.0",
            "id": self._next_id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments or {},
            },
        }
        self._next_id += 1
        response = self._send(request)
        if "error" in response:
            raise RuntimeError(f"MCP tool error for {tool_name!r}: {response['error']}")
        result = response.get("result", {})
        content = result.get("content", [])
        if content and content[0].get("type") == "text":
            raw = content[0]["text"]
            try:
                return json.loads(raw)
            except json.JSONDecodeError:
                return raw
        return result

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _wait_until_listening(self, port: int) -> None:
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    return
            except OSError:
                time.sleep(0.05)
        raise RuntimeError(f"monarch-mcp --http did not start listening on port {port}")

    def _initialize(self) -> None:
        """Send MCP initialize + notifications/initialized, capturing the session id."""
        init_request = {
            "jsonrpc": "2.0",
            "id": self._next_id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "bdd-http-test-client", "version": "0.1.0"},
            },
        }
        self._next_id += 1
        self._send(init_request)

        notification = {"jsonrpc": "2.0", "method": "notifications/initialized"}
        self._post(notification)

    def _send(self, request: dict) -> dict:
        """POST a JSON-RPC request and parse the JSON-RPC payload from the response."""
        response = self._post(request)
        return _parse_sse_json_rpc_payload(response.text)

    def _post(self, payload: dict) -> requests.Response:
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        }
        if self._session_id:
            headers["Mcp-Session-Id"] = self._session_id

        response = requests.post(
            self._mcp_url, json=payload, headers=headers, timeout=_REQUEST_TIMEOUT_S
        )
        session_id = response.headers.get("Mcp-Session-Id")
        if session_id:
            self._session_id = session_id
        return response
