# ADR-006: Proactive Human Alert on Fatal Transport Failure

- Status: Accepted
- Date: 2026-07-08

## Context

The gateway-backoff fix (`.proofs/2027-discord-gateway-backoff.md`) made fatal,
non-retryable Discord failures stop the reconnect loop instead of hammering
Discord: fatal gateway close codes (4004 bad token, 4010–4014 invalid
shard/version/intents) and fatal REST auth (401/403) now surface as a
non-retryable `StartFailed`, landing `TransportConnection` in `Failed` and
showing on the dashboard settings page via `TransportStatus::Error`.

That surface is passive — a human only sees it if they open the dashboard.
TemperPaw's operating rule is that **every error must surface to the human
channel (Discord DMs)**, and recurring fatal failures in particular deserve a
proactive push. A nuance drives the design: a fatal *gateway* close such as 4014
(missing Message Content intent) does **not** invalidate the bot token — Discord
REST still works, so a proactive REST message is deliverable. For 4004 (bad
token) REST is dead too, so the alert must also live on a Discord-independent
surface.

## Decision

Add a proactive alert as a **WASM integration reacting to the state
transition**, not new Rust orchestration.

1. **Trigger on the transition, not on a poll.** `StartFailed` (the action that
   moves `Starting → Failed`) gains a `trigger` effect for a new integration,
   `transport_fatal_alert`. The alert therefore fires exactly when the entity
   enters the fatal state.

2. **Deliver via Discord REST from WASM.** The module calls Discord's
   `POST /channels/{feed}/messages` directly, using `discord_bot_token` and
   `discord_feed_channel_id` injected through `[integration.config]` secrets.
   This follows the repo's anti-pattern table ("Calling external APIs from Rust
   → WASM with secrets from `[integration.config]`") and needs **zero** new Rust:
   the gateway is dead in `Failed`, but REST is independent and (for 4014, 4013,
   4010–4012) still authorized.

3. **4004 vs 4014.** The module classifies `last_error` into a `FatalKind`.
   For `BadToken` (4004 / REST 401-403 / token missing) it knows REST is also
   dead, so the alert message explicitly directs the human to the dashboard
   transport status and to re-enter a valid token; the REST post is still
   attempted but its failure is non-fatal to the integration. For
   `DisallowedIntents` (4014) the message gives the exact remediation (enable the
   Message Content privileged intent in the Developer Portal).

4. **Dedupe / bounded re-alert.** Alerting is only-on-transition by
   construction, and `Failed` has no auto-scheduled retry, so the common case is
   one alert per failure. Because `Start` can be re-dispatched from `Failed`
   (setup reconcile, a human re-saving secrets), a pure `should_send` gate adds a
   bounded rate: a *changed* failure signature alerts immediately; the *same*
   signature re-alerts at most once per 30 minutes. Dedupe state
   (`last_alert_at`, `last_alert_signature`, `alert_status`) persists via an
   `AlertRecorded` `Failed → Failed` self-loop.

## Consequences

- Fatal transport failures now push a proactive Discord alert to the feed
  channel and remain visible on the dashboard (the Discord-independent surface),
  satisfying the human-channel mandate for both 4014 and 4004.
- The entire flow is auditable from entity state transitions:
  `Starting → StartFailed → Failed → AlertRecorded`, with `alert_status`
  recording `delivered` / `no_channel` / `undeliverable` / `suppressed`.
- No new Rust orchestration or background watcher. The only I/O is the WASM REST
  call; all decisions are pure, unit-tested functions.
- Limitation: the alert targets the configured feed channel, not a per-operator
  DM (there is no operator user-id secret today). If a direct DM is later
  desired, add an operator-id secret and open a DM channel — the classification,
  dedupe, and message logic are unchanged. The 30-minute cooldown and the
  feed-channel target are the two knobs a human should review.
