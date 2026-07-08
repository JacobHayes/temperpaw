# Proof Report: Settings — per-provider cards for every category

## Date
2026-07-08

## Branch / Commit
- Branch: `jrh/settings-cards` (base: `origin/jrh/codex-signin-ux`)
- Remote: origin = git@github.com:JacobHayes/temperpaw.git (GitHub)
- File: `dashboard/src/routes/settings/+page.svelte`

## What Was Done
Revamped the Settings page so **every** category renders per-provider cards, not
just Messaging (Discord/Slack) and the OpenAI Codex login. Previously LLM, Web
Search, Sandbox, Integrations, Observability and Custom were flat lists of
variable rows.

Introduced a **provider registry** — `resolveProvider(key)` — that maps each
secret key (by prefix or exact match) to `{ id, name, category }`. Keys are then
grouped into per-provider cards (`groupedProviders` derived), and cards are
grouped by category in a fixed order. A generic `providerCard` snippet renders
name + status dot + rows + inline feedback slot; the three providers that need
extra controls (Discord/Slack Connect, Codex device-code sign-in) keep their
bespoke rendering but now read their field rows from the card's grouped rows.

### Provider mapping chosen
| Category | Providers (card name → matched keys) |
|----------|--------------------------------------|
| LLM | Anthropic (`anthropic_*`), OpenAI (`openai_api_key`), OpenAI Codex (`openai_codex*`, bespoke sign-in), OpenRouter (`openrouter_*`), Hugging Face (`huggingface_*`, `hf_token`), Fireworks (`fireworks_*`), Sakana (`sakana*`), OpenAI-Compatible (`openai_compatible*`), Local (Ollama) (`local_openai*`), Active Model (`llm_provider`, `llm_model`) |
| Web Search | Exa (`exa_*`) |
| Sandbox | Modal (`modal_*`), TensorLake (`tensorlake_*`), Active Provider (`sandbox_provider`) |
| Messaging | Discord (`discord_*`, bespoke), Slack (`slack_*`, bespoke) |
| Integrations | GitHub (`github_*`) |
| Observability | Datadog (`dd_*`) |
| Custom / fallback | "Other" card collects any key with no known provider |

Ordering is specific-before-general so `openai_codex*` and `openai_compatible*`
match before the bare `openai_api_key`. The `category` returned by the registry
lets keys that arrive as `custom` (e.g. `dd_*`, which are not in the backend
schema) be re-homed into the correct section. Anything unmatched lands in an
"Other" card within its own category, so **nothing disappears from the page**.

### Design decisions to note
- `tensorlake_api_key` is categorized under **Sandbox** (its backend schema
  category in `crates/temperpaw/src/setup_api.rs`), not Integrations. The task
  text listed "Integrations → GitHub, Tensorlake" but the backend schema is the
  source of truth and puts it in Sandbox; I followed the schema so the row lands
  where the backend classifies it.
- The managed OpenAI Codex token rows (`openai_codex_access_token`, etc.) now
  render **inside** the Codex card rather than loose in the LLM list.
- Provider status dot is connection-aware for Discord/Slack/Codex (uses
  `discord_connected` / `slack_connected` / codex `configured`) and otherwise
  lights when any of the provider's keys is set.
- Bonus: the Discord card hint now states that **Message Content Intent** must be
  enabled in Discord Developer Portal → Bot (the Gateway requires it).

Preserved all prior behavior: field bindings, save/delete endpoints, Discord
Connect/Disconnect + interaction-URL copy block, Slack Connect/Disconnect, Codex
device-code auto-poll flow (`runCodexPoll`, background timer, `$effect` resume),
the inline `fbSlot`/`varRow` snippet system and per-slot feedback, the Add
Variable form, and the Account section. Removed the now-unused
`.provider-card--inline` CSS. No UI library added.

## Verification Flow
- `npm install` in `dashboard/`.
- `npm run check` (svelte-kit sync + svelte-check).
- `npm run build` (vite + adapter-static).
- Ran `vite dev` on :5199 with a Node mock backend on :3467 (Vite proxies
  `/paw` + `/auth` to it) seeded with a mix of set/unset keys across every
  category, plus a not-in-schema `dd_*` key and a truly-unknown custom key.
- Drove the page with agent-browser: loaded /settings (full-page screenshot),
  clicked Slack "Connect" with no tokens (inline validation error), and saved a
  value on `openai_api_key` (inline success + dot turns on).

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| svelte-check | 0 errors / 0 warnings | 0 errors / 0 warnings (346 files) | PASS |
| vite build | builds clean | built in ~2.0s, static site written | PASS |
| All categories as cards | every category shows per-provider cards, all keys present | LLM(10), Web Search(1), Sandbox(3), Messaging(2), Integrations(1), Observability(1), Custom "Other"(1) all rendered | PASS |
| dd_* re-homed | Datadog card under Observability | `dd_api_key` shown under Observability → Datadog | PASS |
| Unknown key fallback | "Other" card, not lost | `my_custom_thing` shown under Custom → Other | PASS |
| Slack Connect w/o tokens | inline error in Slack card | "Set slack_app_token and slack_bot_token first" inline | PASS |
| Save a variable | inline success under row, dot turns on | "openai_api_key saved" inline; OpenAI card dot green | PASS |

## What Worked
- Every category is now a set of consistent, digestible provider cards matching
  the existing mono/terminal aesthetic.
- All existing bindings, connect/save/delete endpoints, interaction-URL copy,
  Codex device-code flow, and setup-status logic preserved.

## What Didn't Work
- N/A.

## Limitations
- The dashboard has no automated test framework (only `svelte-check`); none was
  added. Behavioral verification done via `vite dev` + mock-backend harness and
  agent-browser, because the live backend in this environment lacks a fully
  seeded setup.
- Not deployed to Railway/Genesis (dashboard-only UX change; per task, push
  branch only, no PR).

## Artifacts
- `.proofs/assets/2026-07-08-settings-cards/01-all-categories-cards.png` — full page: every category as provider cards.
- `.proofs/assets/2026-07-08-settings-cards/02-slack-inline-error.png` — inline error under Slack card.
- `.proofs/assets/2026-07-08-settings-cards/03-openai-save-success.png` — inline success + lit dot on OpenAI card.

## Architecture Diagram
```text
Settings page
 └─ resolveProvider(key) ─► { id, name, category }        (provider registry)
 └─ groupedProviders (derived)
     category → [ProviderCardData{ id, name, special, rows[] }, ...]
       ├─ llm       → Anthropic, OpenAI, OpenAI Codex*, OpenRouter, Hugging Face,
       │              Fireworks, Sakana, OpenAI-Compatible, Local (Ollama), Active Model
       ├─ web_search→ Exa
       ├─ sandbox   → Modal, TensorLake, Active Provider
       ├─ messaging → Discord*, Slack*
       ├─ integrations → GitHub
       ├─ observability → Datadog        (dd_* re-homed from custom)
       └─ custom    → Other              (unmatched keys, nothing lost)
   *bespoke rendering: Connect/Disconnect or device-code sign-in
 render: providerCard snippet | discord/slack/codex bespoke blocks
 feedback: Record<slot, {type,message}>  ──►  showFeedback / clearFeedback
```
