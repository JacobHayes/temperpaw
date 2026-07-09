# Proof Report: Dashboard — "Match System Theme" Mode

## Date
2026-07-07

## Branch / Commit
`jrh/system-theme` (worktree `agent-aca68d2fc4d25c790`, repo `dashboard/`)

## What Was Done
Added a third theme mode, `system`, to the dashboard's `ThemeToggle` alongside the
existing `light` / `dark` modes.

- `dashboard/src/lib/theme.ts` (new): shared theme module — `Theme = 'light' | 'dark' | 'system'`,
  `getStoredTheme()`, `resolveTheme()` (resolves `'system'` via
  `matchMedia('(prefers-color-scheme: dark)')`), `applyTheme()`, `setTheme()`. Storage key
  unchanged: `localStorage['temperpaw-theme']`, but now stores `'light' | 'dark' | 'system'`
  explicitly.
- `dashboard/src/lib/components/ThemeToggle.svelte`: now cycles light → dark → system → light on
  click. Shows a sun/moon/monitor icon per mode plus a small accent dot when in `system` mode.
  `aria-label`/`title` state the current mode. Registers a `matchMedia` `'change'` listener while
  mounted so that, while in `system` mode, the resolved theme live-updates on OS theme change
  without a page reload (listener is a no-op when the stored mode isn't `system`, so it doesn't
  fight an explicit user choice).
- `dashboard/src/app.html`: removed the static `data-theme="dark"` attribute on `<html>` and
  removed the theme decision from `ThemeToggle`'s post-hydration `$effect` as the sole source of
  truth. Added a synchronous inline `<script>` in `<head>` (before `%sveltekit.head%`) that reads
  `localStorage['temperpaw-theme']`, resolves `system` via `matchMedia`, and sets
  `data-theme` on `<html>` before first paint — preventing flash-of-wrong-theme. Falls back to
  `dark` if `localStorage`/`matchMedia` throw (e.g. privacy mode).
- Default: a user who has never chosen a theme has no stored key → treated as `system`. A user who
  previously picked `light`/`dark` (old 2-state toggle, or new explicit pick) keeps that exact
  stored value; picking `system` explicitly is also persisted as `'system'` so it's sticky across
  reloads and distinguishable from "never chose."

## Verification Flow
1. `npm install` in `dashboard/` — clean install, no errors.
2. `npm run check` (svelte-kit sync + svelte-check) — 347 files, 0 errors, 0 warnings.
3. `npm run build` (vite build + adapter-static) — builds clean, writes to `build/`.
4. Ran `vite dev` and drove the app with `agent-browser` (browser automation). Because the
   dashboard route guard in `+layout.svelte` redirects unauthenticated/un-onboarded users away
   from arbitrary routes, and there's no backend running in this sandbox, auth/setup/tdata
   network calls were stubbed via `agent-browser network route` (mock `/auth/me`,
   `/paw/setup/status`, `/tdata/**`, block `/observe/events/stream`) so the app would mount the
   real authenticated shell (including the real `ThemeToggle`, unmodified). A temporary
   `dashboard/src/routes/dev-theme-test/+page.svelte` harness was added to mount `ThemeToggle` in
   isolation; it was deleted before committing (confirmed absent via `git status`/`git diff --stat`
   showing only the three intended files changed).
5. Exercised: initial default (no stored key), click-cycling through all three modes, live
   `prefers-color-scheme` change while in `system` mode (via `agent-browser set media dark|light`,
   which uses CDP media-feature emulation — no page reload), explicit `dark` choice surviving a
   simulated OS-preference flip (no-flash / correctness check), and the "first-time user" fallback
   (no stored key + OS dark → resolves to dark, without writing anything to storage).

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| Fresh load, no stored key, OS prefers dark | `data-theme="dark"`, `localStorage` key absent | `dataTheme: "dark"`, `stored: null` | PASS |
| Click 1 (system → light) | `data-theme="light"`, stored `"light"`, label "Theme: Light..." | matched | PASS |
| Click 2 (light → dark) | `data-theme="dark"`, stored `"dark"` | matched | PASS |
| Click 3 (dark → system) | `data-theme="dark"` (OS still dark), stored `"system"` | matched | PASS |
| `set media dark→light` while in `system` mode | `data-theme` flips to `"light"` live, no reload, stored stays `"system"` | matched (no browser relaunch/reload in log) | PASS |
| `set media light→dark` while in `system` mode | `data-theme` flips to `"dark"` live | matched | PASS |
| Explicit `stored="dark"`, OS emulated to `light`, fresh navigation | `data-theme="dark"` on load (explicit choice wins over OS) | `dataTheme: "dark"` | PASS |
| Cleared stored key, OS dark, fresh navigation | `data-theme="dark"`, `stored` stays `null` (not force-written) | matched | PASS |
| `npm run check` | 0 errors/warnings | 0 errors, 0 warnings, 347 files | PASS |
| `npm run build` | builds cleanly | built in ~1.9s, static site written | PASS |

## What Worked
- Inline bootstrap script in `app.html` correctly resolves `system` before hydration using the
  same precedence rules as the runtime module (`src/lib/theme.ts`), so there is no flash even when
  the resolved theme differs from the previous static default (`dark`).
- `matchMedia('change')` listener correctly live-updates the DOM attribute while in `system` mode
  and is a no-op otherwise — verified with real CDP color-scheme emulation, not just code reading.
- Existing `[data-theme="light"]` / default (dark) CSS variable scheme in `app.css` required no
  changes — `resolveTheme()` always yields a concrete `'light' | 'dark'` for the DOM attribute.

## What Didn't Work / Notable Friction
- This sandbox had multiple concurrent `vite dev` servers from unrelated worktrees fighting over
  the default port, and `agent-browser`'s default (unnamed) session was apparently shared with
  other concurrently-running agents on the same machine, which caused a few misleading
  cross-worktree navigations during manual testing. Resolved by using `--strictPort` on a unique
  port and a dedicated named `agent-browser` session for all verification steps; final results
  above are from that isolated session only.
- The dashboard's route guard needs a logged-in, fully-onboarded user before rendering the
  authenticated shell (where `ThemeToggle` lives), and this sandbox has no backend — mocked the
  relevant endpoints via `agent-browser network route` rather than standing up a full backend.

## Limitations
- Did not verify against a real macOS/browser OS-level theme toggle (used CDP
  `Emulation.setEmulatedMedia` via `agent-browser set media`, which exercises the same
  `matchMedia` change-event path a real OS toggle would fire).
- Did not add automated/unit tests (no test runner configured in `dashboard/package.json` — only
  `check`/`build`/`dev`/`preview` scripts exist); verification here is manual/browser-driven per
  the `check`/`build` scripts available and direct DOM/localStorage inspection.

## Artifacts
- Screenshot (system mode, OS set to light): `theme-system-light.png` in the session scratchpad
  (`/private/tmp/claude-501/.../scratchpad/theme-system-light.png`) — not committed, local
  verification evidence only.

## Architecture Diagram
```text
localStorage['temperpaw-theme']  ──┐
                                    │  (read on every boot / effect)
matchMedia('prefers-color-scheme') ┘
              │
              ▼
    resolveTheme(mode) -> 'light' | 'dark'
              │
     ┌────────┴─────────┐
     ▼                   ▼
app.html inline script   ThemeToggle.svelte $effect
(pre-hydration, no       (post-hydration, sets up
 flash)                   matchMedia 'change' listener
                          for live updates while in
                          'system' mode)
              │
              ▼
   document.documentElement[data-theme]
              │
              ▼
        app.css [data-theme="light"] / default (dark) vars
```
