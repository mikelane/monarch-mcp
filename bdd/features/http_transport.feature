@ISSUE-88
Feature: Streamable-HTTP MCP transport
  Phase 1 of Claude Cowork support: monarch-mcp can also be started with
  --http, serving the same tool set over a loopback-only streamable-HTTP
  MCP transport instead of stdio. Everything the advisor can do over stdio
  must also work over HTTP — the transport is an implementation detail,
  not a change in capability.

  Background:
    Given the budgeting advisor is running in HTTP transport mode

  Rule: The HTTP transport serves the same tools as stdio

    Scenario: The advisor lists the full monarch tool set over HTTP
      When the advisor lists available tools over HTTP
      Then the tool list includes "financial_overview"
      And the tool list includes "apply_changeset"

  Rule: Tool calls over HTTP return the same data as stdio

    Scenario: A financial overview requested over HTTP reports net worth
      Given the household holds 100000 dollars across asset accounts
      And the household owes 30000 dollars across liability accounts
      When the advisor requests a financial overview over HTTP
      Then the HTTP overview reports a net worth of 70000 dollars

  Rule: An expired session over HTTP is reported the same way as over stdio

    Scenario: An expired session over HTTP asks the household to re-authenticate
      Given the household's Monarch session has expired
      When the advisor requests a financial overview over HTTP
      Then the advisor reports that re-authentication is required

  Rule: Requests carrying a cross-site Origin header are rejected

    Scenario: A request from a malicious webpage origin is rejected
      When a request to the HTTP transport carries Origin "https://evil.example"
      Then the HTTP transport responds with status 403

    Scenario: A request whose origin only prefixes localhost is rejected
      When a request to the HTTP transport carries Origin "http://localhost.evil.example"
      Then the HTTP transport responds with status 403

    Scenario: A request with a malformed port is rejected
      When a request to the HTTP transport carries Origin "http://localhost:8770evil.example"
      Then the HTTP transport responds with status 403

    Scenario: A request with trailing bytes after IPv6 bracket is rejected
      When a request to the HTTP transport carries Origin "http://[::1]evil.example"
      Then the HTTP transport responds with status 403

    Scenario: A request carrying a loopback Origin is allowed through
      # Positive control: proves the guard's exact-host allow path lets a real
      # loopback browser origin reach the MCP service (not 403). Without this,
      # a bug that rejected every present Origin would still pass every 403
      # scenario above.
      When a request to the HTTP transport carries Origin "http://127.0.0.1"
      Then the HTTP transport does not respond with status 403
