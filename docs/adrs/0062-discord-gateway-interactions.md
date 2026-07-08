# ADR-0062: Discord Gateway Interaction Delivery

## Status

Accepted.

## Context

TemperPaw already receives Discord DM messages over the Discord Gateway WebSocket, but slash commands and component/button interactions were wired through Discord's Interactions Endpoint URL. That HTTP webhook path requires a publicly reachable `/discord/interaction` endpoint. Private deployments reachable only through Tailscale or private networking can open outbound connections to Discord but cannot receive Discord's inbound webhook POSTs.

Discord supports two mutually exclusive interaction delivery modes for an application: outgoing webhook delivery to an Interactions Endpoint URL, or `INTERACTION_CREATE` events over the Gateway connection.

## Decision

TemperPaw supports both Discord interaction delivery modes behind explicit user configuration:

- `discord_interaction_delivery=gateway` / `DISCORD_INTERACTION_DELIVERY=gateway` receives slash commands and components from Gateway `INTERACTION_CREATE` events and acknowledges them through Discord's interaction callback REST endpoint.
- `discord_interaction_delivery=webhook` / `DISCORD_INTERACTION_DELIVERY=webhook` preserves the existing signed Interactions Endpoint URL flow through `/discord/interaction`.

The default remains `webhook` for compatibility with existing deployments. The dashboard exposes the choice during Discord connection and renders guidance for the selected mode. Gateway mode does not require `PUBLIC_BASE_URL` or ngrok; webhook mode still requires a public Interaction URL.

The delivery mode is stored as a setup secret/config value rather than a TransportConnection state variable. The Discord transport process owns the protocol behavior; the `paw-channels` app still owns transport lifecycle and remains unchanged.

## Consequences

- Private/Tailscale-only TemperPaw nodes can use Discord slash commands and buttons without exposing an inbound endpoint.
- Existing public Interaction URL deployments continue to work unchanged.
- Operators choosing Gateway mode must clear the Interactions Endpoint URL in the Discord Developer Portal; Discord will otherwise continue sending interactions to the webhook endpoint.
- Gateway-mode interactions share the existing Gateway failure domain. Missed interactions are not replayed the way missed DM messages can be caught up from Discord REST.
