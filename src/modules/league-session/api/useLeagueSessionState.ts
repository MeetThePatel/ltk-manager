import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, type AppError, type LeagueSessionStateInner } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/useTauriEvent";
import { queryFn } from "@/utils/query";

import { leagueSessionKeys } from "./keys";

export function useLeagueSessionState() {
  const queryClient = useQueryClient();
  const queryKey = leagueSessionKeys.state();

  useTauriEvent<LeagueSessionStateInner>("league-session-changed", (payload) => {
    queryClient.setQueryData(queryKey, payload);
  });

  return useQuery<LeagueSessionStateInner, AppError>({
    queryKey,
    queryFn: queryFn(api.getLeagueSessionState),
    staleTime: Infinity,
  });
}
