# Proof Report: 081 — Codex Subscription Sign-in UX

## Date
2026-07-07

## Branch / Commit
- Repo: temperpaw (origin = git@github.com:JacobHayes/temperpaw.git)
- Branch: `jrh/codex-signin-ux`
- Commits: see `git log` on the branch (dashboard settings + proof).

## What Was Done
Fixed three UX bugs in the OpenAI Codex subscription device-code sign-in flow on
the Settings page (`dashboard/src/routes/settings/+page.svelte`). Scope kept
strictly to the Codex flow to avoid conflicts with a parallel settings-layout
refactor.

1. **Loading indicator (bug #1).** The device-code request (`POST
   /paw/setup/openai-codex/device-login`) takes ~1s+ before the code appears.
   The Codex button now disables and shows a spinner + "Requesting code…" while
   the request is in flight. Errors from that request are surfaced via the toast
   (existing `showFeedback('error', …)`).

2. **Background auto-poll (bug #2a).** Once the device code is shown, a
   time-bounded background poll starts automatically and calls
   `POST /paw/setup/openai-codex/poll` every 5s (the interval the backend entity
   advertises via `poll_interval_ms=5000`). It stops on success, on `Failed`
   status, when the device code `expires_at_ms` passes, or after a 15-minute wall
   clock cap. Polls never overlap (in-flight guard). The manual **Check** button
   still works as a fallback (shared `runCodexPoll` core). Cleanup runs on
   component destroy (`onDestroy`) and the poll auto-resumes on mount if a code
   is already pending (`$effect` on `codexAwaitingCode`).

3. **Clear device-code block on completion (bug #2b / #3).** The block was gated
   on `codexStatus.user_code`, which the entity keeps set even after reaching
   `Ready`, so it lingered on screen. It is now gated on a derived
   `codexAwaitingCode` (`user_code` present AND not `configured` AND status in
   `Starting|DeviceCodeReady|Polling`). On completion the block disappears and
   the connected state (green dot + Disconnect) shows.

## Verification Flow
No test infra exists in `dashboard/` (no vitest/jest, no test scripts in
`package.json`) — per task instructions this is noted rather than bolting on a
new framework. Verification was done via type-check, build, and a live
browser walk-through against a mock backend that reproduces the real endpoint
contract (1.5s device-login latency; poll returns pending twice then
configured).

- `npm run check` (svelte-check): 0 errors, 0 warnings, 346 files.
- `npm run build` (vite/adapter-static): success.
- Live run: `vite dev` on :5199 proxying `/paw` to a mock backend on :3467,
  driven with agent-browser. Mock backend logged exactly **3** `POST /poll`
  requests (background poll fired 3× at 5s), then the flow completed.

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| Click "Sign in" | Button disables, shows "Requesting code…" spinner | Button showed `Codex Requesting code…` [disabled] | PASS |
| Device code arrives | Block shows code, link, auto-poll status line | Showed `WXYZ-9876`, verification link, "checking automatically every 5s" | PASS |
| Wait (no manual click) | Background poll checks every 5s | Mock logged 3 POST /poll at 5s cadence | PASS |
| Flow completes | Device-code block clears; connected state shows | Block gone; button = `Codex Disconnect` with green dot | PASS |
| `check` / `build` | Pass | 0 errors; build ok | PASS |

## What Worked
- Loading state, auto-poll, and completion-clear all confirmed in a real browser.
- Manual Check button retained as fallback; disabled while a poll is in flight.

## What Didn't Work
- N/A

## Limitations
- Verified against a mock backend (no live Codex OAuth credentials in this
  environment). The mock mirrors the documented endpoint contract and entity
  states from `os-apps/paw-agent/specs/openai_codex_auth.ioa.toml`.
- `poll_interval_ms` is not surfaced in the backend's `OpenAICodexAuthStatus`
  response struct, so the client uses a 5s default (matching the entity's
  `poll_interval_ms` initial value). If the backend later surfaces it, the
  client should prefer it — noted for design review.

## What Still Doesn't Work
- Nothing identified within scope.

## Artifacts
- `.proofs/081-codex-signin-ux/01-initial.png` — Settings, Codex "Sign in".
- `.proofs/081-codex-signin-ux/02-requesting.png` — "Requesting code…" spinner.
- `.proofs/081-codex-signin-ux/03-devicecode.png` — device code + auto-poll line.
- `.proofs/081-codex-signin-ux/04-connected.png` — connected; block cleared.

## Architecture Diagram
```text
Settings +page.svelte (Codex device-code flow)
  click Sign in ──▶ codexStarting=true (spinner) ──▶ POST device-login (~1.5s)
       └─ on user_code ──▶ startCodexBackgroundPoll()
                              │ setInterval 5s, deadline = min(expires_at_ms, now+15m)
                              ▼
                         codexBackgroundTick ──(no overlap)──▶ runCodexPoll(false)
                              │                                   POST /poll
                              ├─ configured ─▶ stop, load(), "connected"  (block clears)
                              ├─ Failed ─────▶ stop, show last_error
                              └─ past deadline ▶ stop, "code expired"
  Manual "Check" ──▶ runCodexPoll(true)   (shared core, fallback)
  onDestroy ──▶ stopCodexBackgroundPoll()
  $effect(codexAwaitingCode) ──▶ auto-resume poll on mount if code pending
```
