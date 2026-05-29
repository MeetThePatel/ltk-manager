use crate::error::{AppError, AppResult};
use crate::mods::SkinRemap;
use camino::{Utf8Path, Utf8PathBuf};
use ltk_meta::{
    property::{values, NoMeta},
    Bin, PropertyValueEnum,
};
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use xxhash_rust::{xxh3::xxh3_64, xxh64::xxh64};

const BASE_LAYER: &str = "base";

pub struct SkinRemapContent {
    entries: HashMap<String, BTreeMap<Utf8PathBuf, Vec<u8>>>,
    fingerprint: u64,
}

impl SkinRemapContent {
    pub fn new(game_dir: PathBuf, remaps: Vec<SkinRemap>) -> AppResult<Self> {
        let mut entries: HashMap<String, BTreeMap<Utf8PathBuf, Vec<u8>>> = HashMap::new();

        for remap in remaps {
            let champion = remap.champion_id.trim();
            if champion.is_empty() {
                continue;
            }

            let target_skin = remap
                .target_chroma_id
                .map(|id| id % 1000)
                .unwrap_or(remap.target_skin_number);
            if target_skin == 0 {
                continue;
            }

            let overrides = build_base_skin_overrides(&game_dir, champion, target_skin)?;

            for (wad_name, wad_overrides) in overrides {
                entries.entry(wad_name).or_default().extend(wad_overrides);
            }
        }

        let fingerprint = skin_remap_fingerprint(&entries);
        Ok(Self {
            entries,
            fingerprint,
        })
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

impl ltk_overlay::ModContentProvider for SkinRemapContent {
    fn mod_project(&mut self) -> ltk_overlay::Result<ModProject> {
        Ok(ModProject {
            name: "ltk_skin_remaps".to_string(),
            display_name: "LTK Skin Remaps".to_string(),
            version: "1.0.0".to_string(),
            description: "Generated regular skin slot remaps".to_string(),
            authors: vec![ModProjectAuthor::Name("LTK Manager".to_string())],
            license: None,
            tags: Vec::new(),
            champions: Vec::new(),
            maps: Vec::new(),
            transformers: Vec::new(),
            layers: vec![ModProjectLayer::base()],
            thumbnail: None,
        })
    }

    fn list_layer_wads(&mut self, layer: &str) -> ltk_overlay::Result<Vec<String>> {
        if layer != BASE_LAYER {
            return Ok(Vec::new());
        }
        Ok(self.entries.keys().cloned().collect())
    }

    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> ltk_overlay::Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        if layer != BASE_LAYER {
            return Ok(Vec::new());
        }
        Ok(self
            .entries
            .get(wad_name)
            .map(|entries| {
                entries
                    .iter()
                    .map(|(path, bytes)| (path.clone(), bytes.clone()))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn content_fingerprint(&self) -> ltk_overlay::Result<Option<u64>> {
        Ok(Some(self.fingerprint))
    }

    fn read_wad_override_file(
        &mut self,
        layer: &str,
        wad_name: &str,
        rel_path: &Utf8Path,
    ) -> ltk_overlay::Result<Vec<u8>> {
        if layer != BASE_LAYER {
            return Err(ltk_overlay::Error::Other(format!(
                "Unknown skin remap layer: {layer}"
            )));
        }
        self.entries
            .get(wad_name)
            .and_then(|entries| entries.get(rel_path))
            .cloned()
            .ok_or_else(|| {
                ltk_overlay::Error::Other(format!(
                    "Skin remap override not found: {wad_name}/{rel_path}"
                ))
            })
    }

    fn read_raw_override_file(&mut self, rel_path: &Utf8Path) -> ltk_overlay::Result<Vec<u8>> {
        Err(ltk_overlay::Error::Other(format!(
            "Skin remap raw override not found: {rel_path}"
        )))
    }
}

fn champion_wad_path(game_dir: &Path, champion_id: &str) -> PathBuf {
    game_dir
        .join("DATA")
        .join("FINAL")
        .join("Champions")
        .join(format!("{champion_id}.wad.client"))
}

fn champion_wad_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("DATA").join("FINAL").join("Champions")
}

fn champion_wad_name(champion_id: &str) -> String {
    format!("{champion_id}.wad.client")
}

fn champion_locale_wad_paths(game_dir: &Path, champion_id: &str) -> AppResult<Vec<PathBuf>> {
    let dir = champion_wad_dir(game_dir);
    let prefix = format!("{}.", champion_id.to_ascii_lowercase());
    let primary = champion_wad_name(champion_id).to_ascii_lowercase();
    let mut paths = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(paths);
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = file_name.to_ascii_lowercase();
        if lower.starts_with(&prefix) && lower.ends_with(".wad.client") && lower != primary {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn wad_file_name(path: &Path) -> AppResult<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
        .ok_or_else(|| AppError::Other(format!("Invalid WAD path: {}", path.display())))
}

fn skin_bin_path(champion_id: &str, skin_number: u32) -> Utf8PathBuf {
    Utf8PathBuf::from(format!(
        "data/characters/{}/skins/skin{}.bin",
        champion_id.to_lowercase(),
        skin_number
    ))
}

fn build_base_skin_overrides(
    game_dir: &Path,
    champion_id: &str,
    target_skin: u32,
) -> AppResult<HashMap<String, BTreeMap<Utf8PathBuf, Vec<u8>>>> {
    let primary_wad_path = champion_wad_path(game_dir, champion_id);
    let file = File::open(&primary_wad_path).map_err(|e| {
        AppError::Other(format!(
            "Failed to open champion WAD {}: {}",
            primary_wad_path.display(),
            e
        ))
    })?;
    let mut wad = ltk_wad::Wad::mount(file)?;
    let source_path = skin_bin_path(champion_id, target_skin);
    let source_bytes = read_wad_chunk_from_mounted(&mut wad, &source_path)?.ok_or_else(|| {
        AppError::Other(format!(
            "Could not find skin bin {} in {}",
            source_path,
            primary_wad_path.display()
        ))
    })?;
    let companion_ids =
        referenced_companion_skin_ids(&wad, champion_id, target_skin, &source_bytes);
    let rewrite = rewrite_skin_to_base(champion_id, target_skin, source_bytes)?;

    let primary_wad_name = wad_file_name(&primary_wad_path)?;
    let mut overrides: HashMap<String, BTreeMap<Utf8PathBuf, Vec<u8>>> = HashMap::new();
    overrides
        .entry(primary_wad_name.clone())
        .or_default()
        .insert(skin_bin_path(champion_id, 0), rewrite.bin_bytes);
    let mut linked_chunks = rewrite.linked_chunks;

    for companion_id in companion_ids {
        let companion_source_path = skin_bin_path(&companion_id, target_skin);
        let Some(companion_source_bytes) =
            read_wad_chunk_from_mounted(&mut wad, &companion_source_path)?
        else {
            continue;
        };
        let companion_rewrite =
            rewrite_skin_to_base(&companion_id, target_skin, companion_source_bytes)?;
        overrides
            .entry(primary_wad_name.clone())
            .or_default()
            .insert(skin_bin_path(&companion_id, 0), companion_rewrite.bin_bytes);
        linked_chunks.extend(companion_rewrite.linked_chunks);
    }

    let mut source_wads = vec![MountedSourceWad {
        name: primary_wad_name,
        wad,
    }];
    for path in champion_locale_wad_paths(game_dir, champion_id)? {
        let file = File::open(&path).map_err(|e| {
            AppError::Other(format!(
                "Failed to open champion locale WAD {}: {}",
                path.display(),
                e
            ))
        })?;
        source_wads.push(MountedSourceWad {
            name: wad_file_name(&path)?,
            wad: ltk_wad::Wad::mount(file)?,
        });
    }

    for (source_path, target_path) in linked_chunks {
        for source_wad in &mut source_wads {
            if let Some(bytes) = read_wad_chunk_from_mounted(&mut source_wad.wad, &source_path)? {
                overrides
                    .entry(source_wad.name.clone())
                    .or_default()
                    .insert(target_path.clone(), bytes);
                break;
            }
        }
    }
    Ok(overrides)
}

fn referenced_companion_skin_ids<TSource: Read + Seek>(
    wad: &ltk_wad::Wad<TSource>,
    champion_id: &str,
    target_skin: u32,
    source_bytes: &[u8],
) -> BTreeSet<String> {
    let Ok(bin) = Bin::from_reader(&mut Cursor::new(source_bytes)) else {
        return BTreeSet::new();
    };
    let champion_lower = champion_id.to_ascii_lowercase();
    let mut ids = BTreeSet::new();
    for object in bin.objects.values() {
        for value in object.properties.values() {
            collect_referenced_companion_skin_ids(
                wad,
                &champion_lower,
                target_skin,
                value,
                &mut ids,
            );
        }
    }
    ids
}

fn collect_referenced_companion_skin_ids<TSource: Read + Seek>(
    wad: &ltk_wad::Wad<TSource>,
    champion_lower: &str,
    target_skin: u32,
    value: &PropertyValueEnum<NoMeta>,
    ids: &mut BTreeSet<String>,
) {
    match value {
        PropertyValueEnum::String(value) => {
            collect_companion_skin_ids_from_string(
                wad,
                champion_lower,
                target_skin,
                &value.value,
                ids,
            );
        }
        PropertyValueEnum::Struct(value) => {
            collect_referenced_companion_skin_ids_from_struct(
                wad,
                champion_lower,
                target_skin,
                value,
                ids,
            );
        }
        PropertyValueEnum::Embedded(value) => {
            collect_referenced_companion_skin_ids_from_struct(
                wad,
                champion_lower,
                target_skin,
                &value.0,
                ids,
            );
        }
        PropertyValueEnum::Container(value) => {
            collect_referenced_companion_skin_ids_from_container(
                wad,
                champion_lower,
                target_skin,
                value,
                ids,
            );
        }
        PropertyValueEnum::UnorderedContainer(value) => {
            collect_referenced_companion_skin_ids_from_container(
                wad,
                champion_lower,
                target_skin,
                &value.0,
                ids,
            );
        }
        PropertyValueEnum::Optional(value) => {
            collect_referenced_companion_skin_ids_from_optional(
                wad,
                champion_lower,
                target_skin,
                value,
                ids,
            );
        }
        _ => {}
    }
}

fn collect_referenced_companion_skin_ids_from_struct<TSource: Read + Seek>(
    wad: &ltk_wad::Wad<TSource>,
    champion_lower: &str,
    target_skin: u32,
    value: &values::Struct<NoMeta>,
    ids: &mut BTreeSet<String>,
) {
    for value in value.properties.values() {
        collect_referenced_companion_skin_ids(wad, champion_lower, target_skin, value, ids);
    }
}

fn collect_referenced_companion_skin_ids_from_container<TSource: Read + Seek>(
    wad: &ltk_wad::Wad<TSource>,
    champion_lower: &str,
    target_skin: u32,
    value: &values::Container<NoMeta>,
    ids: &mut BTreeSet<String>,
) {
    match value {
        values::Container::String { items, .. } => {
            for item in items {
                collect_companion_skin_ids_from_string(
                    wad,
                    champion_lower,
                    target_skin,
                    &item.value,
                    ids,
                );
            }
        }
        values::Container::Struct { items, .. } => {
            for item in items {
                collect_referenced_companion_skin_ids_from_struct(
                    wad,
                    champion_lower,
                    target_skin,
                    item,
                    ids,
                );
            }
        }
        values::Container::Embedded { items, .. } => {
            for item in items {
                collect_referenced_companion_skin_ids_from_struct(
                    wad,
                    champion_lower,
                    target_skin,
                    &item.0,
                    ids,
                );
            }
        }
        _ => {}
    }
}

fn collect_referenced_companion_skin_ids_from_optional<TSource: Read + Seek>(
    wad: &ltk_wad::Wad<TSource>,
    champion_lower: &str,
    target_skin: u32,
    value: &values::Optional<NoMeta>,
    ids: &mut BTreeSet<String>,
) {
    match value {
        values::Optional::String {
            value: Some(value), ..
        } => {
            collect_companion_skin_ids_from_string(
                wad,
                champion_lower,
                target_skin,
                &value.value,
                ids,
            );
        }
        values::Optional::Struct {
            value: Some(value), ..
        } => collect_referenced_companion_skin_ids_from_struct(
            wad,
            champion_lower,
            target_skin,
            value,
            ids,
        ),
        values::Optional::Embedded {
            value: Some(value), ..
        } => collect_referenced_companion_skin_ids_from_struct(
            wad,
            champion_lower,
            target_skin,
            &value.0,
            ids,
        ),
        _ => {}
    }
}

fn collect_companion_skin_ids_from_string<TSource: Read + Seek>(
    wad: &ltk_wad::Wad<TSource>,
    champion_lower: &str,
    target_skin: u32,
    value: &str,
    ids: &mut BTreeSet<String>,
) {
    for candidate in companion_skin_id_candidates_from_string(value, champion_lower) {
        let candidate_path = skin_bin_path(&candidate, target_skin);
        if wad
            .chunks()
            .contains(wad_path_hash(candidate_path.as_str()))
        {
            ids.insert(candidate);
        }
    }
}

fn companion_skin_id_candidates_from_string(value: &str, champion_lower: &str) -> BTreeSet<String> {
    let mut candidates = BTreeSet::new();
    if !value.chars().all(|c| c.is_ascii_alphanumeric()) {
        collect_companion_skin_id_candidates_from_path(value, champion_lower, &mut candidates);
    } else if value.to_ascii_lowercase() != champion_lower {
        candidates.insert(value.to_string());
    }
    collect_companion_skin_id_candidates_from_terms(value, champion_lower, &mut candidates);
    candidates
}

fn collect_companion_skin_id_candidates_from_path(
    value: &str,
    champion_lower: &str,
    candidates: &mut BTreeSet<String>,
) {
    let normalized = value.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    for prefix in ["assets/characters/", "characters/"] {
        let Some(start) = lower.find(prefix) else {
            continue;
        };
        let character_start = start + prefix.len();
        let Some(rest) = normalized.get(character_start..) else {
            continue;
        };
        let Some((character, after_character)) = rest.split_once('/') else {
            continue;
        };
        if !after_character.to_ascii_lowercase().starts_with("skins/") {
            continue;
        }
        if character.to_ascii_lowercase() != champion_lower {
            candidates.insert(character.to_string());
        }
    }
}

fn collect_companion_skin_id_candidates_from_terms(
    value: &str,
    champion_lower: &str,
    candidates: &mut BTreeSet<String>,
) {
    let lower = value.to_ascii_lowercase();
    let suffixes = [
        ("packmate", "packmate"),
        ("ghoulmelee", "ghoul"),
        ("bigghoul", "ghoul"),
        ("maiden", "maiden"),
        ("mistwalker", "mist"),
        ("minion", "minion"),
        ("pet", "pet"),
        ("clone", "clone"),
        ("spiderling", "spider"),
        ("voidling", "voidling"),
        ("tentacle", "tentacle"),
        ("soldier", "soldier"),
        ("turret", "turret"),
        ("plant", "plant"),
        ("seed", "seed"),
    ];
    for (suffix, trigger) in suffixes {
        if lower.contains(trigger) {
            candidates.insert(format!("{champion_lower}{suffix}"));
        }
    }
}

struct MountedSourceWad<TSource: Read + Seek> {
    name: String,
    wad: ltk_wad::Wad<TSource>,
}

fn read_wad_chunk_from_mounted<TSource: Read + Seek>(
    wad: &mut ltk_wad::Wad<TSource>,
    rel_path: &Utf8Path,
) -> AppResult<Option<Vec<u8>>> {
    let path_hash = wad_path_hash(rel_path.as_str());
    let Some(chunk) = wad.chunks().get(path_hash).cloned() else {
        return Ok(None);
    };
    Ok(Some(wad.load_chunk_decompressed(&chunk)?.to_vec()))
}

fn wad_path_hash(path: &str) -> u64 {
    xxh64(path.to_lowercase().as_bytes(), 0)
}

struct SkinBinRewrite {
    bin_bytes: Vec<u8>,
    linked_chunks: BTreeMap<Utf8PathBuf, Utf8PathBuf>,
}

fn rewrite_skin_to_base(
    champion_id: &str,
    target_skin: u32,
    source_bytes: Vec<u8>,
) -> AppResult<SkinBinRewrite> {
    let mut bin = Bin::from_reader(&mut Cursor::new(source_bytes)).map_err(|e| {
        AppError::Other(format!(
            "Failed to parse {champion_id} skin{target_skin}.bin: {e}"
        ))
    })?;

    let object_hash_rewrites = skin_object_hash_rewrites(champion_id, target_skin);
    let source_root = skin_object_hash(champion_id, target_skin, None);
    let base_root = skin_object_hash(champion_id, 0, None);
    let mut object = bin.remove_object(source_root).ok_or_else(|| {
        AppError::Other(format!(
            "Could not find root object Characters/{champion_id}/Skins/Skin{target_skin}"
        ))
    })?;
    object.path_hash = base_root;
    bin.add_object(object);
    for (source_hash, target_hash) in &object_hash_rewrites {
        if *source_hash == source_root {
            continue;
        }
        if let Some(mut object) = bin.remove_object(*source_hash) {
            object.path_hash = *target_hash;
            bin.add_object(object);
        }
    }

    let replacements = SkinTokenReplacements::new(target_skin);
    let mut linked_chunks = BTreeMap::new();
    for object in bin.objects.values_mut() {
        for value in object.properties.values_mut() {
            rewrite_property_skin_refs(
                champion_id,
                value,
                &object_hash_rewrites,
                &replacements,
                &mut linked_chunks,
            );
        }
    }
    let linked_chunk_hashes = linked_chunks
        .iter()
        .map(|(source_path, target_path)| {
            (
                wad_path_hash(source_path.as_str()),
                wad_path_hash(target_path.as_str()),
            )
        })
        .collect::<HashMap<_, _>>();
    for object in bin.objects.values_mut() {
        for value in object.properties.values_mut() {
            rewrite_wad_chunk_links(value, &linked_chunk_hashes);
        }
    }

    let mut output = Cursor::new(Vec::new());
    bin.to_writer(&mut output).map_err(|e| {
        AppError::Other(format!(
            "Failed to write generated {champion_id} skin remap: {e}"
        ))
    })?;
    Ok(SkinBinRewrite {
        bin_bytes: output.into_inner(),
        linked_chunks,
    })
}

struct SkinTokenReplacements {
    source_tokens: Vec<String>,
}

impl SkinTokenReplacements {
    fn new(target_skin: u32) -> Self {
        let mut source_tokens = vec![
            format!("skin{target_skin:03}"),
            format!("skin{target_skin:02}"),
            format!("skin{target_skin}"),
        ];
        source_tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));
        source_tokens.dedup();
        Self { source_tokens }
    }
}

fn rewrite_property_skin_refs(
    champion_id: &str,
    value: &mut PropertyValueEnum<NoMeta>,
    object_hash_rewrites: &HashMap<u32, u32>,
    replacements: &SkinTokenReplacements,
    linked_chunks: &mut BTreeMap<Utf8PathBuf, Utf8PathBuf>,
) {
    match value {
        PropertyValueEnum::String(value) => {
            rewrite_string_skin_refs(champion_id, &mut value.value, replacements, linked_chunks);
        }
        PropertyValueEnum::Hash(value) => {
            if let Some(target_hash) = object_hash_rewrites.get(&value.value) {
                value.value = *target_hash;
            }
        }
        PropertyValueEnum::ObjectLink(value) => {
            if let Some(target_hash) = object_hash_rewrites.get(&value.value) {
                value.value = *target_hash;
            }
        }
        PropertyValueEnum::Struct(value) => {
            rewrite_struct_skin_refs(
                champion_id,
                value,
                object_hash_rewrites,
                replacements,
                linked_chunks,
            );
        }
        PropertyValueEnum::Embedded(value) => {
            rewrite_struct_skin_refs(
                champion_id,
                &mut value.0,
                object_hash_rewrites,
                replacements,
                linked_chunks,
            );
        }
        PropertyValueEnum::Container(value) => {
            rewrite_container_skin_refs(
                champion_id,
                value,
                object_hash_rewrites,
                replacements,
                linked_chunks,
            );
        }
        PropertyValueEnum::UnorderedContainer(value) => {
            rewrite_container_skin_refs(
                champion_id,
                &mut value.0,
                object_hash_rewrites,
                replacements,
                linked_chunks,
            );
        }
        PropertyValueEnum::Optional(value) => {
            rewrite_optional_skin_refs(
                champion_id,
                value,
                object_hash_rewrites,
                replacements,
                linked_chunks,
            );
        }
        _ => {}
    }
}

fn rewrite_wad_chunk_links(
    value: &mut PropertyValueEnum<NoMeta>,
    linked_chunk_hashes: &HashMap<u64, u64>,
) {
    match value {
        PropertyValueEnum::WadChunkLink(value) => {
            if let Some(target_hash) = linked_chunk_hashes.get(&value.value) {
                value.value = *target_hash;
            }
        }
        PropertyValueEnum::Struct(value) => {
            rewrite_struct_wad_chunk_links(value, linked_chunk_hashes);
        }
        PropertyValueEnum::Embedded(value) => {
            rewrite_struct_wad_chunk_links(&mut value.0, linked_chunk_hashes);
        }
        PropertyValueEnum::Container(value) => {
            rewrite_container_wad_chunk_links(value, linked_chunk_hashes);
        }
        PropertyValueEnum::UnorderedContainer(value) => {
            rewrite_container_wad_chunk_links(&mut value.0, linked_chunk_hashes);
        }
        PropertyValueEnum::Optional(value) => {
            rewrite_optional_wad_chunk_links(value, linked_chunk_hashes);
        }
        _ => {}
    }
}

fn rewrite_struct_wad_chunk_links(
    value: &mut values::Struct<NoMeta>,
    linked_chunk_hashes: &HashMap<u64, u64>,
) {
    for value in value.properties.values_mut() {
        rewrite_wad_chunk_links(value, linked_chunk_hashes);
    }
}

fn rewrite_container_wad_chunk_links(
    value: &mut values::Container<NoMeta>,
    linked_chunk_hashes: &HashMap<u64, u64>,
) {
    match value {
        values::Container::WadChunkLink { items, .. } => {
            for item in items {
                if let Some(target_hash) = linked_chunk_hashes.get(&item.value) {
                    item.value = *target_hash;
                }
            }
        }
        values::Container::Struct { items, .. } => {
            for item in items {
                rewrite_struct_wad_chunk_links(item, linked_chunk_hashes);
            }
        }
        values::Container::Embedded { items, .. } => {
            for item in items {
                rewrite_struct_wad_chunk_links(&mut item.0, linked_chunk_hashes);
            }
        }
        _ => {}
    }
}

fn rewrite_optional_wad_chunk_links(
    value: &mut values::Optional<NoMeta>,
    linked_chunk_hashes: &HashMap<u64, u64>,
) {
    match value {
        values::Optional::WadChunkLink {
            value: Some(value), ..
        } => {
            if let Some(target_hash) = linked_chunk_hashes.get(&value.value) {
                value.value = *target_hash;
            }
        }
        values::Optional::Struct {
            value: Some(value), ..
        } => {
            rewrite_struct_wad_chunk_links(value, linked_chunk_hashes);
        }
        values::Optional::Embedded {
            value: Some(value), ..
        } => {
            rewrite_struct_wad_chunk_links(&mut value.0, linked_chunk_hashes);
        }
        _ => {}
    }
}

fn rewrite_struct_skin_refs(
    champion_id: &str,
    value: &mut values::Struct<NoMeta>,
    object_hash_rewrites: &HashMap<u32, u32>,
    replacements: &SkinTokenReplacements,
    linked_chunks: &mut BTreeMap<Utf8PathBuf, Utf8PathBuf>,
) {
    for value in value.properties.values_mut() {
        rewrite_property_skin_refs(
            champion_id,
            value,
            object_hash_rewrites,
            replacements,
            linked_chunks,
        );
    }
}

fn rewrite_container_skin_refs(
    champion_id: &str,
    value: &mut values::Container<NoMeta>,
    object_hash_rewrites: &HashMap<u32, u32>,
    replacements: &SkinTokenReplacements,
    linked_chunks: &mut BTreeMap<Utf8PathBuf, Utf8PathBuf>,
) {
    match value {
        values::Container::String { items, .. } => {
            for item in items {
                rewrite_string_skin_refs(champion_id, &mut item.value, replacements, linked_chunks);
            }
        }
        values::Container::Hash { items, .. } => {
            for item in items {
                if let Some(target_hash) = object_hash_rewrites.get(&item.value) {
                    item.value = *target_hash;
                }
            }
        }
        values::Container::ObjectLink { items, .. } => {
            for item in items {
                if let Some(target_hash) = object_hash_rewrites.get(&item.value) {
                    item.value = *target_hash;
                }
            }
        }
        values::Container::Struct { items, .. } => {
            for item in items {
                rewrite_struct_skin_refs(
                    champion_id,
                    item,
                    object_hash_rewrites,
                    replacements,
                    linked_chunks,
                );
            }
        }
        values::Container::Embedded { items, .. } => {
            for item in items {
                rewrite_struct_skin_refs(
                    champion_id,
                    &mut item.0,
                    object_hash_rewrites,
                    replacements,
                    linked_chunks,
                );
            }
        }
        _ => {}
    }
}

fn rewrite_optional_skin_refs(
    champion_id: &str,
    value: &mut values::Optional<NoMeta>,
    object_hash_rewrites: &HashMap<u32, u32>,
    replacements: &SkinTokenReplacements,
    linked_chunks: &mut BTreeMap<Utf8PathBuf, Utf8PathBuf>,
) {
    match value {
        values::Optional::String {
            value: Some(value), ..
        } => {
            rewrite_string_skin_refs(champion_id, &mut value.value, replacements, linked_chunks);
        }
        values::Optional::Hash {
            value: Some(value), ..
        } => {
            if let Some(target_hash) = object_hash_rewrites.get(&value.value) {
                value.value = *target_hash;
            }
        }
        values::Optional::ObjectLink {
            value: Some(value), ..
        } => {
            if let Some(target_hash) = object_hash_rewrites.get(&value.value) {
                value.value = *target_hash;
            }
        }
        values::Optional::Struct {
            value: Some(value), ..
        } => {
            rewrite_struct_skin_refs(
                champion_id,
                value,
                object_hash_rewrites,
                replacements,
                linked_chunks,
            );
        }
        values::Optional::Embedded {
            value: Some(value), ..
        } => {
            rewrite_struct_skin_refs(
                champion_id,
                &mut value.0,
                object_hash_rewrites,
                replacements,
                linked_chunks,
            );
        }
        _ => {}
    }
}

fn rewrite_string_skin_refs(
    champion_id: &str,
    value: &mut String,
    replacements: &SkinTokenReplacements,
    linked_chunks: &mut BTreeMap<Utf8PathBuf, Utf8PathBuf>,
) {
    if is_unscoped_runtime_identifier(champion_id, value) {
        return;
    }
    let Some(rewritten) = replace_skin_tokens(value, replacements) else {
        return;
    };
    if let Some((source_path, target_path)) = skin_chunk_path_pair(value, &rewritten) {
        linked_chunks.insert(source_path, target_path);
    }
    *value = rewritten;
}

fn replace_skin_tokens(value: &str, replacements: &SkinTokenReplacements) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    let mut changed = false;

    while index < value.len() {
        let mut matched = None;
        for token in &replacements.source_tokens {
            if lower[index..].starts_with(token)
                && lower[index + token.len()..]
                    .chars()
                    .next()
                    .map(|c| !c.is_ascii_digit())
                    .unwrap_or(true)
            {
                matched = Some(token.len());
                break;
            }
        }

        if let Some(len) = matched {
            let original = &value[index..index + len];
            let replacement = if original.starts_with('S') {
                "Skin0"
            } else {
                "skin0"
            };
            output.push_str(replacement);
            index += len;
            changed = true;
        } else {
            let ch = value[index..].chars().next().expect("valid string index");
            output.push(ch);
            index += ch.len_utf8();
        }
    }

    changed.then_some(output)
}

fn skin_chunk_path_pair(source: &str, target: &str) -> Option<(Utf8PathBuf, Utf8PathBuf)> {
    if !source.contains('/') || !source.rsplit('/').next()?.contains('.') {
        return None;
    }
    Some((normalize_wad_path(source)?, normalize_wad_path(target)?))
}

fn is_unscoped_runtime_identifier(champion_id: &str, value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let champion = champion_id.to_ascii_lowercase();
    let is_identifier = !lower.contains('/') && !lower.contains('\\');
    lower.contains("wwise")
        || lower.starts_with("play_sfx_")
        || lower.starts_with("stop_sfx_")
        || lower.starts_with("play_vo_")
        || lower.starts_with("stop_vo_")
        || (is_identifier && lower.starts_with(&format!("{champion}skin")))
        || (is_identifier
            && (lower.contains("_sfx") || lower.contains("_vo") || lower.contains("_audio")))
}

fn normalize_wad_path(path: &str) -> Option<Utf8PathBuf> {
    let normalized = path
        .trim_start_matches('/')
        .replace('\\', "/")
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    Some(Utf8PathBuf::from(normalized))
}

fn skin_object_hash(champion_id: &str, skin_number: u32, suffix: Option<&str>) -> u32 {
    let suffix = suffix.unwrap_or_default();
    ltk_hash::fnv1a::hash_lower(&format!(
        "Characters/{champion_id}/Skins/Skin{skin_number}{suffix}"
    ))
}

fn skin_object_hash_rewrites(champion_id: &str, target_skin: u32) -> HashMap<u32, u32> {
    [None, Some("/Resources")]
        .into_iter()
        .map(|suffix| {
            (
                skin_object_hash(champion_id, target_skin, suffix),
                skin_object_hash(champion_id, 0, suffix),
            )
        })
        .collect()
}

fn skin_remap_fingerprint(entries: &HashMap<String, BTreeMap<Utf8PathBuf, Vec<u8>>>) -> u64 {
    let mut buf = Vec::new();
    let mut wad_names: Vec<&String> = entries.keys().collect();
    wad_names.sort();
    for wad_name in wad_names {
        buf.extend_from_slice(wad_name.as_bytes());
        buf.push(0);
        if let Some(wad_entries) = entries.get(wad_name) {
            for (path, bytes) in wad_entries {
                buf.extend_from_slice(path.as_str().as_bytes());
                buf.push(0);
                buf.extend_from_slice(&xxh3_64(bytes).to_le_bytes());
            }
        }
    }
    xxh3_64(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ltk_meta::{property::values, property::NoMeta, BinObject};
    use ltk_overlay::ModContentProvider;

    fn sample_skin_bin(champion_id: &str, skin_number: u32) -> Vec<u8> {
        let object = BinObject::<NoMeta>::builder(
            skin_object_hash(champion_id, skin_number, None),
            ltk_hash::fnv1a::hash_lower("SkinCharacterDataProperties"),
        )
        .property(
            ltk_hash::fnv1a::hash_lower("championSkinName"),
            values::String::from(format!("{champion_id}Skin{skin_number}")),
        )
        .property(
            ltk_hash::fnv1a::hash_lower("skinNumber"),
            values::U32::from(skin_number),
        )
        .build();
        let bin = Bin::new([object], ["DATA/Characters/Test/Test.bin"]);
        let mut output = Cursor::new(Vec::new());
        bin.to_writer(&mut output).unwrap();
        output.into_inner()
    }

    fn sample_skin_bin_with_strings(
        champion_id: &str,
        skin_number: u32,
        strings: &[(&str, &str)],
    ) -> Vec<u8> {
        let mut object = BinObject::<NoMeta>::builder(
            skin_object_hash(champion_id, skin_number, None),
            ltk_hash::fnv1a::hash_lower("SkinCharacterDataProperties"),
        );
        for (name, value) in strings {
            object = object.property(
                ltk_hash::fnv1a::hash_lower(name),
                values::String::from(*value),
            );
        }
        let bin = Bin::new([object.build()], ["DATA/Characters/Test/Test.bin"]);
        let mut output = Cursor::new(Vec::new());
        bin.to_writer(&mut output).unwrap();
        output.into_inner()
    }

    fn write_test_wad(path: &Path, chunks: Vec<(&str, Vec<u8>)>) {
        let mut builder = ltk_wad::WadBuilder::default();
        let mut chunk_bytes = HashMap::new();
        for (chunk_path, bytes) in chunks {
            builder = builder.with_chunk(ltk_wad::WadChunkBuilder::default().with_path(chunk_path));
            chunk_bytes.insert(wad_path_hash(chunk_path), bytes);
        }

        let mut wad_data = Cursor::new(Vec::new());
        builder
            .build_to_writer(&mut wad_data, |path_hash, cursor| {
                cursor
                    .get_mut()
                    .extend_from_slice(chunk_bytes.get(&path_hash).unwrap());
                Ok(())
            })
            .unwrap();
        std::fs::write(path, wad_data.into_inner()).unwrap();
    }

    #[test]
    fn rewrite_skin_root_moves_target_root_to_base_root() {
        let rewrite = rewrite_skin_to_base("Ahri", 27, sample_skin_bin("Ahri", 27)).unwrap();
        let bin = Bin::from_reader(&mut Cursor::new(rewrite.bin_bytes)).unwrap();

        assert!(bin.contains_object(skin_object_hash("Ahri", 0, None)));
        assert!(!bin.contains_object(skin_object_hash("Ahri", 27, None)));
    }

    #[test]
    fn rewrite_skin_root_moves_resource_object_to_base_resources() {
        let root = BinObject::<NoMeta>::new(
            skin_object_hash("Graves", 3, None),
            ltk_hash::fnv1a::hash_lower("SkinCharacterDataProperties"),
        );
        let resources = BinObject::<NoMeta>::builder(
            skin_object_hash("Graves", 3, Some("/Resources")),
            ltk_hash::fnv1a::hash_lower("ResourceResolver"),
        )
        .property(
            ltk_hash::fnv1a::hash_lower("selfLink"),
            values::ObjectLink::from(skin_object_hash("Graves", 3, Some("/Resources"))),
        )
        .build();
        let bin = Bin::new([root, resources], ["DATA/Characters/Test/Test.bin"]);
        let mut bytes = Cursor::new(Vec::new());
        bin.to_writer(&mut bytes).unwrap();

        let rewrite = rewrite_skin_to_base("Graves", 3, bytes.into_inner()).unwrap();
        let bin = Bin::from_reader(&mut Cursor::new(rewrite.bin_bytes)).unwrap();
        let resources = bin
            .get_object(skin_object_hash("Graves", 0, Some("/Resources")))
            .unwrap();

        assert!(!bin.contains_object(skin_object_hash("Graves", 3, Some("/Resources"))));
        assert_eq!(
            resources
                .get_property(ltk_hash::fnv1a::hash_lower("selfLink"))
                .unwrap(),
            &PropertyValueEnum::ObjectLink(values::ObjectLink::from(skin_object_hash(
                "Graves",
                0,
                Some("/Resources")
            )))
        );
    }

    #[test]
    fn rewrite_skin_root_preserves_skin_scoped_child_object_hashes() {
        let source_path = "Characters/Naafiri/Skins/Skin11/Particles/Naafiri_Skin11_W_Dash";
        let target_path = "Characters/Naafiri/Skins/Skin0/Particles/Naafiri_Skin0_W_Dash";
        let path_property = ltk_hash::fnv1a::hash_lower("path");
        let link_property = ltk_hash::fnv1a::hash_lower("effectLink");
        let root = BinObject::<NoMeta>::builder(
            skin_object_hash("Naafiri", 11, None),
            ltk_hash::fnv1a::hash_lower("SkinCharacterDataProperties"),
        )
        .property(
            link_property,
            values::ObjectLink::from(ltk_hash::fnv1a::hash_lower(source_path)),
        )
        .build();
        let effect = BinObject::<NoMeta>::builder(
            ltk_hash::fnv1a::hash_lower(source_path),
            ltk_hash::fnv1a::hash_lower("VfxSystemDefinitionData"),
        )
        .property(path_property, values::String::from(source_path))
        .build();
        let bin = Bin::new([root, effect], ["DATA/Characters/Test/Test.bin"]);
        let mut bytes = Cursor::new(Vec::new());
        bin.to_writer(&mut bytes).unwrap();

        let rewrite = rewrite_skin_to_base("Naafiri", 11, bytes.into_inner()).unwrap();
        let bin = Bin::from_reader(&mut Cursor::new(rewrite.bin_bytes)).unwrap();
        let root = bin
            .get_object(skin_object_hash("Naafiri", 0, None))
            .unwrap();
        let effect = bin
            .get_object(ltk_hash::fnv1a::hash_lower(source_path))
            .unwrap();

        assert!(!bin.contains_object(ltk_hash::fnv1a::hash_lower(target_path)));
        assert_eq!(
            root.get_property(link_property).unwrap(),
            &PropertyValueEnum::ObjectLink(values::ObjectLink::from(ltk_hash::fnv1a::hash_lower(
                source_path
            )))
        );
        assert_eq!(
            effect.get_property(path_property).unwrap(),
            &PropertyValueEnum::String(values::String::from(target_path))
        );
    }

    #[test]
    fn rewrite_skin_root_preserves_champion_scoped_child_object_hashes() {
        let source_path = "Characters/LeeSin/CAC/LeeSin_Skin11";
        let target_path = "Characters/LeeSin/CAC/LeeSin_Skin0";
        let path_property = ltk_hash::fnv1a::hash_lower("path");
        let link_property = ltk_hash::fnv1a::hash_lower("abilityCatalog");
        let root = BinObject::<NoMeta>::builder(
            skin_object_hash("LeeSin", 11, None),
            ltk_hash::fnv1a::hash_lower("SkinCharacterDataProperties"),
        )
        .property(
            link_property,
            values::ObjectLink::from(ltk_hash::fnv1a::hash_lower(source_path)),
        )
        .build();
        let catalog = BinObject::<NoMeta>::builder(
            ltk_hash::fnv1a::hash_lower(source_path),
            ltk_hash::fnv1a::hash_lower("CharacterAbilityCatalog"),
        )
        .property(path_property, values::String::from(source_path))
        .build();
        let bin = Bin::new([root, catalog], ["DATA/Characters/Test/Test.bin"]);
        let mut bytes = Cursor::new(Vec::new());
        bin.to_writer(&mut bytes).unwrap();

        let rewrite = rewrite_skin_to_base("LeeSin", 11, bytes.into_inner()).unwrap();
        let bin = Bin::from_reader(&mut Cursor::new(rewrite.bin_bytes)).unwrap();
        let root = bin.get_object(skin_object_hash("LeeSin", 0, None)).unwrap();
        let catalog = bin
            .get_object(ltk_hash::fnv1a::hash_lower(source_path))
            .unwrap();

        assert!(!bin.contains_object(ltk_hash::fnv1a::hash_lower(target_path)));
        assert_eq!(
            root.get_property(link_property).unwrap(),
            &PropertyValueEnum::ObjectLink(values::ObjectLink::from(ltk_hash::fnv1a::hash_lower(
                source_path
            )))
        );
        assert_eq!(
            catalog.get_property(path_property).unwrap(),
            &PropertyValueEnum::String(values::String::from(target_path))
        );
    }

    #[test]
    fn rewrite_skin_refs_updates_wad_chunk_links_for_copied_paths() {
        let path_property = ltk_hash::fnv1a::hash_lower("resourcePath");
        let link_property = ltk_hash::fnv1a::hash_lower("resourceLink");
        let source_path = "data/characters/ahri/skins/skin27/particles/ahri_skin27_q.tex";
        let target_path = "data/characters/ahri/skins/skin0/particles/ahri_skin0_q.tex";
        let object = BinObject::<NoMeta>::builder(
            skin_object_hash("Ahri", 27, None),
            ltk_hash::fnv1a::hash_lower("SkinCharacterDataProperties"),
        )
        .property(path_property, values::String::from(source_path))
        .property(
            link_property,
            values::WadChunkLink::from(wad_path_hash(source_path)),
        )
        .build();
        let bin = Bin::new([object], ["DATA/Characters/Test/Test.bin"]);
        let mut bytes = Cursor::new(Vec::new());
        bin.to_writer(&mut bytes).unwrap();

        let rewrite = rewrite_skin_to_base("Ahri", 27, bytes.into_inner()).unwrap();
        let bin = Bin::from_reader(&mut Cursor::new(rewrite.bin_bytes)).unwrap();
        let object = bin.get_object(skin_object_hash("Ahri", 0, None)).unwrap();

        assert!(rewrite
            .linked_chunks
            .contains_key(&Utf8PathBuf::from(source_path)));
        assert_eq!(
            object.get_property(path_property).unwrap(),
            &PropertyValueEnum::String(values::String::from(target_path))
        );
        assert_eq!(
            object.get_property(link_property).unwrap(),
            &PropertyValueEnum::WadChunkLink(values::WadChunkLink::from(wad_path_hash(
                target_path
            )))
        );
    }

    #[test]
    fn rewrite_skin_refs_preserves_runtime_identifiers() {
        let audio_path_property = ltk_hash::fnv1a::hash_lower("audioPath");
        let bank_property = ltk_hash::fnv1a::hash_lower("audioBank");
        let event_property = ltk_hash::fnv1a::hash_lower("audioEvent");
        let skin_name_property = ltk_hash::fnv1a::hash_lower("championSkinName");
        let source_path =
            "ASSETS/Sounds/Wwise2016/VO/en_US/Characters/Ahri/Skins/Skin27/Ahri_Skin27_VO_audio.bnk";
        let event = "Play_vo_AhriSkin27_Move2DStandard";
        let bank = "Ahri_Skin27_VO";
        let skin_name = "AhriSkin27";
        let bytes = sample_skin_bin_with_strings(
            "Ahri",
            27,
            &[
                ("audioPath", source_path),
                ("audioBank", bank),
                ("audioEvent", event),
                ("championSkinName", skin_name),
            ],
        );

        let rewrite = rewrite_skin_to_base("Ahri", 27, bytes).unwrap();
        let bin = Bin::from_reader(&mut Cursor::new(rewrite.bin_bytes)).unwrap();
        let object = bin.get_object(skin_object_hash("Ahri", 0, None)).unwrap();

        assert_eq!(
            object.get_property(audio_path_property).unwrap(),
            &PropertyValueEnum::String(values::String::from(source_path))
        );
        assert_eq!(
            object.get_property(bank_property).unwrap(),
            &PropertyValueEnum::String(values::String::from(bank))
        );
        assert_eq!(
            object.get_property(event_property).unwrap(),
            &PropertyValueEnum::String(values::String::from(event))
        );
        assert_eq!(
            object.get_property(skin_name_property).unwrap(),
            &PropertyValueEnum::String(values::String::from(skin_name))
        );
        assert!(rewrite.linked_chunks.is_empty());
    }

    #[test]
    fn skin_remap_content_routes_linked_chunks_from_locale_wads() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
        std::fs::create_dir_all(&champions_dir).unwrap();
        let primary_wad_path = champions_dir.join("Ahri.wad.client");
        let locale_wad_path = champions_dir.join("Ahri.en_US.wad.client");
        let source_locale_path =
            "assets/characters/ahri/skins/skin27/particles/ahri_skin27_locale.tex";
        let target_locale_path =
            "assets/characters/ahri/skins/skin0/particles/ahri_skin0_locale.tex";

        write_test_wad(
            &primary_wad_path,
            vec![(
                "data/characters/ahri/skins/skin27.bin",
                sample_skin_bin_with_strings(
                    "Ahri",
                    27,
                    &[(
                        "localizedParticlePath",
                        "ASSETS/Characters/Ahri/Skins/Skin27/Particles/Ahri_Skin27_Locale.tex",
                    )],
                ),
            )],
        );
        write_test_wad(
            &locale_wad_path,
            vec![(source_locale_path, b"localized-particle".to_vec())],
        );

        let mut content = SkinRemapContent::new(
            game_dir.to_path_buf(),
            vec![SkinRemap {
                champion_id: "Ahri".to_string(),
                champion_name: "Ahri".to_string(),
                target_skin_number: 27,
                target_skin_name: Some("Spirit Blossom Ahri".to_string()),
                target_chroma_id: None,
                target_chroma_name: None,
            }],
        )
        .unwrap();

        let mut wads = content.list_layer_wads(BASE_LAYER).unwrap();
        wads.sort();
        assert_eq!(wads, vec!["ahri.en_us.wad.client", "ahri.wad.client"]);

        let primary_overrides = content
            .read_wad_overrides(BASE_LAYER, "ahri.wad.client")
            .unwrap();
        assert_eq!(primary_overrides.len(), 1);
        assert_eq!(
            primary_overrides[0].0.as_str(),
            "data/characters/ahri/skins/skin0.bin"
        );

        let locale_overrides = content
            .read_wad_overrides(BASE_LAYER, "ahri.en_us.wad.client")
            .unwrap();
        assert_eq!(
            locale_overrides,
            vec![(
                Utf8PathBuf::from(target_locale_path),
                b"localized-particle".to_vec()
            )]
        );
    }

    #[test]
    fn skin_remap_content_generates_referenced_companion_skin_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
        std::fs::create_dir_all(&champions_dir).unwrap();
        let wad_path = champions_dir.join("Naafiri.wad.client");
        let companion_source_path =
            "assets/characters/naafiripackmate/skins/skin20/naafiripackmate_skin20.skn";
        let companion_target_path =
            "assets/characters/naafiripackmate/skins/skin0/naafiripackmate_skin0.skn";

        write_test_wad(
            &wad_path,
            vec![
                (
                    "data/characters/naafiri/skins/skin20.bin",
                    sample_skin_bin_with_strings(
                        "Naafiri",
                        20,
                        &[("companionCharacter", "NaafiriPackmate")],
                    ),
                ),
                (
                    "data/characters/naafiripackmate/skins/skin20.bin",
                    sample_skin_bin_with_strings(
                        "NaafiriPackmate",
                        20,
                        &[(
                            "modelPath",
                            "ASSETS/Characters/NaafiriPackmate/Skins/Skin20/NaafiriPackmate_Skin20.skn",
                        )],
                    ),
                ),
                (companion_source_path, b"companion-model".to_vec()),
            ],
        );

        let mut content = SkinRemapContent::new(
            game_dir.to_path_buf(),
            vec![SkinRemap {
                champion_id: "Naafiri".to_string(),
                champion_name: "Naafiri".to_string(),
                target_skin_number: 20,
                target_skin_name: Some("Glizzy Naafiri".to_string()),
                target_chroma_id: None,
                target_chroma_name: None,
            }],
        )
        .unwrap();

        let overrides = content
            .read_wad_overrides(BASE_LAYER, "naafiri.wad.client")
            .unwrap();
        let override_paths = overrides
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<BTreeSet<_>>();

        assert!(override_paths.contains("data/characters/naafiri/skins/skin0.bin"));
        assert!(override_paths.contains("data/characters/naafiripackmate/skins/skin0.bin"));
        assert!(override_paths.contains(companion_target_path));

        let companion_bin_bytes = overrides
            .iter()
            .find(|(path, _)| path.as_str() == "data/characters/naafiripackmate/skins/skin0.bin")
            .map(|(_, bytes)| bytes)
            .unwrap();
        let companion_bin = Bin::from_reader(&mut Cursor::new(companion_bin_bytes)).unwrap();
        assert!(companion_bin.contains_object(skin_object_hash("NaafiriPackmate", 0, None)));
        assert!(!companion_bin.contains_object(skin_object_hash("NaafiriPackmate", 20, None)));
    }

    #[test]
    fn skin_remap_content_generates_term_referenced_companion_skin_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
        std::fs::create_dir_all(&champions_dir).unwrap();
        let wad_path = champions_dir.join("Yorick.wad.client");

        write_test_wad(
            &wad_path,
            vec![
                (
                    "data/characters/yorick/skins/skin4.bin",
                    sample_skin_bin_with_strings(
                        "Yorick",
                        4,
                        &[("ghoulSfx", "Play_sfx_YorickSkin04_YorickQ_GhoulAttack_cast")],
                    ),
                ),
                (
                    "data/characters/yorickghoulmelee/skins/skin4.bin",
                    sample_skin_bin_with_strings(
                        "YorickGhoulMelee",
                        4,
                        &[(
                            "modelPath",
                            "ASSETS/Characters/YorickGhoulMelee/Skins/Skin04/YorickGhoulMelee_Skin04.skn",
                        )],
                    ),
                ),
            ],
        );

        let mut content = SkinRemapContent::new(
            game_dir.to_path_buf(),
            vec![SkinRemap {
                champion_id: "Yorick".to_string(),
                champion_name: "Yorick".to_string(),
                target_skin_number: 4,
                target_skin_name: Some("Meowrick".to_string()),
                target_chroma_id: None,
                target_chroma_name: None,
            }],
        )
        .unwrap();

        let overrides = content
            .read_wad_overrides(BASE_LAYER, "yorick.wad.client")
            .unwrap();
        let override_paths = overrides
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<BTreeSet<_>>();

        assert!(override_paths.contains("data/characters/yorick/skins/skin0.bin"));
        assert!(override_paths.contains("data/characters/yorickghoulmelee/skins/skin0.bin"));
    }

    #[test]
    fn skin_remap_content_uses_chroma_id_as_target_skin_slot() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
        std::fs::create_dir_all(&champions_dir).unwrap();
        let wad_path = champions_dir.join("Ahri.wad.client");
        let mut wad_data = Cursor::new(Vec::new());
        ltk_wad::WadBuilder::default()
            .with_chunk(
                ltk_wad::WadChunkBuilder::default()
                    .with_path("data/characters/ahri/skins/skin28.bin"),
            )
            .build_to_writer(&mut wad_data, |_path_hash, cursor| {
                cursor
                    .get_mut()
                    .extend_from_slice(&sample_skin_bin("Ahri", 28));
                Ok(())
            })
            .unwrap();
        std::fs::write(&wad_path, wad_data.into_inner()).unwrap();

        let mut content = SkinRemapContent::new(
            game_dir.to_path_buf(),
            vec![SkinRemap {
                champion_id: "Ahri".to_string(),
                champion_name: "Ahri".to_string(),
                target_skin_number: 27,
                target_skin_name: Some("Spirit Blossom Ahri".to_string()),
                target_chroma_id: Some(103028),
                target_chroma_name: Some("Ruby".to_string()),
            }],
        )
        .unwrap();

        let overrides = content
            .read_wad_overrides(BASE_LAYER, "ahri.wad.client")
            .unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(
            overrides[0].0.as_str(),
            "data/characters/ahri/skins/skin0.bin"
        );

        let bin = Bin::from_reader(&mut Cursor::new(&overrides[0].1)).unwrap();
        assert!(bin.contains_object(skin_object_hash("Ahri", 0, None)));
        assert!(!bin.contains_object(skin_object_hash("Ahri", 28, None)));
    }

    #[test]
    fn skin_remap_fingerprint_changes_with_selected_skin() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        let champions_dir = game_dir.join("DATA").join("FINAL").join("Champions");
        std::fs::create_dir_all(&champions_dir).unwrap();
        let wad_path = champions_dir.join("Ahri.wad.client");
        let mut wad_data = Cursor::new(Vec::new());
        ltk_wad::WadBuilder::default()
            .with_chunk(
                ltk_wad::WadChunkBuilder::default()
                    .with_path("data/characters/ahri/skins/skin27.bin"),
            )
            .with_chunk(
                ltk_wad::WadChunkBuilder::default()
                    .with_path("data/characters/ahri/skins/skin28.bin"),
            )
            .build_to_writer(&mut wad_data, |path_hash, cursor| {
                let skin_number =
                    if path_hash == xxh64("data/characters/ahri/skins/skin27.bin".as_bytes(), 0) {
                        27
                    } else {
                        28
                    };
                cursor
                    .get_mut()
                    .extend_from_slice(&sample_skin_bin("Ahri", skin_number));
                Ok(())
            })
            .unwrap();
        std::fs::write(&wad_path, wad_data.into_inner()).unwrap();

        let first = SkinRemapContent::new(
            game_dir.to_path_buf(),
            vec![SkinRemap {
                champion_id: "Ahri".to_string(),
                champion_name: "Ahri".to_string(),
                target_skin_number: 27,
                target_skin_name: Some("Spirit Blossom Ahri".to_string()),
                target_chroma_id: None,
                target_chroma_name: None,
            }],
        )
        .unwrap();
        let second = SkinRemapContent::new(
            game_dir.to_path_buf(),
            vec![SkinRemap {
                champion_id: "Ahri".to_string(),
                champion_name: "Ahri".to_string(),
                target_skin_number: 28,
                target_skin_name: Some("Arcana Ahri".to_string()),
                target_chroma_id: None,
                target_chroma_name: None,
            }],
        )
        .unwrap();

        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn test_skin_remap_fingerprint_same_entries() {
        let mut entries1 = HashMap::new();
        let mut entry_map1 = BTreeMap::new();
        entry_map1.insert(Utf8PathBuf::from("a/b/c"), vec![1, 2, 3]);
        entries1.insert("wad1".to_string(), entry_map1);

        let mut entries2 = HashMap::new();
        let mut entry_map2 = BTreeMap::new();
        entry_map2.insert(Utf8PathBuf::from("a/b/c"), vec![1, 2, 3]);
        entries2.insert("wad1".to_string(), entry_map2);

        assert_eq!(
            skin_remap_fingerprint(&entries1),
            skin_remap_fingerprint(&entries2)
        );
    }

    #[test]
    fn test_skin_remap_fingerprint_changing_path() {
        let mut entries1 = HashMap::new();
        let mut entry_map1 = BTreeMap::new();
        entry_map1.insert(Utf8PathBuf::from("a/b/c"), vec![1, 2, 3]);
        entries1.insert("wad1".to_string(), entry_map1);

        let mut entries2 = HashMap::new();
        let mut entry_map2 = BTreeMap::new();
        entry_map2.insert(Utf8PathBuf::from("a/b/d"), vec![1, 2, 3]);
        entries2.insert("wad1".to_string(), entry_map2);

        assert_ne!(
            skin_remap_fingerprint(&entries1),
            skin_remap_fingerprint(&entries2)
        );
    }

    #[test]
    fn test_skin_remap_fingerprint_changing_bytes() {
        let mut entries1 = HashMap::new();
        let mut entry_map1 = BTreeMap::new();
        entry_map1.insert(Utf8PathBuf::from("a/b/c"), vec![1, 2, 3]);
        entries1.insert("wad1".to_string(), entry_map1);

        let mut entries2 = HashMap::new();
        let mut entry_map2 = BTreeMap::new();
        entry_map2.insert(Utf8PathBuf::from("a/b/c"), vec![1, 2, 4]);
        entries2.insert("wad1".to_string(), entry_map2);

        assert_ne!(
            skin_remap_fingerprint(&entries1),
            skin_remap_fingerprint(&entries2)
        );
    }

    #[test]
    fn test_skin_remap_fingerprint_changing_wad() {
        let mut entries1 = HashMap::new();
        let mut entry_map1 = BTreeMap::new();
        entry_map1.insert(Utf8PathBuf::from("a/b/c"), vec![1, 2, 3]);
        entries1.insert("wad1".to_string(), entry_map1);

        let mut entries2 = HashMap::new();
        let mut entry_map2 = BTreeMap::new();
        entry_map2.insert(Utf8PathBuf::from("a/b/c"), vec![1, 2, 3]);
        entries2.insert("wad2".to_string(), entry_map2);

        assert_ne!(
            skin_remap_fingerprint(&entries1),
            skin_remap_fingerprint(&entries2)
        );
    }
}
