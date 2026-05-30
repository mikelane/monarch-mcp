"""
Shared step definitions used across all features.

The Background step "the budgeting advisor is connected to the household's
finances" simply asserts that the MCP client was set up. The actual
binary-not-found failure surfaces in the When steps when they attempt a
tool call.

Cross-cutting steps that appear in every feature (expired session,
re-authentication required) also live here to avoid duplication.
"""

from __future__ import annotations

import requests
from behave import given, then


@given("the budgeting advisor is connected to the household's finances")
def step_advisor_connected(context):
    # The client was already created (or attempted) in before_scenario.
    # If the binary is missing, context.mcp_start_error is set and the
    # When steps will raise it.
    pass


@given("the household's Monarch session has expired")
def step_session_expired(context):
    """Configure the mock so every authenticated endpoint returns 401."""
    requests.post(f"{context.mock_base}/configure", json={"session_expired": True})


@then("the advisor reports that re-authentication is required")
def step_assert_reauth_required(context):
    """The MCP tool must surface an auth-expired error, not a crash."""
    # Determine which tool result to inspect based on what the scenario ran.
    result = (
        getattr(context, "overview_result", None)
        or getattr(context, "spending_result", None)
        or getattr(context, "triage_result", None)
        or getattr(context, "progress_result", None)
        or getattr(context, "forecast_result", None)
        or getattr(context, "trend_result", None)
        or getattr(context, "scan_result", None)
    )
    assert result is not None, (
        "No tool result found — make sure the When step ran before this Then step."
    )
    # The tool must set an error key indicating auth expiry.
    error = result.get("error", "")
    assert "auth" in str(error).lower() or "re-authenticate" in str(error).lower(), (
        f"Expected an auth/re-authenticate error, got: {result!r}"
    )


def call_tool(context, tool_name: str, arguments: dict | None = None):
    """Call an MCP tool, re-raising any start error as a clear assertion failure."""
    if context.mcp_start_error is not None:
        raise AssertionError(
            f"Cannot call tool {tool_name!r}: MCP server failed to start — "
            f"{context.mcp_start_error}"
        ) from context.mcp_start_error
    return context.mcp_client.call_tool(tool_name, arguments or {})
