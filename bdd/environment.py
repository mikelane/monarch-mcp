"""
Behave environment hooks.

Per-scenario lifecycle:
  before_scenario — reset mock fixtures, (re)start MCP client
  after_scenario  — stop MCP client, clean up temp files

The mock Monarch server is started once for the entire session (before_all)
and left running; only its fixture state is reset between scenarios.
"""

from __future__ import annotations

import os
import tempfile

from mock_monarch.server import reset_fixtures, start_server
from support.mcp_client import McpClient


def before_all(context):
    """Start the mock Monarch server once for the full test run."""
    _thread, port = start_server()
    context.mock_port = port
    context.mock_base = f"http://127.0.0.1:{port}"


def before_scenario(context, scenario):
    """Reset fixtures and create a fresh MCP client for the scenario."""
    # Reset all mock fixture state to defaults
    reset_fixtures()

    # Behave stores attributes whose names begin with '_' directly in
    # context.__dict__ (bypassing the per-scenario layer stack), so they
    # leak across scenarios.  Explicitly clear them here.
    context._transactions = []
    context._budgets = []
    context.prior_month_spending = 0.0

    # Temporary goals file — each scenario gets a clean slate
    tmp = tempfile.NamedTemporaryFile(
        mode="w", suffix=".toml", delete=False, prefix="monarch_goals_"
    )
    tmp.write("")  # empty = no goals set
    tmp.close()
    context.goals_file = tmp.name
    os.environ["MONARCH_GOALS_FILE"] = tmp.name

    # Build and attempt to start the MCP client
    client = McpClient(
        monarch_base=context.mock_base,
        goals_file=context.goals_file,
    )
    context.mcp_client = client
    context.mcp_start_error = None
    try:
        client.start()
    except FileNotFoundError as exc:
        # Expected RED state: binary doesn't exist yet.
        # Store the error so When-steps can surface it as a meaningful failure.
        context.mcp_start_error = exc


def after_scenario(context, scenario):
    """Tear down the MCP client and remove the temporary goals file."""
    if hasattr(context, "mcp_client") and context.mcp_client is not None:
        context.mcp_client.stop()
    if hasattr(context, "goals_file") and context.goals_file:
        try:
            os.unlink(context.goals_file)
        except OSError:
            pass
