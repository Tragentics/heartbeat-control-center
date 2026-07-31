# Tragentics API mock server

A local stand-in for the two platform endpoints the Heartbeat Control Center talks to. It exists so the app can be tested end-to-end **without ever touching the production platform** — no real agents, no real heartbeats, no production writes of any kind.

```bash
node server.mjs        # listens on http://localhost:4571 (PORT env to change)
```

On boot it prints six freshly generated scenario tokens. In the app, set **Settings → Connection → base URL** to `http://localhost:4571`, then paste tokens into **Add Agent**:

| Agent | Behavior | What it proves |
|---|---|---|
| Mock Invoice Parser | always 200 | happy path, latency stats |
| Mock Support Router | 200 after 1.2s | slow-endpoint latency handling |
| Flaky Gateway | every 3rd beat → 500 | exponential backoff + recovery |
| Rate Limited | every 2nd beat → 429 `Retry-After: 7` | Retry-After honored (engine floors at 70s) |
| Dies After Three | 401 "Your agent has been revoked" after 3 beats | halt with platform reason, no hammering |
| Timeout Tester | heartbeat hangs 20s | client 15s timeout → network backoff |

Faithful to the real API surface:

- `GET /api/agents/me` returns the agent object **bare** (the platform's `ok(agent)` shape).
- `POST /api/agents/:id/heartbeat` returns `{ "heartbeat": "accepted", "agent": { id, status, last_heartbeat } }`.
- Errors are `{ "error": "<message>" }` with the real status codes and, for 429, a `Retry-After` header.
- Token→agent mismatch returns the platform's exact 403 message.
- Invalid status values return the platform's 400 message.

Every request is logged to stdout with its outcome so you can watch the engine behave.
