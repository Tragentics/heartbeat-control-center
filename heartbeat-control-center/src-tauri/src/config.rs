//! Non-secret configuration (config.json). Tokens NEVER live here — they are
//! vault-only. This file holds agent metadata + app settings.

use crate::vault::atomic_write;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_BASE_URL: &str = "https://tragentics.com";
pub const DEFAULT_INTERVAL_SECS: u64 = 300;
// 5 min hard floor (owner decision 2026-07-31): Beat Now exists for on-demand
// beats, so sub-5-minute cadences would only spam the platform.
pub const MIN_INTERVAL_SECS: u64 = 300;
pub const MAX_INTERVAL_SECS: u64 = 840; // 14 min — keeps a healthy margin under the 15-min idle threshold

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCheck {
    pub url: String,
    #[serde(default = "default_expect_min")]
    pub expect_min: u16,
    #[serde(default = "default_expect_max")]
    pub expect_max: u16,
    #[serde(default = "default_check_timeout")]
    pub timeout_secs: u64,
}

fn default_expect_min() -> u16 {
    200
}
fn default_expect_max() -> u16 {
    399
}
fn default_check_timeout() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    /// tk_ab12…cd34 — display only.
    pub fingerprint: String,
    pub in_service: bool,
    pub interval_secs: u64,
    #[serde(default)]
    pub local_check: Option<LocalCheck>,
    /// Epoch seconds.
    pub added_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub base_url: String,
    pub default_interval_secs: u64,
    /// "system" | "dark" | "light"
    pub theme: String,
    pub minimize_to_tray: bool,
    pub notify_on_problems: bool,
    pub agents: Vec<AgentConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            default_interval_secs: DEFAULT_INTERVAL_SECS,
            theme: "system".to_string(),
            minimize_to_tray: true,
            notify_on_problems: true,
            agents: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(raw) => serde_json::from_slice(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        atomic_write(path, &json).map_err(|e| e.to_string())
    }

    pub fn agent(&self, id: &str) -> Option<&AgentConfig> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn agent_mut(&mut self, id: &str) -> Option<&mut AgentConfig> {
        self.agents.iter_mut().find(|a| a.id == id)
    }
}

pub fn clamp_interval(secs: u64) -> u64 {
    secs.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        let missing = AppConfig::load(&path);
        assert_eq!(missing.base_url, DEFAULT_BASE_URL);
        assert!(missing.minimize_to_tray);

        let mut cfg = AppConfig::default();
        cfg.agents.push(AgentConfig {
            id: "a1".into(),
            name: "Agent One".into(),
            fingerprint: "tk_0123…cdef".into(),
            in_service: true,
            interval_secs: 300,
            local_check: None,
            added_at: 1_700_000_000,
        });
        cfg.save(&path).unwrap();
        let loaded = AppConfig::load(&path);
        assert_eq!(loaded.agents.len(), 1);
        assert_eq!(loaded.agents[0].name, "Agent One");
    }

    #[test]
    fn corrupted_config_falls_back_to_defaults() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"{{{{").unwrap();
        let cfg = AppConfig::load(&path);
        assert_eq!(cfg.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn interval_clamping() {
        assert_eq!(clamp_interval(10), MIN_INTERVAL_SECS);
        assert_eq!(clamp_interval(300), 300);
        assert_eq!(clamp_interval(100_000), MAX_INTERVAL_SECS);
    }
}
