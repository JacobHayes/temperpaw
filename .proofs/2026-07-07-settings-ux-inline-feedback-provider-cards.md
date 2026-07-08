# Proof Report: Settings UX — inline feedback + per-provider cards

## Date
2026-07-07

## Branch / Commit
- Branch: `jrh/settings-ux` (origin = git@github.com:JacobHayes/temperpaw.git)
- File: `dashboard/src/routes/settings/+page.svelte`

## What Was Done
Fixed two Settings-page UX bugs.

1. **Global banner → inline feedback slots.** Replaced the single top-of-page
   `feedback` toast with a `Record<string, Feedback>` keyed by "slot". Every
   settings action now renders its status/error/success message adjacent to the
   control that produced it:
   - `discord` / `slack` slots under each provider card's Connect button
   - `codex` slot under the OpenAI Codex sign-in row
   - per-variable-row slots (keyed by the secret key) under each row
   - `add` slot under the Add-variable form
   - `account` slot under the password form (replaced the old `accountFeedback`)
   - `page` slot for load failures
   A shared `showFeedback(slot, type, message)` / `clearFeedback(slot)` pair
   drives them, with per-slot auto-dismiss timers for success messages.

2. **Multi-provider section split into per-provider cards.** MESSAGING is no
   longer one flat list. It now renders a `.provider-card` for **Discord** and a
   separate one for **Slack**, each with its own status dot, Connect/Disconnect
   button, fields, inline feedback slot, and (for Discord) the Interaction URL
   copy block. The OpenAI Codex sign-in was moved into the same card container
   inside the LLM group — its device-code login behavior is unchanged, only the
   container and feedback placement changed (to minimize conflicts with the
   concurrent codex-flow branch).

Reused Svelte 5 snippets (`fbSlot`, `varRow`) to avoid markup duplication.
Removed now-unused CSS (`.toast`, `.cat-actions`, `.cat-act-label`, `.inline-fb`,
`.fb-*`) and added `.slot-fb`, `.provider-card`, `.provider-head` styles that
match the existing mono/terminal aesthetic. No UI library added.

## Verification Flow
- `npm install` in `dashboard/`.
- `npm run check` (svelte-check) — baseline and post-change.
- `npm run build` (vite + adapter-static).
- Built the static site and served it together with a lightweight mock of the
  `/paw/setup/*` and `/auth/*` endpoints (Discord connected + interaction URL,
  Slack disconnected) so the real user flow could be exercised in a browser.
- Drove the page with agent-browser: loaded /settings, clicked Slack "Connect"
  (validation path), and saved a value on `openai_api_key` (success path).

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| svelte-check | 0 errors / 0 warnings | 0 errors / 0 warnings (346 files) | PASS |
| vite build | builds clean | built in ~1.9s, static site written | PASS |
| MESSAGING layout | separate Discord + Slack cards, each with own Connect + fields | rendered as two cards; interaction URL nested in Discord card | PASS |
| Slack Connect w/o tokens | error appears inline in Slack card, not page top | "Set slack_app_token and slack_bot_token first" shown inline under the Slack Connect button | PASS |
| Save a variable | success appears inline under that row | "openai_api_key saved" shown inline under the row | PASS |

## What Worked
- Both reported bugs are fixed and visually confirmed (see artifacts).
- All existing field bindings, connect/save/delete endpoints, interaction-URL
  copy, and setup-status logic preserved.

## What Didn't Work
- N/A.

## Limitations
- The dashboard has no automated test infrastructure (only `svelte-check`); no
  test framework was added. Behavioral verification was done manually via a
  built-static + mock-API harness and agent-browser, because the live backend on
  :3467 in this environment lacks the setup-secrets schema route and reports an
  empty setup (which redirects to /welcome).
- Not deployed to Railway/Genesis (dashboard-only UX change; per task, push
  branch only, no PR).

## Artifacts
- `.proofs/assets/2026-07-07-settings-ux/01-layout-provider-cards.png` — full page: Discord + Slack cards, Codex card.
- `.proofs/assets/2026-07-07-settings-ux/02-slack-inline-error.png` — inline error under Slack Connect.
- `.proofs/assets/2026-07-07-settings-ux/03-varrow-inline-success.png` — inline success under a variable row.

## Architecture Diagram
```text
Settings page
 └─ groupedVars (by category)
     ├─ messaging → [Discord provider-card]{dot, Connect, fbSlot('discord'), rows, interaction-url}
     │              [Slack   provider-card]{dot, Connect, fbSlot('slack'),   rows}
     ├─ llm       → [Codex   provider-card]{dot, Sign in, fbSlot('codex'), device-code}
     │              rows...
     └─ other     → rows... (each row renders fbSlot(row.key))
 feedback: Record<slot, {type,message}>  ──►  showFeedback / clearFeedback
```
