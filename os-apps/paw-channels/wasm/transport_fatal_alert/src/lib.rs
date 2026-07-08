//! Proactive human alert for fatal transport failures.
//!
//! Entity-first: this WASM integration reacts to a `TransportConnection`
//! entering `Failed` (via `StartFailed`, which the reconcile loop dispatches for
//! non-retryable errors — bad token, disallowed intents, invalid shard/version).
//! It pushes a proactive alert to the human Discord channel via Discord's REST
//! API (independent of the dead gateway), then records dedupe state so recurring
//! failures re-alert at a bounded rate instead of once per attempt.
//!
//! Discord-independent surface: the dashboard already shows the `Failed` state
//! and `last_error` (from the base gateway-backoff fix). For a 4004 bad-token
//! failure the REST alert below cannot be delivered either, so the alert message
//! explicitly points the human at that dashboard.
//!
//! The only non-testable part is the HTTP call; all decisions (classification,
//! deliverability, dedupe, message text) are pure functions with unit tests.

use temper_wasm_sdk::prelude::*;

/// Category of fatal transport failure, derived from the entity's `last_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FatalKind {
    /// 4004 / REST 401-403 / token missing — the bot token is invalid or absent.
    BadToken,
    /// 4014 — a privileged intent (e.g. Message Content) is not enabled.
    DisallowedIntents,
    /// 4013 — an invalid intent value was sent.
    InvalidIntents,
    /// 4010 / 4011 — invalid or required sharding.
    InvalidShard,
    /// 4012 — invalid gateway API version.
    InvalidApiVersion,
    /// Any other non-retryable startup failure.
    Unknown,
}

/// Re-alert window for an unchanged failure signature. A new signature always
/// alerts immediately; the same signature only re-alerts once per window.
const ALERT_COOLDOWN_MS: i64 = 30 * 60 * 1000;

const DEFAULT_DISCORD_API_BASE: &str = "https://discord.com/api/v10";

#[unsafe(no_mangle)]
pub extern "C" fn run(_ctx_ptr: i32, _ctx_len: i32) -> i32 {
    let result = (|| -> Result<(), String> {
        let ctx = Context::from_host()?;
        let fields = ctx
            .entity_state
            .get("fields")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let error = str_field(&fields, &["last_error", "LastError", "error_message"])
            .unwrap_or("")
            .to_string();
        let attempt = num_field(&fields, &["attempt_count", "AttemptCount"]).unwrap_or(0);
        let last_alert_at = num_field(&fields, &["last_alert_at", "LastAlertAt"]);
        let last_signature =
            str_field(&fields, &["last_alert_signature", "LastAlertSignature"]).unwrap_or("");

        let kind = classify_fatal(&error);
        let signature = alert_signature(kind);
        let now = Context::get_time_millis();

        let mut alert_status = "suppressed";
        let mut new_alert_at = last_alert_at.unwrap_or(0);

        if should_send(now, last_alert_at, signature, last_signature, ALERT_COOLDOWN_MS) {
            let message = build_alert_message(kind, &error, attempt);
            match deliver_alert(&ctx, kind, &message) {
                Ok(true) => {
                    alert_status = "delivered";
                    new_alert_at = now;
                }
                Ok(false) => {
                    // No feed channel / token configured — nothing to deliver to.
                    // The dashboard Failed state remains the human-visible surface.
                    alert_status = "no_channel";
                    new_alert_at = now;
                }
                Err(err) => {
                    // A REST failure (expected for 4004 bad token) must NOT fail the
                    // integration — the dashboard still surfaces the failure. Advance
                    // the cooldown so we don't retry the doomed post every attempt.
                    ctx.log(
                        "warn",
                        &format!("transport_fatal_alert: Discord alert not delivered: {err}"),
                    );
                    alert_status = "undeliverable";
                    new_alert_at = now;
                }
            }
        }

        set_success_result(
            "AlertRecorded",
            &json!({
                "last_alert_at": new_alert_at.to_string(),
                "last_alert_signature": signature,
                "alert_status": alert_status,
            }),
        );
        Ok(())
    })();

    if let Err(error) = result {
        set_error_result(&error);
    }
    0
}

/// POST the alert to the Discord feed channel via REST. Returns `Ok(true)` when
/// posted, `Ok(false)` when no channel/token is configured to post to.
fn deliver_alert(ctx: &Context, kind: FatalKind, message: &str) -> Result<bool, String> {
    let bot_token = ctx
        .config
        .get("discord_bot_token")
        .map(String::as_str)
        .filter(|v| !v.is_empty() && !v.starts_with("{secret:"))
        .unwrap_or("");
    let channel_id = ctx
        .config
        .get("discord_feed_channel_id")
        .map(String::as_str)
        .filter(|v| !v.is_empty() && !v.starts_with("{secret:"))
        .unwrap_or("");
    if bot_token.is_empty() || channel_id.is_empty() {
        return Ok(false);
    }

    let api_base = ctx
        .config
        .get("discord_api_base")
        .map(String::as_str)
        .filter(|v| !v.is_empty() && !v.starts_with("{secret:"))
        .unwrap_or(DEFAULT_DISCORD_API_BASE)
        .trim_end_matches('/');

    if !discord_rest_alive(kind) {
        ctx.log(
            "warn",
            "transport_fatal_alert: bad token — Discord REST likely dead; \
             attempting alert anyway, dashboard is the fallback surface",
        );
    }

    let url = format!("{api_base}/channels/{channel_id}/messages");
    let headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("authorization".to_string(), format!("Bot {bot_token}")),
    ];
    let body = json!({
        "content": message,
        "embeds": [{
            "title": "Transport connection failed",
            "description": message,
            "color": 0x00ED_4245u32,
        }],
    });

    let resp = ctx.http_call("POST", &url, &headers, &body.to_string())?;
    if (200..300).contains(&resp.status) {
        Ok(true)
    } else {
        Err(format!(
            "Discord channel post failed (HTTP {}): {}",
            resp.status, resp.body
        ))
    }
}

/// Classify the fatal error string into an actionable category.
fn classify_fatal(error: &str) -> FatalKind {
    let e = error.to_ascii_lowercase();
    if e.contains("code=4004")
        || e.contains("fatal auth failure")
        || e.contains("is not configured")
        || e.contains("check the discord bot token")
    {
        FatalKind::BadToken
    } else if e.contains("code=4014") {
        FatalKind::DisallowedIntents
    } else if e.contains("code=4013") {
        FatalKind::InvalidIntents
    } else if e.contains("code=4010") || e.contains("code=4011") {
        FatalKind::InvalidShard
    } else if e.contains("code=4012") {
        FatalKind::InvalidApiVersion
    } else {
        FatalKind::Unknown
    }
}

/// Stable dedupe signature per failure category.
fn alert_signature(kind: FatalKind) -> &'static str {
    match kind {
        FatalKind::BadToken => "bad_token",
        FatalKind::DisallowedIntents => "disallowed_intents",
        FatalKind::InvalidIntents => "invalid_intents",
        FatalKind::InvalidShard => "invalid_shard",
        FatalKind::InvalidApiVersion => "invalid_api_version",
        FatalKind::Unknown => "unknown_fatal",
    }
}

/// Whether Discord REST is expected to still work for this failure kind. A 4004
/// bad token kills REST too; every other fatal gateway code leaves REST usable.
fn discord_rest_alive(kind: FatalKind) -> bool {
    !matches!(kind, FatalKind::BadToken)
}

/// Build the human-facing alert message with actionable remediation.
fn build_alert_message(kind: FatalKind, error: &str, attempt: i64) -> String {
    let guidance = match kind {
        FatalKind::BadToken => {
            "The Discord bot token is invalid or missing. Discord reset or rejected it. \
             Because the token is dead this Discord alert may not have been delivered — \
             check the TemperPaw dashboard settings page (transport status) and re-enter \
             a valid bot token there."
        }
        FatalKind::DisallowedIntents => {
            "A privileged Gateway Intent is disabled. Enable the Message Content intent \
             (and other required intents) under Bot > Privileged Gateway Intents in the \
             Discord Developer Portal, then reconnect."
        }
        FatalKind::InvalidIntents => {
            "The transport requested an invalid Gateway Intent value. This is a \
             configuration bug — review the intents sent at IDENTIFY."
        }
        FatalKind::InvalidShard => {
            "Discord rejected the shard configuration (invalid or sharding required). \
             Review the shard settings."
        }
        FatalKind::InvalidApiVersion => {
            "Discord rejected the gateway API version. The transport needs an update."
        }
        FatalKind::Unknown => {
            "The Discord transport hit a non-retryable startup failure and stopped. \
             Check the dashboard transport status and logs."
        }
    };
    format!(
        "\u{26A0}\u{FE0F} Discord transport stopped (fatal, not retrying) after {attempt} attempt(s).\n\n{guidance}\n\nDetails: {error}"
    )
}

/// Decide whether to push an alert now, given dedupe state.
///
/// A changed signature (new failure kind) always alerts. The same signature
/// re-alerts only once per `cooldown_ms`, so a bot that keeps being re-Started
/// while still broken produces a bounded trickle of alerts, not one per attempt.
fn should_send(
    now_ms: i64,
    last_alert_at_ms: Option<i64>,
    current_sig: &str,
    last_sig: &str,
    cooldown_ms: i64,
) -> bool {
    if current_sig != last_sig {
        return true;
    }
    match last_alert_at_ms {
        None => true,
        Some(last) => now_ms - last >= cooldown_ms,
    }
}

fn str_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn num_field(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_gateway_close_codes() {
        assert_eq!(
            classify_fatal("Discord gateway closed with a fatal, non-retryable code: code=4004 reason=Authentication failed."),
            FatalKind::BadToken
        );
        assert_eq!(
            classify_fatal("... code=4014 reason=Disallowed intent(s)."),
            FatalKind::DisallowedIntents
        );
        assert_eq!(classify_fatal("... code=4013 ..."), FatalKind::InvalidIntents);
        assert_eq!(classify_fatal("... code=4010 ..."), FatalKind::InvalidShard);
        assert_eq!(classify_fatal("... code=4011 ..."), FatalKind::InvalidShard);
        assert_eq!(
            classify_fatal("... code=4012 ..."),
            FatalKind::InvalidApiVersion
        );
    }

    #[test]
    fn classifies_rest_and_config_failures_as_bad_token() {
        assert_eq!(
            classify_fatal(
                "Gateway bot endpoint returned 401 Unauthorized (fatal auth failure): ... Check the Discord bot token."
            ),
            FatalKind::BadToken
        );
        assert_eq!(
            classify_fatal("discord_bot_token is not configured"),
            FatalKind::BadToken
        );
    }

    #[test]
    fn unknown_error_is_unknown_kind() {
        assert_eq!(
            classify_fatal("some unexpected startup failure"),
            FatalKind::Unknown
        );
    }

    #[test]
    fn rest_is_dead_only_for_bad_token() {
        assert!(!discord_rest_alive(FatalKind::BadToken));
        assert!(discord_rest_alive(FatalKind::DisallowedIntents));
        assert!(discord_rest_alive(FatalKind::InvalidIntents));
        assert!(discord_rest_alive(FatalKind::InvalidShard));
        assert!(discord_rest_alive(FatalKind::InvalidApiVersion));
        assert!(discord_rest_alive(FatalKind::Unknown));
    }

    #[test]
    fn signatures_are_stable_and_distinct_per_kind() {
        assert_eq!(alert_signature(FatalKind::BadToken), "bad_token");
        assert_eq!(
            alert_signature(FatalKind::DisallowedIntents),
            "disallowed_intents"
        );
        assert_ne!(
            alert_signature(FatalKind::BadToken),
            alert_signature(FatalKind::DisallowedIntents)
        );
    }

    #[test]
    fn message_carries_actionable_guidance() {
        let intents = build_alert_message(FatalKind::DisallowedIntents, "code=4014", 2);
        assert!(intents.contains("Message Content"));
        assert!(intents.to_lowercase().contains("developer portal"));

        let token = build_alert_message(FatalKind::BadToken, "code=4004", 1);
        assert!(token.to_lowercase().contains("token"));
        assert!(token.to_lowercase().contains("dashboard"));
    }

    #[test]
    fn should_send_on_new_signature() {
        assert!(should_send(
            1_000,
            Some(900),
            "disallowed_intents",
            "bad_token",
            ALERT_COOLDOWN_MS
        ));
    }

    #[test]
    fn should_send_on_first_alert() {
        assert!(should_send(1_000, None, "bad_token", "", ALERT_COOLDOWN_MS));
    }

    #[test]
    fn suppresses_same_signature_within_cooldown() {
        let last = 1_000_000;
        assert!(!should_send(
            last + ALERT_COOLDOWN_MS - 1,
            Some(last),
            "bad_token",
            "bad_token",
            ALERT_COOLDOWN_MS
        ));
    }

    #[test]
    fn re_alerts_same_signature_after_cooldown() {
        let last = 1_000_000;
        assert!(should_send(
            last + ALERT_COOLDOWN_MS,
            Some(last),
            "bad_token",
            "bad_token",
            ALERT_COOLDOWN_MS
        ));
    }
}
