import { useQuery } from "@tanstack/react-query";

import { api, type AppError, type GameChampion } from "@/lib/tauri";
import { queryFn } from "@/utils/query";

import { settingsKeys } from "./keys";

export function useGameChampions() {
  return useQuery<GameChampion[], AppError>({
    queryKey: settingsKeys.gameChampions(),
    queryFn: queryFn(api.listGameChampions),
    staleTime: 60 * 60 * 1000,
    retry: false,
  });
}
