# Proof Report: 062 — Discord Gateway Interaction Delivery

## Date

2026-07-07

## Branch / Commit

- jj change: `qmmtvlux`
- branch/bookmark target: `jrh/discord-gateway-interactions`
- base: `main@upstream` / `main@origin` `38e93529`

## What Was Done

- Added Discord interaction delivery mode selection via `discord_interaction_delivery` / `DISCORD_INTERACTION_DELIVERY`.
- Preserved the existing signed Interaction URL webhook path.
- Added Gateway `INTERACTION_CREATE` handling that reuses the existing slash-command/component logic and acknowledges interactions through Discord's REST callback endpoint.
- Updated setup API, dashboard API/types, connections/settings UX, setup guide, and ADR-0062.

## Verification Flow

1. Red tests added first for delivery-mode parsing and setup-secret reconnect behavior.
2. Focused Rust tests run after implementation.
3. Full package tests run for `paw-transport` and `temperpaw`.
4. Dashboard dependencies installed with `npm ci`; Svelte check run.
5. Local server boot smoke run with a temporary HOME/database after building local OS-app WASM artifacts.

## Verification Results

| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| `cargo test -p paw-transport discord_interaction_delivery -- --nocapture` | Delivery mode parsing passes | 1 passed | Pass |
| `cargo test -p paw-transport gateway_interaction_delivery -- --nocapture` | Gateway mode does not require public URL | 1 passed | Pass |
| `cargo test -p temperpaw discord_interaction_delivery -- --nocapture` | Setup schema exposes delivery mode | 1 passed | Pass |
| `cargo test -p temperpaw discord_secret_update_reconnects_when_interaction_delivery_changes -- --nocapture` | Saving delivery mode schedules Discord reconnect params | 1 passed | Pass |
| `cargo test -p paw-transport` | Transport package tests pass | 38 passed | Pass |
| `cargo test -p temperpaw` | TemperPaw package/tests pass | 75 unit + integration suites passed | Pass |
| `cd dashboard && npm ci && npm run check` | Dashboard type/Svelte check passes | 0 errors, 0 warnings | Pass |
| `cargo run -p temperpaw -- --help` | Binary builds and CLI responds | Help rendered | Pass |
| Local boot smoke, temp HOME/database | Server reaches readiness | `/readyz` returned `{"status":"ready","healthz":"/healthz","discord":{"status":"disconnected","configured":false,"connected":false}}`; startup banner rendered API/dashboard URLs | Pass |

## What Worked

- Gateway and webhook interaction modes are explicit and validated.
- Gateway mode avoids the public URL requirement in `TransportManager::connect_discord`.
- Existing webhook handling remains available at `/discord/interaction` and the local `/interaction` listener.
- Dashboard surfaces the user choice and mode-specific Discord Developer Portal guidance.

## What Didn't Work

- Initial local `/readyz` smoke could not complete before dev setup because required os-app WASM artifacts were absent (`paw-agent`, `paw-channels`, `paw-fs`, etc.). After running `make setup` and the OS-app WASM build scripts, the same smoke reached readiness.

## Limitations

- No live Discord credentialed E2E was run in this environment, so real Discord Gateway delivery was not exercised against Discord's production API.
- Gateway-mode users must clear the Discord Developer Portal Interactions Endpoint URL; Discord's delivery mode remains mutually exclusive on the Discord side.

## What Still Doesn't Work

- No known local readiness blocker after building the required OS-app WASM artifacts and using the local Turso store (`TEMPER_EVENT_STORE=turso`, `TEMPER_PLATFORM_STORE=turso`, `TEMPER_QUERY_PROJECTION_STORE=turso`).

## Artifacts

- ADR: `docs/adrs/0062-discord-gateway-interactions.md`
- Setup guide update: `os-apps/paw-agent/system/skills/setup-guide/SKILL.md`
- Dashboard check output: `svelte-check found 0 errors and 0 warnings`

## Architecture Diagram

```text
Discord app
  ├─ webhook mode: POST /discord/interaction ──> TemperPaw public route ──> 127.0.0.1:3488/interaction
  └─ gateway mode: Gateway INTERACTION_CREATE ──> DiscordTransport
                                                   │
                                                   ├─ process existing slash/button logic
                                                   └─ POST /interactions/{id}/{token}/callback to Discord
```
