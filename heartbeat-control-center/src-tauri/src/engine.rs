//! Heartbeat engine core — pure state machine, no Tauri or network types in
//! the hot paths, so every transition is unit-testable.
//!
//! Cadence model (mirrors platform reality):
//!   - Beat every `interval_secs` (default 300s, clamped 300–840) with ±10% jitter.
//!     The platform marks agents idle at 15 min quiet — the max interval keeps margin;
//!     the 5-min floor keeps heartbeat volume polite (Beat Now covers on-demand needs).
//!   - Network/5xx: exponential backoff 30s·2^(n-1), capped at the interval.
//!   - 429: wait Retry-After + 5s (min 70s — the platform window is 60s).
//!   - 401/403/404/409: HALT — beating stops until the user intervenes.

use crate::api::MeAgent;
use crate::config::{clamp_interval, AppConfig, LocalCheck};
use crate::errors::ApiError;
use crate::history::{ActivityEvent, BeatKind, BeatRecord, HistoryStore};
use serde::Serialize;
use std::collections::HashMap;

pub const MAX_CONCURRENT_BEATS: usize = 4;
pub const MAX_LAUNCH_PER_TICK: usize = 2;

// ── Halt taxonomy ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HaltKind {
    TokenInvalid,
    TokenMismatch,
    Revoked,
    Archived,
    AutoDisabled,
    NotFound,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct Halt {
    pub kind: HaltKind,
    pub message: String,
}

/// Map a halting API error to its reason. The platform's own error strings
/// (runtime-auth.ts / heartbeat route) carry the distinction.
pub fn halt_for(err: &ApiError) -> Option<Halt> {
    match err {
        ApiError::Unauthorized { message } => {
            let lower = message.to_ascii_lowercase();
            let kind = if lower.contains("revoked") {
                HaltKind::Revoked
            } else if lower.contains("archived") {
                HaltKind::Archived
            } else if lower.contains("auto-disabled") {
                HaltKind::AutoDisabled
            } else {
                HaltKind::TokenInvalid
            };
            Some(Halt {
                kind,
                message: message.clone(),
            })
        }
        ApiError::Forbidden { message } => {
            let lower = message.to_ascii_lowercase();
            let kind = if lower.contains("auto-disabled") {
                HaltKind::AutoDisabled
            } else {
                HaltKind::TokenMismatch
            };
            Some(Halt {
                kind,
                message: message.clone(),
            })
        }
        ApiError::NotFound { message } => Some(Halt {
            kind: HaltKind::NotFound,
            message: message.clone(),
        }),
        ApiError::Conflict { message } => Some(Halt {
            kind: HaltKind::Unavailable,
            message: message.clone(),
        }),
        _ => None,
    }
}

// ── Delay math ─────────────────────────────────────────────────────────────

/// Exponential backoff for network/server failures: 30s, 60s, 120s, …
/// capped at the agent's own interval (never wait longer than a normal cycle).
pub fn failure_delay_secs(consecutive_failures: u32, interval_secs: u64) -> u64 {
    let n = consecutive_failures.max(1);
    let exp = 30u64.saturating_mul(1u64 << (n - 1).min(6));
    exp.min(clamp_interval(interval_secs)).max(30)
}

/// 429 delay: honor Retry-After with headroom; floor 70s (platform window 60s).
pub fn rate_limit_delay_secs(retry_after_secs: u64) -> u64 {
    (retry_after_secs + 5).max(70)
}

/// ±10% jitter, driven by a caller-supplied unit value (deterministic in tests).
pub fn jittered_interval_secs(interval_secs: u64, unit: f64) -> u64 {
    let interval = clamp_interval(interval_secs) as f64;
    let factor = 0.9 + unit.clamp(0.0, 1.0) * 0.2;
    (interval * factor).round() as u64
}

// ── Runtime state ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Serialize)]
pub struct AgentRuntime {
    /// Epoch seconds; 0 = beat as soon as possible.
    pub next_beat_at: u64,
    pub in_flight: bool,
    pub consecutive_failures: u32,
    pub last_attempt_at: Option<u64>,
    pub last_success_at: Option<u64>,
    pub last_kind: Option<BeatKind>,
    pub last_note: Option<String>,
    pub last_latency_ms: Option<u32>,
    /// Platform truth from the most recent heartbeat/me response.
    pub platform_status: Option<String>,
    pub platform_last_heartbeat: Option<String>,
    pub me: Option<MeAgent>,
    pub halted: Option<Halt>,
    pub local_check_failing: bool,
}

/// A beat the loop should launch now.
#[derive(Debug, Clone)]
pub struct DueJob {
    pub agent_id: String,
    pub interval_secs: u64,
    pub local_check: Option<LocalCheck>,
    /// Status to report. None = normal cycle ("online" unless local check fails).
    pub explicit_status: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraySeverity {
    Ok,
    Degraded,
    Halted,
    Paused,
}

pub struct Core {
    pub config: AppConfig,
    pub runtime: HashMap<String, AgentRuntime>,
    pub history: HistoryStore,
    pub paused: bool,
    pub dirty_config: bool,
    pub dirty_history: bool,
}

impl Core {
    pub fn new(config: AppConfig, history: HistoryStore) -> Self {
        let mut core = Self {
            config,
            runtime: HashMap::new(),
            history,
            paused: false,
            dirty_config: false,
            dirty_history: false,
        };
        let ids: Vec<String> = core.config.agents.iter().map(|a| a.id.clone()).collect();
        for id in ids {
            core.ensure_runtime(&id);
        }
        core
    }

    pub fn ensure_runtime(&mut self, agent_id: &str) -> &mut AgentRuntime {
        self.runtime.entry(agent_id.to_string()).or_default()
    }

    /// Collect beats due at `now`, marking them in-flight. Respects the global
    /// pause, halts, per-tick launch cap, and the global concurrency cap.
    pub fn due_jobs(&mut self, now: u64) -> Vec<DueJob> {
        if self.paused {
            return Vec::new();
        }
        let in_flight = self.runtime.values().filter(|r| r.in_flight).count();
        if in_flight >= MAX_CONCURRENT_BEATS {
            return Vec::new();
        }
        let budget = (MAX_CONCURRENT_BEATS - in_flight).min(MAX_LAUNCH_PER_TICK);

        let mut due: Vec<(u64, DueJob)> = Vec::new();
        for agent in &self.config.agents {
            if !agent.in_service {
                continue;
            }
            let Some(rt) = self.runtime.get(&agent.id) else {
                continue;
            };
            if rt.in_flight || rt.halted.is_some() {
                continue;
            }
            if now >= rt.next_beat_at {
                due.push((
                    rt.next_beat_at,
                    DueJob {
                        agent_id: agent.id.clone(),
                        interval_secs: agent.interval_secs,
                        local_check: agent.local_check.clone(),
                        explicit_status: None,
                    },
                ));
            }
        }
        due.sort_by_key(|(at, _)| *at);
        let jobs: Vec<DueJob> = due.into_iter().take(budget).map(|(_, j)| j).collect();
        for job in &jobs {
            if let Some(rt) = self.runtime.get_mut(&job.agent_id) {
                rt.in_flight = true;
                rt.last_attempt_at = Some(now);
            }
        }
        jobs
    }

    /// Successful beat (online or honest offline). Schedules the next cycle.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_success(
        &mut self,
        agent_id: &str,
        kind: BeatKind,
        platform_status: String,
        platform_last_heartbeat: Option<String>,
        latency_ms: u32,
        note: Option<String>,
        now: u64,
        jitter_unit: f64,
    ) {
        let interval = self
            .config
            .agent(agent_id)
            .map(|a| a.interval_secs)
            .unwrap_or(crate::config::DEFAULT_INTERVAL_SECS);
        let recovered = self
            .runtime
            .get(agent_id)
            .map(|r| r.consecutive_failures > 0)
            .unwrap_or(false);
        let rt = self.ensure_runtime(agent_id);
        rt.in_flight = false;
        rt.consecutive_failures = 0;
        rt.last_success_at = Some(now);
        rt.last_kind = Some(kind);
        rt.last_note = note.clone();
        rt.last_latency_ms = Some(latency_ms);
        rt.platform_status = Some(platform_status);
        rt.platform_last_heartbeat = platform_last_heartbeat;
        rt.local_check_failing = kind == BeatKind::LocalFailOffline;
        rt.next_beat_at = now + jittered_interval_secs(interval, jitter_unit);

        self.history.push_beat(
            agent_id,
            BeatRecord {
                ts: now,
                kind,
                http_status: Some(200),
                latency_ms: Some(latency_ms),
                note,
            },
        );
        self.dirty_history = true;
        if recovered {
            let name = self.agent_name(agent_id);
            self.push_activity(
                now,
                "info",
                Some(agent_id),
                format!("{name} recovered — beating normally again"),
            );
        }
    }

    /// Failed beat. Returns a notification message when the failure warrants
    /// alerting the user (halt, or the 3rd consecutive delivery failure).
    pub fn apply_error(&mut self, agent_id: &str, err: &ApiError, now: u64) -> Option<String> {
        let interval = self
            .config
            .agent(agent_id)
            .map(|a| a.interval_secs)
            .unwrap_or(crate::config::DEFAULT_INTERVAL_SECS);
        let name = self.agent_name(agent_id);
        let halt = halt_for(err);
        let kind = match err {
            ApiError::Network { .. } => BeatKind::ErrNetwork,
            ApiError::RateLimited { .. } => BeatKind::ErrRateLimited,
            ApiError::Server { .. } | ApiError::Other { .. } => BeatKind::ErrServer,
            ApiError::Decode { .. } => BeatKind::ErrDecode,
            _ => BeatKind::ErrAuth,
        };
        let note = err.message();
        let http_status = err.http_status();

        let rt = self.ensure_runtime(agent_id);
        rt.in_flight = false;
        rt.last_kind = Some(kind);
        rt.last_note = Some(note.clone());
        rt.last_latency_ms = None;

        let mut notification: Option<String> = None;
        if let Some(halt) = halt {
            let text = format!("{name} halted: {}", halt.message);
            rt.halted = Some(halt);
            rt.next_beat_at = u64::MAX;
            notification = Some(text.clone());
            self.push_activity_raw(
                now,
                "error",
                Some(agent_id.to_string()),
                Some(name.clone()),
                text,
            );
        } else {
            rt.consecutive_failures = rt.consecutive_failures.saturating_add(1);
            let failures = rt.consecutive_failures;
            let delay = match err {
                ApiError::RateLimited { retry_after_secs } => {
                    rate_limit_delay_secs(*retry_after_secs)
                }
                _ => failure_delay_secs(failures, interval),
            };
            rt.next_beat_at = now + delay;
            let level = if failures >= 3 { "error" } else { "warn" };
            let text = format!("{name}: {} (retry in {delay}s, attempt {failures})", note);
            if failures == 3 {
                notification = Some(format!("{name} — heartbeats failing: {note}"));
            }
            self.push_activity_raw(
                now,
                level,
                Some(agent_id.to_string()),
                Some(name.clone()),
                text,
            );
        }

        self.history.push_beat(
            agent_id,
            BeatRecord {
                ts: now,
                kind,
                http_status,
                latency_ms: None,
                note: Some(note),
            },
        );
        self.dirty_history = true;
        notification
    }

    pub fn set_paused(&mut self, paused: bool, now: u64) {
        self.paused = paused;
        self.push_activity(
            now,
            "info",
            None,
            if paused {
                "All beating paused".into()
            } else {
                "Beating resumed".into()
            },
        );
    }

    /// Clear a halt so the user can retry after fixing the cause.
    pub fn clear_halt(&mut self, agent_id: &str, now: u64) {
        let rt = self.ensure_runtime(agent_id);
        rt.halted = None;
        rt.consecutive_failures = 0;
        rt.next_beat_at = 0;
        let name = self.agent_name(agent_id);
        self.push_activity(
            now,
            "info",
            Some(agent_id),
            format!("{name}: halt cleared — retrying"),
        );
    }

    pub fn schedule_asap(&mut self, agent_id: &str) {
        let rt = self.ensure_runtime(agent_id);
        if !rt.in_flight {
            rt.next_beat_at = 0;
        }
    }

    pub fn agent_name(&self, agent_id: &str) -> String {
        self.config
            .agent(agent_id)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Unknown agent".to_string())
    }

    pub fn push_activity(&mut self, now: u64, level: &str, agent_id: Option<&str>, text: String) {
        let agent_name = agent_id.map(|id| self.agent_name(id));
        self.push_activity_raw(
            now,
            level,
            agent_id.map(|s| s.to_string()),
            agent_name,
            text,
        );
    }

    fn push_activity_raw(
        &mut self,
        now: u64,
        level: &str,
        agent_id: Option<String>,
        agent_name: Option<String>,
        text: String,
    ) {
        self.history.push_activity(ActivityEvent {
            ts: now,
            level: level.to_string(),
            agent_id,
            agent_name,
            text,
        });
        self.dirty_history = true;
    }

    pub fn tray_severity(&self) -> TraySeverity {
        let any_in_service = self.config.agents.iter().any(|a| a.in_service);
        if self.paused || !any_in_service {
            return TraySeverity::Paused;
        }
        let mut degraded = false;
        for agent in &self.config.agents {
            if !agent.in_service {
                continue;
            }
            if let Some(rt) = self.runtime.get(&agent.id) {
                if rt.halted.is_some() {
                    return TraySeverity::Halted;
                }
                if rt.consecutive_failures > 0 || rt.local_check_failing {
                    degraded = true;
                }
            }
        }
        if degraded {
            TraySeverity::Degraded
        } else {
            TraySeverity::Ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentConfig, MAX_INTERVAL_SECS, MIN_INTERVAL_SECS};

    fn agent(id: &str, in_service: bool) -> AgentConfig {
        AgentConfig {
            id: id.into(),
            name: format!("Agent {id}"),
            fingerprint: "tk_0000…ffff".into(),
            in_service,
            interval_secs: 300,
            local_check: None,
            added_at: 0,
        }
    }

    fn core_with(agents: Vec<AgentConfig>) -> Core {
        let cfg = AppConfig {
            agents,
            ..AppConfig::default()
        };
        Core::new(cfg, HistoryStore::default())
    }

    #[test]
    fn backoff_sequence_caps_at_interval() {
        assert_eq!(failure_delay_secs(1, 300), 30);
        assert_eq!(failure_delay_secs(2, 300), 60);
        assert_eq!(failure_delay_secs(3, 300), 120);
        assert_eq!(failure_delay_secs(4, 300), 240);
        assert_eq!(failure_delay_secs(5, 300), 300); // capped
        assert_eq!(failure_delay_secs(30, 300), 300); // shift saturates safely
                                                      // Sub-floor intervals clamp to the 300s minimum before capping.
        assert_eq!(failure_delay_secs(5, 60), 300);
    }

    #[test]
    fn rate_limit_delay_honors_header_with_floor() {
        assert_eq!(rate_limit_delay_secs(60), 70);
        assert_eq!(rate_limit_delay_secs(120), 125);
        assert_eq!(rate_limit_delay_secs(0), 70);
    }

    #[test]
    fn jitter_stays_within_ten_percent() {
        assert_eq!(jittered_interval_secs(300, 0.0), 270);
        assert_eq!(jittered_interval_secs(300, 0.5), 300);
        assert_eq!(jittered_interval_secs(300, 1.0), 330);
        // Clamps out-of-range intervals first.
        assert_eq!(jittered_interval_secs(10, 0.5), MIN_INTERVAL_SECS);
        assert_eq!(jittered_interval_secs(10_000, 0.5), MAX_INTERVAL_SECS);
    }

    #[test]
    fn halt_mapping_matches_platform_messages() {
        let cases = [
            ("Your agent has been revoked", HaltKind::Revoked),
            ("Your agent is archived", HaltKind::Archived),
            ("Your agent is auto-disabled", HaltKind::AutoDisabled),
            ("Invalid API key", HaltKind::TokenInvalid),
        ];
        for (msg, expected) in cases {
            let halt = halt_for(&ApiError::Unauthorized {
                message: msg.into(),
            })
            .unwrap();
            assert_eq!(halt.kind, expected, "for message {msg:?}");
        }
        let mismatch = halt_for(&ApiError::Forbidden {
            message: "Unauthorized — your API key does not match the requested agent".into(),
        })
        .unwrap();
        assert_eq!(mismatch.kind, HaltKind::TokenMismatch);
        let disabled = halt_for(&ApiError::Forbidden {
            message: "Agent is auto-disabled".into(),
        })
        .unwrap();
        assert_eq!(disabled.kind, HaltKind::AutoDisabled);
        assert_eq!(
            halt_for(&ApiError::NotFound {
                message: "x".into()
            })
            .unwrap()
            .kind,
            HaltKind::NotFound
        );
        assert_eq!(
            halt_for(&ApiError::Conflict {
                message: "Agent is no longer available".into()
            })
            .unwrap()
            .kind,
            HaltKind::Unavailable
        );
        assert!(halt_for(&ApiError::Network {
            message: "x".into()
        })
        .is_none());
        assert!(halt_for(&ApiError::RateLimited {
            retry_after_secs: 60
        })
        .is_none());
        assert!(halt_for(&ApiError::Server {
            status: 500,
            message: "x".into()
        })
        .is_none());
    }

    #[test]
    fn due_jobs_respects_gates() {
        let mut core = core_with(vec![agent("a", true), agent("b", true), agent("c", false)]);
        // All runtimes start at next_beat_at = 0 → due immediately.
        let jobs = core.due_jobs(100);
        // Launch cap = 2 per tick; agent c is out of service.
        assert_eq!(jobs.len(), 2);
        assert!(core.runtime.get("a").unwrap().in_flight);
        assert!(core.runtime.get("b").unwrap().in_flight);
        // Nothing more while both are in flight.
        assert!(core.due_jobs(101).is_empty());
    }

    #[test]
    fn due_jobs_skips_paused_and_halted() {
        let mut core = core_with(vec![agent("a", true), agent("b", true)]);
        core.set_paused(true, 1);
        assert!(core.due_jobs(100).is_empty());
        core.set_paused(false, 2);
        core.ensure_runtime("a").halted = Some(Halt {
            kind: HaltKind::TokenInvalid,
            message: "Invalid API key".into(),
        });
        let jobs = core.due_jobs(100);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].agent_id, "b");
    }

    #[test]
    fn success_resets_failures_and_schedules_next() {
        let mut core = core_with(vec![agent("a", true)]);
        core.due_jobs(1_000);
        core.apply_error(
            "a",
            &ApiError::Network {
                message: "down".into(),
            },
            1_000,
        );
        assert_eq!(core.runtime.get("a").unwrap().consecutive_failures, 1);
        core.due_jobs(1_030); // due again after 30s backoff
        core.apply_success(
            "a",
            BeatKind::OnlineOk,
            "online".into(),
            None,
            120,
            None,
            1_030,
            0.5,
        );
        let rt = core.runtime.get("a").unwrap();
        assert_eq!(rt.consecutive_failures, 0);
        assert_eq!(rt.platform_status.as_deref(), Some("online"));
        assert_eq!(rt.next_beat_at, 1_030 + 300);
        assert!(!rt.in_flight);
        // History recorded both attempts.
        assert_eq!(core.history.beats_for("a").len(), 2);
    }

    #[test]
    fn network_error_backs_off_then_halts_on_auth() {
        let mut core = core_with(vec![agent("a", true)]);
        core.due_jobs(1_000);
        let n1 = core.apply_error(
            "a",
            &ApiError::Network {
                message: "down".into(),
            },
            1_000,
        );
        assert!(n1.is_none());
        assert_eq!(core.runtime.get("a").unwrap().next_beat_at, 1_030);
        core.due_jobs(1_030);
        let n2 = core.apply_error(
            "a",
            &ApiError::Network {
                message: "down".into(),
            },
            1_030,
        );
        assert!(n2.is_none());
        assert_eq!(core.runtime.get("a").unwrap().next_beat_at, 1_030 + 60);
        core.due_jobs(1_090);
        // Third consecutive failure raises a notification.
        let n3 = core.apply_error(
            "a",
            &ApiError::Network {
                message: "down".into(),
            },
            1_090,
        );
        assert!(n3.is_some());
        // Now an auth failure halts outright.
        core.clear_halt("a", 1_100); // no-op halt clear to reset failures
        core.due_jobs(1_100);
        let n4 = core.apply_error(
            "a",
            &ApiError::Unauthorized {
                message: "Invalid API key".into(),
            },
            1_100,
        );
        assert!(n4.is_some());
        let rt = core.runtime.get("a").unwrap();
        assert_eq!(rt.halted.as_ref().unwrap().kind, HaltKind::TokenInvalid);
        assert_eq!(rt.next_beat_at, u64::MAX);
        // Halted agents never come due again.
        assert!(core.due_jobs(u64::MAX - 1).is_empty());
    }

    #[test]
    fn rate_limited_uses_retry_after() {
        let mut core = core_with(vec![agent("a", true)]);
        core.due_jobs(2_000);
        core.apply_error(
            "a",
            &ApiError::RateLimited {
                retry_after_secs: 42,
            },
            2_000,
        );
        let rt = core.runtime.get("a").unwrap();
        // max(42+5, 70) = 70
        assert_eq!(rt.next_beat_at, 2_070);
        assert!(rt.halted.is_none());
    }

    #[test]
    fn tray_severity_ladder() {
        let mut core = core_with(vec![agent("a", true), agent("b", true)]);
        assert_eq!(core.tray_severity(), TraySeverity::Ok);
        core.ensure_runtime("a").consecutive_failures = 1;
        assert_eq!(core.tray_severity(), TraySeverity::Degraded);
        core.ensure_runtime("b").halted = Some(Halt {
            kind: HaltKind::Revoked,
            message: "revoked".into(),
        });
        assert_eq!(core.tray_severity(), TraySeverity::Halted);
        core.set_paused(true, 1);
        assert_eq!(core.tray_severity(), TraySeverity::Paused);
        // No agents in service → paused-gray too.
        let mut idle_core = core_with(vec![agent("x", false)]);
        assert_eq!(idle_core.tray_severity(), TraySeverity::Paused);
        idle_core.config.agents.clear();
        assert_eq!(idle_core.tray_severity(), TraySeverity::Paused);
    }

    #[test]
    fn local_fail_marks_flag_and_recovers() {
        let mut core = core_with(vec![agent("a", true)]);
        core.due_jobs(3_000);
        core.apply_success(
            "a",
            BeatKind::LocalFailOffline,
            "offline".into(),
            None,
            80,
            Some("HTTP 503 outside expected 200–399".into()),
            3_000,
            0.5,
        );
        assert!(core.runtime.get("a").unwrap().local_check_failing);
        core.due_jobs(3_300);
        core.apply_success(
            "a",
            BeatKind::OnlineOk,
            "online".into(),
            None,
            60,
            None,
            3_300,
            0.5,
        );
        assert!(!core.runtime.get("a").unwrap().local_check_failing);
    }
}
