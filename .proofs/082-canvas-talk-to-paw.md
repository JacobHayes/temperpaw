# Proof Report: 082 — Canvas Talk to Paw Link

## Date

2026-07-09

## Branch / Commit

- Local branch: `jrh/canvas-talk-to-paw`
- Local jj change: `rqumwmmy`
- Deployment target: `temperpaw-vm:/opt/temperpaw/current`
- Functional deploy old commit: `28054bcbdb45`
- Functional deploy code commit: `56219c6fc4dc`
- Server backup branch from functional deploy: `deploy-backup/20260709152316-28054bcbdb45`

Note: jj commit hashes are content-addressed and changed when this proof file was added. The final VM deploy was re-run from the same jj change after adding the proof, and the post-deploy verification checks the live server commit directly.

## What Was Done

Fixed the Canvas empty-state `Talk to Paw` control so it opens the existing Paw chat panel instead of navigating completed setup users back to `/welcome`.

Changed:

- `dashboard/src/routes/+page.svelte`
  - Removed the hardcoded `{base}/welcome` anchor.
  - Imported `openPanel` from `$lib/stores/paw-chat`.
  - Rendered `Talk to Paw` as a button with `onclick={openPanel}`.
- `crates/temperpaw/tests/session_lifecycle_and_config.rs`
  - Added a regression test covering the Canvas empty-state behavior.

No ADR: this is a small dashboard interaction bug fix, not an architecture, entity, policy, storage, trigger, or agent capability change.

## Verification Flow

1. Inspected live server source at `temperpaw-vm`.
2. Added the regression test before changing implementation.
3. Ran the targeted test and confirmed it failed red.
4. Implemented the Canvas button behavior.
5. Re-ran targeted and focused local checks.
6. Built the dashboard locally.
7. Deployed to `temperpaw-vm` using the direct VM deployment path.
8. Verified server health/readiness and checked the deployed source/built asset.

## Verification Results

| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| Live source inspection | Existing behavior is unconditional, not setup-state dependent | `dashboard/src/routes/+page.svelte:232` had `<a href="{base}/welcome" class="empty-link">Talk to Paw</a>` on `temperpaw-vm` | Pass |
| Red test | New regression test fails before fix | `canvas_empty_state_talk_to_paw_opens_chat_panel` failed with `Canvas page should import the Paw chat panel opener` | Pass |
| Targeted green test | Regression test passes after fix | `cargo test -p temperpaw canvas_empty_state_talk_to_paw_opens_chat_panel` passed | Pass |
| Focused Rust test file | Existing source-contract tests remain green | `cargo test -p temperpaw --test session_lifecycle_and_config` passed: 12 tests | Pass |
| Svelte check | No dashboard type/check diagnostics | `cd dashboard && npm run check` reported 0 errors and 0 warnings | Pass |
| Dashboard production build | Build succeeds | `cd dashboard && npm run build` succeeded | Pass |
| VM deployment | Server builds, restarts, and becomes healthy | Functional deploy moved VM from `28054bcbdb45` to `56219c6fc4dc`; `systemctl is-active temperpaw.service` returned `active`; `/healthz` 200; `/readyz` 200 | Pass |
| Deployed source check | Source uses button/openPanel and no old welcome link | `dashboard/src/routes/+page.svelte` has `<button type="button" class="empty-link" onclick={openPanel}>Talk to Paw</button>`; old Talk-to-Paw welcome link absent | Pass |
| Deployed built asset check | Built dashboard includes the button | `dashboard/build/_app/immutable/nodes/2.CDsR9wLb.js` contains `button type="button" class="empty-link ...">Talk to Paw</button>` | Pass |
| Post-deploy logs | No recent error/panic markers | Deploy command tailed recent `temperpaw.service` logs for `ERROR|panic|Soul generation failed|OpenAI API error`; no matches printed | Pass |

## What Worked

- The bug was a hardcoded empty-state link, not a setup-state issue.
- Existing Paw chat store already exposed `openPanel`, so the fix stayed inside dashboard primitives.
- VM deployment built the dashboard and restarted cleanly.

## What Didn't Work

- `/paw/setup/status` on the VM returned `401` from localhost without an authenticated session, so setup status was not used as evidence.

## Limitations

- I did not use a browser-authenticated live session to click the button in the UI because that would require user dashboard credentials/cookies. The deployed source and production asset were inspected instead, and server health/readiness were verified.

## What Still Doesn't Work

- Nothing known for this specific issue.

## Artifacts

- Functional deploy code commit: `56219c6fc4dc`
- VM backup branch: `deploy-backup/20260709152316-28054bcbdb45`
- Final deploy source check: `git rev-parse --short=12 HEAD` on `temperpaw-vm:/opt/temperpaw/current`

## Architecture Diagram

```text
Canvas empty state
       |
       | click "Talk to Paw"
       v
openPanel() from $lib/stores/paw-chat
       |
       v
Global <PawPanel /> already mounted by +layout.svelte
```
