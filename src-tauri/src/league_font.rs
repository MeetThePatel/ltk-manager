use std::fs;
use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::mods::{
    FontSelection, FontValidation, FontValidationIssue, FontValidationSeverity, SystemFont,
};

/// Validate a selected font file based on path, size, format, parsing, and glyph coverage.
pub fn validate_font_file(path: &Path, face_index: Option<u32>) -> FontValidation {
    let mut issues = Vec::new();

    // 1. Path exists and is readable
    if !path.exists() {
        issues.push(FontValidationIssue {
            severity: FontValidationSeverity::Error,
            message: "Font file path does not exist".to_string(),
        });
        return FontValidation {
            is_valid: false,
            issues,
        };
    }

    if !path.is_file() {
        issues.push(FontValidationIssue {
            severity: FontValidationSeverity::Error,
            message: "Font path is not a file".to_string(),
        });
        return FontValidation {
            is_valid: false,
            issues,
        };
    }

    // 2. Extension is .ttf or .otf
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase());

    let is_collection = match ext.as_deref() {
        Some("ttf") | Some("otf") => false,
        Some("ttc") | Some("otc") => true,
        _ => {
            issues.push(FontValidationIssue {
                severity: FontValidationSeverity::Error,
                message: "Unsupported font file extension. Must be .ttf or .otf".to_string(),
            });
            return FontValidation {
                is_valid: false,
                issues,
            };
        }
    };

    if is_collection {
        issues.push(FontValidationIssue {
            severity: FontValidationSeverity::Error,
            message: "TTC/OTC font collections are not supported in this version".to_string(),
        });
        return FontValidation {
            is_valid: false,
            issues,
        };
    }

    // 3. Reject files above a conservative size cap (30MB)
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            issues.push(FontValidationIssue {
                severity: FontValidationSeverity::Error,
                message: format!("Failed to read font file metadata: {}", e),
            });
            return FontValidation {
                is_valid: false,
                issues,
            };
        }
    };

    if metadata.len() > 30 * 1024 * 1024 {
        issues.push(FontValidationIssue {
            severity: FontValidationSeverity::Error,
            message: "Font file size exceeds 30MB limit".to_string(),
        });
        return FontValidation {
            is_valid: false,
            issues,
        };
    }

    // 4. Parse font successfully with ttf-parser
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            issues.push(FontValidationIssue {
                severity: FontValidationSeverity::Error,
                message: format!("Failed to read font file bytes: {}", e),
            });
            return FontValidation {
                is_valid: false,
                issues,
            };
        }
    };

    let idx = face_index.unwrap_or(0);
    let face = match ttf_parser::Face::parse(&bytes, idx) {
        Ok(f) => f,
        Err(e) => {
            issues.push(FontValidationIssue {
                severity: FontValidationSeverity::Error,
                message: format!("Failed to parse font file: {}", e),
            });
            return FontValidation {
                is_valid: false,
                issues,
            };
        }
    };

    // 5. Require basic Latin glyph coverage for letters, digits, and common punctuation
    let mut missing_latin = Vec::new();

    // Letters, digits, common symbols
    let latin_chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
    for ch in latin_chars.chars() {
        if face.glyph_index(ch).is_none() {
            missing_latin.push(ch);
        }
    }

    if !missing_latin.is_empty() {
        let sample: String = missing_latin.iter().take(5).collect();
        issues.push(FontValidationIssue {
            severity: FontValidationSeverity::Error,
            message: format!(
                "Font is missing glyphs for required basic Latin characters (e.g. '{}')",
                sample
            ),
        });
    }

    // 6. Korean coverage check - return warning (not error) if missing
    let mut missing_korean = 0;
    let mut checked_korean = 0;
    // Check Hangul Syllables range (AC00 - D7A3), sample a subset to keep it fast
    let step = 11172 / 100;
    for i in 0..100 {
        let ch_val = 0xAC00 + i * step;
        if let Some(ch) = std::char::from_u32(ch_val) {
            checked_korean += 1;
            if face.glyph_index(ch).is_none() {
                missing_korean += 1;
            }
        }
    }

    let korean_missing_pct = if checked_korean > 0 {
        (missing_korean as f32 / checked_korean as f32) * 100.0
    } else {
        0.0
    };

    if korean_missing_pct > 20.0 {
        issues.push(FontValidationIssue {
            severity: FontValidationSeverity::Warning,
            message: "This font is missing Korean Hangul glyphs. In-game Korean text may render as blocks.".to_string(),
        });
    }

    let is_valid = !issues
        .iter()
        .any(|i| matches!(i.severity, FontValidationSeverity::Error));

    FontValidation { is_valid, issues }
}

/// Discover all system fonts on the OS, validate them, and return.
pub fn discover_system_fonts() -> Vec<SystemFont> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for face in db.faces() {
        if let fontdb::Source::File(path) = &face.source {
            let raw_family = face
                .families
                .first()
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let mut family = raw_family.trim().to_string();
            let mut extra_style_parts = Vec::new();

            let suffixes = [
                "Semi Bold Italic",
                "SemiBoldItalic",
                "Extra Bold Italic",
                "ExtraBoldItalic",
                "Light Italic",
                "LightItalic",
                "Bold Italic",
                "BoldItalic",
                "Extra Light",
                "ExtraLight",
                "Extra Bold",
                "ExtraBold",
                "Semi Bold",
                "SemiBold",
                "Ultra Light",
                "UltraLight",
                "Ultra Bold",
                "UltraBold",
                "Demi Bold",
                "DemiBold",
                "Italic",
                "Italics",
                "Oblique",
                "Light",
                "Bold",
                "Medium",
                "Regular",
                "Thin",
                "Black",
                "Heavy",
                "Book",
            ];

            let mut changed = true;
            while changed {
                changed = false;
                let lower = family.to_lowercase();
                for suffix in &suffixes {
                    let suffix_lower = suffix.to_lowercase();
                    let pattern = format!(" {}", suffix_lower);
                    if lower.ends_with(&pattern) {
                        family.truncate(family.len() - (suffix.len() + 1));
                        family = family.trim().to_string();
                        extra_style_parts.push(*suffix);
                        changed = true;
                        break;
                    }
                }
            }

            let mut style = format!("{:?}", face.style);
            for part in extra_style_parts.iter().rev() {
                if !style.to_lowercase().contains(&part.to_lowercase()) {
                    if style == "Normal" || style == "Regular" {
                        style = part.to_string();
                    } else {
                        style = format!("{} {}", style, part);
                    }
                }
            }

            let weight = face.weight.0;
            let face_index = face.index;

            let key = (family.clone(), style.clone(), weight, path.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            let full_name = if face.post_script_name.is_empty() {
                format!("{} {}", family, style)
            } else {
                face.post_script_name.clone()
            };

            // Run validation
            let validation = validate_font_file(path, Some(face_index));

            result.push(SystemFont {
                family,
                full_name,
                style,
                weight,
                path: path.clone(),
                face_index: Some(face_index),
                is_valid: validation.is_valid,
                issues: validation.issues,
            });
        }
    }

    // Sort fonts: valid ones first, then by family name
    result.sort_by(|a, b| {
        b.is_valid
            .cmp(&a.is_valid)
            .then_with(|| a.family.cmp(&b.family))
            .then_with(|| a.style.cmp(&b.style))
    });

    result
}

/// Validate a single font selection.
pub fn validate_league_font(selection: FontSelection) -> FontValidation {
    validate_font_file(&selection.path, selection.face_index)
}

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer};
use xxhash_rust::xxh3::xxh3_64;

pub const FONT_WAD_NAME: &str = "Bootstrap.windows.wad.client";

pub const FONT_HASH_SLOTS: &[&str] = &[
    "065d75937b643df6",
    "08c6a8c39375fc4f",
    "09d762cfed5714e6",
    "0cecb136ac3567a7",
    "0d24cf15f815cad5",
    "0e73af395498f65c",
    "1542fffe3fc16a27",
    "1cc0ca466c4c4e49",
    "3a6b43f848e3cd4f",
    "3f1d77cbeb531cb3",
    "413b6c1febaf3e44",
    "4f640ba0cd1f5719",
    "50051dc7856a99f6",
    "5821c10be01e1887",
    "5accaf033d0bae00",
    "6020520e05ccac5f",
    "64a3153c058bbaad",
    "715dd36ce6f16bb0",
    "7230d6bacb8c417e",
    "c2fa98cd6e8b86fd",
    "cc4f33959823a195",
    "dcc90acf26cf01fa",
    "e7b1d404020f720d",
    "e98fc2fbdf5cfc27",
];

pub struct LeagueFontContent {
    font_bytes: Vec<u8>,
    fingerprint: u64,
}

impl LeagueFontContent {
    pub fn new(selection: FontSelection) -> AppResult<Self> {
        let path = selection.path.clone();

        if !path.exists() {
            return Err(AppError::Other(format!(
                "Selected font path does not exist: {}",
                path.display()
            )));
        }

        let font_bytes = fs::read(&path)?;

        let metadata = fs::metadata(&path)?;
        let mtime: u64 = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = metadata.len();
        let bytes_hash = xxh3_64(&font_bytes);

        let mut hasher = xxhash_rust::xxh3::Xxh3::new();
        hasher.update(&[1]); // version
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(&selection.face_index.unwrap_or(0).to_le_bytes());
        hasher.update(&size.to_le_bytes());
        hasher.update(&mtime.to_le_bytes());
        hasher.update(&bytes_hash.to_le_bytes());
        let fingerprint = hasher.digest();

        Ok(Self {
            font_bytes,
            fingerprint,
        })
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

impl ltk_overlay::ModContentProvider for LeagueFontContent {
    fn mod_project(&mut self) -> ltk_overlay::Result<ModProject> {
        Ok(ModProject {
            name: "ltk_generated_font".to_string(),
            display_name: "LTK Generated Font".to_string(),
            version: "1.0.0".to_string(),
            description: "Automatically generated font overrides".to_string(),
            authors: vec![ModProjectAuthor::Name("LTK Manager".to_string())],
            license: None,
            tags: vec![ltk_mod_project::ModTag::from("font".to_string())],
            champions: Vec::new(),
            maps: Vec::new(),
            transformers: Vec::new(),
            layers: vec![ModProjectLayer::base()],
            thumbnail: None,
        })
    }

    fn list_layer_wads(&mut self, layer: &str) -> ltk_overlay::Result<Vec<String>> {
        if layer != "base" {
            return Ok(Vec::new());
        }
        Ok(vec![FONT_WAD_NAME.to_string()])
    }

    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> ltk_overlay::Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        if layer != "base" || wad_name != FONT_WAD_NAME {
            return Ok(Vec::new());
        }

        let mut overrides = Vec::new();
        for slot in FONT_HASH_SLOTS {
            let rel_path = Utf8PathBuf::from(format!("{}.bin", slot));
            overrides.push((rel_path, self.font_bytes.clone()));
        }
        Ok(overrides)
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
        if layer != "base" {
            return Err(ltk_overlay::Error::Other(format!(
                "Unknown font override layer: {layer}"
            )));
        }
        if wad_name != FONT_WAD_NAME {
            return Err(ltk_overlay::Error::Other(format!(
                "Unknown font override WAD: {wad_name}"
            )));
        }

        let stem = rel_path.file_stem().unwrap_or("");
        if FONT_HASH_SLOTS.contains(&stem) {
            Ok(self.font_bytes.clone())
        } else {
            Err(ltk_overlay::Error::Other(format!(
                "Font override not found: {wad_name}/{rel_path}"
            )))
        }
    }

    fn read_raw_override_file(&mut self, rel_path: &Utf8Path) -> ltk_overlay::Result<Vec<u8>> {
        Err(ltk_overlay::Error::Other(format!(
            "Font raw override not found: {rel_path}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::Profile;
    use ltk_overlay::ModContentProvider;
    use std::io::Write;

    #[test]
    fn test_font_content_basic() {
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        let dummy_bytes = b"mock font bytes";
        temp_file.write_all(dummy_bytes).unwrap();
        temp_file.flush().unwrap();

        let selection = FontSelection {
            family: "MockFont".to_string(),
            full_name: "MockFont Regular".to_string(),
            style: "Regular".to_string(),
            weight: 400,
            path: temp_file.path().to_path_buf(),
            face_index: Some(0),
        };

        let mut provider = LeagueFontContent::new(selection).unwrap();

        // Check project info
        let project = provider.mod_project().unwrap();
        assert_eq!(project.name, "ltk_generated_font");
        assert_eq!(project.tags[0].to_string(), "font");

        // Check WAD list
        let wads = provider.list_layer_wads("base").unwrap();
        assert_eq!(wads, vec!["Bootstrap.windows.wad.client".to_string()]);

        // Check overrides list
        let overrides = provider
            .read_wad_overrides("base", "Bootstrap.windows.wad.client")
            .unwrap();
        assert_eq!(overrides.len(), 24);
        for (path, bytes) in &overrides {
            assert_eq!(bytes, dummy_bytes);
            let stem = path.file_stem().unwrap();
            assert!(FONT_HASH_SLOTS.contains(&stem));
        }
    }

    #[test]
    fn test_read_wad_override_file() {
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        let dummy_bytes = b"mock font bytes";
        temp_file.write_all(dummy_bytes).unwrap();
        temp_file.flush().unwrap();

        let selection = FontSelection {
            family: "MockFont".to_string(),
            full_name: "MockFont Regular".to_string(),
            style: "Regular".to_string(),
            weight: 400,
            path: temp_file.path().to_path_buf(),
            face_index: Some(0),
        };

        let mut provider = LeagueFontContent::new(selection).unwrap();

        // Valid slot
        let bytes = provider
            .read_wad_override_file(
                "base",
                "Bootstrap.windows.wad.client",
                Utf8Path::new("50051dc7856a99f6.bin"),
            )
            .unwrap();
        assert_eq!(bytes, dummy_bytes);

        // Invalid slot
        let err = provider.read_wad_override_file(
            "base",
            "Bootstrap.windows.wad.client",
            Utf8Path::new("invalid_slot.bin"),
        );
        assert!(err.is_err());

        // Invalid layer
        let err_layer = provider.read_wad_override_file(
            "invalid_layer",
            "Bootstrap.windows.wad.client",
            Utf8Path::new("50051dc7856a99f6.bin"),
        );
        assert!(err_layer.is_err());
    }

    #[test]
    fn test_fingerprint_changes() {
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        let dummy_bytes = b"mock font bytes";
        temp_file.write_all(dummy_bytes).unwrap();
        temp_file.flush().unwrap();

        let selection = FontSelection {
            family: "MockFont".to_string(),
            full_name: "MockFont Regular".to_string(),
            style: "Regular".to_string(),
            weight: 400,
            path: temp_file.path().to_path_buf(),
            face_index: Some(0),
        };

        let provider1 = LeagueFontContent::new(selection.clone()).unwrap();

        // Face index change
        let mut selection_face = selection.clone();
        selection_face.face_index = Some(1);
        let provider_face = LeagueFontContent::new(selection_face).unwrap();
        assert_ne!(provider1.fingerprint(), provider_face.fingerprint());

        // Bytes change
        let mut temp_file2 = tempfile::NamedTempFile::new().unwrap();
        temp_file2.write_all(b"different mock font bytes").unwrap();
        temp_file2.flush().unwrap();

        let mut selection_bytes = selection.clone();
        selection_bytes.path = temp_file2.path().to_path_buf();
        let provider_bytes = LeagueFontContent::new(selection_bytes).unwrap();
        assert_ne!(provider1.fingerprint(), provider_bytes.fingerprint());
    }

    #[test]
    fn test_deserialization_without_font_settings() {
        let json = r#"{
            "id": "p1",
            "name": "Default",
            "slug": "default",
            "enabledMods": [],
            "modOrder": [],
            "layerStates": {},
            "skinRemaps": [],
            "createdAt": "2025-01-01T00:00:00Z",
            "lastUsed": "2025-01-01T00:00:00Z"
        }"#;

        let profile: Profile = serde_json::from_str(json).unwrap();
        assert!(!profile.font_settings.enabled);
        assert!(profile.font_settings.single_font.is_none());
    }
}
