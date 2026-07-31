//! Beat history — per-agent ring buffer of heartbeat outcomes plus a global
//! activity feed. Persisted to history.json (non-secret). Powers the Health tab.

use crate::vault::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

/// 7 days at the 5-minute default cadence.
pub const RING_CAP: usize = 2016;
pub const ACTIVITY_CAP: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeatKind {
    /// Heartbeat online accepted.
    OnlineOk,
    /// Explicit offline report accepted (toggle off / shutdown / local check fail).
    OfflineOk,
    /// Local health check failed → honest offline was sent.
    LocalFailOffline,
    /// Network-level failure (no HTTP response).
    ErrNetwork,
    /// 401/403/404/409 — halting class.
    ErrAuth,
    /// 429.
    ErrRateLimited,
    /// 5xx.
    ErrServer,
    /// Response shape mismatch.
    ErrDecode,
}

impl BeatKind {
    pub fn is_success(self) -> bool {
        matches!(
            self,
            BeatKind::OnlineOk | BeatKind::OfflineOk | BeatKind::LocalFailOffline
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeatRecord {
    /// Epoch seconds.
    pub ts: u64,
    pub kind: BeatKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub ts: u64,
    /// "info" | "warn" | "error"
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    pub text: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HistoryStore {
    #[serde(default)]
    pub beats: HashMap<String, VecDeque<BeatRecord>>,
    #[serde(default)]
    pub activity: VecDeque<ActivityEvent>,
}

impl HistoryStore {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(raw) => serde_json::from_slice(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        atomic_write(path, &json).map_err(|e| e.to_string())
    }

    pub fn push_beat(&mut self, agent_id: &str, record: BeatRecord) {
        let ring = self.beats.entry(agent_id.to_string()).or_default();
        ring.push_back(record);
        while ring.len() > RING_CAP {
            ring.pop_front();
        }
    }

    pub fn push_activity(&mut self, event: ActivityEvent) {
        self.activity.push_back(event);
        while self.activity.len() > ACTIVITY_CAP {
            self.activity.pop_front();
        }
    }

    pub fn remove_agent(&mut self, agent_id: &str) {
        self.beats.remove(agent_id);
    }

    pub fn beats_for(&self, agent_id: &str) -> Vec<BeatRecord> {
        self.beats
            .get(agent_id)
            .map(|r| r.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn recent_activity(&self, n: usize) -> Vec<ActivityEvent> {
        self.activity.iter().rev().take(n).cloned().collect()
    }

    /// Aggregate stats over a trailing window, per agent.
    pub fn stats_for(&self, agent_id: &str, now: u64, window_secs: u64) -> AgentStats {
        let cutoff = now.saturating_sub(window_secs);
        let mut total = 0u32;
        let mut ok = 0u32;
        let mut latency_sum: u64 = 0;
        let mut latency_count = 0u32;
        if let Some(ring) = self.beats.get(agent_id) {
            for r in ring.iter().filter(|r| r.ts >= cutoff) {
                total += 1;
                if r.kind.is_success() {
                    ok += 1;
                }
                if let Some(l) = r.latency_ms {
                    latency_sum += u64::from(l);
                    latency_count += 1;
                }
            }
        }
        AgentStats {
            total,
            ok,
            success_rate: if total > 0 {
                (f64::from(ok) / f64::from(total) * 100.0).round()
            } else {
                0.0
            },
            avg_latency_ms: if latency_count > 0 {
                (latency_sum / u64::from(latency_count)) as u32
            } else {
                0
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStats {
    pub total: u32,
    pub ok: u32,
    pub success_rate: f64,
    pub avg_latency_ms: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rec(ts: u64, kind: BeatKind, latency: Option<u32>) -> BeatRecord {
        BeatRecord {
            ts,
            kind,
            http_status: None,
            latency_ms: latency,
            note: None,
        }
    }

    #[test]
    fn ring_caps_at_limit() {
        let mut h = HistoryStore::default();
        for i in 0..(RING_CAP + 50) {
            h.push_beat("a", rec(i as u64, BeatKind::OnlineOk, Some(10)));
        }
        let ring = h.beats.get("a").unwrap();
        assert_eq!(ring.len(), RING_CAP);
        // Oldest entries were evicted.
        assert_eq!(ring.front().unwrap().ts, 50);
    }

    #[test]
    fn activity_caps_at_limit() {
        let mut h = HistoryStore::default();
        for i in 0..(ACTIVITY_CAP + 10) {
            h.push_activity(ActivityEvent {
                ts: i as u64,
                level: "info".into(),
                agent_id: None,
                agent_name: None,
                text: format!("e{i}"),
            });
        }
        assert_eq!(h.activity.len(), ACTIVITY_CAP);
        let recent = h.recent_activity(5);
        assert_eq!(recent[0].text, format!("e{}", ACTIVITY_CAP + 9));
    }

    #[test]
    fn stats_window_math() {
        let mut h = HistoryStore::default();
        let now = 100_000u64;
        // Inside window: 3 ok (latencies 100/200/300) + 1 network error.
        h.push_beat("a", rec(now - 100, BeatKind::OnlineOk, Some(100)));
        h.push_beat("a", rec(now - 200, BeatKind::OnlineOk, Some(200)));
        h.push_beat("a", rec(now - 300, BeatKind::OfflineOk, Some(300)));
        h.push_beat("a", rec(now - 400, BeatKind::ErrNetwork, None));
        // Outside window.
        h.push_beat("a", rec(now - 10_000, BeatKind::ErrServer, Some(999)));
        let s = h.stats_for("a", now, 1_000);
        assert_eq!(s.total, 4);
        assert_eq!(s.ok, 3);
        assert_eq!(s.success_rate, 75.0);
        assert_eq!(s.avg_latency_ms, 200);
        // Empty agent → zeros, no division by zero.
        let empty = h.stats_for("nope", now, 1_000);
        assert_eq!(empty.total, 0);
        assert_eq!(empty.success_rate, 0.0);
    }

    #[test]
    fn local_fail_offline_counts_as_success() {
        // The beat DELIVERY succeeded (honest offline) even though the local
        // check failed — success_rate measures delivery, not agent health.
        assert!(BeatKind::LocalFailOffline.is_success());
        assert!(!BeatKind::ErrNetwork.is_success());
    }

    #[test]
    fn persistence_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("history.json");
        let mut h = HistoryStore::default();
        h.push_beat("a", rec(1, BeatKind::OnlineOk, Some(42)));
        h.push_activity(ActivityEvent {
            ts: 2,
            level: "warn".into(),
            agent_id: Some("a".into()),
            agent_name: Some("Agent".into()),
            text: "test".into(),
        });
        h.save(&path).unwrap();
        let loaded = HistoryStore::load(&path);
        assert_eq!(loaded.beats.get("a").unwrap().len(), 1);
        assert_eq!(loaded.activity.len(), 1);
    }
}
