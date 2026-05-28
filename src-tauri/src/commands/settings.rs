use crate::error::{AppError, AppResult, IpcResult, MutexResultExt};
use crate::overlay::{list_game_wads, resolve_game_dir};
use crate::state::{get_app_data_dir, save_settings_to_disk, Settings, SettingsState};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use ts_rs::TS;
use walkdir::WalkDir;

const MAX_GAME_CHAMPION_JSON_BYTES: usize = 1024 * 1024;
const GAME_CHAMPION_CACHE_VERSION: u32 = 2;
const GAME_CHAMPION_CACHE_FILE: &str = "game-champions.json";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GameChampion {
    pub champion_id: String,
    pub champion_key: Option<u32>,
    pub champion_name: String,
    pub skins: Vec<GameSkin>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GameSkin {
    pub skin_number: u32,
    pub skin_name: String,
    pub chromas: Vec<GameChroma>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GameChroma {
    pub chroma_id: u32,
    pub chroma_name: String,
    pub colors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct GameChampionCacheSource {
    path: String,
    len: u64,
    modified_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameChampionCacheFile {
    version: u32,
    sources: Vec<GameChampionCacheSource>,
    champions: Vec<GameChampion>,
}

/// Get current settings.
#[tauri::command]
pub fn get_settings(state: State<SettingsState>) -> IpcResult<Settings> {
    get_settings_inner(&state).into()
}

fn get_settings_inner(state: &State<SettingsState>) -> AppResult<Settings> {
    let settings = state.0.lock().mutex_err()?;
    Ok(settings.clone())
}

/// Save settings.
#[tauri::command]
pub fn save_settings(
    settings: Settings,
    app_handle: AppHandle,
    state: State<SettingsState>,
) -> IpcResult<()> {
    save_settings_inner(settings, &app_handle, &state).into()
}

fn save_settings_inner(
    settings: Settings,
    app_handle: &AppHandle,
    state: &State<SettingsState>,
) -> AppResult<()> {
    // Sync OS autolaunch with the updated setting
    let autolaunch = app_handle.autolaunch();
    if settings.auto_run {
        let _ = autolaunch.enable();
    } else {
        let _ = autolaunch.disable();
    }

    save_settings_to_disk(app_handle, &settings)?;

    let mut current = state.0.lock().mutex_err()?;
    *current = settings;

    Ok(())
}

/// Auto-detect League of Legends installation path.
#[tauri::command]
pub fn auto_detect_league_path() -> IpcResult<Option<PathBuf>> {
    IpcResult::ok(auto_detect_league_path_inner())
}

fn auto_detect_league_path_inner() -> Option<PathBuf> {
    let exe_path = ltk_mod_core::auto_detect_league_path()?;
    let path = std::path::Path::new(&exe_path);

    #[cfg(target_os = "macos")]
    if let Some(app_root) = macos_app_bundle_root(path) {
        tracing::info!("Found League app bundle at: {:?}", app_root);
        return Some(app_root);
    }

    // Navigate from "Game/League of Legends.exe" to installation root
    let install_root = path.parent()?.parent()?;

    tracing::info!("Found League installation at: {:?}", install_root);
    Some(install_root.to_path_buf())
}

/// Validate a League installation path.
#[tauri::command]
pub fn validate_league_path(path: PathBuf) -> IpcResult<bool> {
    let valid = resolve_game_dir(&Settings {
        league_path: Some(path),
        ..Settings::default()
    })
    .is_ok();
    IpcResult::ok(valid)
}

/// List every WAD filename under the configured League install's `DATA` directory.
///
/// Used by the WAD blocklist editor for autocomplete and regex match previews.
/// Returns lowercased filenames sorted alphabetically.
#[tauri::command]
pub fn list_available_wads(state: State<SettingsState>) -> IpcResult<Vec<String>> {
    list_available_wads_inner(&state).into()
}

fn list_available_wads_inner(state: &State<SettingsState>) -> AppResult<Vec<String>> {
    let settings = state.0.lock().mutex_err()?.clone();
    let game_dir = resolve_game_dir(&settings)?;
    list_game_wads(&game_dir)
}

/// List champions and Riot skin slots from the local League game-data plugin.
#[tauri::command]
pub fn list_game_champions(
    app_handle: AppHandle,
    state: State<SettingsState>,
) -> IpcResult<Vec<GameChampion>> {
    list_game_champions_inner(&app_handle, &state).into()
}

pub(crate) fn list_game_champions_inner(
    app_handle: &AppHandle,
    state: &State<SettingsState>,
) -> AppResult<Vec<GameChampion>> {
    let settings = state.0.lock().mutex_err()?.clone();
    let game_dir = resolve_game_dir(&settings)?;

    let source = if let Some(champion_dir) = find_game_data_champion_dir(&settings, &game_dir) {
        GameChampionSource::JsonFiles(collect_champion_json_files(&champion_dir)?)
    } else {
        GameChampionSource::Wads(find_game_data_wad_files(&settings, &game_dir))
    };
    let source_paths = source.paths();
    let cache_sources = game_champion_cache_sources(&source_paths)?;

    if let Some(champions) = read_game_champion_cache(app_handle, &cache_sources) {
        return Ok(champions);
    }

    let mut champions = match source {
        GameChampionSource::JsonFiles(paths) => read_game_champions_from_files(&paths)?,
        GameChampionSource::Wads(paths) => read_game_champions_from_wads(&paths)?,
    };

    champions = dedupe_game_champions(champions);
    champions.sort_by(|a, b| a.champion_name.cmp(&b.champion_name));

    if champions.is_empty() {
        return Err(AppError::ValidationFailed(
            "Could not find League champion game data".to_string(),
        ));
    }

    write_game_champion_cache(app_handle, cache_sources, &champions);

    Ok(champions)
}

enum GameChampionSource {
    JsonFiles(Vec<PathBuf>),
    Wads(Vec<PathBuf>),
}

impl GameChampionSource {
    fn paths(&self) -> Vec<PathBuf> {
        match self {
            Self::JsonFiles(paths) | Self::Wads(paths) => paths.clone(),
        }
    }
}

fn collect_champion_json_files(champion_dir: &Path) -> AppResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(champion_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_game_champions_from_files(paths: &[PathBuf]) -> AppResult<Vec<GameChampion>> {
    let mut champions = Vec::new();
    for path in paths {
        if let Some(champion) = parse_game_champion_file(path) {
            champions.push(champion);
        }
    }

    Ok(champions)
}

fn game_champion_cache_path(app_handle: &AppHandle) -> Option<PathBuf> {
    get_app_data_dir(app_handle).map(|dir| dir.join("cache").join(GAME_CHAMPION_CACHE_FILE))
}

fn game_champion_cache_sources(paths: &[PathBuf]) -> AppResult<Vec<GameChampionCacheSource>> {
    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = fs::metadata(path)?;
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        sources.push(GameChampionCacheSource {
            path: path.display().to_string(),
            len: metadata.len(),
            modified_ms,
        });
    }
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(sources)
}

fn read_game_champion_cache(
    app_handle: &AppHandle,
    sources: &[GameChampionCacheSource],
) -> Option<Vec<GameChampion>> {
    let path = game_champion_cache_path(app_handle)?;
    let contents = fs::read_to_string(&path).ok()?;
    let cache: GameChampionCacheFile = match serde_json::from_str(&contents) {
        Ok(cache) => cache,
        Err(e) => {
            tracing::warn!("Ignoring corrupt game champion cache at {:?}: {}", path, e);
            return None;
        }
    };
    if cache.version == GAME_CHAMPION_CACHE_VERSION && cache.sources == sources {
        tracing::info!(
            "Loaded {} champions from game-data cache",
            cache.champions.len()
        );
        return Some(cache.champions);
    }
    None
}

fn write_game_champion_cache(
    app_handle: &AppHandle,
    sources: Vec<GameChampionCacheSource>,
    champions: &[GameChampion],
) {
    let Some(path) = game_champion_cache_path(app_handle) else {
        return;
    };
    let cache = GameChampionCacheFile {
        version: GAME_CHAMPION_CACHE_VERSION,
        sources,
        champions: champions.to_vec(),
    };
    let result: Result<(), Box<dyn std::error::Error>> = (|| {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_vec(&cache)?)?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::warn!("Failed to write game champion cache at {:?}: {}", path, e);
    }
}

fn find_game_data_champion_dir(settings: &Settings, game_dir: &Path) -> Option<PathBuf> {
    let relative = Path::new("Plugins")
        .join("rcp-be-lol-game-data")
        .join("global")
        .join("default")
        .join("v1")
        .join("champions");

    for root in league_root_candidates(settings, game_dir) {
        let direct = root.join(&relative);
        if direct.is_dir() {
            return Some(direct);
        }

        let league_client_app = root
            .join("LeagueClient.app")
            .join("Contents")
            .join("LoL")
            .join(&relative);
        if league_client_app.is_dir() {
            return Some(league_client_app);
        }

        for entry in WalkDir::new(&root)
            .max_depth(16)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_dir() {
                continue;
            }

            let path = entry.path();
            if entry.file_name() == "rcp-be-lol-game-data" {
                let champion_dir = path
                    .join("global")
                    .join("default")
                    .join("v1")
                    .join("champions");
                if champion_dir.is_dir() {
                    return Some(champion_dir);
                }
            }

            if entry.file_name() != "champions" {
                continue;
            }

            let normalized = path.to_string_lossy().replace('\\', "/").to_lowercase();
            if normalized.contains("rcp-be-lol-game-data/global/default/v1/champions") {
                return Some(path.to_path_buf());
            }
        }
    }

    None
}

fn find_game_data_wad_files(settings: &Settings, game_dir: &Path) -> Vec<PathBuf> {
    let mut wad_paths = Vec::new();

    for root in league_root_candidates(settings, game_dir) {
        let before_direct_candidates = wad_paths.len();
        for plugin_dir in game_data_plugin_dir_candidates(&root) {
            collect_wads_in_plugin_dir(&plugin_dir, &mut wad_paths);
        }
        if wad_paths.len() > before_direct_candidates {
            continue;
        }

        for entry in WalkDir::new(&root)
            .max_depth(16)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_dir() || entry.file_name() != "rcp-be-lol-game-data" {
                continue;
            }
            collect_wads_in_plugin_dir(entry.path(), &mut wad_paths);
        }
    }

    wad_paths.sort();
    wad_paths.dedup();
    wad_paths
}

fn game_data_plugin_dir_candidates(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("Plugins").join("rcp-be-lol-game-data"),
        root.join("Contents")
            .join("LoL")
            .join("Plugins")
            .join("rcp-be-lol-game-data"),
        root.join("LeagueClient.app")
            .join("Contents")
            .join("LoL")
            .join("Plugins")
            .join("rcp-be-lol-game-data"),
    ]
}

fn collect_wads_in_plugin_dir(plugin_dir: &Path, wad_paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(plugin_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("wad") {
            continue;
        }
        wad_paths.push(path);
    }
}

fn read_game_champions_from_wads(wad_paths: &[PathBuf]) -> AppResult<Vec<GameChampion>> {
    let mut champions = Vec::new();
    for path in wad_paths {
        let file = File::open(path)?;
        champions.extend(read_game_champions_from_wad_reader(file)?);
    }
    Ok(champions)
}

fn read_game_champions_from_wad_reader<R: Read + Seek>(source: R) -> AppResult<Vec<GameChampion>> {
    let mut wad = ltk_wad::Wad::mount(source)?;
    let chunks: Vec<ltk_wad::WadChunk> = wad
        .chunks()
        .iter()
        .copied()
        .filter(|chunk| {
            chunk.uncompressed_size > 0
                && chunk.uncompressed_size <= MAX_GAME_CHAMPION_JSON_BYTES
                && chunk.compression_type != ltk_wad::WadChunkCompression::Satellite
        })
        .collect();

    let mut champions = Vec::new();
    for chunk in chunks {
        let Ok(data) = wad.load_chunk_decompressed(&chunk) else {
            continue;
        };
        if !looks_like_json_object(&data) || !data.windows(7).any(|window| window == b"\"skins\"") {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) else {
            continue;
        };
        if let Some(champion) = parse_game_champion_value(&value, None) {
            champions.push(champion);
        }
    }

    Ok(champions)
}

fn looks_like_json_object(data: &[u8]) -> bool {
    data.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'{')
}

fn dedupe_game_champions(champions: Vec<GameChampion>) -> Vec<GameChampion> {
    let mut by_id = HashMap::new();
    for champion in champions {
        by_id
            .entry(champion.champion_id.clone())
            .and_modify(|existing: &mut GameChampion| {
                if champion.skins.len() > existing.skins.len() {
                    *existing = champion.clone();
                }
            })
            .or_insert(champion);
    }
    by_id.into_values().collect()
}

fn league_root_candidates(settings: &Settings, game_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = &settings.league_path {
        roots.push(path.clone());
        if path.file_name().and_then(|name| name.to_str()) == Some("Game") {
            if let Some(parent) = path.parent() {
                roots.push(parent.to_path_buf());
            }
        }
    }
    if let Some(parent) = game_dir.parent() {
        roots.push(parent.to_path_buf());
        if let Some(grandparent) = parent.parent() {
            roots.push(grandparent.to_path_buf());
            if let Some(great_grandparent) = grandparent.parent() {
                roots.push(great_grandparent.to_path_buf());
            }
        }
    }

    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn parse_game_champion_file(path: &Path) -> Option<GameChampion> {
    let contents = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&contents).ok()?;

    let fallback_id = path.file_stem()?.to_string_lossy().to_string();
    parse_game_champion_value(&value, Some(&fallback_id))
}

fn parse_game_champion_value(
    value: &serde_json::Value,
    fallback_id: Option<&str>,
) -> Option<GameChampion> {
    if value.get("isVisibleInClient").and_then(|v| v.as_bool()) == Some(false) {
        return None;
    }
    if value
        .get("id")
        .and_then(|v| v.as_i64())
        .is_some_and(|id| id < 0)
    {
        return None;
    }

    let raw_id = value
        .get("alias")
        .or_else(|| value.get("key"))
        .and_then(|v| v.as_str())
        .or(fallback_id)?;
    let champion_id = raw_id.trim().to_ascii_lowercase();
    if champion_id.is_empty() {
        return None;
    }

    let champion_key = value
        .get("id")
        .and_then(|v| v.as_i64())
        .filter(|&id| id > 0)
        .map(|id| id as u32);

    let champion_name = value
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.trim().to_string())
        .unwrap_or_else(|| title_case_identifier(&champion_id));

    let mut skins: Vec<GameSkin> = value
        .get("skins")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(parse_game_skin)
        .collect();
    if skins.is_empty() {
        return None;
    }
    skins.sort_by_key(|skin| skin.skin_number);

    Some(GameChampion {
        champion_id,
        champion_key,
        champion_name,
        skins,
    })
}

fn parse_game_skin(value: &serde_json::Value) -> Option<GameSkin> {
    let skin_number = value
        .get("num")
        .and_then(|v| v.as_u64())
        .or_else(|| value.get("id").and_then(|v| v.as_u64()).map(|id| id % 1000))?
        as u32;
    let raw_name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Default")
        .trim();
    let skin_name = if raw_name.eq_ignore_ascii_case("default") || raw_name.is_empty() {
        "Default".to_string()
    } else {
        raw_name.to_string()
    };
    let mut chromas: Vec<GameChroma> = value
        .get("chromas")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(parse_game_chroma)
        .collect();
    chromas.sort_by_key(|chroma| chroma.chroma_id);

    Some(GameSkin {
        skin_number,
        skin_name,
        chromas,
    })
}

fn parse_game_chroma(value: &serde_json::Value) -> Option<GameChroma> {
    let chroma_id = value.get("id").and_then(|v| v.as_u64())? as u32;
    let raw_name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Chroma")
        .trim();
    let chroma_name = if raw_name.is_empty() {
        "Chroma".to_string()
    } else {
        short_chroma_name(raw_name).to_string()
    };
    let colors = value
        .get("colors")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|color| color.as_str())
        .map(str::trim)
        .filter(|color| !color.is_empty())
        .map(ToString::to_string)
        .collect();

    Some(GameChroma {
        chroma_id,
        chroma_name,
        colors,
    })
}

fn short_chroma_name(name: &str) -> &str {
    let trimmed = name.trim();
    if let Some(prefix_end) = trimmed.rfind(" (") {
        if trimmed.ends_with(')') && prefix_end + 2 < trimmed.len() - 1 {
            return &trimmed[prefix_end + 2..trimmed.len() - 1];
        }
    }
    trimmed
}

fn title_case_identifier(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(target_os = "macos")]
fn macos_app_bundle_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.extension().and_then(|ext| ext.to_str()) == Some("app"))
        .map(Path::to_path_buf)
}

/// Check if initial setup is required (league path not configured).
#[tauri::command]
pub fn check_setup_required(state: State<SettingsState>) -> IpcResult<bool> {
    check_setup_required_inner(&state).into()
}

fn check_setup_required_inner(state: &State<SettingsState>) -> AppResult<bool> {
    let settings = state.0.lock().mutex_err()?;

    Ok(settings.league_path.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_league_path_accepts_resolvable_game_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("League of Legends");
        std::fs::create_dir_all(root.join("Game")).unwrap();

        assert!(matches!(
            validate_league_path(root),
            IpcResult::Ok { value: true }
        ));
    }

    #[test]
    fn parse_game_champion_file_reads_skin_slots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ahri.json");
        std::fs::write(
            &path,
            r##"{
                "id": 103,
                "alias": "Ahri",
                "name": "Ahri",
                "skins": [
                    { "num": 0, "name": "default" },
                    {
                        "num": 27,
                        "name": "Star Guardian Ahri",
                        "chromas": [
                            {
                                "id": 103027,
                                "name": "Star Guardian Ahri (Ruby)",
                                "colors": ["#D33528", "#F8E076"]
                            }
                        ]
                    }
                ]
            }"##,
        )
        .unwrap();

        let champion = parse_game_champion_file(&path).unwrap();
        assert_eq!(champion.champion_id, "ahri");
        assert_eq!(champion.champion_key, Some(103));
        assert_eq!(champion.champion_name, "Ahri");
        assert_eq!(champion.skins.len(), 2);
        assert_eq!(champion.skins[0].skin_number, 0);
        assert_eq!(champion.skins[0].skin_name, "Default");
        assert_eq!(champion.skins[1].skin_number, 27);
        assert_eq!(champion.skins[1].skin_name, "Star Guardian Ahri");
        assert_eq!(champion.skins[1].chromas.len(), 1);
        assert_eq!(champion.skins[1].chromas[0].chroma_id, 103027);
        assert_eq!(champion.skins[1].chromas[0].chroma_name, "Ruby");
        assert_eq!(
            champion.skins[1].chromas[0].colors,
            vec!["#D33528".to_string(), "#F8E076".to_string()]
        );
    }

    #[test]
    fn read_game_champions_from_wad_reader_scans_json_chunks() {
        use std::io::{Cursor, Write};

        let champion_json = br#"{
            "alias": "Ahri",
            "name": "Ahri",
            "skins": [
                { "num": 0, "name": "default" },
                {
                    "num": 27,
                    "name": "Star Guardian Ahri",
                    "chromas": [{ "id": 103028, "name": "Star Guardian Ahri (Ruby)" }]
                }
            ]
        }"#;
        let mut wad_data = Cursor::new(Vec::new());
        let builder = ltk_wad::WadBuilder::default().with_chunk(
            ltk_wad::WadChunkBuilder::default()
                .with_path("plugins/rcp-be-lol-game-data/global/default/v1/champions/ahri.json"),
        );
        builder
            .build_to_writer(&mut wad_data, |_path_hash, cursor| {
                cursor.write_all(champion_json)?;
                Ok(())
            })
            .unwrap();
        wad_data.set_position(0);

        let champions = read_game_champions_from_wad_reader(wad_data).unwrap();
        assert_eq!(champions.len(), 1);
        assert_eq!(champions[0].champion_id, "ahri");
        assert_eq!(champions[0].skins.len(), 2);
        assert_eq!(champions[0].skins[1].skin_number, 27);
        assert_eq!(champions[0].skins[1].chromas[0].chroma_id, 103028);
        assert_eq!(champions[0].skins[1].chromas[0].chroma_name, "Ruby");
    }

    #[test]
    fn short_chroma_name_strips_trailing_parenthetical() {
        assert_eq!(
            short_chroma_name("Cyber Pop Akshan (Tanzanite)"),
            "Tanzanite"
        );
        assert_eq!(short_chroma_name("Ruby"), "Ruby");
    }

    #[test]
    fn game_champion_cache_sources_are_sorted_and_include_file_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let b_path = dir.path().join("b.wad");
        let a_path = dir.path().join("a.wad");
        std::fs::write(&b_path, b"bbbb").unwrap();
        std::fs::write(&a_path, b"aa").unwrap();

        let sources = game_champion_cache_sources(&[b_path.clone(), a_path.clone()]).unwrap();

        assert_eq!(sources.len(), 2);
        assert!(sources[0].path.ends_with("a.wad"));
        assert_eq!(sources[0].len, 2);
        assert!(sources[1].path.ends_with("b.wad"));
        assert_eq!(sources[1].len, 4);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn auto_detect_macos_app_bundle_root_from_executable_path() {
        let exe = Path::new("/Applications/League of Legends.app")
            .join("Contents")
            .join("LoL")
            .join("Game")
            .join("League of Legends");

        assert_eq!(
            macos_app_bundle_root(&exe).unwrap(),
            PathBuf::from("/Applications/League of Legends.app")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn validate_league_path_accepts_macos_inner_lol_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir
            .path()
            .join("League of Legends.app")
            .join("Contents")
            .join("LoL");
        std::fs::create_dir_all(root.join("Game")).unwrap();

        assert!(matches!(
            validate_league_path(root),
            IpcResult::Ok { value: true }
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn validate_league_path_accepts_macos_outer_app_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("League of Legends.app");
        std::fs::create_dir_all(root.join("Contents").join("LoL").join("Game")).unwrap();

        assert!(matches!(
            validate_league_path(root),
            IpcResult::Ok { value: true }
        ));
    }
}
