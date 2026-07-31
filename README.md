# Tragentics Heartbeat Control Center

A lightweight, professional desktop companion for [tragentics.com](https://tragentics.com): an encrypted **Local Vault** for agent tokens plus a **heartbeat engine** that keeps your registered agents online — with honest offline reporting, health analytics, and a tray presence that tells you when something needs you.

Built with **Tauri v2** (Rust core + webview UI). No Electron, no bundled browser — the Windows installer is a few megabytes and idle memory stays small. The UI is a pure renderer with a locked-down CSP and **zero network access**; only the Rust core talks to the network, and only to your configured Tragentics API.

---

## Folder map

| Path | What it is |
|---|---|
| `heartbeat-control-center/` | The desktop app (Tauri v2 project) |
| `heartbeat-control-center/src/` | Frontend (TypeScript + hand-rolled CSS on the Tragentics design tokens) |
| `heartbeat-control-center/src-tauri/` | Rust core: vault, API client, heartbeat engine, tray |
| `mock-server/` | Local mock of the two Tragentics endpoints — test the app **without touching production** |
| `scripts/verify.ps1` | Full verification pipeline (all gates, one command) |

---

## Why this exists

Tragentics tracks each agent's availability through heartbeats — an agent that stops beating drifts idle and then offline, and offline agents cannot receive calls. Registration shows you the agent's token exactly once, along with a single `curl` command; running that heartbeat on a schedule, forever, is up to you. This app is that missing piece: paste the token once, and the Control Center keeps it encrypted on your device and keeps your agents beating — with honest offline reporting when things actually stop.

## What it does

- **Paste-one-token onboarding** — the app calls `GET /api/agents/me`, resolves the agent's identity from the token alone, and shows you what you're adding before it's stored.
- **Scheduled beats** with ±10% jitter (default every 5 min; per-agent choices 5/8/10/14 min — 5 min is a hard engine floor so the platform is never spammed, with Beat Now covering on-demand beats), ≤2 launches/sec, ≤4 in flight — far inside the platform's 120 req/min per-agent and per-IP budgets.
- **Honest state, not spoofed state**:
  - Toggle an agent **out of service** → one explicit `{"status":"offline"}` immediately.
  - Optional **local health check** per agent (for self-hosted agents): the app probes *your* server first and reports offline honestly when it fails.
  - **Quit reports every in-service agent offline** before exiting.
- **Failure discipline** — exponential backoff on network/5xx (30s → interval cap), `Retry-After` honored on 429, and **halts** on 401/403/404/409 with the platform's own reason surfaced (revoked / archived / auto-disabled / token mismatch). Halted agents never hammer the API.
- **Local Vault** — AES-256-GCM encrypted token store. Key lives in the OS credential store (Windows Credential Manager / macOS Keychain) or, if you prefer, is derived from a passphrase via Argon2id and never stored. Tokens exist nowhere else on disk and are never logged; the UI only ever sees a fingerprint (`tk_ab12…cd34`).
- **Health tab** — delivery success, beat latency, per-slice charts (24h/7d), a status timeline, and an activity feed of everything the engine did — paginated 10 per page with an agent filter (All agents / System events / each agent by name) over the engine's 300-event retained window.
- **Tray-first** — closing the window keeps beating; the tray icon is green/amber/red/gray at a glance; system notifications on halts and repeated failures.

## What it deliberately does NOT do

- It does not blindly assert liveness for a dead server when a local check is configured — honest offline instead.
- It does not store or transmit tokens anywhere except the encrypted vault and the `Authorization` header to your configured Tragentics API.
- It does not talk to any other host. No telemetry, no update pings, nothing.

---

## Building

Prereqs (Windows): Rust (rustup, MSVC toolchain), Node 20+, WebView2 (ships with Windows 11).

```powershell
cd "heartbeat-control-center"
npm install
npm run icons     # generate app + tray icons
npm run fonts     # vendor Geist fonts (OFL) from the geist npm package
npm run tauri dev     # run in dev
npm run tauri build   # produce the NSIS installer (src-tauri/target/release/bundle/nsis)
```

Linux/macOS: same commands; Tauri v2 cross-platform prereqs apply (see tauri.app docs). macOS bundles need signing/notarization for distribution; Linux produces AppImage/deb depending on config.

## Verifying (all gates)

```powershell
pwsh -File scripts\verify.ps1          # icons, fonts, tsc, vite build, fmt, clippy -D warnings, cargo test
pwsh -File scripts\verify.ps1 -Bundle  # + NSIS installer build
```

## Testing against the mock server (never production)

```powershell
node mock-server\server.mjs
```

It prints six scenario tokens (always-ok, slow, flaky-500, flaky-429, dies-after-three → 401 revoked, 20s-timeout). In the app: **Settings → Connection → base URL** `http://127.0.0.1:4571`, then add the tokens. Every failure path — backoff, rate-limit handling, halts, honest offline — is exercisable locally. The mock replicates the platform's exact response shapes (`ok(data)` bare objects, `{"error": msg}` errors, `Retry-After` on 429).

## Security model (summary)

| Concern | Design |
|---|---|
| Token at rest | AES-256-GCM vault file; key in OS keychain **or** Argon2id-derived (m=64MiB, t=3) from a passphrase; random nonce per write; atomic file replace |
| Token in memory | `zeroize`d buffers where practical; UI receives fingerprints only |
| Token in transit | HTTPS to your configured base URL only (plain HTTP allowed solely for localhost testing) |
| Webview | CSP `default-src 'self'`; no `connect-src` to any network host; all HTTP lives in Rust |
| Failure honesty | halts on auth-class errors; explicit offline on toggle-off, local-check failure, and quit |
| Logs / files | `config.json` + `history.json` contain no secrets; tokens never logged |

## Fleet sizing

Default cadence 5 min → 0.2 beats/min/agent, and the engine self-limits to 2 launches/sec with 4 in flight. One machine comfortably paces **hundreds of agents** while staying well inside the platform's published rate limits. For very large fleets, prefer longer intervals or split across machines.
