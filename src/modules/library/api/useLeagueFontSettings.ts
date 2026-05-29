import {
  type QueryClient,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import {
  api,
  type AppError,
  type LeagueFontSettings,
  type Profile,
  type SystemFont,
} from "@/lib/tauri";
import { queryFn, unwrapForQuery } from "@/utils/query";

import { libraryKeys } from "./keys";

interface SetLeagueFontSettingsVariables {
  profileId?: string | null;
  fontSettings: LeagueFontSettings;
}

function invalidateProfileFontSettings(queryClient: QueryClient, profile: Profile) {
  queryClient.invalidateQueries({ queryKey: libraryKeys.profiles() });
  queryClient.invalidateQueries({ queryKey: libraryKeys.activeProfile() });
  queryClient.invalidateQueries({ queryKey: libraryKeys.leagueFontSettings(profile.id) });
  queryClient.invalidateQueries({ queryKey: libraryKeys.leagueFontSettings(null) });
}

export function leagueFontSettingsQueryOptions(profileId?: string | null) {
  return queryOptions<LeagueFontSettings, AppError>({
    queryKey: libraryKeys.leagueFontSettings(profileId),
    queryFn: queryFn(() => api.getLeagueFontSettings(profileId)),
  });
}

export function useLeagueFontSettings(profileId?: string | null) {
  return useQuery(leagueFontSettingsQueryOptions(profileId));
}

export function useSetLeagueFontSettings() {
  const queryClient = useQueryClient();

  return useMutation<Profile, AppError, SetLeagueFontSettingsVariables>({
    mutationFn: async ({ profileId, fontSettings }) => {
      const result = await api.setLeagueFontSettings(fontSettings, profileId);
      return unwrapForQuery(result);
    },
    onSuccess: (profile) => {
      invalidateProfileFontSettings(queryClient, profile);
    },
  });
}

export function systemFontsQueryOptions() {
  return queryOptions<SystemFont[], AppError>({
    queryKey: ["library", "systemFonts"] as const,
    queryFn: queryFn(api.listSystemFonts),
    staleTime: 5 * 60 * 1000,
  });
}

export function useSystemFonts() {
  return useQuery(systemFontsQueryOptions());
}
