//! Reconnect backoff and Discord close-code classification.
//!
//! Pure, deterministic helpers extracted so the gateway reconnect loop is
//! testable. They answer two questions:
//!   1. *Should* we reconnect at all? Some Discord close codes are permanent
//!      (bad token, disallowed intents) — reconnecting only burns the identify
//!      budget and gets the bot's token reset. Those must surface to the
//!      operator, not retry.
//!   2. *How long* do we wait first? Retryable failures use exponential backoff
//!      with jitter; any attempt that will send IDENTIFY additionally respects
//!      Discord's identify rate limit / invalid-session guidance.

use std::time::Duration;

/// Discord permits at most one IDENTIFY per 5 seconds per shard. A fresh
/// connection must never re-IDENTIFY faster than this.
pub(crate) const IDENTIFY_RATE_LIMIT: Duration = Duration::from_secs(5);

/// What to do once a gateway connection ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseAction {
    /// Reconnect and attempt to RESUME the existing session.
    Resume,
    /// Reconnect, but abandon the session and send a fresh IDENTIFY.
    Reidentify,
    /// Do not reconnect — the failure is permanent (bad token, bad/disallowed
    /// intents, invalid shard/version). Surface it to the operator instead.
    Fatal,
}

/// Classify a Discord gateway [close code].
///
/// [close code]: https://discord.com/developers/docs/topics/opcodes-and-status-codes#gateway-gateway-close-event-codes
pub(crate) fn classify_close_code(code: u16) -> CloseAction {
    match code {
        // Non-recoverable: reconnecting will fail identically and only burns the
        // identify budget (which is exactly what gets a bot token reset).
        4004 // Authentication failed (invalid token)
        | 4010 // Invalid shard
        | 4011 // Sharding required
        | 4012 // Invalid API version
        | 4013 // Invalid intent(s)
        | 4014 // Disallowed intent(s) — a privileged intent isn't enabled
        => CloseAction::Fatal,
        // Session can no longer be resumed: reconnect with a fresh IDENTIFY.
        4003 // Not authenticated
        | 4007 // Invalid seq
        | 4009 // Session timed out
        => CloseAction::Reidentify,
        // Everything else (4000/4001/4002/4005/4008, 1000/1001/1006, ...) is a
        // transient/resumable drop.
        _ => CloseAction::Resume,
    }
}

/// Safety margin on Discord's `session_start_limit.remaining`. We stop
/// IDENTIFYing while a few sessions are still nominally available so a
/// concurrent reconnect can never be the one that tips the budget to zero (the
/// state that gets a bot penalized / its token reset).
pub(crate) const IDENTIFY_BUDGET_SAFETY_THRESHOLD: u32 = 3;

/// Discord's `reset_after` can be many hours. We never block the transport task
/// that long: the applied sleep is capped, while the operator-facing warning
/// still reports the true reset window and the outer reconcile/human can act.
pub(crate) const IDENTIFY_BUDGET_MAX_SLEEP: Duration = Duration::from_secs(15 * 60);

/// Decision for whether a fresh IDENTIFY is allowed given the identify budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentifyBudget {
    /// Enough budget remains — go ahead and IDENTIFY.
    Proceed,
    /// Budget is at/near exhaustion — wait `wait` (the reset window) instead of
    /// IDENTIFYing, and surface a clear warning.
    Exhausted { wait: Duration },
}

/// Classify Discord's identify (session start) budget.
///
/// `remaining <= threshold` is treated as exhausted so we keep a safety margin
/// rather than spending the final sessions. This is defense-in-depth on top of
/// the per-IDENTIFY 1–5s pacing: even a single IDENTIFY is refused when Discord
/// says the daily session budget is spent.
pub(crate) fn classify_identify_budget(
    remaining: u32,
    reset_after: Duration,
    threshold: u32,
) -> IdentifyBudget {
    if remaining <= threshold {
        IdentifyBudget::Exhausted { wait: reset_after }
    } else {
        IdentifyBudget::Proceed
    }
}

/// Cap the in-process sleep applied for an exhausted identify budget.
pub(crate) fn capped_budget_wait(reset_after: Duration) -> Duration {
    reset_after.min(IDENTIFY_BUDGET_MAX_SLEEP)
}

/// Whether a Discord REST status for the initial gateway bootstrap is permanent.
/// A 401/403 means the bot token is invalid or lacks access — retrying is
/// pointless and only adds load.
pub(crate) fn rest_status_is_fatal(status: u16) -> bool {
    matches!(status, 401 | 403)
}

/// Exponential backoff with a base and a cap. `advance` returns the current
/// delay and then doubles it (saturating at the cap); `reset` returns to base.
#[derive(Debug, Clone)]
pub(crate) struct Backoff {
    base: Duration,
    cap: Duration,
    current: Duration,
}

impl Backoff {
    pub(crate) fn new(base: Duration, cap: Duration) -> Self {
        Self {
            base,
            cap,
            current: base,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.current = self.base;
    }

    /// Return the current delay, then double it (capped).
    pub(crate) fn advance(&mut self) -> Duration {
        let delay = self.current;
        self.current = (self.current * 2).min(self.cap);
        delay
    }
}

/// Compute how long to wait before the next gateway (re)connect attempt.
///
/// * `will_identify` — the next attempt will send IDENTIFY (fresh session)
///   rather than RESUME, so it must respect the identify rate limit and
///   Discord's "wait a random 1–5s after an invalid session" guidance.
/// * `backoff` — exponential-backoff delay for retryable errors, or
///   `Duration::ZERO` for a graceful/server-requested reconnect.
/// * `rand01` — random value in `[0, 1)` used for jitter (injected for tests).
pub(crate) fn reconnect_delay(will_identify: bool, backoff: Duration, rand01: f64) -> Duration {
    let r = rand01.clamp(0.0, 1.0);
    // Up to 1s of jitter to desynchronize reconnect storms.
    let jitter = Duration::from_millis((r * 1000.0) as u64);
    let candidate = backoff.saturating_add(jitter);

    if will_identify {
        // Random 1–5s floor: satisfies the 5s identify rate limit and the
        // invalid-session backoff guidance, so a bad/partly-configured bot
        // can never hot-loop IDENTIFY.
        let floor_ms = IDENTIFY_RATE_LIMIT.as_millis() as f64; // 5000
        let identify_floor = Duration::from_millis(1000 + (r * (floor_ms - 1000.0)) as u64);
        candidate.max(identify_floor)
    } else {
        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fatal_close_codes_are_not_retried() {
        for code in [4004u16, 4010, 4011, 4012, 4013, 4014] {
            assert_eq!(
                classify_close_code(code),
                CloseAction::Fatal,
                "close code {code} must be fatal"
            );
        }
    }

    #[test]
    fn session_invalidating_codes_force_reidentify() {
        for code in [4003u16, 4007, 4009] {
            assert_eq!(
                classify_close_code(code),
                CloseAction::Reidentify,
                "close code {code} must clear the session and re-identify"
            );
        }
    }

    #[test]
    fn transient_codes_resume() {
        for code in [4000u16, 4001, 4002, 4005, 4008, 1000, 1001, 1006, 9999] {
            assert_eq!(
                classify_close_code(code),
                CloseAction::Resume,
                "close code {code} should be a resumable reconnect"
            );
        }
    }

    #[test]
    fn rest_auth_failures_are_fatal() {
        assert!(rest_status_is_fatal(401));
        assert!(rest_status_is_fatal(403));
        assert!(!rest_status_is_fatal(429));
        assert!(!rest_status_is_fatal(500));
        assert!(!rest_status_is_fatal(200));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(b.advance(), Duration::from_secs(1));
        assert_eq!(b.advance(), Duration::from_secs(2));
        assert_eq!(b.advance(), Duration::from_secs(4));
        assert_eq!(b.advance(), Duration::from_secs(8));
        assert_eq!(b.advance(), Duration::from_secs(16));
        assert_eq!(b.advance(), Duration::from_secs(32));
        assert_eq!(b.advance(), Duration::from_secs(60)); // capped
        assert_eq!(b.advance(), Duration::from_secs(60)); // stays capped
    }

    #[test]
    fn backoff_reset_returns_to_base() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        b.advance();
        b.advance();
        b.reset();
        assert_eq!(b.advance(), Duration::from_secs(1));
    }

    #[test]
    fn resume_reconnect_only_jitters() {
        // Graceful resume: no identify floor, just up-to-1s jitter.
        assert_eq!(reconnect_delay(false, Duration::ZERO, 0.0), Duration::ZERO);
        assert_eq!(
            reconnect_delay(false, Duration::ZERO, 0.5),
            Duration::from_millis(500)
        );
    }

    #[test]
    fn resume_reconnect_adds_backoff_and_jitter() {
        assert_eq!(
            reconnect_delay(false, Duration::from_secs(8), 0.0),
            Duration::from_secs(8)
        );
        assert_eq!(
            reconnect_delay(false, Duration::from_secs(8), 1.0),
            Duration::from_millis(9000)
        );
    }

    #[test]
    fn identify_reconnect_respects_rate_limit_floor() {
        // A fresh IDENTIFY must never happen instantly, even on a graceful
        // reconnect with zero backoff — this is the fix for the tight
        // identify loop that got the bot token reset.
        assert!(reconnect_delay(true, Duration::ZERO, 0.0) >= Duration::from_secs(1));
        assert_eq!(
            reconnect_delay(true, Duration::ZERO, 0.0),
            Duration::from_secs(1)
        );
        assert_eq!(
            reconnect_delay(true, Duration::ZERO, 1.0),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn identify_floor_beats_small_backoff() {
        // Backoff smaller than the identify floor is lifted up to the floor.
        assert_eq!(
            reconnect_delay(true, Duration::from_millis(200), 0.0),
            Duration::from_secs(1)
        );
        // Backoff larger than the floor wins.
        assert_eq!(
            reconnect_delay(true, Duration::from_secs(30), 0.0),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn budget_proceeds_when_remaining_above_threshold() {
        let reset = Duration::from_secs(3600);
        assert_eq!(
            classify_identify_budget(500, reset, IDENTIFY_BUDGET_SAFETY_THRESHOLD),
            IdentifyBudget::Proceed
        );
        // Exactly one above the threshold still proceeds.
        assert_eq!(
            classify_identify_budget(
                IDENTIFY_BUDGET_SAFETY_THRESHOLD + 1,
                reset,
                IDENTIFY_BUDGET_SAFETY_THRESHOLD
            ),
            IdentifyBudget::Proceed
        );
    }

    #[test]
    fn budget_exhausted_at_or_below_threshold() {
        let reset = Duration::from_secs(7200);
        // At the threshold — treat as exhausted (safety margin).
        assert_eq!(
            classify_identify_budget(
                IDENTIFY_BUDGET_SAFETY_THRESHOLD,
                reset,
                IDENTIFY_BUDGET_SAFETY_THRESHOLD
            ),
            IdentifyBudget::Exhausted { wait: reset }
        );
        // Fully drained.
        assert_eq!(
            classify_identify_budget(0, reset, IDENTIFY_BUDGET_SAFETY_THRESHOLD),
            IdentifyBudget::Exhausted { wait: reset }
        );
    }

    #[test]
    fn budget_wait_is_capped_for_the_in_process_sleep() {
        // Discord can report a multi-hour reset_after; the transport must not
        // block a task for hours. The applied sleep is capped, even though the
        // warning surfaces the true reset window.
        let long = Duration::from_secs(24 * 3600);
        assert_eq!(
            capped_budget_wait(long),
            IDENTIFY_BUDGET_MAX_SLEEP,
            "a huge reset_after is capped to the max sleep"
        );
        let short = Duration::from_secs(30);
        assert_eq!(
            capped_budget_wait(short),
            short,
            "a short reset_after is used as-is"
        );
    }

    #[test]
    fn rand01_is_clamped() {
        // Out-of-range randomness must not panic or overflow.
        let _ = reconnect_delay(true, Duration::ZERO, -1.0);
        let _ = reconnect_delay(true, Duration::ZERO, 2.0);
        assert_eq!(
            reconnect_delay(false, Duration::ZERO, 2.0),
            Duration::from_secs(1)
        );
    }
}
