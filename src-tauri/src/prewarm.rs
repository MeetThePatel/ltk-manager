use crate::error::{AppError, AppResult};
use crate::mods::ModLibraryState;
use crate::state::{Settings, SettingsState};
use camino::Utf8PathBuf;
use tauri::{Manager, State};

pub fn spawn(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        if let Err(e) = run(&app_handle) {
            tracing::warn!("Prewarming warning: {:?}", e);
        }
    });
}

fn run(app_handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Starting background prewarming...");

    let settings_state = app_handle
        .try_state::<SettingsState>()
        .ok_or("SettingsState not managed")?;
    let settings = settings_state
        .0
        .lock()
        .map_err(|e| format!("Failed to lock settings: {}", e))?
        .clone();

    if settings.league_path.is_none() {
        tracing::info!("Prewarming: no League path configured, skipping");
        return Ok(());
    }

    warm_champion_cache(app_handle, &settings_state);

    if settings.session_managed_patching_enabled {
        if let Some(library_state) = app_handle.try_state::<ModLibraryState>() {
            warm_active_profile_game_index(&settings, &library_state);
        }
    }

    tracing::info!("Background prewarming complete");
    Ok(())
}

fn warm_champion_cache(app_handle: &tauri::AppHandle, settings_state: &State<'_, SettingsState>) {
    tracing::info!("Prewarming: populating game champion cache...");
    match crate::commands::list_game_champions_inner(app_handle, settings_state) {
        Ok(_) => tracing::info!("Prewarming: game champion cache ready"),
        Err(e) => tracing::warn!(
            "Prewarming: failed to populate game champion cache: {:?}",
            e
        ),
    }
}

fn warm_active_profile_game_index(settings: &Settings, library_state: &ModLibraryState) {
    tracing::info!("Prewarming: warming active profile game index...");
    match load_active_profile_game_index(settings, library_state) {
        Ok(()) => tracing::info!("Prewarming: active profile game index ready"),
        Err(e) => tracing::warn!(
            "Prewarming: failed to warm active profile game index: {:?}",
            e
        ),
    }
}

fn load_active_profile_game_index(
    settings: &Settings,
    library_state: &ModLibraryState,
) -> AppResult<()> {
    let game_dir = crate::overlay::resolve_game_dir(settings)?;
    let profile_dir = library_state.0.active_profile_dir(settings)?;
    let utf8_game_dir = Utf8PathBuf::from_path_buf(game_dir.clone())
        .map_err(|p| AppError::Other(format!("Non-UTF-8 game directory path: {}", p.display())))?;
    let utf8_profile_dir = Utf8PathBuf::from_path_buf(profile_dir.clone()).map_err(|p| {
        AppError::Other(format!("Non-UTF-8 profile directory path: {}", p.display()))
    })?;
    let cache_path = utf8_profile_dir.join("game_index.bin");

    std::fs::create_dir_all(&profile_dir)?;
    ltk_overlay::GameIndex::load_or_build(&utf8_game_dir, &cache_path)
        .map_err(|e| AppError::Other(format!("Game index prewarm failed: {}", e)))?;
    Ok(())
}
