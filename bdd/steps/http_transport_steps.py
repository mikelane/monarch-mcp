"""Step definitions for http_transport.feature (@ISSUE-88).

Reuses the mock-fixture Given steps already defined in
financial_overview_steps.py (e.g. "the household holds ... dollars across
asset accounts") since fixture configuration is transport-agnostic — it
talks to the mock Monarch server over HTTP regardless of which MCP
transport the advisor itself uses.

The overview result is stored on context.overview_result (not a separate
http_overview_result) so the existing "the advisor reports that
re-authentication is required" step in common.py can be reused unchanged.
"""

from __future__ import annotations

import requests
from behave import given, then, when

from support.http_mcp_client import HttpMcpClient


@given("the budgeting advisor is running in HTTP transport mode")
def step_advisor_running_http(context):
    # The HTTP client was already created (or attempted) in before_scenario
    # via before_tag / a dedicated hook — see environment.py.
    pass


@when("the advisor lists available tools over HTTP")
def step_list_tools_over_http(context):
    context.tool_list_result = context.http_mcp_client.list_tools()


@then('the tool list includes "{tool_name}"')
def step_assert_tool_present(context, tool_name: str):
    names = [tool["name"] for tool in context.tool_list_result]
    assert tool_name in names, f"Expected {tool_name!r} in tool list, got: {names}"


@when("the advisor requests a financial overview over HTTP")
def step_request_overview_over_http(context):
    context.overview_result = context.http_mcp_client.call_tool("financial_overview")


@then("the HTTP overview reports a net worth of {amount:d} dollars")
def step_assert_http_net_worth(context, amount: int):
    result = context.overview_result
    actual = result.get("net_worth")
    assert actual == float(amount), (
        f"Expected net worth {amount}, got {actual!r}. Full result: {result}"
    )


@when('a request to the HTTP transport carries Origin "{origin}"')
def step_request_with_origin(context, origin: str):
    context.raw_http_response = requests.post(
        context.http_mcp_client.mcp_url,
        json={
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "evil-page", "version": "0.0.1"},
            },
        },
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
            "Origin": origin,
        },
    )


@then("the HTTP transport responds with status {status:d}")
def step_assert_status(context, status: int):
    actual = context.raw_http_response.status_code
    assert actual == status, f"Expected status {status}, got {actual}"
