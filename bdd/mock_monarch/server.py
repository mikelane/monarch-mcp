"""
Mock Monarch GraphQL server — real response shapes (ADR 0002).

Runs a local Flask HTTP server that answers:
  POST /auth/login/   — returns a session token (or 401 if session_expired)
  POST /graphql       — dispatches to fixture-backed handlers (or 401 if session_expired)

All GraphQL handlers return the real Monarch response shapes validated in ADR 0002.
Fixture data uses synthetic values; no real financial data is stored here.

Control endpoints (called by environment.py and step definitions):
  POST /reset         — reset all fixture state to defaults
  POST /configure     — merge new fixture values into state
  GET  /applied_changes — inspect mutations recorded by UpdateTransaction
"""

from __future__ import annotations

import os
import threading
from typing import Any

from flask import Flask, jsonify, request

# ---------------------------------------------------------------------------
# Fixture store — replaced atomically at scenario boundaries
# ---------------------------------------------------------------------------

_DEFAULT_FIXTURES: dict[str, Any] = {
    # List of account dicts: {name, type, currentBalance}
    "accounts": [],
    # List of transaction dicts: {id?, merchant, amount, category, date, tags?, notes?}
    "transactions": [],
    # List of budget dicts: {category, amount} — used to build budget response
    "budgets": [],
    # Cashflow summary: {income, spending} — income stored directly, spending as positive
    "cashflow": {"income": 0.0, "spending": 0.0},
    # Prior-month net worth balance (scalar)
    "prior_month_net_worth": 0.0,
    # Prior-month spending (used as prior-period sumExpense)
    "prior_month_spending": 0.0,
    # Mutation audit log
    "applied_changes": [],
    # When True every authenticated endpoint returns 401
    "session_expired": False,
    # --- Epic B fixtures ---
    # List of recurring charge dicts for Web_GetUpcomingRecurringTransactionItems:
    #   {merchant, stream_amount, actual_amount?, frequency, is_approximate, is_past}
    "recurring_items": [],
    # List of net-worth-by-type snapshot dicts for GetSnapshotsByAccountType:
    #   {month (YYYY-MM), account_type, balance}
    "snapshots_by_type": [],
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


def _session_expired() -> bool:
    with _fixtures_lock:
        return bool(_fixtures.get("session_expired", False))


@app.route("/auth/login/", methods=["POST"])
def login() -> Any:
    """Simulate Monarch's DRF token login endpoint."""
    if _session_expired():
        return jsonify({"detail": "Invalid token."}), 401
    return jsonify({"token": "mock-test-token-abc123"})


@app.route("/graphql", methods=["POST"])
def graphql() -> Any:
    """Dispatch GraphQL operations to fixture-backed handlers."""
    if _session_expired():
        return (
            jsonify({"errors": [{"message": "Authentication credentials were not provided."}]}),
            401,
        )
    body = request.get_json(force=True, silent=True) or {}
    operation = body.get("operationName", "")
    handler = _OPERATION_HANDLERS.get(operation)
    if handler is None:
        return jsonify({"errors": [{"message": f"Unknown operation: {operation}"}]}), 400
    return jsonify(handler(body))


# ---------------------------------------------------------------------------
# Control endpoints
# ---------------------------------------------------------------------------

@app.route("/reset", methods=["POST"])
def reset() -> Any:
    reset_fixtures()
    return jsonify({"ok": True})


@app.route("/configure", methods=["POST"])
def configure() -> Any:
    data = request.get_json(force=True, silent=True) or {}

    # The BDD step definitions may post `transactions_needing_review` separately
    # from `transactions`. In the real Monarch API (and the new client), needsReview
    # is a filter on GetTransactionsList — there's no separate root field.
    # We merge them here: mark transactions_needing_review entries with
    # needsReview=True and fold them into the main transactions list so the
    # GetTransactionsList handler serves them correctly.
    if "transactions_needing_review" in data:
        review_txns = data.pop("transactions_needing_review")
        for t in review_txns:
            t["needsReview"] = True
        # Merge into existing transactions fixture (don't replace)
        with _fixtures_lock:
            existing = _fixtures.get("transactions", [])
            existing_ids = {str(t.get("id", "")) for t in existing if t.get("id")}
            for t in review_txns:
                tid = str(t.get("id", ""))
                if tid and tid in existing_ids:
                    # Update existing entry
                    for ex in existing:
                        if str(ex.get("id", "")) == tid:
                            ex.update(t)
                            break
                else:
                    existing.append(t)
            _fixtures["transactions"] = existing

    # When the step sets "transactions" after a prior "transactions_needing_review"
    # configure call, preserve the needsReview=True flag for entries that were
    # previously marked (matched by merchant + date, since id may be absent).
    if "transactions" in data:
        with _fixtures_lock:
            previously_review = {
                (t.get("merchant", ""), t.get("date", "")): True
                for t in _fixtures.get("transactions", [])
                if t.get("needsReview")
            }
        incoming = data["transactions"]
        for t in incoming:
            key = (t.get("merchant", ""), t.get("date", ""))
            if key in previously_review:
                t.setdefault("needsReview", True)

    set_fixtures(data)
    return jsonify({"ok": True})


@app.route("/applied_changes", methods=["GET"])
def get_applied_changes() -> Any:
    with _fixtures_lock:
        return jsonify(_fixtures.get("applied_changes", []))


# ---------------------------------------------------------------------------
# GraphQL operation handlers — real Monarch response shapes (ADR 0002)
# ---------------------------------------------------------------------------

def _make_account(index: int, acct: dict) -> dict:
    """Build one account in real GetAccounts shape.

    When the fixture sets currentBalance=None, the mock passes null through to
    the client — matching real Monarch behaviour for unsynced/manual accounts.
    The client must handle null gracefully (Bug B2: use serde default=0.0).
    """
    # Use sentinel to distinguish "not set" (default 0.0) from explicit None (null).
    raw_balance = acct.get("currentBalance", 0.0)
    return {
        "id": str(acct.get("id", index)),
        "displayName": acct.get("name", f"Account {index}"),
        "currentBalance": raw_balance,  # None serialises as JSON null — intentional
        "isHidden": False,
        "type": {
            "name": acct.get("type", "depository"),
            "display": acct.get("type", "Depository").title(),
            "__typename": "AccountType",
        },
        "subtype": {
            "name": "checking",
            "display": "Checking",
            "__typename": "AccountSubtype",
        },
        "__typename": "Account",
    }


def _handle_get_accounts(body: dict) -> dict:
    """GetAccounts → accounts[]"""
    fixtures = get_fixtures()
    accounts = [
        _make_account(i, acct)
        for i, acct in enumerate(fixtures["accounts"])
    ]
    return {"data": {"accounts": accounts}}


def _make_transaction(index: int, t: dict) -> dict:
    """Build one transaction in real GetTransactionsList shape."""
    cat_name = t.get("category", "Uncategorized")
    merchant_name = t.get("merchant", "")
    tags_raw = t.get("tags", [])
    # Support both bare-string tags (legacy fixture) and {id,name} tag objects
    tags = [
        {
            "id": tag if isinstance(tag, str) else tag.get("id", str(i2)),
            "name": tag if isinstance(tag, str) else tag.get("name", ""),
            "color": "#000000",
            "order": i2,
            "__typename": "TransactionTag",
        }
        for i2, tag in enumerate(tags_raw)
    ]
    # group_type: fixture may override per-transaction; default "expense".
    # income/transfer transactions must set "group_type" in the fixture dict.
    group_type = t.get("group_type", "expense")
    return {
        "id": str(t.get("id", index)),
        "amount": t.get("amount", 0.0),
        "date": t.get("date", "2026-05-01"),
        "pending": False,
        "hideFromReports": False,
        "plaidName": merchant_name,
        "notes": t.get("notes", "") or "",
        "isRecurring": False,
        "reviewStatus": None,
        "needsReview": t.get("needsReview", False),
        "isSplitTransaction": False,
        "createdAt": "2026-05-01T00:00:00+00:00",
        "updatedAt": "2026-05-01T00:00:00+00:00",
        "category": {
            "id": str(t.get("category_id", f"cat-{index}")),
            "name": cat_name,
            "group": {
                "type": group_type,
                "__typename": "CategoryGroup",
            },
            "__typename": "Category",
        },
        "merchant": {
            "id": str(t.get("merchant_id", f"m-{index}")),
            "name": merchant_name,
            "transactionsCount": 1,
            "__typename": "Merchant",
        },
        "account": {
            "id": "mock-account-1",
            "displayName": "Mock Checking",
            "__typename": "Account",
        },
        "tags": tags,
        "__typename": "Transaction",
    }


def _handle_get_transactions_list(body: dict) -> dict:
    """GetTransactionsList → allTransactions.{totalCount, results[]}

    The real Monarch API uses one operation for both regular transactions and
    needsReview queries — the filter object controls what is returned.
    """
    fixtures = get_fixtures()
    variables = body.get("variables", {})
    filters = variables.get("filters", {})
    needs_review_filter = filters.get("needsReview")

    transactions = fixtures["transactions"]

    # Apply needsReview filter when present
    if needs_review_filter is True:
        transactions = [t for t in transactions if t.get("needsReview", False)]

    results = [_make_transaction(i, t) for i, t in enumerate(transactions)]
    return {
        "data": {
            "allTransactions": {
                "totalCount": len(results),
                "results": results,
                "__typename": "TransactionList",
            }
        }
    }


def _handle_web_get_cash_flow_page(body: dict) -> dict:
    """Web_GetCashFlowPage → summary[0].summary.{sumIncome, sumExpense, savings, savingsRate}

    The real Monarch returns sumExpense as a negative number. Income is positive.
    The mock inspects the request's startDate to decide whether to return current
    or prior-month data — prior month uses prior_month_spending fixture.
    """
    fixtures = get_fixtures()
    variables = body.get("variables", {})
    filters = variables.get("filters", {})
    start_date = filters.get("startDate", "")

    cf = fixtures["cashflow"]
    income = cf.get("income", 0.0)

    # Distinguish current vs prior period by startDate: parse the YYYY-MM prefix
    # and treat any month earlier than today's (year, month) as the prior period.
    # Prior period returns 0 income / -prior_month_spending; current returns the
    # cashflow fixture's income / -spending.
    spending = cf.get("spending", 0.0)
    prior_spending = fixtures.get("prior_month_spending", 0.0)

    # Parse year-month from startDate.
    # The mock must share the client's clock basis via MONARCH_NOW so period
    # classification is deterministic regardless of wall-clock time.
    is_prior = False
    if start_date and len(start_date) >= 7:
        ym = start_date[:7]  # "YYYY-MM"
        try:
            yr, mo = map(int, ym.split("-"))
            import datetime
            pinned_now = os.environ.get("MONARCH_NOW")
            today = datetime.date.fromisoformat(pinned_now) if pinned_now else datetime.date.today()
            current_ym = (today.year, today.month)
            is_prior = (yr, mo) < current_ym
        except (ValueError, AttributeError):
            pass

    if is_prior:
        sum_income = 0.0
        sum_expense = -prior_spending  # negative in Monarch convention
    else:
        sum_income = income
        sum_expense = -spending  # negative in Monarch convention

    savings = sum_income + sum_expense  # income - |expense|
    savings_rate = (savings / sum_income) if sum_income != 0.0 else 0.0

    return {
        "data": {
            "summary": [
                {
                    "summary": {
                        "sumIncome": sum_income,
                        "sumExpense": sum_expense,
                        "savings": savings,
                        "savingsRate": savings_rate,
                        "__typename": "AggregateSummary",
                    },
                    "__typename": "Aggregate",
                }
            ]
        }
    }


def _handle_get_aggregate_snapshots(body: dict) -> dict:
    """GetAggregateSnapshots → aggregateSnapshots[{date, balance}]

    Returns two synthetic snapshots in the requested date window.
    The last snapshot's balance is used as prior_month_net_worth by the client.
    """
    fixtures = get_fixtures()
    prior_nw = fixtures.get("prior_month_net_worth", 0.0)
    variables = body.get("variables", {})
    filters = variables.get("filters", {})
    start = filters.get("startDate", "2026-04-01")
    end = filters.get("endDate", "2026-04-30")

    snapshots = [
        {"date": start, "balance": prior_nw * 0.98, "__typename": "AggregateSnapshot"},
        {"date": end, "balance": prior_nw, "__typename": "AggregateSnapshot"},
    ]
    return {"data": {"aggregateSnapshots": snapshots}}


def _handle_get_categories(body: dict) -> dict:
    """GetCategories → categories[{id, order, name, ...}]

    Returns one category per budget entry (so budget join works) plus any
    extra categories derived from the transactions fixture.
    """
    fixtures = get_fixtures()
    seen: dict[str, dict] = {}

    # Build from budgets
    for i, b in enumerate(fixtures.get("budgets", [])):
        name = b.get("category", f"Category {i}")
        cat_id = f"cat-budget-{i}"
        seen[name] = {
            "id": cat_id,
            "order": i,
            "name": name,
            "systemCategory": None,
            "isSystemCategory": False,
            "isDisabled": False,
            "updatedAt": "2026-01-01T00:00:00+00:00",
            "createdAt": "2026-01-01T00:00:00+00:00",
            "group": {
                "id": "grp-expense",
                "name": "Expense",
                "type": "expense",
                "__typename": "CategoryGroup",
            },
            "__typename": "Category",
        }

    # Also include categories from transactions so triage works
    for i, t in enumerate(fixtures.get("transactions", [])):
        name = t.get("category", "Uncategorized")
        if name not in seen:
            cat_id = f"cat-txn-{i}"
            seen[name] = {
                "id": cat_id,
                "order": 100 + i,
                "name": name,
                "systemCategory": None,
                "isSystemCategory": False,
                "isDisabled": False,
                "updatedAt": "2026-01-01T00:00:00+00:00",
                "createdAt": "2026-01-01T00:00:00+00:00",
                "group": {
                    "id": "grp-expense",
                    "name": "Expense",
                    "type": "expense",
                    "__typename": "CategoryGroup",
                },
                "__typename": "Category",
            }

    return {"data": {"categories": list(seen.values())}}


def _handle_get_joint_planning_data(body: dict) -> dict:
    """GetJointPlanningData → budgetData.monthlyAmountsByCategory[{category.id, monthlyAmounts}]

    Budget entries from the fixture are joined with category IDs. The mock
    re-uses the same category IDs that _handle_get_categories produces, so
    the client's join (id → name) resolves correctly.
    """
    fixtures = get_fixtures()
    variables = body.get("variables", {})
    start_date = variables.get("startDate", "2026-05-01")

    monthly_by_cat = []
    for i, b in enumerate(fixtures.get("budgets", [])):
        cat_id = f"cat-budget-{i}"
        amount = b.get("amount", 0.0)
        monthly_by_cat.append({
            "category": {"id": cat_id, "__typename": "Category"},
            "monthlyAmounts": [
                {
                    "month": start_date,
                    "plannedCashFlowAmount": amount,
                    "actualAmount": 0.0,
                    "remainingAmount": amount,
                    "__typename": "BudgetMonthlyAmounts",
                }
            ],
            "__typename": "BudgetDataByCategory",
        })

    return {
        "data": {
            "budgetData": {
                "monthlyAmountsByCategory": monthly_by_cat,
                "__typename": "BudgetData",
            }
        }
    }


def _handle_get_household_transaction_tags(body: dict) -> dict:
    """GetHouseholdTransactionTags → householdTransactionTags[{id, name, color, order, transactionCount}]"""
    return {
        "data": {
            "householdTransactionTags": []
        }
    }


def _handle_common_update_transaction(body: dict) -> dict:
    """Common_UpdateTransactionMutation — record the change, reject amount mutations.

    The real Monarch mutation takes `input: {id, categoryId?, notes?, tagIds?}`.
    Amount changes are forbidden and are not passed by the Rust client anyway
    (triage.rs rejects them before the API call).
    """
    variables = body.get("variables", {})
    input_data = variables.get("input", {})
    txn_id = input_data.get("id", "")
    category_id = input_data.get("categoryId")
    tag_ids = input_data.get("tagIds")
    notes = input_data.get("notes")

    change_record: dict[str, Any] = {"id": txn_id}
    if category_id is not None:
        change_record["categoryId"] = category_id
        # Also store as "category" for BDD assertions that check c.get("category")
        change_record["category"] = category_id
    if tag_ids is not None:
        change_record["tagIds"] = tag_ids
    if notes is not None:
        change_record["notes"] = notes

    with _fixtures_lock:
        _fixtures.setdefault("applied_changes", []).append(change_record)
        # Update the in-memory transaction so subsequent reads reflect it
        for txn in _fixtures.get("transactions", []):
            if str(txn.get("id", "")) == str(txn_id):
                if category_id is not None:
                    txn["category"] = category_id
                if notes is not None:
                    txn["notes"] = notes
                break

    return {
        "data": {
            "updateTransaction": {
                "transaction": {
                    "id": txn_id,
                    "notes": notes,
                    "category": {"id": category_id, "name": category_id, "__typename": "Category"},
                    "__typename": "Transaction",
                },
                "errors": None,
                "__typename": "UpdateTransactionPayload",
            }
        }
    }


def _handle_web_get_upcoming_recurring(body: dict) -> dict:
    """Web_GetUpcomingRecurringTransactionItems → recurringTransactionItems[]

    Returns recurring items in real Monarch response shape (ADR 0003).
    Each fixture item drives one RecurringTransactionItem with a nested stream.
    `is_past` controls whether the item has already occurred this period.
    `is_approximate` on the stream marks utility-style charges whose amount
    varies — the recurring_scan tool must not flag these as creeping.
    `actual_amount` defaults to `stream_amount` when omitted (stable charge).

    Nullable fields (ADR 0003 §Nullable):
    - `merchant`: set fixture key `merchant` to None to emit `"merchant": null`.
      The client defaults the display name to "Unknown" when absent.
    - `amountDiff`: set fixture key `amount_diff_null` to True to emit
      `"amountDiff": null`. The client maps null → 0.0 (no measurable drift).
    """
    fixtures = get_fixtures()
    items = []
    for i, r in enumerate(fixtures.get("recurring_items", [])):
        merchant_name = r.get("merchant", f"Merchant {i}")
        stream_amount = float(r.get("stream_amount", 0.0))
        actual_amount = float(r.get("actual_amount", stream_amount))
        frequency = r.get("frequency", "monthly")
        is_approximate = bool(r.get("is_approximate", False))
        is_past = bool(r.get("is_past", False))

        # amountDiff: null when the fixture explicitly requests it (new/unresolved
        # stream with no prior occurrence). Otherwise compute from amounts.
        if r.get("amount_diff_null", False):
            amount_diff = None
        else:
            # Positive when actual > stream (price increase), negative when less.
            amount_diff = round(actual_amount - abs(stream_amount), 2)

        # merchant: null when the fixture sets merchant to None explicitly.
        if merchant_name is None:
            merchant_obj = None
        else:
            merchant_obj = {
                "id": f"merch-{i}",
                "name": merchant_name,
                "logoUrl": None,
                "__typename": "RecurringTransactionStream",
            }

        # transactionId present only for past items
        transaction_id = f"txn-recur-{i}" if is_past else None
        items.append({
            "stream": {
                "id": f"stream-{i}",
                "frequency": frequency,
                # Stream amount stored as negative (outflow) per Monarch convention
                "amount": -abs(stream_amount),
                "isApproximate": is_approximate,
                "merchant": merchant_obj,
                "__typename": "RecurringTransactionStream",
            },
            "date": r.get("date", "2026-05-15"),
            "isPast": is_past,
            "transactionId": transaction_id,
            # Actual amount negative (outflow) per Monarch convention
            "amount": -abs(actual_amount),
            "amountDiff": amount_diff,
            "category": {
                "id": f"cat-recur-{i}",
                "name": r.get("category", "Bills & Utilities"),
                "__typename": "Category",
            },
            "account": {
                "id": "mock-account-1",
                "displayName": "Mock Checking",
                "logoUrl": None,
                "__typename": "Account",
            },
            "__typename": "RecurringTransactionItem",
        })
    return {"data": {"recurringTransactionItems": items}}


def _handle_get_snapshots_by_account_type(body: dict) -> dict:
    """GetSnapshotsByAccountType → {snapshotsByAccountType[], accountTypes[]}

    Returns monthly net-worth snapshots per account type (ADR 0003).
    The flat list is grouped client-side by the production tool.
    Balance is negative for liability types (credit, loan) per Monarch convention.
    """
    fixtures = get_fixtures()
    snapshots = []
    seen_types: dict[str, str] = {}  # type → group
    for row in fixtures.get("snapshots_by_type", []):
        account_type = row.get("account_type", "depository")
        balance = float(row.get("balance", 0.0))
        month = row.get("month", "2026-05")
        snapshots.append({
            "accountType": account_type,
            "month": month,
            "balance": balance,
            "__typename": "AccountTypeSnapshot",
        })
        # Infer group from type name: credit/loan are liabilities, rest are assets
        if account_type not in seen_types:
            group = "liability" if account_type in ("credit", "loan") else "asset"
            seen_types[account_type] = group

    account_types = [
        {"name": atype, "group": group, "__typename": "AccountType"}
        for atype, group in seen_types.items()
    ]
    return {
        "data": {
            "snapshotsByAccountType": snapshots,
            "accountTypes": account_types,
        }
    }


_OPERATION_HANDLERS: dict[str, Any] = {
    # Real operations (validated in ADR 0002)
    "GetAccounts": _handle_get_accounts,
    "GetTransactionsList": _handle_get_transactions_list,
    "Web_GetCashFlowPage": _handle_web_get_cash_flow_page,
    "GetAggregateSnapshots": _handle_get_aggregate_snapshots,
    "GetCategories": _handle_get_categories,
    "GetJointPlanningData": _handle_get_joint_planning_data,
    "GetHouseholdTransactionTags": _handle_get_household_transaction_tags,
    "Common_UpdateTransactionMutation": _handle_common_update_transaction,
    # Epic B operations (validated in ADR 0003)
    "Web_GetUpcomingRecurringTransactionItems": _handle_web_get_upcoming_recurring,
    "GetSnapshotsByAccountType": _handle_get_snapshots_by_account_type,
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
