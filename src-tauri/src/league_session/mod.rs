use crate::commands::list_game_champions_inner;
use crate::commands::patcher::{
    start_patcher_inner_with_actualization, stop_patcher_inner, PatcherConfig,
};
use crate::league_client::{
    models::{ChampSelectSession, LeagueGameflowPhase},
    LeagueClient,
};
use crate::mods::ModLibraryState;
use crate::patcher::PatcherState;
use crate::state::SettingsState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DetectedChampion {
    pub champion_id: String, // lowercase stored alias, e.g. "lux"
    pub champion_key: u32,   // numeric key from LCU, e.g. 99
    pub champion_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
pub enum LeagueSessionLifecycle {
    Disabled,
    ClientUnavailable,
    Idle,
    ObservingChampSelect,
    PendingActualization,
    Actualizing,
    Patching,
    Clearing,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LeagueSessionStateInner {
    pub enabled: bool,
    pub client_available: bool,
    pub phase: LeagueGameflowPhase,
    pub observed_champion: Option<DetectedChampion>,
    pub actualized_champion: Option<DetectedChampion>,
    pub lifecycle: LeagueSessionLifecycle,
    pub last_error: Option<String>,
    pub last_updated_ms: u64,
    pub current_session_id: Option<String>,
}

#[derive(Clone)]
pub struct LeagueSessionState(pub Arc<Mutex<LeagueSessionStateInner>>);

pub enum TransitionAction {
    None,
    Actualize(u32),
    StopPatcher,
}

/// Pure function to calculate state machine transitions
pub fn determine_transition(
    enabled: bool,
    current_lifecycle: LeagueSessionLifecycle,
    client_available: bool,
    phase: LeagueGameflowPhase,
    last_known_observed: Option<DetectedChampion>,
) -> (LeagueSessionLifecycle, TransitionAction) {
    if !enabled {
        return (
            LeagueSessionLifecycle::Disabled,
            if current_lifecycle == LeagueSessionLifecycle::Patching {
                TransitionAction::StopPatcher
            } else {
                TransitionAction::None
            },
        );
    }

    if !client_available {
        return (
            LeagueSessionLifecycle::ClientUnavailable,
            if current_lifecycle == LeagueSessionLifecycle::Patching {
                TransitionAction::StopPatcher
            } else {
                TransitionAction::None
            },
        );
    }

    match current_lifecycle {
        LeagueSessionLifecycle::Disabled | LeagueSessionLifecycle::ClientUnavailable => {
            (LeagueSessionLifecycle::Idle, TransitionAction::None)
        }
        LeagueSessionLifecycle::Idle => {
            if phase == LeagueGameflowPhase::ChampSelect {
                (
                    LeagueSessionLifecycle::ObservingChampSelect,
                    TransitionAction::None,
                )
            } else if matches!(
                phase,
                LeagueGameflowPhase::GameStart
                    | LeagueGameflowPhase::InProgress
                    | LeagueGameflowPhase::Reconnect
            ) {
                // In case they launched/reconnected mid-session
                if let Some(champ) = last_known_observed {
                    (
                        LeagueSessionLifecycle::PendingActualization,
                        TransitionAction::Actualize(champ.champion_key),
                    )
                } else {
                    (
                        LeagueSessionLifecycle::PendingActualization,
                        TransitionAction::Actualize(0),
                    )
                }
            } else {
                (LeagueSessionLifecycle::Idle, TransitionAction::None)
            }
        }
        LeagueSessionLifecycle::ObservingChampSelect => {
            if phase == LeagueGameflowPhase::ChampSelect {
                (
                    LeagueSessionLifecycle::ObservingChampSelect,
                    TransitionAction::None,
                )
            } else if matches!(
                phase,
                LeagueGameflowPhase::GameStart
                    | LeagueGameflowPhase::InProgress
                    | LeagueGameflowPhase::Reconnect
            ) {
                if let Some(champ) = last_known_observed {
                    (
                        LeagueSessionLifecycle::PendingActualization,
                        TransitionAction::Actualize(champ.champion_key),
                    )
                } else {
                    (
                        LeagueSessionLifecycle::PendingActualization,
                        TransitionAction::Actualize(0),
                    )
                }
            } else if matches!(
                phase,
                LeagueGameflowPhase::None
                    | LeagueGameflowPhase::Lobby
                    | LeagueGameflowPhase::Matchmaking
                    | LeagueGameflowPhase::ReadyCheck
            ) {
                (LeagueSessionLifecycle::Idle, TransitionAction::None)
            } else {
                (
                    LeagueSessionLifecycle::ObservingChampSelect,
                    TransitionAction::None,
                )
            }
        }
        LeagueSessionLifecycle::PendingActualization
        | LeagueSessionLifecycle::Actualizing
        | LeagueSessionLifecycle::Patching => {
            if matches!(
                phase,
                LeagueGameflowPhase::WaitingForStats
                    | LeagueGameflowPhase::PreEndOfGame
                    | LeagueGameflowPhase::EndOfGame
                    | LeagueGameflowPhase::Lobby
                    | LeagueGameflowPhase::None
            ) {
                (
                    LeagueSessionLifecycle::Clearing,
                    TransitionAction::StopPatcher,
                )
            } else {
                (current_lifecycle, TransitionAction::None)
            }
        }
        LeagueSessionLifecycle::Clearing => (LeagueSessionLifecycle::Idle, TransitionAction::None),
        LeagueSessionLifecycle::Error => {
            if matches!(
                phase,
                LeagueGameflowPhase::None | LeagueGameflowPhase::Lobby
            ) {
                (LeagueSessionLifecycle::Idle, TransitionAction::None)
            } else {
                (LeagueSessionLifecycle::Error, TransitionAction::None)
            }
        }
    }
}

pub fn champion_alias_by_key(
    app_handle: &AppHandle,
    settings_state: &State<SettingsState>,
) -> HashMap<u32, DetectedChampion> {
    let mut map = HashMap::new();
    if let Ok(champions) = list_game_champions_inner(app_handle, settings_state) {
        for champ in champions {
            if let Some(key) = champ.champion_key {
                map.insert(
                    key,
                    DetectedChampion {
                        champion_id: champ.champion_id.clone(),
                        champion_key: key,
                        champion_name: champ.champion_name.clone(),
                    },
                );
            }
        }
    }
    map
}

/// Background watcher task that polls the LCU and manages the state machine
pub fn spawn_league_session_watcher(app_handle: AppHandle) {
    std::thread::spawn(move || {
        let mut active_game_champions: HashMap<u32, DetectedChampion> = HashMap::new();

        loop {
            // Read settings to check enabled state and league path
            let settings_state = app_handle.state::<SettingsState>();
            let (enabled, league_path) = {
                if let Ok(settings) = settings_state.0.lock() {
                    (
                        settings.session_managed_patching_enabled,
                        settings.league_path.clone(),
                    )
                } else {
                    (true, None)
                }
            };

            if !enabled {
                // If disabled, transition immediately to Disabled
                update_session_state(&app_handle, |state| {
                    state.enabled = false;
                    let (next_lifecycle, action) = determine_transition(
                        false,
                        state.lifecycle,
                        state.client_available,
                        state.phase,
                        state.observed_champion.clone(),
                    );
                    state.lifecycle = next_lifecycle;
                    handle_transition_action(&app_handle, action, None);
                });
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }

            let league_path_resolved = match league_path {
                Some(p) => p,
                None => {
                    update_session_state(&app_handle, |state| {
                        state.client_available = false;
                        state.lifecycle = LeagueSessionLifecycle::ClientUnavailable;
                    });
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            // Build LCU client
            let client = LeagueClient::new(&league_path_resolved);
            let client_available = client.is_some();

            let (phase, hovered_champ_id) = if let Some(ref c) = client {
                let phase_res: Result<LeagueGameflowPhase, _> =
                    c.get_json("/lol-gameflow/v1/gameflow-phase");
                let phase = phase_res.unwrap_or(LeagueGameflowPhase::Unknown);

                let hovered = if phase == LeagueGameflowPhase::ChampSelect {
                    let session_res: Result<ChampSelectSession, _> =
                        c.get_json("/lol-champ-select/v1/session");
                    if let Ok(session) = session_res {
                        session.current_player_champion_id()
                    } else {
                        None
                    }
                } else {
                    None
                };

                (phase, hovered)
            } else {
                (LeagueGameflowPhase::None, None)
            };

            // Lazily load champion database when client becomes available
            if client_available && active_game_champions.is_empty() {
                active_game_champions = champion_alias_by_key(&app_handle, &settings_state);
            } else if !client_available {
                active_game_champions.clear();
            }

            let mut action_to_run = TransitionAction::None;
            let mut observed_champ_to_pass = None;

            // Update sessionInner state
            update_session_state(&app_handle, |state| {
                state.enabled = true;
                state.client_available = client_available;
                state.phase = phase;

                // Handle session tracking (generate a session ID when transitioning into game/champselect)
                if state.current_session_id.is_none() && phase != LeagueGameflowPhase::None {
                    let id = uuid::Uuid::new_v4().to_string();
                    state.current_session_id = Some(id);
                } else if phase == LeagueGameflowPhase::None {
                    state.current_session_id = None;
                }

                // If in ChampSelect and hovered champion is valid, update observed_champion
                if phase == LeagueGameflowPhase::ChampSelect {
                    if let Some(champ_id) = hovered_champ_id {
                        if let Some(champ) = active_game_champions.get(&champ_id) {
                            state.observed_champion = Some(champ.clone());
                        }
                    }
                }

                // Transition check
                let (next_lifecycle, action) = determine_transition(
                    true,
                    state.lifecycle,
                    client_available,
                    phase,
                    state.observed_champion.clone(),
                );

                if state.lifecycle != next_lifecycle {
                    tracing::info!(
                        "League session watcher transition: {:?} -> {:?}",
                        state.lifecycle,
                        next_lifecycle
                    );
                    state.lifecycle = next_lifecycle;

                    action_to_run = action;
                    observed_champ_to_pass = state.observed_champion.clone();

                    if next_lifecycle == LeagueSessionLifecycle::Idle {
                        // Reset session fields on returning to Idle
                        state.observed_champion = None;
                        state.actualized_champion = None;
                        state.last_error = None;
                    }
                }
            });

            if !matches!(action_to_run, TransitionAction::None) {
                handle_transition_action(&app_handle, action_to_run, observed_champ_to_pass);
            }

            // Adjust polling rate
            let sleep_duration = if !client_available {
                Duration::from_secs(2)
            } else if phase == LeagueGameflowPhase::ChampSelect {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(1)
            };

            std::thread::sleep(sleep_duration);
        }
    });
}

fn update_session_state<F>(app_handle: &AppHandle, f: F)
where
    F: FnOnce(&mut LeagueSessionStateInner),
{
    if let Some(state) = app_handle.try_state::<LeagueSessionState>() {
        if let Ok(mut inner) = state.0.lock() {
            f(&mut inner);
            inner.last_updated_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            // Emit changed event
            let _ = app_handle.emit("league-session-changed", inner.clone());
        }
    }
}

fn handle_transition_action(
    app_handle: &AppHandle,
    action: TransitionAction,
    observed_champ: Option<DetectedChampion>,
) {
    match action {
        TransitionAction::Actualize(champ_key) => {
            tracing::info!(
                "League session watcher actualization requested: key={}",
                champ_key
            );

            // Determine actualization mode
            let actualization = if champ_key > 0 {
                if let Some(ref champ) = observed_champ {
                    crate::overlay::SkinRemapActualization::ChampionAlias(champ.champion_id.clone())
                } else {
                    crate::overlay::SkinRemapActualization::None
                }
            } else {
                crate::overlay::SkinRemapActualization::None
            };

            // Set state to Actualizing
            update_session_state(app_handle, |state| {
                state.lifecycle = LeagueSessionLifecycle::Actualizing;
                state.actualized_champion = observed_champ.clone();
                if state.actualized_champion.is_none() {
                    state.last_error = Some("No champion detected or configured".to_string());
                }
            });

            // Trigger Patcher
            let config = PatcherConfig {
                log_file: None,
                timeout_ms: None,
                flags: None,
                workshop_projects: None,
            };

            // Run in a background thread to prevent blocking the LCU watcher loop
            let app_handle_clone = app_handle.clone();
            std::thread::spawn(move || {
                let patcher_state = app_handle_clone.state::<PatcherState>();
                let settings_state = app_handle_clone.state::<SettingsState>();
                let library_state = app_handle_clone.state::<ModLibraryState>();

                let start_res = start_patcher_inner_with_actualization(
                    config,
                    &app_handle_clone,
                    &patcher_state,
                    &settings_state,
                    &library_state,
                    actualization,
                );

                update_session_state(&app_handle_clone, |state| match start_res {
                    Ok(_) => {
                        state.lifecycle = LeagueSessionLifecycle::Patching;
                    }
                    Err(e) => {
                        tracing::error!("League session watcher failed to start patcher: {}", e);
                        state.lifecycle = LeagueSessionLifecycle::Error;
                        state.last_error = Some(e.to_string());
                    }
                });
            });
        }
        TransitionAction::StopPatcher => {
            tracing::info!("League session watcher: stopping patcher loop");
            let patcher_state = app_handle.state::<PatcherState>();
            let _ = stop_patcher_inner(&patcher_state);

            update_session_state(app_handle, |state| {
                state.lifecycle = LeagueSessionLifecycle::Clearing;
            });

            // Wait for patcher to fully stop in background
            let app_handle_clone = app_handle.clone();
            std::thread::spawn(move || {
                let patcher_state = app_handle_clone.state::<PatcherState>();
                loop {
                    let running = {
                        if let Ok(ps) = patcher_state.0.lock() {
                            ps.is_running()
                        } else {
                            false
                        }
                    };

                    if !running {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }

                update_session_state(&app_handle_clone, |state| {
                    state.lifecycle = LeagueSessionLifecycle::Idle;
                    state.actualized_champion = None;
                });
            });
        }
        TransitionAction::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_transition() {
        let lux = DetectedChampion {
            champion_id: "lux".to_string(),
            champion_key: 99,
            champion_name: "Lux".to_string(),
        };

        // Disabled
        let (lifecycle, action) = determine_transition(
            false,
            LeagueSessionLifecycle::Idle,
            true,
            LeagueGameflowPhase::ChampSelect,
            None,
        );
        assert_eq!(lifecycle, LeagueSessionLifecycle::Disabled);
        assert!(matches!(action, TransitionAction::None));

        // Client Unavailable
        let (lifecycle, action) = determine_transition(
            true,
            LeagueSessionLifecycle::Idle,
            false,
            LeagueGameflowPhase::None,
            None,
        );
        assert_eq!(lifecycle, LeagueSessionLifecycle::ClientUnavailable);
        assert!(matches!(action, TransitionAction::None));

        // Idle to Observing ChampSelect
        let (lifecycle, action) = determine_transition(
            true,
            LeagueSessionLifecycle::Idle,
            true,
            LeagueGameflowPhase::ChampSelect,
            None,
        );
        assert_eq!(lifecycle, LeagueSessionLifecycle::ObservingChampSelect);
        assert!(matches!(action, TransitionAction::None));

        // ChampSelect dodged/cancelled
        let (lifecycle, action) = determine_transition(
            true,
            LeagueSessionLifecycle::ObservingChampSelect,
            true,
            LeagueGameflowPhase::Lobby,
            None,
        );
        assert_eq!(lifecycle, LeagueSessionLifecycle::Idle);
        assert!(matches!(action, TransitionAction::None));

        // ChampSelect to GameStart (actualization!)
        let (lifecycle, action) = determine_transition(
            true,
            LeagueSessionLifecycle::ObservingChampSelect,
            true,
            LeagueGameflowPhase::GameStart,
            Some(lux.clone()),
        );
        assert_eq!(lifecycle, LeagueSessionLifecycle::PendingActualization);
        assert!(matches!(action, TransitionAction::Actualize(99)));

        // Patching to Clearing (game ended)
        let (lifecycle, action) = determine_transition(
            true,
            LeagueSessionLifecycle::Patching,
            true,
            LeagueGameflowPhase::WaitingForStats,
            Some(lux),
        );
        assert_eq!(lifecycle, LeagueSessionLifecycle::Clearing);
        assert!(matches!(action, TransitionAction::StopPatcher));
    }
}
