use crate::error::{AppError, AppResult};
use crate::state::Settings;
use std::collections::HashSet;
use uuid::Uuid;

use super::{LibraryFolder, LibraryIndex, ModLibrary, ROOT_FOLDER_ID};

/// Flatten folder_order + folder contents into a linear mod ID sequence.
///
/// Iterates folders in `folder_order` order, expanding each folder's `mod_ids`
/// in place. This is the canonical derivation of `Profile.mod_order`.
pub(super) fn flatten_folder_order(index: &LibraryIndex) -> Vec<String> {
    let mut result = Vec::new();
    for folder_id in &index.folder_order {
        if let Some(folder) = index.folders.iter().find(|f| &f.id == folder_id) {
            result.extend(folder.mod_ids.iter().cloned());
        }
    }
    result
}

/// Reconcile folders and folder_order with the current mod set.
///
/// - Removes orphaned mod IDs from all folders.
/// - Removes folder_order entries referencing nonexistent folders.
/// - Appends untracked mods to the root folder.
/// - Ensures all folders appear in folder_order.
pub(super) fn sync_folders(index: &mut LibraryIndex) -> bool {
    let mut changed = false;
    let valid_ids: HashSet<&str> = index.mods.iter().map(|m| m.id.as_str()).collect();

    // Remove orphaned mods from all folders
    for folder in &mut index.folders {
        let before = folder.mod_ids.len();
        folder.mod_ids.retain(|id| valid_ids.contains(id.as_str()));
        if folder.mod_ids.len() != before {
            changed = true;
        }
    }

    // Remove folder_order entries for nonexistent folders
    let valid_folder_ids: HashSet<&str> = index.folders.iter().map(|f| f.id.as_str()).collect();
    let before = index.folder_order.len();
    index
        .folder_order
        .retain(|id| valid_folder_ids.contains(id.as_str()));
    if index.folder_order.len() != before {
        changed = true;
    }

    // Ensure all folders appear in folder_order
    for folder in &index.folders {
        if !index.folder_order.contains(&folder.id) {
            index.folder_order.push(folder.id.clone());
            changed = true;
        }
    }

    // Find mods not in any folder and add them to the root folder
    let tracked: HashSet<String> = index
        .folders
        .iter()
        .flat_map(|f| f.mod_ids.iter().cloned())
        .collect();

    let untracked: Vec<String> = index
        .mods
        .iter()
        .filter(|m| !tracked.contains(&m.id))
        .map(|m| m.id.clone())
        .collect();

    if !untracked.is_empty() {
        if let Some(root) = index.folders.iter_mut().find(|f| f.id == ROOT_FOLDER_ID) {
            root.mod_ids.extend(untracked);
            changed = true;
        }
    }

    changed
}

/// Re-derive `profile.mod_order` for all profiles from folder_order + folders.
fn sync_all_profile_mod_orders(index: &mut LibraryIndex) {
    let flat = flatten_folder_order(index);
    let flat_set: HashSet<&str> = flat.iter().map(|s| s.as_str()).collect();

    for profile in &mut index.profiles {
        profile.mod_order = flat.clone();

        let enabled_set: HashSet<&str> = profile.enabled_mods.iter().map(|s| s.as_str()).collect();
        profile.enabled_mods = flat
            .iter()
            .filter(|id| enabled_set.contains(id.as_str()))
            .cloned()
            .collect();

        profile
            .layer_states
            .retain(|id, _| flat_set.contains(id.as_str()));
    }
}

impl ModLibrary {
    /// Create a new named folder.
    pub fn create_folder(&self, settings: &Settings, name: &str) -> AppResult<LibraryFolder> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::ValidationFailed(
                "Folder name cannot be empty".to_string(),
            ));
        }

        self.mutate_index(settings, |_storage_dir, index| {
            let folder = LibraryFolder {
                id: Uuid::new_v4().to_string(),
                name,
                mod_ids: Vec::new(),
            };
            index.folder_order.push(folder.id.clone());
            index.folders.push(folder.clone());
            Ok(folder)
        })
    }

    /// Rename an existing folder. Cannot rename the root folder.
    pub fn rename_folder(
        &self,
        settings: &Settings,
        folder_id: &str,
        new_name: &str,
    ) -> AppResult<()> {
        if folder_id == ROOT_FOLDER_ID {
            return Err(AppError::ValidationFailed(
                "Cannot rename the root folder".to_string(),
            ));
        }
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return Err(AppError::ValidationFailed(
                "Folder name cannot be empty".to_string(),
            ));
        }

        self.mutate_index(settings, |_storage_dir, index| {
            let folder = index
                .folders
                .iter_mut()
                .find(|f| f.id == folder_id)
                .ok_or_else(|| {
                    AppError::ValidationFailed(format!("Folder {} not found", folder_id))
                })?;
            folder.name = new_name;
            Ok(())
        })
    }

    /// Delete a folder, moving its mods to the root folder. Cannot delete root.
    pub fn delete_folder(&self, settings: &Settings, folder_id: &str) -> AppResult<()> {
        if folder_id == ROOT_FOLDER_ID {
            return Err(AppError::ValidationFailed(
                "Cannot delete the root folder".to_string(),
            ));
        }

        self.mutate_index(settings, |_storage_dir, index| {
            let folder_idx = index
                .folders
                .iter()
                .position(|f| f.id == folder_id)
                .ok_or_else(|| {
                    AppError::ValidationFailed(format!("Folder {} not found", folder_id))
                })?;

            let folder = index.folders.remove(folder_idx);
            index.folder_order.retain(|id| id != folder_id);

            // Move contained mods to the root folder
            if let Some(root) = index.folders.iter_mut().find(|f| f.id == ROOT_FOLDER_ID) {
                root.mod_ids.extend(folder.mod_ids);
            }

            sync_all_profile_mod_orders(index);
            Ok(())
        })
    }

    /// Move a mod into a folder (from any folder, including root).
    pub fn move_mod_to_folder(
        &self,
        settings: &Settings,
        mod_id: &str,
        folder_id: &str,
    ) -> AppResult<()> {
        self.mutate_index(settings, |_storage_dir, index| {
            if !index.mods.iter().any(|m| m.id == mod_id) {
                return Err(AppError::ModNotFound(mod_id.to_string()));
            }

            if !index.folders.iter().any(|f| f.id == folder_id) {
                return Err(AppError::ValidationFailed(format!(
                    "Folder {} not found",
                    folder_id
                )));
            }

            // Check if already in target folder
            if index
                .folders
                .iter()
                .find(|f| f.id == folder_id)
                .is_some_and(|f| f.mod_ids.iter().any(|id| id == mod_id))
            {
                return Err(AppError::ValidationFailed(
                    "Mod is already in the target folder".to_string(),
                ));
            }

            // Remove from current folder
            for folder in &mut index.folders {
                folder.mod_ids.retain(|id| id != mod_id);
            }

            // Add to target folder
            let target = index
                .folders
                .iter_mut()
                .find(|f| f.id == folder_id)
                .unwrap();
            target.mod_ids.push(mod_id.to_string());

            sync_all_profile_mod_orders(index);
            Ok(())
        })
    }

    /// Enable or disable all mods in a folder for the active profile.
    pub fn toggle_folder(
        &self,
        settings: &Settings,
        folder_id: &str,
        enabled: bool,
    ) -> AppResult<()> {
        self.mutate_index(settings, |_storage_dir, index| {
            let folder = index
                .folders
                .iter()
                .find(|f| f.id == folder_id)
                .ok_or_else(|| {
                    AppError::ValidationFailed(format!("Folder {} not found", folder_id))
                })?;

            if folder.mod_ids.is_empty() {
                return Err(AppError::ValidationFailed(
                    "Cannot toggle an empty folder".to_string(),
                ));
            }

            let mod_ids: Vec<String> = folder.mod_ids.clone();
            let active_profile_id = index.active_profile_id.clone();
            let profile = index
                .profiles
                .iter_mut()
                .find(|p| p.id == active_profile_id)
                .ok_or_else(|| AppError::Other("Active profile not found".to_string()))?;

            if enabled {
                let already_enabled: HashSet<String> =
                    profile.enabled_mods.iter().cloned().collect();
                let to_add: Vec<String> = mod_ids
                    .iter()
                    .filter(|id| !already_enabled.contains(id.as_str()))
                    .cloned()
                    .collect();
                for id in to_add {
                    let pos = profile
                        .mod_order
                        .iter()
                        .position(|m| m == &id)
                        .unwrap_or(profile.enabled_mods.len());
                    let insert_at = profile
                        .enabled_mods
                        .iter()
                        .position(|m| {
                            profile.mod_order.iter().position(|o| o == m).unwrap_or(0) > pos
                        })
                        .unwrap_or(profile.enabled_mods.len());
                    profile.enabled_mods.insert(insert_at, id);
                }
            } else {
                let remove_set: HashSet<&str> = mod_ids.iter().map(|s| s.as_str()).collect();
                profile
                    .enabled_mods
                    .retain(|id| !remove_set.contains(id.as_str()));
            }

            Ok(())
        })
    }

    /// Reorder mods within a folder.
    pub fn reorder_folder_mods(
        &self,
        settings: &Settings,
        folder_id: &str,
        mod_ids: Vec<String>,
    ) -> AppResult<()> {
        self.mutate_index(settings, |_storage_dir, index| {
            let folder = index
                .folders
                .iter_mut()
                .find(|f| f.id == folder_id)
                .ok_or_else(|| {
                    AppError::ValidationFailed(format!("Folder {} not found", folder_id))
                })?;

            let mut expected: Vec<&str> = folder.mod_ids.iter().map(|s| s.as_str()).collect();
            expected.sort();
            let mut provided: Vec<&str> = mod_ids.iter().map(|s| s.as_str()).collect();
            provided.sort();

            if expected != provided {
                return Err(AppError::ValidationFailed(
                    "Provided mod IDs do not match the folder's contents".to_string(),
                ));
            }

            folder.mod_ids = mod_ids;
            sync_all_profile_mod_orders(index);
            Ok(())
        })
    }

    /// Reorder top-level folders.
    pub fn reorder_folders(&self, settings: &Settings, folder_order: Vec<String>) -> AppResult<()> {
        self.mutate_index(settings, |_storage_dir, index| {
            let mut expected: Vec<&str> = index.folder_order.iter().map(|s| s.as_str()).collect();
            expected.sort();
            let mut provided: Vec<&str> = folder_order.iter().map(|s| s.as_str()).collect();
            provided.sort();

            if expected != provided {
                return Err(AppError::ValidationFailed(
                    "Provided folder IDs do not match the current folder order".to_string(),
                ));
            }

            index.folder_order = folder_order;
            sync_all_profile_mod_orders(index);
            Ok(())
        })
    }

    /// Get all folders (including root).
    pub fn get_folders(&self, settings: &Settings) -> AppResult<Vec<LibraryFolder>> {
        self.with_index(settings, |_storage_dir, index| Ok(index.folders.clone()))
    }

    /// Get the current folder ordering.
    pub fn get_folder_order(&self, settings: &Settings) -> AppResult<Vec<String>> {
        self.with_index(settings, |_storage_dir, index| {
            Ok(index.folder_order.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::{LibraryModEntry, ModArchiveFormat, Profile, ProfileSlug};
    use chrono::Utc;
    use std::collections::HashMap;

    fn make_mod_entry(id: &str) -> LibraryModEntry {
        LibraryModEntry {
            id: id.to_string(),
            installed_at: Utc::now(),
            format: ModArchiveFormat::Fantome,
        }
    }

    fn make_folder(id: &str, name: &str, mod_ids: Vec<&str>) -> LibraryFolder {
        LibraryFolder {
            id: id.to_string(),
            name: name.to_string(),
            mod_ids: mod_ids.into_iter().map(String::from).collect(),
        }
    }

    fn make_profile(id: &str, mod_order: Vec<&str>, enabled: Vec<&str>) -> Profile {
        Profile {
            id: id.to_string(),
            name: id.to_string(),
            slug: ProfileSlug::from(id.to_string()),
            enabled_mods: enabled.into_iter().map(String::from).collect(),
            mod_order: mod_order.into_iter().map(String::from).collect(),
            layer_states: HashMap::new(),
            skin_remaps: Vec::new(),
            created_at: Utc::now(),
            last_used: Utc::now(),
        }
    }

    fn make_index(
        mods: Vec<&str>,
        folders: Vec<LibraryFolder>,
        folder_order: Vec<&str>,
    ) -> LibraryIndex {
        LibraryIndex {
            version: 0,
            mods: mods.into_iter().map(make_mod_entry).collect(),
            profiles: vec![make_profile("default", vec![], vec![])],
            active_profile_id: "default".to_string(),
            folders,
            folder_order: folder_order.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn flatten_folder_order_empty_index() {
        let index = make_index(vec![], vec![], vec![]);
        assert!(flatten_folder_order(&index).is_empty());
    }

    #[test]
    fn flatten_folder_order_preserves_folder_then_mod_order() {
        let index = make_index(
            vec!["m1", "m2", "m3"],
            vec![
                make_folder(ROOT_FOLDER_ID, "Root", vec!["m3"]),
                make_folder("f1", "Folder 1", vec!["m1", "m2"]),
            ],
            vec![ROOT_FOLDER_ID, "f1"],
        );
        assert_eq!(flatten_folder_order(&index), vec!["m3", "m1", "m2"]);
    }

    #[test]
    fn flatten_folder_order_skips_missing_folder() {
        let index = make_index(
            vec!["m1"],
            vec![make_folder(ROOT_FOLDER_ID, "Root", vec!["m1"])],
            vec![ROOT_FOLDER_ID, "nonexistent"],
        );
        assert_eq!(flatten_folder_order(&index), vec!["m1"]);
    }

    #[test]
    fn sync_folders_removes_orphaned_mods() {
        let mut index = make_index(
            vec!["m1"],
            vec![make_folder(ROOT_FOLDER_ID, "Root", vec!["m1", "m_gone"])],
            vec![ROOT_FOLDER_ID],
        );
        let changed = sync_folders(&mut index);
        assert!(changed);
        assert_eq!(index.folders[0].mod_ids, vec!["m1"]);
    }

    #[test]
    fn sync_folders_removes_orphaned_folder_order_entries() {
        let mut index = make_index(
            vec![],
            vec![make_folder(ROOT_FOLDER_ID, "Root", vec![])],
            vec![ROOT_FOLDER_ID, "gone_folder"],
        );
        let changed = sync_folders(&mut index);
        assert!(changed);
        assert_eq!(index.folder_order, vec![ROOT_FOLDER_ID]);
    }

    #[test]
    fn sync_folders_appends_untracked_mods_to_root() {
        let mut index = make_index(
            vec!["m1", "m2"],
            vec![make_folder(ROOT_FOLDER_ID, "Root", vec!["m1"])],
            vec![ROOT_FOLDER_ID],
        );
        let changed = sync_folders(&mut index);
        assert!(changed);
        assert_eq!(index.folders[0].mod_ids, vec!["m1", "m2"]);
    }

    #[test]
    fn sync_folders_ensures_all_folders_in_order() {
        let mut index = make_index(
            vec![],
            vec![
                make_folder(ROOT_FOLDER_ID, "Root", vec![]),
                make_folder("f1", "Folder 1", vec![]),
            ],
            vec![ROOT_FOLDER_ID],
        );
        let changed = sync_folders(&mut index);
        assert!(changed);
        assert_eq!(index.folder_order, vec![ROOT_FOLDER_ID, "f1"]);
    }

    #[test]
    fn sync_folders_returns_false_when_clean() {
        let mut index = make_index(
            vec!["m1"],
            vec![make_folder(ROOT_FOLDER_ID, "Root", vec!["m1"])],
            vec![ROOT_FOLDER_ID],
        );
        let changed = sync_folders(&mut index);
        assert!(!changed);
    }
}
