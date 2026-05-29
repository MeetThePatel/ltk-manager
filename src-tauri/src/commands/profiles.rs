use crate::error::{AppError, AppResult, IpcResult, MutexResultExt};
use crate::mods::{LeagueFontSettings, ModLibraryState, Profile, SkinRemap};
use crate::patcher::PatcherState;
use crate::state::SettingsState;
use tauri::State;

use super::mods::reject_if_patcher_running;

/// Get all profiles.
#[tauri::command]
pub fn list_mod_profiles(
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
) -> IpcResult<Vec<Profile>> {
    let result: AppResult<Vec<Profile>> = (|| {
        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.get_profiles(&settings)
    })();
    result.into()
}

/// Get the currently active profile.
#[tauri::command]
pub fn get_active_mod_profile(
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
) -> IpcResult<Profile> {
    let result: AppResult<Profile> = (|| {
        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.get_active_profile_info(&settings)
    })();
    result.into()
}

/// Create a new profile with the given name.
#[tauri::command]
pub fn create_mod_profile(
    name: String,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
) -> IpcResult<Profile> {
    let result: AppResult<Profile> = (|| {
        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.create_profile(&settings, name)
    })();
    result.into()
}

/// Delete a profile by ID.
#[tauri::command]
pub fn delete_mod_profile(
    profile_id: String,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
) -> IpcResult<()> {
    let result: AppResult<()> = (|| {
        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.delete_profile(&settings, profile_id)
    })();
    result.into()
}

/// Switch to a different profile.
/// Returns an error if the patcher is currently running.
#[tauri::command]
pub fn switch_mod_profile(
    profile_id: String,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
    patcher_state: State<PatcherState>,
) -> IpcResult<Profile> {
    let result: AppResult<Profile> = (|| {
        let patcher = patcher_state.0.lock().mutex_err()?;
        if patcher.is_running() {
            return Err(AppError::Other(
                "Cannot switch profiles while patcher is running. Please stop the patcher first."
                    .to_string(),
            ));
        }
        drop(patcher);

        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.switch_profile(&settings, profile_id)
    })();
    result.into()
}

/// Rename a profile.
/// Returns an error if the patcher is currently running (rename touches the filesystem).
#[tauri::command]
pub fn rename_mod_profile(
    profile_id: String,
    new_name: String,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
    patcher_state: State<PatcherState>,
) -> IpcResult<Profile> {
    let result: AppResult<Profile> = (|| {
        let patcher = patcher_state.0.lock().mutex_err()?;
        if patcher.is_running() {
            return Err(AppError::Other(
                "Cannot rename profiles while patcher is running. Please stop the patcher first."
                    .to_string(),
            ));
        }
        drop(patcher);

        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.rename_profile(&settings, profile_id, new_name)
    })();
    result.into()
}

/// Get skin remaps for a profile. Defaults to the active profile when profile_id is null.
#[tauri::command]
pub fn get_skin_remaps(
    profile_id: Option<String>,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
) -> IpcResult<Vec<SkinRemap>> {
    let result: AppResult<Vec<SkinRemap>> = (|| {
        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.get_skin_remaps(&settings, profile_id)
    })();
    result.into()
}

/// Add or replace one champion skin remap. Defaults to the active profile when profile_id is null.
#[tauri::command]
pub fn set_skin_remap(
    profile_id: Option<String>,
    remap: SkinRemap,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
    patcher: State<PatcherState>,
) -> IpcResult<Profile> {
    let result: AppResult<Profile> = (|| {
        reject_if_patcher_running(&patcher)?;
        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.set_skin_remap(&settings, profile_id, remap)
    })();
    result.into()
}

/// Remove one champion skin remap. Defaults to the active profile when profile_id is null.
#[tauri::command]
pub fn remove_skin_remap(
    profile_id: Option<String>,
    champion_id: String,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
    patcher: State<PatcherState>,
) -> IpcResult<Profile> {
    let result: AppResult<Profile> = (|| {
        reject_if_patcher_running(&patcher)?;
        let settings = settings.0.lock().mutex_err()?.clone();
        library
            .0
            .remove_skin_remap(&settings, profile_id, champion_id)
    })();
    result.into()
}

/// Get league font settings for a profile. Defaults to the active profile when profile_id is null.
#[tauri::command]
pub fn get_league_font_settings(
    profile_id: Option<String>,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
) -> IpcResult<LeagueFontSettings> {
    let result: AppResult<LeagueFontSettings> = (|| {
        let settings = settings.0.lock().mutex_err()?.clone();
        library.0.get_league_font_settings(&settings, profile_id)
    })();
    result.into()
}

/// Set league font settings for a profile. Defaults to the active profile when profile_id is null.
#[tauri::command]
pub fn set_league_font_settings(
    profile_id: Option<String>,
    font_settings: LeagueFontSettings,
    library: State<ModLibraryState>,
    settings: State<SettingsState>,
    patcher: State<PatcherState>,
) -> IpcResult<Profile> {
    let result: AppResult<Profile> = (|| {
        reject_if_patcher_running(&patcher)?;
        let settings = settings.0.lock().mutex_err()?.clone();
        library
            .0
            .set_league_font_settings(&settings, profile_id, font_settings)
    })();
    result.into()
}
