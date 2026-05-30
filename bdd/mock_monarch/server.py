"""
Mock Monarch GraphQL server.

Runs a local Flask HTTP server that answers:
  POST /auth/login/   — returns a session token
  POST /graphql       — dispatches to fixture-backed handlers

Fixture state is reset per scenario via the /reset and /configure endpoints.
"""

from __future__ import annotations

import json
import threading
from typing import Any

from flask import Flask, jsonify, request

# ---------------------------------------------------------------------------
# Fixture store — replaced atomically at scenario boundaries
# ---------------------------------------------------------------------------

_DEFAULT_FIXTURES: dict[str, Any] = {
    "accounts": [],
    "transactions": [],
    "transactions_needing_review": [],
    "budgets": [],
    "cashflow": {"income": 0.0, "spending": 0.0},
    "categories": [],
    "tags": [],
    "prior_month_net_worth": 0.0,
    "applied_changes": [],
}


def _fresh_fixtures() -> dict[str, Any]:
    import copy
    return copy.deepcopy(_DEFAULT_FIXTURES)


_fixtures: dict[str, Any] = _fresh_fixtures()
_fixtures_lock = threading.Lock()


def get_fixtures() -> dict[str, Any]:
    with _fixtures_lock:
        import copy
        return copy.deepcopy(_fixtures)


def set_fixtures(data: dict[str, Any]) -> None:
    with _fixtures_lock:
        _fixtures.update(data)


def reset_fixtures() -> None:
    global _fixtures
    with _fixtures_lock:
        _fixtures = _fresh_fixtures()


# ---------------------------------------------------------------------------
# Flask application
# ---------------------------------------------------------------------------

app = Flask(__name__)
app.config["TESTING"] = False


@app.route("/auth/login/", methods=["POST"])
def login() -> Any:
    """Simulate Monarch's DRF token login.

    Accepts any credentials and returns a static test token.
    Returns 403 with MFA signal when the body lacks a totp field on first call
    only if the fixture is configured to require MFA — for simplicity we always
    succeed so step definitions don't need to manage MFA state.
    """
    return jsonify({"token": "mock-test-token-abc123"})


@app.route("/graphql", methods=["POST"])
def graphql() -> Any:
    """Dispatch GraphQL operations to fixture-backed handlers."""
    body = request.get_json(force=True, silent=True) or {}
    operation = body.get("operationName", "")
    handler = _OPERATION_HANDLERS.get(operation)
    if handler is None:
        return jsonify({"errors": [{"message": f"Unknown operation: {operation}"}]}), 400
    return jsonify(handler(body))


# ---------------------------------------------------------------------------
# Control endpoints (called by environment.py / step definitions)
# ---------------------------------------------------------------------------

@app.route("/reset", methods=["POST"])
def reset() -> Any:
    reset_fixtures()
    return jsonify({"ok": True})


@app.route("/configure", methods=["POST"])
def configure() -> Any:
    data = request.get_json(force=True, silent=True) or {}
    set_fixtures(data)
    return jsonify({"ok": True})


@app.route("/applied_changes", methods=["GET"])
def get_applied_changes() -> Any:
    with _fixtures_lock:
        return jsonify(_fixtures.get("applied_changes", []))


# ---------------------------------------------------------------------------
# GraphQL operation handlers
# ---------------------------------------------------------------------------

def _handle_get_accounts(body: dict) -> dict:
    fixtures = get_fixtures()
    accounts = [
        {
            "id": str(i),
            "displayName": acct.get("name", f"Account {i}"),
            "currentBalance": acct.get("currentBalance", 0.0),
            "type": {"name": acct.get("type", "checking")},
        }
        for i, acct in enumerate(fixtures["accounts"])
    ]
    return {"data": {"accounts": accounts}}


def _handle_get_transactions(body: dict) -> dict:
    fixtures = get_fixtures()
    txns = [
        {
            "id": str(i),
            "amount": t.get("amount", 0.0),
            "date": t.get("date", "2026-05-01"),
            "merchantName": t.get("merchant", ""),
            "category": {"name": t.get("category", "Uncategorized")},
            "tags": t.get("tags", []),
            "notes": t.get("notes", ""),
        }
        for i, t in enumerate(fixtures["transactions"])
    ]
    return {"data": {"transactions": txns}}


def _handle_get_transactions_needing_review(body: dict) -> dict:
    fixtures = get_fixtures()
    txns = [
        {
            "id": str(i),
            "amount": t.get("amount", 0.0),
            "date": t.get("date", "2026-05-01"),
            "merchantName": t.get("merchant", ""),
            "category": {"name": t.get("category", "Uncategorized")},
            "tags": t.get("tags", []),
            "notes": t.get("notes", ""),
        }
        for i, t in enumerate(fixtures["transactions_needing_review"])
    ]
    return {"data": {"transactionsNeedingReview": txns}}


def _handle_get_budgets(body: dict) -> dict:
    fixtures = get_fixtures()
    budgets = [
        {
            "category": {"name": b.get("category", "")},
            "amount": b.get("amount", 0.0),
        }
        for b in fixtures["budgets"]
    ]
    return {"data": {"budgets": budgets}}


def _handle_get_cashflow(body: dict) -> dict:
    fixtures = get_fixtures()
    cf = fixtures["cashflow"]
    return {
        "data": {
            "cashflow": {
                "income": cf.get("income", 0.0),
                "spending": cf.get("spending", 0.0),
            }
        }
    }


def _handle_get_categories(body: dict) -> dict:
    fixtures = get_fixtures()
    cats = [{"id": str(i), "name": c} for i, c in enumerate(fixtures["categories"])]
    return {"data": {"categories": cats}}


def _handle_get_tags(body: dict) -> dict:
    fixtures = get_fixtures()
    tags = [{"id": str(i), "name": t} for i, t in enumerate(fixtures["tags"])]
    return {"data": {"tags": tags}}


def _handle_update_transaction(body: dict) -> dict:
    """Simulate applying a category/tags/notes change to one transaction."""
    variables = body.get("variables", {})
    txn_id = variables.get("id", "")
    category = variables.get("category")
    tags = variables.get("tags")
    notes = variables.get("notes")

    change_record = {"id": txn_id}
    if category is not None:
        change_record["category"] = category
    if tags is not None:
        change_record["tags"] = tags
    if notes is not None:
        change_record["notes"] = notes

    with _fixtures_lock:
        _fixtures.setdefault("applied_changes", []).append(change_record)
        # Also update the transaction in the transactions list so subsequent reads reflect it
        for txn in _fixtures.get("transactions", []):
            if str(txn.get("id", "")) == str(txn_id):
                if category is not None:
                    txn["category"] = category
                break

    return {
        "data": {
            "updateTransaction": {
                "id": txn_id,
                "category": {"name": category} if category else None,
            }
        }
    }


def _handle_get_net_worth_history(body: dict) -> dict:
    fixtures = get_fixtures()
    return {
        "data": {
            "netWorthHistory": {
                "priorMonthNetWorth": fixtures.get("prior_month_net_worth", 0.0),
            }
        }
    }


_OPERATION_HANDLERS: dict[str, Any] = {
    "GetAccounts": _handle_get_accounts,
    "GetTransactions": _handle_get_transactions,
    "GetTransactionsNeedingReview": _handle_get_transactions_needing_review,
    "GetBudgets": _handle_get_budgets,
    "GetCashflow": _handle_get_cashflow,
    "GetCategories": _handle_get_categories,
    "GetTags": _handle_get_tags,
    "UpdateTransaction": _handle_update_transaction,
    "GetNetWorthHistory": _handle_get_net_worth_history,
}


# ---------------------------------------------------------------------------
# Server lifecycle helpers (used by environment.py)
# ---------------------------------------------------------------------------

def start_server(host: str = "127.0.0.1", port: int = 0) -> tuple[threading.Thread, int]:
    """Start the mock server in a daemon thread. Returns (thread, bound_port)."""
    import socket

    # Find a free port
    if port == 0:
        with socket.socket() as sock:
            sock.bind((host, 0))
            port = sock.getsockname()[1]

    thread = threading.Thread(
        target=lambda: app.run(host=host, port=port, use_reloader=False, threaded=True),
        daemon=True,
    )
    thread.start()

    # Wait until the server is accepting connections
    import time
    deadline = time.monotonic() + 5.0
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.1):
                break
        except OSError:
            time.sleep(0.05)
    else:
        raise RuntimeError(f"Mock Monarch server did not start on port {port} within 5 s")

    return thread, port
