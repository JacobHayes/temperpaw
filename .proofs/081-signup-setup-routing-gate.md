# Proof Report: 081 — First-signup routes to setup screen

## Date
2026-07-07

## Branch / Commit
- Repo: TemperPaw (`origin` = git@github.com:JacobHayes/temperpaw.git)
- Branch: `jrh/setup-routing`
- Base: origin/main @ 38e93529

## What Was Done
Fixed a routing/onboarding bug where the `/welcome` setup screen was not shown on
first app access after account creation, but appeared after any full page reload.

Root cause: the "setup incomplete -> redirect to /welcome" decision lived only in
the root `+layout.svelte` `onMount`, which runs once per hard page load. With
`ssr = false` (SPA), the root layout is not re-mounted during client-side
navigation, so after signup/login the login page's `goto('/')` never re-ran the
gate — the user landed on the canvas. A hard reload re-mounted the layout, re-ran
`onMount`, and only then redirected to `/welcome`.

Changes (routing/gating only):
- `dashboard/src/lib/api.ts`: added `isSetupIncomplete(status)` — single source of
  truth for the gate criteria (`!has_anthropic_key || !has_agents ||
  !has_personalized_soul`; Discord intentionally optional).
- `dashboard/src/routes/login/+page.svelte`: after successful register/login,
  consult `fetchSetupStatus()` and navigate to `/welcome` when setup is incomplete,
  else `/`. Failures fall through to the dashboard.
- `dashboard/src/routes/+layout.svelte`: refactored `onMount` to reuse
  `isSetupIncomplete` (behavior preserved).

No settings-page markup/CSS touched (a separate branch owns that restructure).

## Verification Flow
1. `npm install`, `npm run check`, `npm run build` in `dashboard/`.
2. Mock backend on :3467 serving `/auth/*` and `/paw/*` (setup incomplete by
   default, toggleable to complete). Vite dev on :5173 (proxies /auth,/paw ->
   :3467). Driven with agent-browser.
3. Walked signup -> setup, reload -> setup, and setup-complete -> free navigation.

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| `npm run check` | 0 errors | 346 files, 0 errors, 0 warnings | PASS |
| `npm run build` | clean build | built + adapter-static wrote site | PASS |
| Signup (client-side nav), setup incomplete | land on `/welcome` | URL `/dashboard/welcome`, "Setup" + "0 of 4 steps complete" | PASS |
| Direct/reload nav to `/` (canvas), setup incomplete | redirect to `/welcome` | URL `/dashboard/welcome` | PASS |
| Setup complete, nav to `/settings` | stays on settings | URL `/dashboard/settings` | PASS |
| Setup complete, nav to `/` | stays on canvas | URL `/dashboard/` | PASS |

## What Worked
- Signup now consistently lands on the setup screen when setup is incomplete,
  matching the post-reload behavior.
- Deliberate in-app navigation is not hijacked once setup is complete.

## What Didn't Work
- N/A

## Limitations
- Verified against a mock backend (no live Rust server / real vault in this
  worktree). The backend `/paw/setup/status` contract is unchanged; only frontend
  routing changed.
- Red-green TDD: the dashboard has no test harness (only `check`/`build`; no
  vitest/lint). Adding a runner was out of scope for this routing fix, so the gate
  logic was extracted into the pure, testable `isSetupIncomplete` helper and
  verified via the browser walkthrough above. Judgement recorded here per CLAUDE.md.

## What Still Doesn't Work
- N/A for this scope.

## Artifacts
- Screenshot: signup -> setup screen (`proof-signup-welcome.png`, scratchpad).

## Architecture Diagram
```text
BEFORE (buggy):
  hard load  -> layout onMount -> getCurrentUser -> fetchSetupStatus -> /welcome  (gate runs)
  signup     -> login submit -> register -> goto('/')                             (gate NEVER runs;
               (layout already mounted on /login; onMount does not re-run)         lands on canvas)

AFTER (fixed):
  hard load  -> layout onMount -> isSetupIncomplete? -> /welcome
  signup     -> login submit -> register -> isSetupIncomplete? -> /welcome | /
  in-app nav (setup complete) -> no gate -> destination as clicked
```
