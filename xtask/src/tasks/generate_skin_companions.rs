use ltk_meta::{
    property::{values, NoMeta},
    Bin, PropertyValueEnum,
};
use rayon::prelude::*;
use serde::Deserialize;
use std::{
    collections::{BTreeSet, HashMap},
    env,
    fs::File,
    io::{Cursor, Read, Seek},
    path::PathBuf,
};
use xxhash_rust::xxh64::xxh64;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChampionCache {
    champions: Vec<CachedChampion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedChampion {
    champion_id: String,
    skins: Vec<CachedSkin>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedSkin {
    skin_number: u32,
}

#[derive(Debug)]
struct CompanionScanResult {
    champion_id: String,
    _skin_number: u32,
    companion_id: String,
    _detected_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AliasRule {
    suffix: String,
    trigger: String,
}

type ScanOutput = Result<(String, BTreeSet<String>, Vec<CompanionScanResult>), String>;

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut game_dir_arg = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--game-dir" && i + 1 < args.len() {
            game_dir_arg = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else {
            eprintln!("Unknown argument: {}", args[i]);
            std::process::exit(1);
        }
    }

    let game_dir = game_dir_arg
        .unwrap_or_else(|| PathBuf::from("/Applications/League of Legends.app/Contents/LoL/Game"));

    let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
    if !champions_dir.exists() {
        return Err(format!(
            "Champions directory not found: {}. Please check your League installation or provide --game-dir.",
            champions_dir.display()
        )
        .into());
    }

    let known_skins = load_known_skins().unwrap_or_default();
    let wads: Vec<PathBuf> = std::fs::read_dir(&champions_dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
                if let Some(champion_id) = file_name.strip_suffix(".wad.client") {
                    return !champion_id.contains('.') && !champion_id.contains('_');
                }
            }
            false
        })
        .collect();

    let scan_outputs: Vec<ScanOutput> = wads
        .into_par_iter()
        .map(|path| {
            let file_name = path.file_name().unwrap().to_str().unwrap();
            let champion_id = file_name.strip_suffix(".wad.client").unwrap();
            let champion_lower = champion_id.to_ascii_lowercase();

            let file = File::open(&path).map_err(|e| e.to_string())?;
            let mut wad = ltk_wad::Wad::mount(file).map_err(|e| e.to_string())?;
            let skin_numbers = known_skins
                .get(&champion_lower)
                .cloned()
                .unwrap_or_else(|| (1..1000).collect());

            let mut terms = BTreeSet::new();
            let mut results = Vec::new();

            scan_champion_wad(
                &mut wad,
                champion_id,
                &skin_numbers,
                &mut terms,
                &mut results,
            )
            .map_err(|e| e.to_string())?;

            Ok((champion_lower, terms, results))
        })
        .collect();

    let mut all_champion_terms: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut scan_results = Vec::new();

    for output in scan_outputs {
        let (champion_lower, terms, results) =
            output.map_err(Box::<dyn std::error::Error>::from)?;
        all_champion_terms.insert(champion_lower, terms);
        scan_results.extend(results);
    }

    let mut direct_suffixes = BTreeSet::new();
    let mut alias_rules = BTreeSet::new();

    for res in &scan_results {
        let champion_lower = res.champion_id.to_ascii_lowercase();
        let companion_lower = res.companion_id.to_ascii_lowercase();
        let suffix = candidate_suffix(&companion_lower, &champion_lower).to_string();
        if suffix == "<non-prefix>" || suffix.is_empty() {
            continue;
        }

        if let Some(terms) = all_champion_terms.get(&champion_lower) {
            let best_trigger = terms
                .iter()
                .filter(|t| *t != &suffix && (suffix.starts_with(*t) || suffix.ends_with(*t)))
                .max_by(|a, b| {
                    a.len().cmp(&b.len()).then_with(|| {
                        let a_is_prefix = suffix.starts_with(*a);
                        let b_is_prefix = suffix.starts_with(*b);
                        a_is_prefix.cmp(&b_is_prefix)
                    })
                });

            if let Some(trigger) = best_trigger {
                alias_rules.insert(AliasRule {
                    suffix: suffix.clone(),
                    trigger: trigger.clone(),
                });
            } else if terms.contains(&suffix) {
                direct_suffixes.insert(suffix);
            }
        }
    }

    let mut toml_content = String::new();
    toml_content.push_str("direct = [\n");
    let direct_list: Vec<_> = direct_suffixes.into_iter().collect();
    for (idx, suffix) in direct_list.iter().enumerate() {
        toml_content.push_str(&format!("  \"{}\"", suffix));
        if idx + 1 < direct_list.len() {
            toml_content.push_str(",\n");
        } else {
            toml_content.push('\n');
        }
    }
    toml_content.push_str("]\n");

    let alias_list: Vec<_> = alias_rules.into_iter().collect();
    for alias in &alias_list {
        toml_content.push_str("\n[[alias]]\n");
        toml_content.push_str(&format!("suffix = \"{}\"\n", alias.suffix));
        toml_content.push_str(&format!("trigger = \"{}\"\n", alias.trigger));
    }

    let dest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-tauri")
        .join("data")
        .join("skin_companion_suffixes.toml");

    std::fs::create_dir_all(dest_path.parent().unwrap())?;
    std::fs::write(&dest_path, toml_content)?;

    println!(
        "Generated {} direct suffixes and {} aliases",
        direct_list.len(),
        alias_list.len()
    );
    println!("Wrote src-tauri/data/skin_companion_suffixes.toml");

    Ok(())
}

fn load_known_skins() -> Result<HashMap<String, Vec<u32>>, Box<dyn std::error::Error>> {
    let cache_path = if let Some(appdata) = env::var_os("APPDATA") {
        PathBuf::from(appdata)
            .join("dev.leaguetoolkit.manager")
            .join("cache")
            .join("game-champions.json")
    } else if let Some(home) = env::var_os("HOME") {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("dev.leaguetoolkit.manager")
            .join("cache")
            .join("game-champions.json")
    } else {
        return Ok(HashMap::new());
    };

    if !cache_path.exists() {
        return Ok(HashMap::new());
    }

    let bytes = std::fs::read(cache_path)?;
    let cache: ChampionCache = serde_json::from_slice(&bytes)?;
    Ok(cache
        .champions
        .into_iter()
        .map(|champion| {
            (
                champion.champion_id.to_ascii_lowercase(),
                champion
                    .skins
                    .into_iter()
                    .map(|skin| skin.skin_number)
                    .filter(|skin_number| *skin_number != 0)
                    .collect(),
            )
        })
        .collect())
}

fn scan_champion_wad<TSource: Read + Seek>(
    wad: &mut ltk_wad::Wad<TSource>,
    champion_id: &str,
    skin_numbers: &[u32],
    all_champion_terms: &mut BTreeSet<String>,
    results: &mut Vec<CompanionScanResult>,
) -> Result<(), Box<dyn std::error::Error>> {
    let champion_lower = champion_id.to_ascii_lowercase();
    for &skin_number in skin_numbers {
        let skin_path = skin_bin_path(&champion_lower, skin_number);
        let Some(bytes) = read_wad_chunk(wad, &skin_path)? else {
            continue;
        };
        let Ok(bin) = Bin::from_reader(&mut Cursor::new(bytes)) else {
            continue;
        };
        let strings = collect_strings(&bin);
        let mut seen = BTreeSet::new();

        let terms = candidate_terms(&strings, &champion_lower);
        all_champion_terms.extend(terms.clone());

        for value in &strings {
            for companion_id in path_companion_ids(value, &champion_lower) {
                if has_skin_bin(wad, &companion_id, skin_number)
                    && seen.insert(companion_id.to_ascii_lowercase())
                {
                    results.push(CompanionScanResult {
                        champion_id: champion_id.to_string(),
                        _skin_number: skin_number,
                        companion_id: companion_id.clone(),
                        _detected_by: "path".to_string(),
                    });
                }
            }
        }

        for candidate in candidate_ids_from_terms(&champion_lower, &terms) {
            if has_skin_bin(wad, &candidate, skin_number)
                && seen.insert(candidate.to_ascii_lowercase())
            {
                results.push(CompanionScanResult {
                    champion_id: champion_id.to_string(),
                    _skin_number: skin_number,
                    companion_id: candidate.clone(),
                    _detected_by: "term".to_string(),
                });
            }
        }
    }
    Ok(())
}

fn skin_bin_path(character_id: &str, skin_number: u32) -> String {
    format!(
        "data/characters/{}/skins/skin{}.bin",
        character_id.to_ascii_lowercase(),
        skin_number
    )
}

fn read_wad_chunk<TSource: Read + Seek>(
    wad: &mut ltk_wad::Wad<TSource>,
    rel_path: &str,
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    let hash = wad_path_hash(rel_path);
    let Some(chunk) = wad.chunks().get(hash).cloned() else {
        return Ok(None);
    };
    Ok(Some(wad.load_chunk_decompressed(&chunk)?.to_vec()))
}

fn has_skin_bin<TSource: Read + Seek>(
    wad: &ltk_wad::Wad<TSource>,
    character_id: &str,
    skin_number: u32,
) -> bool {
    wad.chunks()
        .contains(wad_path_hash(&skin_bin_path(character_id, skin_number)))
}

fn wad_path_hash(path: &str) -> u64 {
    xxh64(path.to_ascii_lowercase().as_bytes(), 0)
}

fn collect_strings(bin: &Bin) -> Vec<String> {
    let mut strings = Vec::new();
    for object in bin.objects.values() {
        for value in object.properties.values() {
            collect_strings_from_value(value, &mut strings);
        }
    }
    strings
}

fn collect_strings_from_value(value: &PropertyValueEnum<NoMeta>, strings: &mut Vec<String>) {
    match value {
        PropertyValueEnum::String(value) => strings.push(value.value.clone()),
        PropertyValueEnum::Struct(value) => collect_strings_from_struct(value, strings),
        PropertyValueEnum::Embedded(value) => collect_strings_from_struct(&value.0, strings),
        PropertyValueEnum::Container(value) => collect_strings_from_container(value, strings),
        PropertyValueEnum::UnorderedContainer(value) => {
            collect_strings_from_container(&value.0, strings);
        }
        PropertyValueEnum::Optional(value) => collect_strings_from_optional(value, strings),
        _ => {}
    }
}

fn collect_strings_from_struct(value: &values::Struct<NoMeta>, strings: &mut Vec<String>) {
    for value in value.properties.values() {
        collect_strings_from_value(value, strings);
    }
}

fn collect_strings_from_container(value: &values::Container<NoMeta>, strings: &mut Vec<String>) {
    match value {
        values::Container::String { items, .. } => {
            strings.extend(items.iter().map(|item| item.value.clone()));
        }
        values::Container::Struct { items, .. } => {
            for item in items {
                collect_strings_from_struct(item, strings);
            }
        }
        values::Container::Embedded { items, .. } => {
            for item in items {
                collect_strings_from_struct(&item.0, strings);
            }
        }
        _ => {}
    }
}

fn collect_strings_from_optional(value: &values::Optional<NoMeta>, strings: &mut Vec<String>) {
    match value {
        values::Optional::String {
            value: Some(value), ..
        } => strings.push(value.value.clone()),
        values::Optional::Struct {
            value: Some(value), ..
        } => collect_strings_from_struct(value, strings),
        values::Optional::Embedded {
            value: Some(value), ..
        } => collect_strings_from_struct(&value.0, strings),
        _ => {}
    }
}

fn path_companion_ids(value: &str, champion_lower: &str) -> Vec<String> {
    let normalized = value.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    let mut ids = BTreeSet::new();

    for prefix in ["assets/characters/", "data/characters/", "characters/"] {
        let mut search_from = 0;
        while let Some(start) = lower[search_from..].find(prefix) {
            let character_start = search_from + start + prefix.len();
            let Some(rest) = normalized.get(character_start..) else {
                break;
            };
            let Some((character, after_character)) = rest.split_once('/') else {
                break;
            };
            if after_character.to_ascii_lowercase().starts_with("skins/")
                && character.to_ascii_lowercase() != champion_lower
            {
                ids.insert(character.to_string());
            }
            search_from = character_start + character.len();
        }
    }

    ids.into_iter().collect()
}

fn candidate_terms(strings: &[String], champion_lower: &str) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for value in strings {
        let lower = value.to_ascii_lowercase();
        if !lower.contains(champion_lower) {
            continue;
        }
        let split = split_terms(value)
            .into_iter()
            .map(|token| token.to_ascii_lowercase())
            .collect::<Vec<_>>();
        for token in &split {
            if is_candidate_term(token, champion_lower) {
                terms.insert(token.clone());
            }
        }
        for pair in split.windows(2) {
            let combined = format!("{}{}", pair[0], pair[1]);
            if is_candidate_term(&pair[0], champion_lower)
                && is_candidate_term(&pair[1], champion_lower)
                && is_candidate_term(&combined, champion_lower)
            {
                terms.insert(combined);
            }
        }
    }
    terms
}

fn split_terms(value: &str) -> Vec<String> {
    let mut spaced = String::with_capacity(value.len() * 2);
    let mut prev_lower_or_digit = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if prev_lower_or_digit && ch.is_ascii_uppercase() {
                spaced.push(' ');
            }
            spaced.push(ch);
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            spaced.push(' ');
            prev_lower_or_digit = false;
        }
    }
    spaced
        .split_whitespace()
        .map(|term| term.trim_matches(|c: char| c.is_ascii_digit()).to_string())
        .filter(|term| !term.is_empty())
        .collect()
}

fn is_candidate_term(term: &str, champion_lower: &str) -> bool {
    const IGNORED: &[&str] = &[
        "ability",
        "attack",
        "audio",
        "bank",
        "base",
        "cast",
        "character",
        "characters",
        "data",
        "death",
        "emote",
        "idle",
        "loop",
        "material",
        "move",
        "music",
        "particle",
        "particles",
        "play",
        "resource",
        "skin",
        "skins",
        "sfx",
        "sound",
        "sounds",
        "spell",
        "stop",
        "tex",
        "texture",
        "textures",
        "vo",
        "voice",
        "wwise",
    ];
    term.len() >= 3
        && term.len() <= 24
        && term != champion_lower
        && !term.starts_with("skin")
        && !IGNORED.contains(&term)
}

fn candidate_ids_from_terms(champion_lower: &str, terms: &BTreeSet<String>) -> BTreeSet<String> {
    terms
        .iter()
        .map(|term| format!("{champion_lower}{term}"))
        .collect()
}

fn candidate_suffix<'a>(companion_id: &'a str, champion_lower: &str) -> &'a str {
    companion_id
        .strip_prefix(champion_lower)
        .or_else(|| {
            let lower = companion_id.to_ascii_lowercase();
            lower
                .strip_prefix(champion_lower)
                .map(|suffix| &companion_id[companion_id.len() - suffix.len()..])
        })
        .unwrap_or("<non-prefix>")
}
