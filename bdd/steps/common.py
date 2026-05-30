"""
Shared step definitions used across all features.

The Background step "the budgeting advisor is connected to the household's
finances" simply asserts that the MCP client was set up. The actual
binary-not-found failure surfaces in the When steps when they attempt a
tool call.
"""

from __future__ import annotations

from behave import given


@given("the budgeting advisor is connected to the household's finances")
def step_advisor_connected(context):
    # The client was already created (or attempted) in before_scenario.
    # If the binary is missing, context.mcp_start_error is set and the
    # When steps will raise it.
    pass


def call_tool(context, tool_name: str, arguments: dict | None = None):
    """Call an MCP tool, re-raising any start error as a clear assertion failure."""
    if context.mcp_start_error is not None:
        raise AssertionError(
            f"Cannot call tool {tool_name!r}: MCP server failed to start — "
            f"{context.mcp_start_error}"
        ) from context.mcp_start_error
    return context.mcp_client.call_tool(tool_name, arguments or {})
