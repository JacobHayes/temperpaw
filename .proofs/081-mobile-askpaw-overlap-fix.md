# Proof Report: 081 — Mobile Ask Paw FAB overlapping bottom nav Settings

## Date
2026-07-07

## Branch / Commit
`jrh/mobile-askpaw-overlap` — dashboard/src/app.html, dashboard/src/routes/+layout.svelte

## What Was Done

Bug report: on iOS/mobile Safari, the "Ask Paw" floating icon covers part of the
bottom menu bar, mostly obscuring "Settings".

Root cause (confirmed by reproduction, see below): the mobile bottom-nav layout
(`@media (max-width: 640px)`) already contained `.paw-fab { display: none; }`,
intended to hide the floating action button on mobile because the bottom nav
already has its own "Paw" toggle item. But the unconditional base `.paw-fab { ...
display: flex; ... }` rule was declared *later* in the stylesheet's source order.
Both selectors have identical specificity (`.paw-fab`), so per the CSS cascade,
source order breaks the tie — the later rule wins regardless of which one is
inside a media query. The base rule's `display: flex` therefore silently
overrode the mobile `display: none`, leaving the FAB visible and fixed-positioned
at `bottom: var(--sp-6); right: var(--sp-6); z-index: 40`, floating directly on
top of the bottom nav (`z-index: 30`) — which visually covers the rightmost nav
item, Settings.

A second, related defect: `dashboard/src/app.html`'s viewport meta tag lacked
`viewport-fit=cover`, so on iOS Safari `env(safe-area-inset-bottom)` always
resolved to `0`. This meant the bottom nav's existing safe-area padding
(`.sidebar-nav { padding-bottom: calc(var(--sp-2) + env(safe-area-inset-bottom, 0px)); }`)
was inert on notched/home-indicator iPhones.

### Fixes
1. `dashboard/src/app.html`: added `viewport-fit=cover` to the viewport meta tag
   so `env(safe-area-inset-*)` resolves correctly on iOS Safari.
2. `dashboard/src/routes/+layout.svelte`:
   - Moved the unconditional `.paw-fab` base rule to appear *before* the
     responsive media queries, so the mobile `display: none` override
     (unchanged, same selector/specificity) correctly wins the cascade via
     source order on small viewports. Added a comment explaining why the
     ordering matters, to prevent regression.
   - Gave the FAB's `bottom` offset a safe-area-aware value
     (`calc(var(--sp-6) + env(safe-area-inset-bottom, 0px))`) for the tablet/
     landscape ranges where it remains visible near a screen edge.
   - Made `.main` / `.main--canvas` mobile `margin-bottom` safe-area-aware
     (`calc(64px + env(safe-area-inset-bottom, 0px))`) so page content isn't
     hidden under the now-taller (safe-area-padded) bottom nav on notched
     devices.

No changes were needed to the bottom nav's own safe-area handling — it already
had `env(safe-area-inset-bottom)` padding; it just wasn't taking effect due to
the missing `viewport-fit=cover`.

## Verification Flow
1. `npm install` in `dashboard/`.
2. `npm run check` (svelte-check) — 0 errors / 0 warnings.
3. `npm run build` (adapter-static) — succeeds.
4. `npm run dev` and used `agent-browser` (Chrome via CDP) at an iPhone 14
   viewport (390x844) to screenshot the dashboard.
5. Because the dev server has no backend API (auth endpoint returns 500 and
   the app redirects to `/login`), a temporary local-only bypass was added to
   `onMount` to force `authReady = true` with a mock user, purely to render the
   shell for screenshotting. This was reverted before committing (verified via
   `git diff` showing only the intended app.html + layout CSS changes).
6. Reproduced the exact reported bug by temporarily restoring the pre-fix
   `.paw-fab` rule order (base rule after the mobile media query) while
   keeping everything else fixed, screenshotted, then restored the fix.
7. Verified desktop viewport (1440x900) still shows the FAB in its normal
   bottom-right position — no regression.

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| `npm run check` | 0 errors | `COMPLETED 346 FILES 0 ERRORS 0 WARNINGS 0 FILES_WITH_PROBLEMS` | PASS |
| `npm run build` | build succeeds | `✓ built in ~2s`, static site written to `build/` | PASS |
| Mobile (390x844) before fix (repro) | FAB overlaps Settings | Confirmed: white circular FAB sits directly on top of "Settings" nav item | REPRODUCED |
| Mobile (390x844) after fix | FAB hidden, Settings fully visible | Confirmed: FAB gone, all 6 nav items incl. Settings fully visible and unobstructed | PASS |
| Desktop (1440x900) after fix | FAB visible bottom-right, unaffected | Confirmed: FAB rendered normally bottom-right, sidebar unaffected | PASS |

## What Worked
- Reproducing the bug deterministically by isolating the single CSS ordering
  change confirmed the cascade/specificity root cause (not a guess).
- `agent-browser` device emulation (`set device "iPhone 14"`) gave an accurate
  visual repro matching the user's report exactly.

## What Didn't Work
- N/A

## Limitations
- Dev-server visual verification required a temporary, non-committed auth
  bypass since there's no backend running in this environment; this was fully
  reverted (confirmed via `git diff` before commit) and does not affect the
  shipped diff.
- Did not test on a physical iOS Safari device; verification was via Chrome
  CDP device emulation (viewport + safe-area env var support), which faithfully
  reproduces the CSS layout in question.

## What Still Doesn't Work
- N/A — bug fixed and verified.

## Artifacts
- `.proofs/mobile-before-fix-bug-repro.png` — reproduction of reported bug (FAB over Settings)
- `.proofs/mobile-after-fix.png`, `.proofs/mobile-after-fix-confirm.png` — fixed mobile view
- `.proofs/desktop-after-fix.png` — desktop view, no regression
