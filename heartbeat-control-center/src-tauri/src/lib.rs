//! Tragentics Heartbeat Control Center — Tauri shell.
//!
//! Architecture:
//!   - `engine::Core` (pure state machine) behind a Mutex
//!   - `vault::Vault` (encrypted token store) behind a Mutex — lock order is
//!     ALWAYS core → vault, never nested the other way
//!   - a 1s scheduler tick that launches due beats (≤2/tick, ≤4 in flight)
//!   - a 3s persistence tick that flushes dirty config/history atomically
//!   - the webview is a pure renderer: no network access (CSP), no tokens

pub mod api;
pub mod commands;
pub mod config;
pub mod engine;
pub mod errors;
pub mod history;
pub mod vault;

use api::ApiClient;
use config::AppConfig;
use engine::{Core, DueJob, TraySeverity};
use history::{ActivityEvent, BeatKind, HistoryStore};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_notification::NotificationExt;

pub const SNAPSHOT_EVENT: &str = "hcc://snapshot";

pub struct Paths {
    pub config: PathBuf,
    pub vault: PathBuf,
    pub history: PathBuf,
}

pub struct AppState {
    pub core: Mutex<Core>,
    pub vault: Mutex<vault::Vault>,
    pub api: ApiClient,
    pub paths: Paths,
    pub quitting: AtomicBool,
    pub tray_hide_notified: AtomicBool,
    pub pause_menu_item: Mutex<Option<MenuItem<Wry>>>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Snapshot (frontend view model) ─────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct SparkPoint {
    pub ok: bool,
    pub latency_ms: Option<u32>,
}

#[derive(Serialize)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub fingerprint: String,
    pub in_service: bool,
    pub interval_secs: u64,
    pub local_check: Option<config::LocalCheck>,
    pub added_at: u64,
    pub in_flight: bool,
    pub consecutive_failures: u32,
    pub next_beat_at: Option<u64>,
    pub last_attempt_at: Option<u64>,
    pub last_success_at: Option<u64>,
    pub last_kind: Option<BeatKind>,
    pub last_note: Option<String>,
    pub last_latency_ms: Option<u32>,
    pub platform_status: Option<String>,
    pub platform_last_heartbeat: Option<String>,
    pub halted: Option<engine::Halt>,
    pub local_check_failing: bool,
    pub stats_24h: history::AgentStats,
    pub stats_7d: history::AgentStats,
    pub spark: Vec<SparkPoint>,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub version: String,
    pub vault_state: vault::VaultState,
    pub vault_mode: Option<vault::VaultMode>,
    pub paused: bool,
    pub base_url: String,
    pub theme: String,
    pub minimize_to_tray: bool,
    pub notify_on_problems: bool,
    pub default_interval_secs: u64,
    pub autostart_enabled: bool,
    pub tray: TraySeverity,
    pub now: u64,
    pub agents: Vec<AgentView>,
    pub activity: Vec<ActivityEvent>,
}

pub fn build_snapshot(app: &AppHandle, state: &AppState) -> Snapshot {
    let (vault_state, vault_mode) = {
        let vault = state.vault.lock().unwrap();
        (vault.state(), vault.mode())
    };
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let core = state.core.lock().unwrap();
    let now = now_secs();
    let agents = core
        .config
        .agents
        .iter()
        .map(|a| {
            let rt = core.runtime.get(&a.id).cloned().unwrap_or_default();
            let beats = core.history.beats_for(&a.id);
            let spark = beats
                .iter()
                .rev()
                .take(30)
                .rev()
                .map(|b| SparkPoint {
                    ok: b.kind.is_success(),
                    latency_ms: b.latency_ms,
                })
                .collect();
            AgentView {
                id: a.id.clone(),
                name: a.name.clone(),
                fingerprint: a.fingerprint.clone(),
                in_service: a.in_service,
                interval_secs: a.interval_secs,
                local_check: a.local_check.clone(),
                added_at: a.added_at,
                in_flight: rt.in_flight,
                consecutive_failures: rt.consecutive_failures,
                next_beat_at: if a.in_service && rt.halted.is_none() && rt.next_beat_at != u64::MAX
                {
                    Some(rt.next_beat_at)
                } else {
                    None
                },
                last_attempt_at: rt.last_attempt_at,
                last_success_at: rt.last_success_at,
                last_kind: rt.last_kind,
                last_note: rt.last_note.clone(),
                last_latency_ms: rt.last_latency_ms,
                platform_status: rt.platform_status.clone(),
                platform_last_heartbeat: rt.platform_last_heartbeat.clone(),
                halted: rt.halted.clone(),
                local_check_failing: rt.local_check_failing,
                stats_24h: core.history.stats_for(&a.id, now, 24 * 3600),
                stats_7d: core.history.stats_for(&a.id, now, 7 * 24 * 3600),
                spark,
            }
        })
        .collect();

    Snapshot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        vault_state,
        vault_mode,
        paused: core.paused,
        base_url: core.config.base_url.clone(),
        theme: core.config.theme.clone(),
        minimize_to_tray: core.config.minimize_to_tray,
        notify_on_problems: core.config.notify_on_problems,
        default_interval_secs: core.config.default_interval_secs,
        autostart_enabled,
        tray: core.tray_severity(),
        now,
        agents,
        // Full retained window (ACTIVITY_CAP) — the Health tab paginates it.
        activity: core.history.recent_activity(history::ACTIVITY_CAP),
    }
}

pub fn emit_snapshot(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = build_snapshot(app, &state);
    let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
}

pub fn maybe_notify(app: &AppHandle, message: Option<String>) {
    let Some(message) = message else { return };
    let state = app.state::<AppState>();
    let enabled = state.core.lock().unwrap().config.notify_on_problems;
    if enabled {
        let _ = app
            .notification()
            .builder()
            .title("Tragentics Heartbeat Control Center")
            .body(&message)
            .show();
    }
}

// ── Tray ───────────────────────────────────────────────────────────────────

const TRAY_ID: &str = "hcc-tray";

fn tray_icon_bytes(severity: TraySeverity) -> &'static [u8] {
    match severity {
        TraySeverity::Ok => include_bytes!("../icons/tray-ok.png"),
        TraySeverity::Degraded => include_bytes!("../icons/tray-warn.png"),
        TraySeverity::Halted => include_bytes!("../icons/tray-err.png"),
        TraySeverity::Paused => include_bytes!("../icons/tray-paused.png"),
    }
}

pub fn update_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (severity, tooltip, paused) = {
        let core = state.core.lock().unwrap();
        let severity = core.tray_severity();
        let in_service = core.config.agents.iter().filter(|a| a.in_service).count();
        let halted = core
            .config
            .agents
            .iter()
            .filter(|a| {
                core.runtime
                    .get(&a.id)
                    .map(|r| r.halted.is_some())
                    .unwrap_or(false)
            })
            .count();
        let tooltip = if core.paused {
            "Tragentics HCC — paused".to_string()
        } else if halted > 0 {
            format!("Tragentics HCC — {in_service} in service, {halted} halted")
        } else {
            format!("Tragentics HCC — {in_service} in service")
        };
        (severity, tooltip, core.paused)
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(icon) = tauri::image::Image::from_bytes(tray_icon_bytes(severity)) {
            let _ = tray.set_icon(Some(icon));
        }
        let _ = tray.set_tooltip(Some(tooltip));
    }
    let menu_guard = state.pause_menu_item.lock().unwrap();
    if let Some(item) = menu_guard.as_ref() {
        let _ = item.set_text(if paused {
            "Resume beating"
        } else {
            "Pause beating"
        });
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

// ── Beat execution ─────────────────────────────────────────────────────────

async fn run_beat(app: AppHandle, job: DueJob) {
    let state = app.state::<AppState>();
    let base_url = { state.core.lock().unwrap().config.base_url.clone() };
    let token = {
        let vault = state.vault.lock().unwrap();
        vault.token_for(&job.agent_id)
    };
    let Some(token) = token else {
        // Vault locked or token missing mid-flight: release the slot, retry later.
        let mut core = state.core.lock().unwrap();
        if let Some(rt) = core.runtime.get_mut(&job.agent_id) {
            rt.in_flight = false;
            rt.next_beat_at = now_secs() + 60;
        }
        let name = core.agent_name(&job.agent_id);
        core.push_activity(
            now_secs(),
            "warn",
            Some(&job.agent_id),
            format!("{name}: token unavailable (vault locked?) — retrying in 60s"),
        );
        drop(core);
        emit_snapshot(&app);
        return;
    };

    // Optional local health check — honest offline when the user's own server fails.
    let mut desired = "online";
    let mut local_note: Option<String> = None;
    if let Some(check) = &job.local_check {
        match state
            .api
            .local_check(
                &check.url,
                check.expect_min,
                check.expect_max,
                check.timeout_secs,
            )
            .await
        {
            Ok(_) => {}
            Err(reason) => {
                desired = "offline";
                local_note = Some(format!("local check failed: {reason}"));
            }
        }
    }

    let outcome = state
        .api
        .send_heartbeat(&base_url, &token, &job.agent_id, desired)
        .await;

    let notification = {
        let mut core = state.core.lock().unwrap();
        match outcome {
            Ok(success) => {
                let kind = if desired == "online" {
                    BeatKind::OnlineOk
                } else {
                    BeatKind::LocalFailOffline
                };
                let was_failing_locally = core
                    .runtime
                    .get(&job.agent_id)
                    .map(|r| r.local_check_failing)
                    .unwrap_or(false);
                core.apply_success(
                    &job.agent_id,
                    kind,
                    success.platform_status,
                    success.platform_last_heartbeat,
                    success.latency_ms,
                    local_note.clone(),
                    now_secs(),
                    rand::random::<f64>(),
                );
                if kind == BeatKind::LocalFailOffline && !was_failing_locally {
                    let name = core.agent_name(&job.agent_id);
                    let reason = local_note.unwrap_or_default();
                    core.push_activity(
                        now_secs(),
                        "warn",
                        Some(&job.agent_id),
                        format!("{name}: {reason} — reported offline to Tragentics"),
                    );
                    Some(format!(
                        "{name} failed its local health check — reported offline"
                    ))
                } else {
                    None
                }
            }
            Err(err) => core.apply_error(&job.agent_id, &err, now_secs()),
        }
    };
    maybe_notify(&app, notification);
    emit_snapshot(&app);
    update_tray(&app);
}

/// Send one explicit offline report outside the scheduler (toggle-off, remove,
/// quit). Never touches in_flight bookkeeping.
pub async fn run_oneshot_offline(app: &AppHandle, agent_id: &str, reason: &str) {
    let state = app.state::<AppState>();
    let (base_url, token) = {
        let core = state.core.lock().unwrap();
        let base = core.config.base_url.clone();
        drop(core);
        let vault = state.vault.lock().unwrap();
        (base, vault.token_for(agent_id))
    };
    let Some(token) = token else { return };
    let outcome = state
        .api
        .send_heartbeat(&base_url, &token, agent_id, "offline")
        .await;
    commands::record_offline_result(app, agent_id, outcome, reason);
}

pub async fn graceful_quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.quitting.store(true, Ordering::SeqCst);
    // Honest shutdown: report offline for every in-service agent, max 4s total.
    let ids: Vec<String> = {
        let core = state.core.lock().unwrap();
        core.config
            .agents
            .iter()
            .filter(|a| a.in_service)
            .map(|a| a.id.clone())
            .collect()
    };
    let sends = ids
        .iter()
        .map(|id| run_oneshot_offline(app, id, "Control Center shutting down"));
    let _ = tokio::time::timeout(Duration::from_secs(4), futures_join_all(sends)).await;
    persist_now(&state);
    app.exit(0);
}

/// Tiny join_all to avoid pulling the futures crate for one call site.
async fn futures_join_all<F: std::future::Future<Output = ()>>(futs: impl Iterator<Item = F>) {
    for f in futs {
        f.await;
    }
}

fn persist_now(state: &AppState) {
    let mut core = state.core.lock().unwrap();
    if core.dirty_config && core.config.save(&state.paths.config).is_ok() {
        core.dirty_config = false;
    }
    if core.dirty_history && core.history.save(&state.paths.history).is_ok() {
        core.dirty_history = false;
    }
}

// ── App entry ──────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let paths = Paths {
                config: data_dir.join("config.json"),
                vault: data_dir.join("vault.bin"),
                history: data_dir.join("history.json"),
            };

            let config = AppConfig::load(&paths.config);
            let history = HistoryStore::load(&paths.history);
            let mut vault = vault::Vault::new(paths.vault.clone());
            if let Err(e) = vault.open() {
                eprintln!("[hcc] vault open error: {e}");
            }

            let core = Core::new(config, history);
            app.manage(AppState {
                core: Mutex::new(core),
                vault: Mutex::new(vault),
                api: ApiClient::new(),
                paths,
                quitting: AtomicBool::new(false),
                tray_hide_notified: AtomicBool::new(false),
                pause_menu_item: Mutex::new(None),
            });

            // ── Tray ────────────────────────────────────────────────
            let open_item = MenuItemBuilder::with_id("open", "Open Control Center").build(app)?;
            let pause_item = MenuItemBuilder::with_id("pause", "Pause beating").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit (reports agents offline)").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&open_item, &pause_item, &quit_item])
                .build()?;
            {
                let state = app.state::<AppState>();
                *state.pause_menu_item.lock().unwrap() = Some(pause_item);
            }
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tauri::image::Image::from_bytes(tray_icon_bytes(TraySeverity::Ok))?)
                .tooltip("Tragentics Heartbeat Control Center")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "pause" => {
                        let state = app.state::<AppState>();
                        let paused = {
                            let mut core = state.core.lock().unwrap();
                            let next = !core.paused;
                            core.set_paused(next, now_secs());
                            next
                        };
                        let _ = paused;
                        emit_snapshot(app);
                        update_tray(app);
                    }
                    "quit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            graceful_quit(&app).await;
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // ── Window visibility ───────────────────────────────────
            let autostarted = std::env::args().any(|a| a == "--autostart");
            if !autostarted {
                show_main_window(app.handle());
            }

            // ── Scheduler tick ──────────────────────────────────────
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    let state = handle.state::<AppState>();
                    if state.quitting.load(Ordering::SeqCst) {
                        break;
                    }
                    let unlocked = {
                        let vault = state.vault.lock().unwrap();
                        vault.state() == vault::VaultState::Unlocked
                    };
                    if !unlocked {
                        continue;
                    }
                    let jobs = {
                        let mut core = state.core.lock().unwrap();
                        core.due_jobs(now_secs())
                    };
                    for job in jobs {
                        let app = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            run_beat(app, job).await;
                        });
                    }
                }
            });

            // ── Persistence tick ────────────────────────────────────
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(3));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    let state = handle.state::<AppState>();
                    persist_now(&state);
                    if state.quitting.load(Ordering::SeqCst) {
                        break;
                    }
                }
            });

            update_tray(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                if state.quitting.load(Ordering::SeqCst) {
                    return;
                }
                let minimize = { state.core.lock().unwrap().config.minimize_to_tray };
                if minimize {
                    api.prevent_close();
                    let _ = window.hide();
                    if !state.tray_hide_notified.swap(true, Ordering::SeqCst) {
                        let _ = app
                            .notification()
                            .builder()
                            .title("Still beating")
                            .body("Heartbeat Control Center keeps running in the tray. Quit from the tray menu to stop.")
                            .show();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::vault_initialize,
            commands::vault_unlock,
            commands::vault_lock,
            commands::vault_change_passphrase,
            commands::add_agent,
            commands::remove_agent,
            commands::set_in_service,
            commands::beat_now,
            commands::update_agent,
            commands::refresh_me,
            commands::get_agent_history,
            commands::update_settings,
            commands::set_paused,
            commands::set_autostart,
            commands::test_base_url,
            commands::open_data_dir,
            commands::quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running Heartbeat Control Center");
}
