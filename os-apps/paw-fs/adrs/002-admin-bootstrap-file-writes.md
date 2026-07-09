# ADR-002: Admin Bootstrap File Writes

## Status
Accepted

## Context
TemperPaw setup and startup bootstrap create and update File entities while installing default agent souls and saving the personalized Paw soul. These calls are authenticated as Admin OData requests. Soul policies already permit Admins to create/publish/update Soul entities, but PawFS File policy only permitted workspace/system agents to create/update File content.

That mismatch left Paw Agents without attached Soul content when startup tried to create `Paw.soul.md`: `POST /tdata/Files` was denied by Cedar, and the later setup save path failed with `Paw Soul entity not found`.

## Decision
Permit Admin principals to create File metadata and upload File content for setup/bootstrap paths:

- `create`
- `Create`
- `update`
- `StreamUpdated`

The setup save path also creates and attaches a missing Paw Soul instead of assuming startup already created one.

## Consequences
Fresh or partially bootstrapped tenants can recover through the setup UI without manual database repair. Admin File authority is intentionally scoped to authenticated Admin principals; broad read/list and system-agent hot-path policies remain unchanged.
