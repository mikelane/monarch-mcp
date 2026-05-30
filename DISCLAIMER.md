# Disclaimer

**monarch-mcp is an independent, unofficial project.** It is not affiliated with, endorsed by,
sponsored by, or supported by Monarch Money or Monarch Money, Inc. "Monarch Money" is a
trademark of its respective owner; it is used here only to describe interoperability.

## No official API

Monarch Money does not offer a public API. This software communicates with Monarch's
**private, undocumented GraphQL API** — the same one Monarch's own web app uses — by
reverse-engineered convention. Consequences you accept by using it:

- **It can break at any time.** Monarch can change or remove endpoints, fields, or auth
  without notice. When that happens, tools will error until the project is updated.
- **It may be against Monarch's Terms of Service.** Automated/programmatic access to Monarch
  may violate their ToS. **You are solely responsible** for ensuring your use complies with
  any agreements you have with Monarch. The authors take no responsibility for account
  suspension or any other consequence.

## No warranty

This software is provided "AS IS", without warranty of any kind, express or implied. See the
[MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses. It is **not financial advice**.
Output may be inaccurate, incomplete, or out of date — verify anything that matters against
Monarch directly before acting on it.

## Your data

This tool reads sensitive financial data. It is **read + categorize only** and contains no
code to move money. Your Monarch session token is stored locally
(`~/.config/monarch-mcp/session.json`); you are responsible for the security of the machine it
runs on and of any MCP client you connect it to. See [SECURITY.md](SECURITY.md).
