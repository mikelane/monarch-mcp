"""
MCP stdio client helper.

Launches the production Monarch MCP server binary as a subprocess and
communicates via the MCP JSON-RPC protocol over stdin/stdout.

Binary path: env MONARCH_MCP_BIN, defaulting to ../target/debug/monarch-mcp
Monarch base URL: env MONARCH_BASE (passed to the server process)
Goals file: env MONARCH_GOALS_FILE (passed to the server process)

Protocol flow:
  1. send initialize request
  2. receive initialize response (ignore, but wait for it)
  3. send notifications/initialized notification
  4. issue tools/call requests and read responses
  5. on teardown, terminate the subprocess
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any


_DEFAULT_BIN = str(
    Path(__file__).parent.parent.parent / "target" / "debug" / "monarch-mcp"
)


class McpClient:
    """Thin MCP stdio client that drives the monarch-mcp binary."""

    def __init__(self, monarch_base: str, goals_file: str | None = None) -> None:
        self._bin = os.environ.get("MONARCH_MCP_BIN", _DEFAULT_BIN)
        self._monarch_base = monarch_base
        self._goals_file = goals_file
        self._process: subprocess.Popen | None = None
        self._next_id = 1

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def start(self) -> None:
        """Launch the MCP server binary. Raises if the binary is missing."""
        env = os.environ.copy()
        env["MONARCH_BASE"] = self._monarch_base
        # Provide a dummy token so the server skips interactive auth
        env.setdefault("MONARCH_TOKEN", "mock-test-token-abc123")
        if self._goals_file:
            env["MONARCH_GOALS_FILE"] = self._goals_file

        if not Path(self._bin).exists():
            raise FileNotFoundError(
                f"MCP binary not found: {self._bin!r}. "
                "Build it with `cargo build` or set MONARCH_MCP_BIN."
            )

        self._process = subprocess.Popen(
            [self._bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=True,
            bufsize=1,
        )
        self._initialize()

    def stop(self) -> None:
        """Terminate the MCP server process."""
        if self._process is not None:
            self._process.terminate()
            try:
                self._process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self._process.kill()
            self._process = None

    # ------------------------------------------------------------------
    # MCP protocol
    # ------------------------------------------------------------------

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
            raise RuntimeError(
                f"MCP tool error for {tool_name!r}: {response['error']}"
            )
        result = response.get("result", {})
        # MCP returns content as a list of content items; extract text
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

    def _initialize(self) -> None:
        """Send MCP initialize + initialized notification."""
        init_request = {
            "jsonrpc": "2.0",
            "id": self._next_id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "bdd-test-client", "version": "0.1.0"},
            },
        }
        self._next_id += 1
        self._send(init_request)  # wait for initialize response

        # Send the initialized notification (no id, no response expected)
        notification = {
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        }
        self._write_line(json.dumps(notification))

    def _send(self, request: dict) -> dict:
        """Write a JSON-RPC request and read the response line."""
        if self._process is None:
            raise RuntimeError("McpClient.start() has not been called")
        self._write_line(json.dumps(request))
        return self._read_response()

    def _write_line(self, line: str) -> None:
        assert self._process and self._process.stdin
        self._process.stdin.write(line + "\n")
        self._process.stdin.flush()

    def _read_response(self) -> dict:
        assert self._process and self._process.stdout
        line = self._process.stdout.readline()
        if not line:
            stderr_output = ""
            if self._process.stderr:
                import select as _select
                readable, _, _ = _select.select([self._process.stderr], [], [], 0.1)
                if readable:
                    stderr_output = self._process.stderr.read(4096)
            raise RuntimeError(
                f"MCP server closed stdout unexpectedly. stderr: {stderr_output!r}"
            )
        return json.loads(line.strip())
