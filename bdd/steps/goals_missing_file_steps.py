"""Step definitions for goals_missing_file.feature (@ISSUE-22).

These steps cover the three scenarios for missing/empty/unreadable goals files.
Each step that changes the goals-file state must restart the MCP client so the
server subprocess inherits the updated MONARCH_GOALS_FILE environment variable.
"""

from __future__ import annotations

import os
import stat
import tempfile

from behave import given, then, when

from steps.common import call_tool
from support.mcp_client import McpClient


def _restart_client_with_goals_file(context, goals_file: str | None) -> None:
    """Stop the current MCP client, update MONARCH_GOALS_FILE, and restart.

    This is necessary because the MCP server subprocess inherits environment
    variables at start time — changes after launch have no effect.
    """
    if hasattr(context, "mcp_client") and context.mcp_client is not None:
        context.mcp_client.stop()

    if goals_file is not None:
        os.environ["MONARCH_GOALS_FILE"] = goals_file
        context.goals_file = goals_file
    else:
        # Unset the variable so load_from_env returns default
        os.environ.pop("MONARCH_GOALS_FILE", None)
        context.goals_file = None

    client = McpClient(
        monarch_base=context.mock_base,
        goals_file=goals_file,
    )
    context.mcp_client = client
    context.mcp_start_error = None
    try:
        client.start()
    except FileNotFoundError as exc:
        context.mcp_start_error = exc


# ---------------------------------------------------------------------------
# Given — goals-file state variations
# ---------------------------------------------------------------------------


@given("the MCP server is running with no goals file configured")
def step_no_goals_file(context):
    """Delete the default temp goals file so MONARCH_GOALS_FILE points at nothing."""
    # Remove the temp file created by before_scenario so the path is genuinely absent
    existing = getattr(context, "goals_file", None)
    if existing and os.path.exists(existing):
        os.unlink(existing)
    # Restart the client pointing at the now-missing path
    _restart_client_with_goals_file(context, existing)


@given("the MCP server is running with an empty goals file")
def step_empty_goals_file(context):
    """The default before_scenario already creates an empty temp file — just restart."""
    # before_scenario already created an empty temp file; a restart ensures
    # the client's subprocess sees the current MONARCH_GOALS_FILE value.
    _restart_client_with_goals_file(context, context.goals_file)


@given("the MCP server is running with a goals file that cannot be read due to permissions")
def step_unreadable_goals_file(context):
    """Write a valid goals file, then seal it so the process cannot read it."""
    import subprocess

    # Skip on macOS/Linux when running as root (root bypasses file permissions)
    uid = subprocess.run(["id", "-u"], capture_output=True, text=True).stdout.strip()
    if uid == "0":
        context.scenario.skip("Skipping permission test when running as root")
        return

    # Write content so it's a real file, then remove read permission
    goals_path = getattr(context, "goals_file", None)
    if not goals_path:
        tmp = tempfile.NamedTemporaryFile(
            mode="w", suffix=".toml", delete=False, prefix="monarch_goals_noperm_"
        )
        tmp.write("[savings_rate]\ntarget_percent = 20.0\n")
        tmp.close()
        goals_path = tmp.name
        context.goals_file = goals_path
    else:
        with open(goals_path, "w") as fh:
            fh.write("[savings_rate]\ntarget_percent = 20.0\n")

    os.chmod(goals_path, stat.S_IRUSR | stat.S_IWUSR)  # ensure writable first
    os.chmod(goals_path, 0o000)  # then seal it
    context._unreadable_goals_path = goals_path

    _restart_client_with_goals_file(context, goals_path)


# ---------------------------------------------------------------------------
# When
# ---------------------------------------------------------------------------


@when("an agent calls the progress_vs_goals tool")
def step_call_progress_vs_goals(context):
    """Call progress_vs_goals and store the result (or catch MCP errors)."""
    context.goals_tool_error = None
    context.goals_tool_result = None
    try:
        context.goals_tool_result = call_tool(context, "progress_vs_goals")
    except RuntimeError as exc:
        # MCP protocol-level error (server returned an error response)
        context.goals_tool_error = str(exc)


# ---------------------------------------------------------------------------
# Then
# ---------------------------------------------------------------------------


@then("the response indicates no goals are configured")
def step_assert_no_goals_indication(context):
    result = context.goals_tool_result
    assert result is not None, (
        f"Expected a successful result but got an MCP error: {context.goals_tool_error!r}"
    )
    guidance = result.get("guidance", "")
    assert guidance, (
        f"Expected a 'guidance' key in the response but got: {result!r}"
    )
    assert "no goals" in guidance.lower() or "goals.toml" in guidance.lower(), (
        f"Expected guidance to mention 'no goals' or 'goals.toml', got: {guidance!r}"
    )


@then("the response includes guidance on how to add goals")
def step_assert_guidance_present(context):
    result = context.goals_tool_result
    assert result is not None, (
        f"Expected a successful result but got an MCP error: {context.goals_tool_error!r}"
    )
    guidance = result.get("guidance", "")
    assert guidance, f"Expected 'guidance' field in result, got: {result!r}"
    assert "goals.toml" in guidance, (
        f"Expected guidance to mention 'goals.toml', got: {guidance!r}"
    )


@then("the response is not an MCP error")
def step_assert_not_mcp_error(context):
    assert context.goals_tool_error is None, (
        f"Expected no MCP error but got: {context.goals_tool_error!r}"
    )
    assert context.goals_tool_result is not None, (
        "Expected a result payload but got None"
    )


@then("the response is an MCP error")
def step_assert_is_mcp_error(context):
    # Restore permissions so the temp file can be cleaned up by after_scenario
    if hasattr(context, "_unreadable_goals_path"):
        try:
            os.chmod(context._unreadable_goals_path, 0o600)
        except OSError:
            pass
    assert context.goals_tool_error is not None, (
        f"Expected an MCP error but got a successful result: {context.goals_tool_result!r}"
    )


@then("the error message mentions the goals file path")
def step_assert_error_mentions_path(context):
    err = context.goals_tool_error or ""
    goals_path = getattr(context, "_unreadable_goals_path", "") or ""
    assert goals_path and goals_path in err, (
        f"Expected error to mention goals file path {goals_path!r}, got: {err!r}"
    )
