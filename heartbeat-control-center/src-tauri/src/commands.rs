//! Tauri command surface — the ONLY door between the webview and the core.
//! Tokens cross this boundary exactly once (add_agent input) and never back out.

use crate::api::{is_valid_token_format, normalize_base_url, token_fingerprint};
use crate::config::{clamp_interval, AgentConfig, LocalCheck};
use crate::errors::{ApiError, AppError};
use crate::history::{BeatKind, BeatRecord};
use crate::vault::VaultMode;
use crate::{emit_snapshot, now_secs, run_oneshot_offline, update_tray, AppState, Snapshot};
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

#[tauri::command]
pub fn get_snapshot(app: AppHandle, state: State<'_, AppState>) -> Snapshot {
    crate::build_snapshot(&app, &state)
}

// ── Vault ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn vault_initialize(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: String,
    passphrase: Option<String>,
) -> Result<(), AppError> {
    let vault_mode = match mode.as_str() {
        "keyring" => VaultMode::Keyring,
        "passphrase" => VaultMode::Passphrase,
        _ => return Err(AppError::msg("Unknown vault mode")),
    };
    {
        let mut vault = state.vault.lock().unwrap();
        vault
            .initialize(vault_mode, passphrase.as_deref())
            .map_err(AppError::Vault)?;
    }
    {
        let mut core = state.core.lock().unwrap();
        core.push_activity(now_secs(), "info", None, "Local Vault created".into());
    }
    emit_snapshot(&app);
    Ok(())
}

#[tauri::command]
pub fn vault_unlock(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
) -> Result<(), AppError> {
    {
        let mut vault = state.vault.lock().unwrap();
        vault.unlock(&passphrase).map_err(AppError::Vault)?;
    }
    {
        let mut core = state.core.lock().unwrap();
        core.push_activity(
            now_secs(),
            "info",
            None,
            "Vault unlocked — beating enabled".into(),
        );
        // Vault was locked at startup: schedule everything fresh.
        let ids: Vec<String> = core.config.agents.iter().map(|a| a.id.clone()).collect();
        for id in ids {
            core.schedule_asap(&id);
        }
    }
    emit_snapshot(&app);
    update_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn vault_lock(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    {
        let mut vault = state.vault.lock().unwrap();
        if vault.mode() != Some(VaultMode::Passphrase) {
            return Err(AppError::msg(
                "Only passphrase vaults can be locked manually",
            ));
        }
        vault.lock();
    }
    {
        let mut core = state.core.lock().unwrap();
        core.push_activity(
            now_secs(),
            "warn",
            None,
            "Vault locked — beating suspended".into(),
        );
    }
    emit_snapshot(&app);
    update_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn vault_change_passphrase(
    state: State<'_, AppState>,
    current: String,
    next: String,
) -> Result<(), AppError> {
    let mut vault = state.vault.lock().unwrap();
    vault
        .change_passphrase(&current, &next)
        .map_err(AppError::Vault)
}

// ── Agents ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn add_agent(app: AppHandle, token: String) -> Result<String, AppError> {
    let token = token.trim().to_string();
    if !is_valid_token_format(&token) {
        return Err(AppError::msg(
            "That doesn't look like an agent token — expected tk_ followed by 64 hex characters",
        ));
    }
    let state = app.state::<AppState>();
    let base_url = { state.core.lock().unwrap().config.base_url.clone() };

    // Resolve identity from the token alone (GET /api/agents/me).
    let me = state
        .api
        .verify_token(&base_url, &token)
        .await
        .map_err(|e| match e {
            ApiError::Unauthorized { .. } => AppError::msg(
                "Tragentics rejected this token — check that you copied the full tk_ token",
            ),
            other => AppError::Api(other),
        })?;

    let now = now_secs();
    let fingerprint = token_fingerprint(&token);
    {
        let mut core = state.core.lock().unwrap();
        if core.config.agent(&me.id).is_some() {
            return Err(AppError::msg(format!(
                "{} is already in your fleet",
                me.name
            )));
        }
        // Vault write FIRST — config must never reference a token that isn't stored.
        {
            let mut vault = state.vault.lock().unwrap();
            vault
                .insert_token(&me.id, &token)
                .map_err(AppError::Vault)?;
        }
        let interval = core.config.default_interval_secs;
        core.config.agents.push(AgentConfig {
            id: me.id.clone(),
            name: me.name.clone(),
            fingerprint,
            in_service: true,
            interval_secs: clamp_interval(interval),
            local_check: None,
            added_at: now,
        });
        core.dirty_config = true;
        let rt = core.ensure_runtime(&me.id);
        rt.me = Some(me.clone());
        rt.platform_status = Some(me.status.clone());
        rt.platform_last_heartbeat = me.last_heartbeat.clone();
        core.push_activity(
            now,
            "info",
            Some(&me.id),
            format!("{} added to the fleet — first beat on the way", me.name),
        );
        core.schedule_asap(&me.id);
    }
    emit_snapshot(&app);
    update_tray(&app);
    Ok(me.id)
}

#[tauri::command]
pub async fn remove_agent(
    app: AppHandle,
    agent_id: String,
    send_offline: bool,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    if send_offline {
        run_oneshot_offline(&app, &agent_id, "removed from Control Center").await;
    }
    {
        let mut core = state.core.lock().unwrap();
        let name = core.agent_name(&agent_id);
        core.config.agents.retain(|a| a.id != agent_id);
        core.runtime.remove(&agent_id);
        core.history.remove_agent(&agent_id);
        core.dirty_config = true;
        core.dirty_history = true;
        core.push_activity(
            now_secs(),
            "info",
            None,
            format!("{name} removed from the fleet"),
        );
    }
    {
        let mut vault = state.vault.lock().unwrap();
        // Best effort: if the vault is locked we still removed the config;
        // the orphaned token is cleaned on next unlock via reconcile.
        let _ = vault.remove_token(&agent_id);
    }
    emit_snapshot(&app);
    update_tray(&app);
    Ok(())
}

#[tauri::command]
pub async fn set_in_service(
    app: AppHandle,
    agent_id: String,
    in_service: bool,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    {
        let mut core = state.core.lock().unwrap();
        let name = core.agent_name(&agent_id);
        let Some(agent) = core.config.agent_mut(&agent_id) else {
            return Err(AppError::msg("Agent not found"));
        };
        agent.in_service = in_service;
        core.dirty_config = true;
        if in_service {
            core.clear_halt(&agent_id, now_secs());
            core.push_activity(
                now_secs(),
                "info",
                Some(&agent_id),
                format!("{name} put in service"),
            );
        } else {
            core.push_activity(
                now_secs(),
                "info",
                Some(&agent_id),
                format!("{name} taken out of service — reporting offline"),
            );
        }
    }
    if !in_service {
        // Honest offline: tell the platform immediately instead of drifting idle.
        run_oneshot_offline(&app, &agent_id, "taken out of service").await;
    }
    emit_snapshot(&app);
    update_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn beat_now(
    app: AppHandle,
    state: State<'_, AppState>,
    agent_id: String,
) -> Result<(), AppError> {
    {
        let mut core = state.core.lock().unwrap();
        if core.config.agent(&agent_id).is_none() {
            return Err(AppError::msg("Agent not found"));
        }
        if core
            .runtime
            .get(&agent_id)
            .map(|r| r.halted.is_some())
            .unwrap_or(false)
        {
            core.clear_halt(&agent_id, now_secs());
        } else {
            core.schedule_asap(&agent_id);
        }
    }
    emit_snapshot(&app);
    Ok(())
}

#[derive(Deserialize)]
pub struct AgentSettingsPatch {
    pub interval_secs: Option<u64>,
    /// Some(None) clears the local check; Some(Some(x)) sets it; None leaves it.
    #[serde(default, with = "double_option")]
    pub local_check: Option<Option<LocalCheck>>,
}

/// Distinguish "field absent" from "field null" for local_check.
mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Deserialize::deserialize(de).map(Some)
    }
}

#[tauri::command]
pub fn update_agent(
    app: AppHandle,
    state: State<'_, AppState>,
    agent_id: String,
    patch: AgentSettingsPatch,
) -> Result<(), AppError> {
    {
        let mut core = state.core.lock().unwrap();
        let Some(agent) = core.config.agent_mut(&agent_id) else {
            return Err(AppError::msg("Agent not found"));
        };
        if let Some(interval) = patch.interval_secs {
            agent.interval_secs = clamp_interval(interval);
        }
        if let Some(lc) = patch.local_check {
            if let Some(check) = &lc {
                normalize_local_check_url(&check.url)?;
            }
            agent.local_check = lc;
        }
        core.dirty_config = true;
    }
    emit_snapshot(&app);
    Ok(())
}

fn normalize_local_check_url(url: &str) -> Result<(), AppError> {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(AppError::msg(
            "Local check URL must start with http:// or https://",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn refresh_me(app: AppHandle, agent_id: String) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let (base_url, token) = {
        let core = state.core.lock().unwrap();
        if core.config.agent(&agent_id).is_none() {
            return Err(AppError::msg("Agent not found"));
        }
        let vault = state.vault.lock().unwrap();
        let Some(token) = vault.token_for(&agent_id) else {
            return Err(AppError::msg("Vault is locked"));
        };
        (core.config.base_url.clone(), token)
    };
    let result = state.api.verify_token(&base_url, &token).await;
    {
        let mut core = state.core.lock().unwrap();
        match result {
            Ok(me) => {
                let name_changed = core
                    .config
                    .agent(&agent_id)
                    .map(|a| a.name != me.name)
                    .unwrap_or(false);
                if name_changed {
                    if let Some(agent) = core.config.agent_mut(&agent_id) {
                        agent.name = me.name.clone();
                    }
                    core.dirty_config = true;
                }
                let rt = core.ensure_runtime(&agent_id);
                rt.platform_status = Some(me.status.clone());
                rt.platform_last_heartbeat = me.last_heartbeat.clone();
                rt.me = Some(me);
            }
            Err(err) => {
                let notification = core.apply_error(&agent_id, &err, now_secs());
                drop(core);
                crate::maybe_notify(&app, notification);
                emit_snapshot(&app);
                update_tray(&app);
                return Err(AppError::Api(err));
            }
        }
    }
    emit_snapshot(&app);
    Ok(())
}

#[tauri::command]
pub fn get_agent_history(state: State<'_, AppState>, agent_id: String) -> Vec<BeatRecord> {
    state.core.lock().unwrap().history.beats_for(&agent_id)
}

// ── Settings ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SettingsPatch {
    pub base_url: Option<String>,
    pub theme: Option<String>,
    pub minimize_to_tray: Option<bool>,
    pub notify_on_problems: Option<bool>,
    pub default_interval_secs: Option<u64>,
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<(), AppError> {
    {
        let mut core = state.core.lock().unwrap();
        if let Some(url) = patch.base_url {
            let normalized = normalize_base_url(&url).map_err(AppError::Msg)?;
            if normalized != core.config.base_url {
                core.config.base_url = normalized.clone();
                core.push_activity(
                    now_secs(),
                    "warn",
                    None,
                    format!("API base URL changed to {normalized}"),
                );
            }
        }
        if let Some(theme) = patch.theme {
            if !["system", "dark", "light"].contains(&theme.as_str()) {
                return Err(AppError::msg("Unknown theme"));
            }
            core.config.theme = theme;
        }
        if let Some(v) = patch.minimize_to_tray {
            core.config.minimize_to_tray = v;
        }
        if let Some(v) = patch.notify_on_problems {
            core.config.notify_on_problems = v;
        }
        if let Some(v) = patch.default_interval_secs {
            core.config.default_interval_secs = clamp_interval(v);
        }
        core.dirty_config = true;
    }
    emit_snapshot(&app);
    Ok(())
}

#[tauri::command]
pub fn set_paused(
    app: AppHandle,
    state: State<'_, AppState>,
    paused: bool,
) -> Result<(), AppError> {
    {
        let mut core = state.core.lock().unwrap();
        core.set_paused(paused, now_secs());
    }
    emit_snapshot(&app);
    update_tray(&app);
    Ok(())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), AppError> {
    let autolaunch = app.autolaunch();
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|e| AppError::msg(format!("autostart: {e}")))?;
    emit_snapshot(&app);
    Ok(())
}

#[tauri::command]
pub async fn test_base_url(app: AppHandle, url: String) -> Result<String, AppError> {
    let normalized = normalize_base_url(&url).map_err(AppError::Msg)?;
    let state = app.state::<AppState>();
    state
        .api
        .test_base_url(&normalized)
        .await
        .map_err(AppError::Api)
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    let path = state.paths.config.clone();
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| AppError::msg(format!("open data dir: {e}")))
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) {
    crate::graceful_quit(&app).await;
}

/// Record for an offline one-shot initiated by a UI flow that already put the
/// agent out of service (used by run_oneshot_offline).
pub fn record_offline_result(
    app: &AppHandle,
    agent_id: &str,
    outcome: Result<crate::api::BeatSuccess, ApiError>,
    reason: &str,
) {
    let state = app.state::<AppState>();
    let mut core = state.core.lock().unwrap();
    let now = now_secs();
    match outcome {
        Ok(success) => {
            core.apply_success(
                agent_id,
                BeatKind::OfflineOk,
                success.platform_status,
                success.platform_last_heartbeat,
                success.latency_ms,
                Some(reason.to_string()),
                now,
                0.5,
            );
        }
        Err(err) => {
            let name = core.agent_name(agent_id);
            core.push_activity(
                now,
                "warn",
                Some(agent_id),
                format!("{name}: offline report failed ({}) — the platform will drift it idle→offline on its own", err.message()),
            );
        }
    }
}
