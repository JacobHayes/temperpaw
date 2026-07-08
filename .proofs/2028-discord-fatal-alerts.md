# Proof Report: 2028 — Discord Fatal-Transport Human Alert + session_start_limit Enforcement

## Date
2026-07-08

## Branch / Commit
- Branch: `jrh/discord-fatal-alerts` (based on `origin/jrh/discord-gateway-backoff`)
- Repo/remote: `temperpaw` → `origin` = `git@github.com:JacobHayes/temperpaw.git` (GitHub)
- Commits:
  - `f61f2dd0` discord: parse and enforce session_start_limit (TDD)
  - `e6fce889` channels: proactive human alert on fatal transport failure (TDD)
  - (this proof + ADR commit)

## What Was Done

### 1. Proactive human alert on fatal transport failure (entity-first)
Fatal, non-retryable transport failures already land `TransportConnection` in
`Failed` (via `StartFailed`) and surface passively on the dashboard
(`TransportStatus::Error`). This adds the proactive human-channel push:

- `StartFailed` gains a `trigger` effect for a new WASM integration,
  `transport_fatal_alert`, so the alert reacts to the `Failed` transition —
  no polling, no Rust orchestration.
- `os-apps/paw-channels/wasm/transport_fatal_alert/` classifies `last_error`
  into a `FatalKind`, builds an actionable message, and calls Discord REST
  (`POST /channels/{feed}/messages`) directly with `discord_bot_token` +
  `discord_feed_channel_id` injected via `[integration.config]` secrets.
- Dedupe/rate-limit via pure `should_send`: a changed failure signature alerts
  immediately; the same signature re-alerts at most once per 30 min. Dedupe
  state persists via an `AlertRecorded` `Failed → Failed` self-loop.
- 4004 vs 4014 handled explicitly (see below).
- Wired: entity fields/action (`transport_connection.ioa.toml`, `model.csdl.xml`),
  Cedar permits (`AlertRecorded`, `http_call` module allowlist), `app.toml`
  and `build.sh` registration.
- Rationale captured in ADR-006.

### 2. Parse + enforce `session_start_limit`
- `GatewayBotResponse` now parses `session_start_limit`
  (`total`/`remaining`/`reset_after`/`max_concurrency`); optional + defaulted so
  parsing never regresses.
- Pure `classify_identify_budget` (backoff.rs): `remaining <= threshold`
  (threshold = 3, a safety margin) ⇒ `Exhausted { wait: reset_after }`, else
  `Proceed`. `capped_budget_wait` caps the in-process sleep at 15 min (the true
  reset window is still logged).
- `fetch_gateway_url` → `fetch_gateway_bot` returns the full response; the
  gateway bootstrap now waits (capped) with a clear warning instead of
  IDENTIFYing when the identify budget is at/near exhaustion. Defense-in-depth
  on top of the existing per-IDENTIFY 1–5 s pacing.

## 4004 (bad token) vs 4014 (disallowed intents)
| | 4014 disallowed intents | 4004 bad token |
|---|---|---|
| Bot token valid? | Yes | No (Discord reset/rejected it) |
| Discord REST alive? | Yes → REST alert **delivers** to feed channel | No → REST alert **cannot** deliver |
| `FatalKind` | `DisallowedIntents` | `BadToken` |
| Alert guidance | "Enable the Message Content privileged intent in the Developer Portal, then reconnect." | "Token invalid — this Discord alert may not have landed; check the dashboard transport status and re-enter a valid token." |
| Discord-independent surface | dashboard `Failed` state (base branch) | dashboard `Failed` state (base branch) — the primary surface here |
| REST post failure handling | n/a (succeeds) | logged, non-fatal to the integration; `alert_status = "undeliverable"` |

## session_start_limit behavior
- `remaining > 3` → `Proceed` (IDENTIFY normally).
- `remaining <= 3` → wait `min(reset_after, 15m)` before IDENTIFY, log a warning
  with `remaining`, `total`, true `reset_after_ms`, and the applied wait.
- Missing `session_start_limit` in the response → tolerated, no wait.

## Verification Flow (Red-Green TDD)
1. `types.rs`: added serde tests referencing `session_start_limit` → compile
   error `no field session_start_limit` (red) → added `SessionStartLimit` +
   field → green.
2. `backoff.rs`: added budget-classification tests referencing
   `classify_identify_budget` / `IdentifyBudget` / `capped_budget_wait` →
   `cannot find` errors (red) → implemented → green.
3. `transport_fatal_alert/src/lib.rs`: wrote 10 tests + `unimplemented!()`
   stubs → 10 FAILED (red) → implemented pure logic → 10 passed (green).

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| `cargo test -p paw-transport --lib discord::types` (pre-impl) | compile fail | `E0609 no field session_start_limit` | PASS (red) |
| `cargo test -p paw-transport --lib discord::types` (post) | pass | 6 passed | PASS |
| `cargo test -p paw-transport --lib discord::backoff` (pre) | compile fail | `E0425 cannot find classify_identify_budget` | PASS (red) |
| `cargo test -p paw-transport --lib discord::backoff` (post) | pass | 14 passed | PASS |
| `cargo test` in `transport_fatal_alert` (pre-impl) | fail | 10 FAILED (`not implemented`) | PASS (red) |
| `cargo test` in `transport_fatal_alert` (post) | pass | 10 passed | PASS |
| `cargo test -p paw-transport` | all pass | 52 passed; 0 failed | PASS |
| `cargo clippy -p paw-transport` | clean | no warnings/errors | PASS |
| `cargo check -p temperpaw` (consumer) | compiles | Finished | PASS |
| `transport_fatal_alert` wasm32-unknown-unknown build | builds | 193–198 KB artifact | PASS |
| `os-apps/paw-channels/wasm/build.sh` | all modules build | 5 modules incl. transport_fatal_alert | PASS |

## What Worked
- Fatal transition → proactive alert is fully entity-first (WASM on `StartFailed`
  effect); no Rust orchestration added.
- All decision logic is pure and unit-tested; the sole I/O is the WASM REST call.
- Dedupe bounds re-alerts to a 30 min trickle per signature.

## What Didn't Work / Limitations
- **No live Discord verification here** (no bot token / gateway / feed channel).
  Behavior is proven by unit tests of the pure logic + guest wasm build + review
  of the entity wiring; the REST post and the state-machine effect wiring are not
  integration-tested against a live gateway/Discord.
- Alert targets the configured **feed channel**, not a per-operator DM (no
  operator user-id secret exists today). See ADR-006.
- `session_start_limit` is enforced at gateway **bootstrap** (the loop fetches
  `/gateway/bot` once). In-loop re-IDENTIFY budget tracking was intentionally not
  added; the per-IDENTIFY 1–5 s pacing already bounds in-loop identify rate.

## What A Human Must Verify In Production
1. **4014 path (repro of original report):** connect a bot **without** the
   Message Content intent → expect close 4014 → `TransportConnection` reaches
   `Failed`, and a proactive alert appears in the Discord **feed channel** with
   the "enable Message Content intent" guidance. Confirm via OData that the entity
   went `Starting → Failed → AlertRecorded` with `alert_status = "delivered"`.
2. **4004 path:** connect a bot with a **bad token** → `Failed` on the dashboard;
   the feed-channel REST alert will NOT deliver (token dead) → confirm
   `alert_status = "undeliverable"` and that the human still sees the failure on
   the dashboard. Confirm no reconnect storm in Datadog.
3. **Dedupe:** re-trigger `Start` on a still-broken bot several times within
   30 min → confirm only ONE feed-channel alert (subsequent transitions record
   `alert_status = "suppressed"`), then a second alert after 30 min.
4. **session_start_limit:** (hard to force) confirm via Datadog logs the
   "identify (session start) budget is near exhaustion" warning path is reachable;
   verify a healthy bot with ample `remaining` connects without the extra wait.
5. Publish paw-channels to **Genesis**, verify the installed pinned ref includes
   the new `transport_fatal_alert` module, and confirm live on Railway.

## Artifacts
- `crates/paw-transport/src/discord/types.rs` (session_start_limit parse + tests)
- `crates/paw-transport/src/discord/backoff.rs` (budget classifier + tests)
- `crates/paw-transport/src/discord/gateway.rs` (`fetch_gateway_bot`)
- `crates/paw-transport/src/discord/transport.rs` (budget enforcement at bootstrap)
- `os-apps/paw-channels/wasm/transport_fatal_alert/` (new module + 10 tests)
- `os-apps/paw-channels/specs/transport_connection.ioa.toml` (effect, action, fields, integration)
- `os-apps/paw-channels/specs/model.csdl.xml` (AlertRecorded + properties)
- `os-apps/paw-channels/policies/channels.cedar` (permits)
- `os-apps/paw-channels/app.toml`, `os-apps/paw-channels/wasm/build.sh`
- `os-apps/paw-channels/adrs/006-fatal-transport-human-alert.md`

## Architecture Diagram
```text
Discord gateway/REST fatal (4004 / 4010-4014 / 401-403)
  └─ DiscordTransport::run() returns Err  (paw-transport)
       └─ transport_manager → TransportStatus::Error
            └─ /paw/internal/transports/discord/start  → retryable=false
                 └─ transport_reconcile WASM → StartFailed
                      │  (TransportConnection: Starting → Failed)
                      └─ effect: trigger transport_fatal_alert
                           ├─ classify_fatal(last_error) → FatalKind
                           ├─ should_send(now, last_alert_at, sig, cooldown=30m)?
                           │      no  → AlertRecorded(status="suppressed")
                           │      yes → POST discord.com/.../channels/{feed}/messages
                           │             (Bot token from [integration.config])
                           │             4014 → delivered | 4004 → undeliverable (dashboard)
                           └─ AlertRecorded(last_alert_at, signature, status)
                                (TransportConnection: Failed → Failed)

session_start_limit (defense-in-depth, paw-transport):
  fetch_gateway_bot() → session_start_limit
    └─ classify_identify_budget(remaining, reset_after, threshold=3)
         Exhausted ⇒ warn + sleep(capped 15m) before first IDENTIFY
         Proceed   ⇒ IDENTIFY (still 1-5s paced by reconnect_delay)
```
