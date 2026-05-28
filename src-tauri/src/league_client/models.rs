use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum LeagueGameflowPhase {
    None,
    Lobby,
    Matchmaking,
    ReadyCheck,
    ChampSelect,
    GameStart,
    InProgress,
    Reconnect,
    WaitingForStats,
    PreEndOfGame,
    EndOfGame,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectSession {
    pub local_player_cell_id: i64,
    pub my_team: Vec<ChampSelectPlayerSelection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectPlayerSelection {
    pub cell_id: i64,
    pub champion_id: u32,
    pub champion_pick_intent: u32,
    pub selected_skin_id: u32,
}

impl ChampSelectSession {
    pub fn current_player_champion_id(&self) -> Option<u32> {
        self.my_team
            .iter()
            .find(|player| player.cell_id == self.local_player_cell_id)
            .map(|player| player.champion_id)
            .filter(|&id| id > 0)
    }
}
