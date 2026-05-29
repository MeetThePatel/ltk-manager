use crate::error::{AppError, AppResult};
use crate::state::Settings;
use chrono::Utc;
use std::collections::HashMap;
use std::fs;
use uuid::Uuid;

use super::{
    get_active_profile, get_profile_by_id, resolve_profile_dirs, LeagueFontSettings, ModLibrary,
    Profile, ProfileSlug, SkinRemap,
};

impl ModLibrary {
    /// Create a new profile.
    pub fn create_profile(&self, settings: &Settings, name: String) -> AppResult<Profile> {
        self.mutate_index(settings, |storage_dir, index| {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(AppError::Other("Profile name cannot be empty".to_string()));
            }

            if index.profiles.iter().any(|p| p.name == name) {
                return Err(AppError::Other(format!(
                    "Profile '{}' already exists",
                    name
                )));
            }

            let slug = ProfileSlug::from_name(&name).ok_or_else(|| {
                AppError::Other(
                    "Profile name must contain at least one alphanumeric character".to_string(),
                )
            })?;
            if !slug.is_unique_in(index, None) {
                return Err(AppError::Other(format!(
                    "Profile '{}' already exists",
                    name
                )));
            }

            let mod_order: Vec<String> = index.mods.iter().map(|m| m.id.clone()).collect();

            let profile = Profile {
                id: Uuid::new_v4().to_string(),
                name,
                slug,
                enabled_mods: Vec::new(),
                mod_order,
                layer_states: HashMap::new(),
                skin_remaps: Vec::new(),
                font_settings: LeagueFontSettings::default(),
                created_at: Utc::now(),
                last_used: Utc::now(),
            };

            let (overlay_dir, cache_dir) = resolve_profile_dirs(storage_dir, &profile.slug);
            fs::create_dir_all(&overlay_dir)?;
            fs::create_dir_all(&cache_dir)?;

            index.profiles.push(profile.clone());

            tracing::info!("Created profile: {} (id={})", profile.name, profile.id);
            Ok(profile)
        })
    }

    /// Delete a profile by ID.
    pub fn delete_profile(&self, settings: &Settings, profile_id: String) -> AppResult<()> {
        self.mutate_index(settings, |storage_dir, index| {
            let profile = get_profile_by_id(index, &profile_id)?;

            if profile.name == "Default" {
                return Err(AppError::Other("Cannot delete Default profile".to_string()));
            }

            if profile_id == index.active_profile_id {
                return Err(AppError::Other(
                    "Cannot delete active profile. Switch to another profile first.".to_string(),
                ));
            }

            let profile_slug = profile.slug.clone();
            index.profiles.retain(|p| p.id != profile_id);

            let profile_dir = storage_dir.join("profiles").join(profile_slug.as_str());
            if profile_dir.exists() {
                fs::remove_dir_all(&profile_dir)?;
                tracing::info!("Deleted profile directory: {}", profile_dir.display());
            }

            tracing::info!("Deleted profile: {}", profile_id);
            Ok(())
        })
    }

    /// Switch to a different profile.
    pub fn switch_profile(&self, settings: &Settings, profile_id: String) -> AppResult<Profile> {
        self.mutate_index(settings, |_storage_dir, index| {
            get_profile_by_id(index, &profile_id)?;
            index.active_profile_id = profile_id.clone();

            let profile = index
                .profiles
                .iter_mut()
                .find(|p| p.id == profile_id)
                .ok_or_else(|| AppError::Other("Profile not found after validation".to_string()))?;

            profile.last_used = Utc::now();
            let result = profile.clone();

            tracing::info!("Switched to profile: {} (id={})", result.name, result.id);
            Ok(result)
        })
    }

    /// Get all profiles.
    pub fn get_profiles(&self, settings: &Settings) -> AppResult<Vec<Profile>> {
        self.with_index(settings, |_storage_dir, index| Ok(index.profiles.clone()))
    }

    /// Rename a profile.
    pub fn rename_profile(
        &self,
        settings: &Settings,
        profile_id: String,
        new_name: String,
    ) -> AppResult<Profile> {
        self.mutate_index(settings, |storage_dir, index| {
            let new_name = new_name.trim().to_string();
            if new_name.is_empty() {
                return Err(AppError::Other("Profile name cannot be empty".to_string()));
            }

            let new_slug = ProfileSlug::from_name(&new_name).ok_or_else(|| {
                AppError::Other(
                    "Profile name must contain at least one alphanumeric character".to_string(),
                )
            })?;

            if index
                .profiles
                .iter()
                .any(|p| p.id != profile_id && p.name == new_name)
            {
                return Err(AppError::Other(format!(
                    "Profile '{}' already exists",
                    new_name
                )));
            }

            if !new_slug.is_unique_in(index, Some(&profile_id)) {
                return Err(AppError::Other(format!(
                    "Profile directory name '{}' conflicts with another profile",
                    new_slug
                )));
            }

            let profile = index
                .profiles
                .iter_mut()
                .find(|p| p.id == profile_id)
                .ok_or_else(|| AppError::Other("Profile not found".to_string()))?;

            if profile.name == "Default" {
                return Err(AppError::Other("Cannot rename Default profile".to_string()));
            }

            // Rename directory on disk if slug changed — done before index update
            // so that if rename fails, the closure returns Err and the index is NOT saved.
            if profile.slug != new_slug {
                let old_dir = storage_dir.join("profiles").join(profile.slug.as_str());
                let new_dir = storage_dir.join("profiles").join(new_slug.as_str());
                if old_dir.exists() {
                    fs::rename(&old_dir, &new_dir)?;
                    tracing::info!(
                        "Renamed profile dir: {} -> {}",
                        old_dir.display(),
                        new_dir.display()
                    );
                }
            }

            profile.name = new_name;
            profile.slug = new_slug;
            let result = profile.clone();

            tracing::info!("Renamed profile {} to: {}", profile_id, result.name);
            Ok(result)
        })
    }

    /// Get the active profile.
    pub fn get_active_profile_info(&self, settings: &Settings) -> AppResult<Profile> {
        self.with_index(settings, |_storage_dir, index| {
            let profile = get_active_profile(index)?;
            Ok(profile.clone())
        })
    }

    /// Get league font settings for a profile, defaulting to the active profile.
    pub fn get_league_font_settings(
        &self,
        settings: &Settings,
        profile_id: Option<String>,
    ) -> AppResult<LeagueFontSettings> {
        self.with_index(settings, |_storage_dir, index| {
            let profile_id = profile_id.unwrap_or_else(|| index.active_profile_id.clone());
            let profile = get_profile_by_id(index, &profile_id)?;
            Ok(profile.font_settings.clone())
        })
    }

    /// Set league font settings for a profile, defaulting to the active profile.
    pub fn set_league_font_settings(
        &self,
        settings: &Settings,
        profile_id: Option<String>,
        font_settings: LeagueFontSettings,
    ) -> AppResult<Profile> {
        let font_settings = normalize_league_font_settings(font_settings)?;

        self.mutate_index(settings, |_storage_dir, index| {
            let profile_id = profile_id.unwrap_or_else(|| index.active_profile_id.clone());
            let profile = index
                .profiles
                .iter_mut()
                .find(|p| p.id == profile_id)
                .ok_or_else(|| AppError::Other(format!("Profile {} not found", profile_id)))?;

            profile.font_settings = font_settings;

            Ok(profile.clone())
        })
    }

    /// Get skin remaps for a profile, defaulting to the active profile.
    pub fn get_skin_remaps(
        &self,
        settings: &Settings,
        profile_id: Option<String>,
    ) -> AppResult<Vec<SkinRemap>> {
        self.with_index(settings, |_storage_dir, index| {
            let profile_id = profile_id.unwrap_or_else(|| index.active_profile_id.clone());
            let profile = get_profile_by_id(index, &profile_id)?;
            Ok(profile.skin_remaps.clone())
        })
    }

    /// Add or replace a champion skin remap for a profile, defaulting to the active profile.
    pub fn set_skin_remap(
        &self,
        settings: &Settings,
        profile_id: Option<String>,
        remap: SkinRemap,
    ) -> AppResult<Profile> {
        let remap = normalize_skin_remap(remap)?;
        self.mutate_index(settings, |_storage_dir, index| {
            let profile_id = profile_id.unwrap_or_else(|| index.active_profile_id.clone());
            let profile = index
                .profiles
                .iter_mut()
                .find(|p| p.id == profile_id)
                .ok_or_else(|| AppError::Other(format!("Profile {} not found", profile_id)))?;

            profile
                .skin_remaps
                .retain(|existing| existing.champion_id != remap.champion_id);
            profile.skin_remaps.push(remap);
            profile
                .skin_remaps
                .sort_by(|a, b| a.champion_name.cmp(&b.champion_name));

            Ok(profile.clone())
        })
    }

    /// Remove a champion skin remap from a profile, defaulting to the active profile.
    pub fn remove_skin_remap(
        &self,
        settings: &Settings,
        profile_id: Option<String>,
        champion_id: String,
    ) -> AppResult<Profile> {
        let champion_id = normalize_champion_id(&champion_id)?;
        self.mutate_index(settings, |_storage_dir, index| {
            let profile_id = profile_id.unwrap_or_else(|| index.active_profile_id.clone());
            let profile = index
                .profiles
                .iter_mut()
                .find(|p| p.id == profile_id)
                .ok_or_else(|| AppError::Other(format!("Profile {} not found", profile_id)))?;

            profile
                .skin_remaps
                .retain(|remap| remap.champion_id != champion_id);

            Ok(profile.clone())
        })
    }
}

fn normalize_skin_remap(remap: SkinRemap) -> AppResult<SkinRemap> {
    if remap.target_skin_number == 0 {
        return Err(AppError::Other(
            "Target skin must be a non-default skin slot".to_string(),
        ));
    }

    let champion_id = normalize_champion_id(&remap.champion_id)?;
    let champion_name = remap.champion_name.trim().to_string();
    if champion_name.is_empty() {
        return Err(AppError::Other("Champion name cannot be empty".to_string()));
    }

    let target_skin_name = remap.target_skin_name.and_then(|name| {
        let name = name.trim().to_string();
        (!name.is_empty()).then_some(name)
    });
    let target_chroma_name = remap.target_chroma_name.and_then(|name| {
        let name = name.trim().to_string();
        (!name.is_empty()).then_some(name)
    });

    Ok(SkinRemap {
        champion_id,
        champion_name,
        target_skin_number: remap.target_skin_number,
        target_skin_name,
        target_chroma_id: remap.target_chroma_id,
        target_chroma_name,
    })
}

pub fn normalize_league_font_settings(
    mut font_settings: LeagueFontSettings,
) -> AppResult<LeagueFontSettings> {
    if !font_settings.enabled {
        font_settings.single_font = None;
    } else if font_settings.single_font.is_none() {
        return Err(AppError::Other(
            "Cannot enable custom font without a selected font".to_string(),
        ));
    } else if let Some(selection) = &font_settings.single_font {
        let validation = crate::league_font::validate_league_font(selection.clone());
        if !validation.is_valid {
            let errors: Vec<String> = validation
                .issues
                .iter()
                .filter(|issue| matches!(issue.severity, super::FontValidationSeverity::Error))
                .map(|issue| issue.message.clone())
                .collect();
            return Err(AppError::Other(format!(
                "Selected League font '{}' ({}) is invalid: {}",
                selection.full_name,
                selection.path.display(),
                errors.join("; ")
            )));
        }
    }
    Ok(font_settings)
}

fn normalize_champion_id(champion_id: &str) -> AppResult<String> {
    let champion_id = champion_id.trim().to_ascii_lowercase();
    if champion_id.is_empty() {
        return Err(AppError::Other("Champion id cannot be empty".to_string()));
    }
    Ok(champion_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_skin_remap_trims_and_normalizes_fields() {
        let remap = normalize_skin_remap(SkinRemap {
            champion_id: " Ahri ".to_string(),
            champion_name: " Ahri ".to_string(),
            target_skin_number: 27,
            target_skin_name: Some(" Star Guardian Ahri ".to_string()),
            target_chroma_id: Some(103027),
            target_chroma_name: Some(" Ruby ".to_string()),
        })
        .unwrap();

        assert_eq!(remap.champion_id, "ahri");
        assert_eq!(remap.champion_name, "Ahri");
        assert_eq!(remap.target_skin_number, 27);
        assert_eq!(
            remap.target_skin_name,
            Some("Star Guardian Ahri".to_string())
        );
        assert_eq!(remap.target_chroma_id, Some(103027));
        assert_eq!(remap.target_chroma_name, Some("Ruby".to_string()));
    }

    #[test]
    fn normalize_skin_remap_rejects_skin_zero() {
        let err = normalize_skin_remap(SkinRemap {
            champion_id: "ahri".to_string(),
            champion_name: "Ahri".to_string(),
            target_skin_number: 0,
            target_skin_name: None,
            target_chroma_id: None,
            target_chroma_name: None,
        });

        assert!(err.is_err());
    }

    #[test]
    fn test_normalize_league_font_settings() {
        use crate::mods::FontSelection;
        use std::path::PathBuf;

        let invalid = LeagueFontSettings {
            enabled: true,
            single_font: None,
        };
        assert!(normalize_league_font_settings(invalid).is_err());

        let selection = FontSelection {
            family: "Mock".to_string(),
            full_name: "Mock Regular".to_string(),
            style: "Regular".to_string(),
            weight: 400,
            path: PathBuf::from("mock_path"),
            face_index: None,
        };
        let disabled_with_font = LeagueFontSettings {
            enabled: false,
            single_font: Some(selection),
        };
        let normalized = normalize_league_font_settings(disabled_with_font).unwrap();
        assert!(!normalized.enabled);
        assert!(normalized.single_font.is_none());
    }

    #[test]
    fn normalize_league_font_settings_rejects_invalid_selected_font() {
        use crate::mods::FontSelection;
        use std::path::PathBuf;

        let invalid = LeagueFontSettings {
            enabled: true,
            single_font: Some(FontSelection {
                family: "Missing".to_string(),
                full_name: "Missing Regular".to_string(),
                style: "Regular".to_string(),
                weight: 400,
                path: PathBuf::from("/definitely/missing/font.ttf"),
                face_index: None,
            }),
        };

        let err = normalize_league_font_settings(invalid).unwrap_err();
        assert!(err
            .to_string()
            .contains("Selected League font 'Missing Regular'"));
    }
}
