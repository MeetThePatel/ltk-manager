import { type QueryClient, useMutation, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type Profile, type SkinRemap } from "@/lib/tauri";
import { unwrapForQuery } from "@/utils/query";

import { libraryKeys } from "./keys";

interface SetSkinRemapVariables {
  profileId?: string | null;
  remap: SkinRemap;
}

interface RemoveSkinRemapVariables {
  profileId?: string | null;
  championId: string;
}

function invalidateProfileRemaps(queryClient: QueryClient, profile: Profile) {
  queryClient.invalidateQueries({ queryKey: libraryKeys.profiles() });
  queryClient.invalidateQueries({ queryKey: libraryKeys.activeProfile() });
  queryClient.invalidateQueries({ queryKey: libraryKeys.skinRemaps(profile.id) });
  queryClient.invalidateQueries({ queryKey: libraryKeys.skinRemaps(null) });
}

export function useSetSkinRemap() {
  const queryClient = useQueryClient();

  return useMutation<Profile, AppError, SetSkinRemapVariables>({
    mutationFn: async ({ profileId, remap }) => {
      const result = await api.setSkinRemap(remap, profileId);
      return unwrapForQuery(result);
    },
    onSuccess: (profile) => {
      invalidateProfileRemaps(queryClient, profile);
    },
  });
}

export function useRemoveSkinRemap() {
  const queryClient = useQueryClient();

  return useMutation<Profile, AppError, RemoveSkinRemapVariables>({
    mutationFn: async ({ profileId, championId }) => {
      const result = await api.removeSkinRemap(championId, profileId);
      return unwrapForQuery(result);
    },
    onSuccess: (profile) => {
      invalidateProfileRemaps(queryClient, profile);
    },
  });
}
