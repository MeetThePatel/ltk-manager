import { queryOptions, useQuery } from "@tanstack/react-query";

import {
  api,
  type AppError,
  type InstalledMod,
  type LibraryFolder,
  type Profile,
  type SkinRemap,
} from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { libraryKeys } from "./keys";

// ============================================================================
// Profiles
// ============================================================================

/**
 * Query options for fetching all profiles.
 */
export function profilesQueryOptions() {
  return queryOptions<Profile[], AppError>({
    queryKey: libraryKeys.profiles(),
    queryFn: queryFn(api.listModProfiles),
  });
}

/**
 * Hook to fetch all profiles.
 */
export function useProfiles() {
  return useQuery(profilesQueryOptions());
}

/**
 * Query options for fetching the currently active profile.
 */
export function activeProfileQueryOptions() {
  return queryOptions<Profile, AppError>({
    queryKey: libraryKeys.activeProfile(),
    queryFn: queryFn(api.getActiveModProfile),
  });
}

/**
 * Hook to fetch the currently active profile.
 */
export function useActiveProfile() {
  return useQuery(activeProfileQueryOptions());
}

/**
 * Query options for fetching skin remaps for a profile.
 */
export function skinRemapsQueryOptions(profileId?: string | null) {
  return queryOptions<SkinRemap[], AppError>({
    queryKey: libraryKeys.skinRemaps(profileId),
    queryFn: queryFn(() => api.getSkinRemaps(profileId)),
  });
}

/**
 * Hook to fetch skin remaps for a profile. Defaults to the active profile.
 */
export function useSkinRemaps(profileId?: string | null) {
  return useQuery(skinRemapsQueryOptions(profileId));
}

// ============================================================================
// Mods
// ============================================================================

/**
 * Query options for fetching all installed mods.
 */
export function installedModsQueryOptions() {
  return queryOptions<InstalledMod[], AppError>({
    queryKey: libraryKeys.mods(),
    queryFn: queryFn(api.getInstalledMods),
  });
}

/**
 * Hook to fetch all installed mods.
 */
export function useInstalledMods() {
  return useQuery(installedModsQueryOptions());
}

// ============================================================================
// Folders
// ============================================================================

export function foldersQueryOptions() {
  return queryOptions<LibraryFolder[], AppError>({
    queryKey: libraryKeys.folders(),
    queryFn: queryFn(api.getFolders),
  });
}

export function useFolders() {
  return useQuery(foldersQueryOptions());
}

export function folderOrderQueryOptions() {
  return queryOptions<string[], AppError>({
    queryKey: libraryKeys.folderOrder(),
    queryFn: queryFn(api.getFolderOrder),
  });
}

export function useFolderOrder() {
  return useQuery(folderOrderQueryOptions());
}
