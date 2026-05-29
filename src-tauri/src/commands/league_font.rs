use crate::error::{AppResult, IpcResult};
use crate::league_font;
use crate::mods::{FontSelection, FontValidation, SystemFont};

/// List all system fonts with validation information.
#[tauri::command]
pub fn list_system_fonts() -> IpcResult<Vec<SystemFont>> {
    let result: AppResult<Vec<SystemFont>> = Ok(league_font::discover_system_fonts());
    result.into()
}

/// Validate a selected system/local font file.
#[tauri::command]
pub fn validate_league_font(selection: FontSelection) -> IpcResult<FontValidation> {
    let result: AppResult<FontValidation> = Ok(league_font::validate_league_font(selection));
    result.into()
}
