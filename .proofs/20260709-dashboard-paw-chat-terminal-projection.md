# Proof Report: 20260709 — Dashboard Paw Chat Terminal Projection

## Date
2026-07-09

## Scope
Second stacked fix for the canvas/Paw panel Talk to Paw flow.

## What Was Done
The Paw panel now waits briefly for the terminal OData read-model projection after a terminal SSE event before finalizing the assistant bubble. This prevents the UI from freezing a completed message with empty content when `Session.result` is not visible to OData yet.

Root cause: live Session `ss-019f4793-341c-7233-877d-4e0b91275b1b` completed with a non-empty backend result for `hey!`, but the browser rendered an empty assistant box. Logs showed the browser fetching the Session immediately after `RecordResult`/`Completed`; durable state later showed the non-empty result. The failure was therefore a UI/read-model timing race, not a provider failure.

No ADR was added: this is a dashboard read-path stabilization for existing Session semantics and does not change architecture.

## Why This Is a Bounded Wait, Not a Provider Retry
The dashboard is not retrying the OpenAI/Codex call, not redispatching a Session action, and not mutating state. It is re-reading the already-completed Session for up to a short bounded window because the SSE terminal event and the OData read projection are not guaranteed to become visible at exactly the same instant.

A platform-level alternative would be to include terminal result fields in the SSE event, make terminal SSE delivery wait for query projection durability, or have the dashboard read a strongly consistent actor/entity snapshot. Those would remove the dashboard-side wait, but they are broader platform changes.

## Verification Results
| Step | Expected | Actual | Status |
|------|----------|--------|--------|
| User report check | `hey!` Session should have backend result | `ss-019f4793-341c-7233-877d-4e0b91275b1b`: `Completed`, result `Hey — I’m here...` | PASS |
| Focused dashboard test | Completed Session with initially empty result is re-read until result is projected | `cd dashboard && node --test tests/paw-chat-terminal.test.mjs` passed 2/2 | PASS |
| Dashboard typecheck | No Svelte/TS diagnostics | `cd dashboard && npm run check` found 0 errors/warnings | PASS |
| Dashboard build | Production dashboard builds | `cd dashboard && npm run build` completed | PASS |
| Live post-fix smoke | New `hey!` Session should complete with visible backend result | `ss-019f4799-15d8-7aa2-8244-736b90a51a88`: `Completed`, `result_len=152` | PASS |
| Deploy | VM runs the fixed dashboard and is healthy | final stack deployed to `temperpaw-vm`; `healthz=200`, `readyz=200`, service active | PASS |

## Artifacts
- Changed files in this fix:
  - `dashboard/src/lib/stores/paw-chat.ts`
  - `dashboard/src/lib/stores/paw-chat-terminal.ts`
  - `dashboard/tests/paw-chat-terminal.test.mjs`
- Live completed user Session after config fix:
  - `ss-019f4793-341c-7233-877d-4e0b91275b1b` (`hey!`, non-empty result)
- Live post-fix smoke Session:
  - `ss-019f4799-15d8-7aa2-8244-736b90a51a88`

## What Still Doesn't Work
- OTS trajectory emission intermittently logs `database is locked`/503. This does not block Paw chat completion, but it is a separate observability-storage issue.
