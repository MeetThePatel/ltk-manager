use crate::error::{IpcResult, MutexResultExt};
use crate::league_session::{LeagueSessionState, LeagueSessionStateInner};
use tauri::State;

#[tauri::command]
pub fn get_league_session_state(
    state: State<'_, LeagueSessionState>,
) -> IpcResult<LeagueSessionStateInner> {
    let inner = match state.0.lock().mutex_err() {
        Ok(guard) => guard.clone(),
        Err(e) => return IpcResult::err(e),
    };
    IpcResult::ok(inner)
}
