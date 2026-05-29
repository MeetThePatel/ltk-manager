use tauri::Manager;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_deep_link::DeepLinkExt;

use crate::deep_link::DeepLinkState;
use crate::mods::{ModLibrary, ModLibraryState, WadReportState};
use crate::patcher::PatcherState;
use crate::state::SettingsState;
use crate::workshop::{Workshop, WorkshopState};

pub fn run(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();

    #[cfg(debug_assertions)]
    {
        let logging_guards: tauri::State<'_, crate::logging::LoggingGuards> = app_handle.state();
        let _ = logging_guards.app_handle_holder.set(app_handle.clone());
    }

    let settings_state = SettingsState::new(&app_handle);
    let patcher_state = PatcherState::new();
    let mod_library = ModLibraryState(ModLibrary::new(&app_handle));
    let workshop = WorkshopState(Workshop::new(&app_handle));

    initialize_first_run(&app_handle, &settings_state);

    let settings = settings_state.0.lock().unwrap().clone();

    // Register WadReportState BEFORE reconcile so that the reconcile pass can
    // invalidate stale reports and prune orphans on the first startup.
    let storage_dir = mod_library.0.storage_dir(&settings).ok();
    let wad_report_state = WadReportState::new(storage_dir.as_deref());
    app.manage(wad_report_state);

    match mod_library.0.reconcile_index(&settings) {
        Ok(true) => tracing::info!("Library index reconciled on startup"),
        Ok(false) => {}
        Err(e) => tracing::warn!("Failed to reconcile library on startup: {}", e),
    }

    let hotkey_manager = crate::hotkeys::HotkeyManager::new(&app_handle);
    hotkey_manager.register_from_settings(&settings);

    let autolaunch = app_handle.autolaunch();
    if settings.auto_run {
        let _ = autolaunch.enable();
    } else {
        let _ = autolaunch.disable();
    }

    let deep_link_state = DeepLinkState::new();

    let league_session = crate::league_session::LeagueSessionState(std::sync::Arc::new(
        std::sync::Mutex::new(crate::league_session::LeagueSessionStateInner {
            enabled: true,
            client_available: false,
            phase: crate::league_client::models::LeagueGameflowPhase::None,
            observed_champion: None,
            actualized_champion: None,
            lifecycle: crate::league_session::LeagueSessionLifecycle::Idle,
            last_error: None,
            last_updated_ms: 0,
            current_session_id: None,
        }),
    ));

    app.manage(settings_state);
    app.manage(patcher_state);
    app.manage(mod_library);
    app.manage(workshop);
    app.manage(hotkey_manager);
    app.manage(deep_link_state);
    app.manage(league_session);

    crate::league_session::spawn_league_session_watcher(app_handle.clone());

    crate::tray::setup(app)?;

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.set_decorations(false);
        }
    }

    {
        let settings_state: tauri::State<'_, SettingsState> = app_handle.state();
        let settings = settings_state.0.lock().unwrap();
        if settings.watcher_enabled {
            crate::mods::watcher::start_library_watcher(&app_handle);
        }
    }

    {
        let settings_state: tauri::State<'_, SettingsState> = app_handle.state();
        let settings = settings_state.0.lock().unwrap();
        if settings.start_in_tray || settings.start_in_tray_unless_update {
            if let Some(window) = app_handle.get_webview_window("main") {
                let _ = window.hide();
            }
        }
    }

    if let Ok(Some(urls)) = app.deep_link().get_current() {
        crate::deep_link::handle_urls(&app_handle, &urls);
    }

    let handle_clone = app_handle.clone();
    app.deep_link().on_open_url(move |event| {
        crate::deep_link::handle_urls(&handle_clone, &event.urls());
    });

    crate::prewarm::spawn(app_handle.clone());

    Ok(())
}

/// Perform first-run initialization:
/// - If league_path is not set, attempt auto-detection
/// - If auto-detection succeeds, save the path
fn initialize_first_run(app_handle: &tauri::AppHandle, settings_state: &SettingsState) {
    let mut settings = match settings_state.0.lock() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to lock settings: {}", e);
            return;
        }
    };

    if settings.league_path.is_some() {
        tracing::info!("League path already configured, skipping auto-detection");
        return;
    }

    tracing::info!("Attempting auto-detection of League installation...");

    if let Some(exe_path) = ltk_mod_core::auto_detect_league_path() {
        let path = std::path::Path::new(exe_path.as_str());

        if let Some(install_root) = resolve_auto_detected_install_root(path) {
            tracing::info!("Auto-detected League at: {:?}", install_root);
            settings.league_path = Some(install_root.to_path_buf());
            settings.first_run_complete = true;

            if let Err(e) = crate::state::save_settings_to_disk(app_handle, &settings) {
                tracing::error!("Failed to save auto-detected settings: {}", e);
            }
        }
    } else {
        tracing::info!("Auto-detection did not find League installation");
    }
}

fn resolve_auto_detected_install_root(path: &std::path::Path) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    if let Some(app_root) = macos_app_bundle_root(path) {
        return Some(app_root);
    }

    // Navigate from "Game/League of Legends.exe" to installation root.
    path.parent()
        .and_then(|p| p.parent())
        .map(std::path::Path::to_path_buf)
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_root(path: &std::path::Path) -> Option<std::path::PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.extension().and_then(|ext| ext.to_str()) == Some("app"))
        .map(std::path::Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn resolves_regular_install_root_from_executable_path() {
        let exe = Path::new("/Games/League of Legends")
            .join("Game")
            .join("League of Legends.exe");

        assert_eq!(
            resolve_auto_detected_install_root(&exe).unwrap(),
            PathBuf::from("/Games/League of Legends")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resolves_macos_app_bundle_from_executable_path() {
        let exe = Path::new("/Applications/League of Legends.app")
            .join("Contents")
            .join("LoL")
            .join("Game")
            .join("League of Legends");

        assert_eq!(
            resolve_auto_detected_install_root(&exe).unwrap(),
            PathBuf::from("/Applications/League of Legends.app")
        );
    }
}
