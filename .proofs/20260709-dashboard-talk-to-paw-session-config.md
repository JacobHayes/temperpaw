# Proof Report: 20260709 — Dashboard Talk to Paw Session Config

## Date
2026-07-09

## Scope
First stacked fix for the canvas/Paw panel Talk to Paw flow.

## What Was Done
Dashboard-created Paw chat Sessions now pass the selected Agent's runtime template to `Session.Configure`: `model`, `provider`, `provider_options_json`, `temperature`, `tools_enabled`, `max_turns`, and `soul_id`.

Root cause: the canvas/Paw panel dashboard path created a Session with only `agent_id` and `user_message`. The Session state machine requires `model` and `provider`, so `context_preparer` failed before any OpenAI/Codex provider call.

No ADR was added: this restores the existing Agent -> Session configuration contract and does not change architecture.

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| Red test | New test fails before implementation | `dashboard_session_creation_copies_agent_runtime_config` failed before the dashboard helper existed | PASS |
| Regression test | Dashboard `createSession()` copies Agent runtime config | `cargo test -p temperpaw --test session_lifecycle_and_config dashboard_session_creation_copies_agent_runtime_config` passed | PASS |
| Focused suite | Session/dashboard contract tests pass | `cargo test -p temperpaw --test session_lifecycle_and_config` passed: 13/13 | PASS |
| Dashboard typecheck | No Svelte/TS diagnostics | `cd dashboard && npm run check` found 0 errors/warnings | PASS |
| Dashboard build | Production dashboard builds | `cd dashboard && npm run build` completed | PASS |
| Root cause query | Failed user Sessions should show missing runtime config | `ss-019f4781...` and `ss-019f4782...`: `model=''`, `provider=''`, `error_message='context_preparer requires Session.model'` | PASS |
| Live smoke | New Session should carry provider/model and complete | `ss-019f478c-aa2a-7871-8690-9a2878626d25`: provider `openai_codex`, model `gpt-5.5`, completed with `dashboard session config smoke test passed.` | PASS |

## Artifacts
- Changed files in this fix:
  - `dashboard/src/lib/api.ts`
  - `crates/temperpaw/tests/session_lifecycle_and_config.rs`
- Live failed Sessions:
  - `ss-019f4781-f746-7150-9add-c50cbe5ff712`
  - `ss-019f4782-4cbe-74e2-8364-1072c4432021`
- Live passing smoke Session:
  - `ss-019f478c-aa2a-7871-8690-9a2878626d25`

## Limitations
- Browser automation could load dashboard assets with principal headers, but the SPA redirected to login because `/auth/me` requires a real dashboard login cookie. The live smoke exercised the deployed OData Session path directly.
