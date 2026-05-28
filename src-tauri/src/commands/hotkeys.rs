use crate::error::{AppError, AppResult, IpcResult, MutexResultExt};
use crate::hotkeys::{HotkeyAction, HotkeyManager};
use crate::mods::ModLibraryState;
use crate::patcher::PatcherState;
use crate::state::{save_settings_to_disk, SettingsState};

use std::process::Command;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

use super::patcher::{start_patcher_inner, PatcherConfig};

// ── Hotkey action implementations (called from shortcut callbacks) ──

/// Execute hot-reload: stop patcher → kill League → restart patcher.
pub(crate) fn execute_hot_reload(app_handle: &AppHandle) -> AppResult<()> {
    let patcher_state = app_handle.state::<PatcherState>();
    let settings_state = app_handle.state::<SettingsState>();
    let library_state = app_handle.state::<ModLibraryState>();

    // Get the last config before stopping
    let last_config = {
        let ps = patcher_state.0.lock().mutex_err()?;
        ps.last_config.clone()
    };

    let config = match last_config {
        Some(c) => c,
        None => {
            tracing::trace!("Hot reload: no previous patcher session, ignoring");
            return Ok(());
        }
    };

    // Stop patcher if running
    {
        let ps = patcher_state.0.lock().mutex_err()?;
        if ps.is_running() {
            tracing::trace!("Hot reload: stopping patcher...");
            ps.stop_flag.store(true, Ordering::SeqCst);
        }
    }

    wait_for_patcher_stop(&patcher_state)?;
    kill_league_process();
    std::thread::sleep(std::time::Duration::from_millis(500));

    let workshop_projects = config.workshop_projects.clone();

    let patcher_config = PatcherConfig {
        log_file: config.log_file,
        timeout_ms: config.timeout_ms,
        flags: config.flags,
        workshop_projects: config.workshop_projects,
    };

    tracing::info!("Hot reload: restarting patcher");
    start_patcher_inner(
        patcher_config,
        app_handle,
        &patcher_state,
        &settings_state,
        &library_state,
    )?;

    // Best-effort LCU reconnect (in background — retries take time)
    let league_path = {
        let s = settings_state.0.lock().mutex_err()?;
        s.league_path.clone()
    };
    if let Some(path) = league_path {
        std::thread::spawn(move || crate::league_client::try_lcu_reconnect(&path));
    }

    // Emit workshop project paths so frontend can re-sync testing state
    let _ = app_handle.emit("hotkey-reload-complete", workshop_projects);
    Ok(())
}

/// Execute kill-league action.
pub(crate) fn execute_kill_league(app_handle: &AppHandle) -> AppResult<()> {
    let patcher_state = app_handle.state::<PatcherState>();
    let settings_state = app_handle.state::<SettingsState>();

    let should_stop_patcher = {
        let s = settings_state.0.lock().mutex_err()?;
        s.kill_league_stops_patcher
    };

    if should_stop_patcher {
        let ps = patcher_state.0.lock().mutex_err()?;
        if ps.is_running() {
            tracing::trace!("Kill league: also stopping patcher");
            ps.stop_flag.store(true, Ordering::SeqCst);
        }
        drop(ps);
        wait_for_patcher_stop(&patcher_state)?;
    }

    kill_league_process();
    Ok(())
}

// ── IPC commands (called from frontend) ──

/// Temporarily unregister all hotkeys (e.g. while capturing a new binding).
#[tauri::command]
pub fn pause_hotkeys(
    hotkeys: State<HotkeyManager>,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    pause_hotkeys_inner(&hotkeys, &settings).into()
}

fn pause_hotkeys_inner(
    hotkeys: &State<HotkeyManager>,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let s = settings.0.lock().mutex_err()?;
    hotkeys.pause(&s);
    Ok(())
}

/// Re-register all hotkeys after capture mode ends.
#[tauri::command]
pub fn resume_hotkeys(
    hotkeys: State<HotkeyManager>,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    resume_hotkeys_inner(&hotkeys, &settings).into()
}

fn resume_hotkeys_inner(
    hotkeys: &State<HotkeyManager>,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let s = settings.0.lock().mutex_err()?;
    hotkeys.resume(&s);
    Ok(())
}

/// Set (or clear) a global hotkey for the given action.
#[tauri::command]
pub fn set_hotkey(
    action: HotkeyAction,
    accelerator: Option<String>,
    app_handle: AppHandle,
    hotkeys: State<HotkeyManager>,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    set_hotkey_inner(action, accelerator, &app_handle, &hotkeys, &settings).into()
}

fn set_hotkey_inner(
    action: HotkeyAction,
    accelerator: Option<String>,
    app_handle: &AppHandle,
    hotkeys: &State<HotkeyManager>,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let mut s = settings.0.lock().mutex_err()?;
    let old_hotkey = action.get_accelerator(&s).map(str::to_string);

    match accelerator {
        Some(ref accel) if !accel.trim().is_empty() => {
            let trimmed = accel.trim().to_string();
            action.check_no_conflict(&s, &trimmed)?;
            hotkeys.register(action, &trimmed)?;
            if let Some(ref old) = old_hotkey {
                hotkeys.unregister(old);
            }
            action.set_accelerator(&mut s, Some(trimmed));
        }
        _ => {
            if let Some(ref old) = old_hotkey {
                hotkeys.unregister(old);
            }
            action.set_accelerator(&mut s, None);
        }
    }

    save_settings_to_disk(app_handle, &s)?;
    Ok(())
}

/// Hot-reload: stop patcher, kill League, restart patcher with the last config.
#[tauri::command]
pub fn hot_reload_mods(
    app_handle: AppHandle,
    state: State<PatcherState>,
    settings: State<SettingsState>,
    library: State<ModLibraryState>,
) -> IpcResult<()> {
    hot_reload_mods_inner(&app_handle, &state, &settings, &library).into()
}

fn hot_reload_mods_inner(
    app_handle: &AppHandle,
    state: &State<PatcherState>,
    settings: &State<SettingsState>,
    library: &State<ModLibraryState>,
) -> AppResult<()> {
    tracing::info!("Hot reload triggered via command");

    let last_config = {
        let ps = state.0.lock().mutex_err()?;
        ps.last_config.clone()
    };

    let config = last_config
        .ok_or_else(|| AppError::Other("No previous patcher config to reload with".to_string()))?;

    // Stop patcher if running
    {
        let ps = state.0.lock().mutex_err()?;
        if ps.is_running() {
            tracing::trace!("Stopping patcher for hot reload...");
            ps.stop_flag.store(true, Ordering::SeqCst);
        }
    }

    wait_for_patcher_stop(state)?;
    kill_league_process();
    std::thread::sleep(std::time::Duration::from_millis(500));

    let patcher_config = PatcherConfig {
        log_file: config.log_file,
        timeout_ms: config.timeout_ms,
        flags: config.flags,
        workshop_projects: config.workshop_projects,
    };

    tracing::info!("Restarting patcher after hot reload");
    start_patcher_inner(patcher_config, app_handle, state, settings, library)?;

    // Best-effort LCU reconnect (in background — retries take time)
    let league_path = {
        let s = settings.0.lock().mutex_err()?;
        s.league_path.clone()
    };
    if let Some(path) = league_path {
        std::thread::spawn(move || crate::league_client::try_lcu_reconnect(&path));
    }

    Ok(())
}

/// Kill League of Legends process, optionally stopping the patcher.
#[tauri::command]
pub fn kill_league(state: State<PatcherState>, settings: State<SettingsState>) -> IpcResult<()> {
    kill_league_inner(&state, &settings).into()
}

fn kill_league_inner(
    state: &State<PatcherState>,
    settings: &State<SettingsState>,
) -> AppResult<()> {
    let should_stop_patcher = {
        let s = settings.0.lock().mutex_err()?;
        s.kill_league_stops_patcher
    };

    if should_stop_patcher {
        let ps = state.0.lock().mutex_err()?;
        if ps.is_running() {
            tracing::info!("Kill league: also stopping patcher");
            ps.stop_flag.store(true, Ordering::SeqCst);
        }
        drop(ps);
        wait_for_patcher_stop(state)?;
    }

    kill_league_process();
    Ok(())
}

// ── Helpers ──

/// Wait for the patcher thread to finish (with timeout).
fn wait_for_patcher_stop(state: &PatcherState) -> AppResult<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        {
            let ps = state.0.lock().mutex_err()?;
            if !ps.is_running() {
                return Ok(());
            }
        }
        if std::time::Instant::now() > deadline {
            return Err(AppError::Other(
                "Timed out waiting for patcher to stop".to_string(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Kill the League of Legends game process.
fn kill_league_process() {
    tracing::info!("Killing League of Legends process");

    #[cfg(target_os = "windows")]
    let result = Command::new("taskkill")
        .args(["/F", "/IM", "League of Legends.exe"])
        .spawn();

    #[cfg(target_os = "macos")]
    let result = Command::new("pkill")
        .args(["-f", "League of Legends"])
        .spawn();

    #[cfg(target_os = "linux")]
    let result = Command::new("pkill")
        .args(["-f", "League of Legends"])
        .spawn();

    match result {
        Ok(mut child) => {
            let timeout = std::time::Duration::from_secs(3);
            let start = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        if start.elapsed() > timeout {
                            tracing::warn!("kill command timed out");
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        tracing::warn!("Error waiting for kill command: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to spawn kill command: {}", e);
        }
    }
}
