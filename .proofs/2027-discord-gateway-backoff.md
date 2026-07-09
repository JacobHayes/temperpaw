# Proof Report: 2027 — Discord Gateway Reconnect Backoff & Fatal Close-Code Handling

## Date
2026-07-07

## Branch / Commit
- Branch: `jrh/discord-gateway-backoff`
- Repo/remote: `temperpaw` → `origin` = `git@github.com:JacobHayes/temperpaw.git` (GitHub)
- Commits: see `git log` on the branch (backoff module, loop wiring, proof).

## What Was Done

### Bug report
A partially-configured Discord bot (bot token set, interaction URL not yet set,
non-public server) received Discord's automated warning: *"your bot has connected
to Discord more than 1000 times within a short time period ... we have reset your
bot's token."* Something was reconnecting / re-identifying in a tight loop with no
backoff.

### Root cause (causal story)
The Discord gateway reconnect loop lives in
`crates/paw-transport/src/discord/transport.rs` (`DiscordTransport::run`). Two
defects combined to produce an unbounded, delay-free IDENTIFY loop within a single
`run()` invocation:

1. **Zero-delay reconnect on the graceful path.** `connect_and_run` returned
   `Ok(())` whenever the gateway asked to reconnect — Opcode 7 (Reconnect) or
   Opcode 9 (Invalid Session). On the `Ok(())` branch the outer loop **reset
   backoff to 1s and immediately reconnected with no sleep at all**:
   ```rust
   match self.connect_and_run(&url).await {
       Ok(()) => backoff = Duration::from_secs(1),   // reset, then loop with NO sleep
       Err(e) => { sleep(backoff); backoff = (backoff*2).min(60s); }
   }
   ```
   Discord's protocol requires waiting a random 1–5s after an Invalid Session
   before re-identifying. Because the code re-identified instantly, a bot that
   Discord keeps invalidating (very common for a freshly-created / partly-configured
   bot, or when a RESUME fails and Discord replies Invalid Session → we clear the
   session → IDENTIFY → invalid again) enters a **hot IDENTIFY loop** — thousands
   of IDENTIFYs in seconds. That is exactly the ">1000 connections in a short
   period" Discord penalized by resetting the token.

2. **Fatal close codes were treated as retryable.** On a WebSocket Close frame the
   code returned a generic `Err(...)` regardless of the close code, so the loop
   retried forever (backoff capped at 60s). Codes 4004 (auth failed / bad token),
   4010/4011/4012 (invalid shard/version), 4013/4014 (invalid / disallowed
   intents) are **not recoverable** — reconnecting always fails identically and
   still consumes the identify budget. `intents::DEFAULT` requests the privileged
   `MESSAGE_CONTENT` intent (`1 << 15`); if it isn't enabled in the Developer
   Portal, Discord closes with 4014 and the loop hammers it.

The outer entity-driven reconcile (`transport_reconcile` WASM, 30s StartRetry) and
`connect_discord`'s 30s READY timeout bound the *outer* restart cadence, but the
*inner* loop was unbounded and delay-free — that is where the 1000+ connections
came from.

### Fix
- Extracted pure, unit-tested helpers into
  `crates/paw-transport/src/discord/backoff.rs`:
  - `classify_close_code(u16) -> CloseAction` — `Fatal` for {4004, 4010, 4011,
    4012, 4013, 4014}; `Reidentify` for {4003, 4007, 4009}; `Resume` otherwise.
  - `rest_status_is_fatal(u16)` — 401/403 (bad token / no access).
  - `Backoff { advance(), reset() }` — exponential doubling with a cap.
  - `reconnect_delay(will_identify, backoff, rand01)` — backoff + up-to-1s jitter,
    with a **1–5s floor whenever the next attempt will send IDENTIFY** (respects
    Discord's 5s identify rate limit and the invalid-session guidance). This is
    the core fix: a fresh IDENTIFY can never happen instantly.
- Rewrote the reconnect loop:
  - `connect_and_run` now returns `Ok(GatewayOutcome::{Reconnect,Reidentify,Fatal})`
    or `Err(String)` (retryable). Close frames are classified; **fatal close codes
    return `Fatal`, and the loop logs an error and returns from `run()` instead of
    reconnecting** — surfacing to `TransportStatus::Error` and the setup/reconcile
    flow (non-retryable → `StartFailed`).
  - Every reconnect now sleeps `reconnect_delay(...)` first. Backoff escalates
    across attempts and is **reset only when a session actually reached READY**
    (tracked via `session_ready`), so a stable connection that later drops
    reconnects quickly while a never-stabilizing bot escalates its backoff.
  - RESUME is preferred when a session is still held; otherwise a paced fresh
    IDENTIFY is used. Invalid-Session(resumable=false) and Reidentify close codes
    clear the session so the next attempt IDENTIFYs under the rate-limit floor.
- `fetch_gateway_url` now flags 401/403 as a fatal auth failure with a clear
  operator-facing message.

## Verification Flow
Red-green TDD:
1. Wrote `backoff.rs` tests referencing not-yet-existing functions → compile
   failure (red): `error[E0425]: cannot find function reconnect_delay` (25 errors).
2. Implemented the pure functions → tests pass (green).
3. Wired the loop; rebuilt and ran the full crate test suite + workspace consumer
   check + clippy.

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| `cargo test -p paw-transport --lib discord::backoff` (pre-impl) | fails to compile | E0425 missing functions (red) | PASS (red) |
| `cargo test -p paw-transport --lib discord::backoff` (post-impl) | 11 pass | 11 passed; 0 failed | PASS |
| `cargo build -p paw-transport` | clean | Finished, no warnings | PASS |
| `cargo test -p paw-transport` | all pass | 47 passed; 0 failed | PASS |
| `cargo check -p temperpaw` (consumer) | compiles | Finished | PASS |
| `cargo clippy -p paw-transport` | no warnings | no warnings/errors | PASS |

## What Worked
- Pure classification/backoff logic is fully unit-tested and deterministic
  (`rand01` injected).
- Fatal close codes now stop the loop and surface an error instead of hammering
  Discord.
- IDENTIFY attempts are floored at 1–5s, making a >1000-connections storm
  impossible from this loop.

## What Didn't Work / Limitations
- **No live Discord verification was possible in this environment** (no bot token,
  no gateway access). The behavioral fix is proven via unit tests of the pure
  decision logic plus code review of the loop wiring; the WebSocket loop itself is
  not integration-tested against a live/simulated gateway.
- `session_start_limit` from `/gateway/bot` is **not** parsed/enforced (only the
  per-identify 5s pacing is). Deferred — see below.

## What A Human Should Verify In Production
1. Deploy, connect a **valid** bot with `MESSAGE_CONTENT` enabled → confirm READY,
   DMs flow, and Datadog shows a single stable connection (no reconnect storm).
2. Connect a bot with a **bad token** → confirm the transport goes to
   `Error`/`StartFailed`, logs the fatal message, does **not** retry, and the error
   surfaces to the human Discord channel. Confirm Datadog shows no repeated
   `/gateway/bot` or gateway connects.
3. Connect a bot **without** the privileged `MESSAGE_CONTENT` intent (repro of the
   original report) → expect close code 4014 → transport stops with a clear
   "enable Gateway Intents" message, **no** tight reconnect loop.
4. Kill/restore network mid-session → confirm it RESUMEs (or re-IDENTIFYs after
   1–5s) and that reconnect intervals in Datadog respect the backoff/jitter.

## Ambiguous / Needs Design Review
- `intents::DEFAULT` includes the privileged `MESSAGE_CONTENT` intent. If the
  product intent is DMs-only, that privileged intent may be unnecessary and is a
  common cause of 4014. Worth a product decision (kept as-is here — not in scope).
- Whether a fatal gateway failure should additionally push a proactive Discord DM
  alert to the operator (CLAUDE.md: "every error must surface to the human
  channel"). Today it surfaces via `TransportStatus::Error` → setup/reconcile
  StartFailed. A dedicated DM alert path may be desirable but is out of scope.
- Enforcing `session_start_limit` (remaining/reset_after) from `/gateway/bot`
  would add another safety layer; deferred as "easy if incorporated" was optional.

## Artifacts
- `crates/paw-transport/src/discord/backoff.rs` (new — pure logic + 11 tests)
- `crates/paw-transport/src/discord/transport.rs` (reconnect loop rewrite)
- `crates/paw-transport/src/discord/gateway.rs` (`fetch_gateway_url` fatal auth)
- `crates/paw-transport/src/discord/mod.rs` (`mod backoff;`)

## Architecture Diagram
```text
DiscordTransport::run()   (crates/paw-transport/src/discord/transport.rs)
  │
  ├─ fetch_gateway_url ──401/403──► Fatal (return Err → TransportStatus::Error)
  │
  └─ loop:
       ├─ choose url: session? RESUME(resume_url) : IDENTIFY(base_url)
       ├─ delay = reconnect_delay(will_identify, backoff.advance(), jitter)
       │        └─ IDENTIFY ⇒ 1..5s floor  (fix: never instant)
       ├─ sleep(delay)
       ├─ connect_and_run(url, session_ready)
       │     ├─ READY            ⇒ session_ready = true
       │     ├─ op7 Reconnect    ⇒ Reconnect (RESUME)
       │     ├─ op9 InvalidSess  ⇒ resumable? Reconnect : Reidentify
       │     ├─ Close(code)      ⇒ classify_close_code:
       │     │                       4004/4010-4014 ⇒ Fatal (STOP, surface)
       │     │                       4003/4007/4009 ⇒ Reidentify
       │     │                       else           ⇒ Reconnect
       │     └─ I/O error        ⇒ Err ⇒ Retryable
       ├─ Fatal    ⇒ log error, return Err (no reconnect)
       ├─ Reidentify ⇒ clear session_id
       └─ session_ready ? backoff.reset() : (backoff keeps escalating)
```
