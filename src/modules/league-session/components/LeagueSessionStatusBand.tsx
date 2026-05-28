import { Activity, Gamepad2, ShieldAlert, Sparkles, User, Zap } from "lucide-react";

import { useLeagueSessionState } from "../api/useLeagueSessionState";

export function LeagueSessionStatusBand() {
  const { data: session } = useLeagueSessionState();

  if (!session) {
    return null;
  }

  const { enabled, clientAvailable, observedChampion, actualizedChampion, lifecycle, lastError } =
    session;

  if (!enabled) {
    return (
      <div className="border-b border-surface-700 bg-surface-800/25 px-8 py-2.5 transition-all duration-300">
        <div className="mx-auto flex max-w-4xl items-center gap-3 text-xs text-surface-400">
          <Zap className="h-3.5 w-3.5 text-surface-500" />
          <span>
            Session-managed patching is disabled. Patcher control is manual. Enable it in settings
            to auto-manage skin remaps.
          </span>
        </div>
      </div>
    );
  }

  if (!clientAvailable) {
    return (
      <div className="border-b border-amber-950/20 bg-amber-950/5 px-8 py-3 transition-all duration-300">
        <div className="mx-auto flex max-w-4xl items-center justify-between gap-4">
          <div className="flex items-center gap-3 text-sm text-amber-300/90">
            <div className="relative flex h-2 w-2 shrink-0">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-amber-400 opacity-75"></span>
              <span className="relative inline-flex h-2 w-2 rounded-full bg-amber-500"></span>
            </div>
            <Gamepad2 className="h-4 w-4 text-amber-400" />
            <span>
              League Client is not running. LTK will automatically hook when League starts.
            </span>
          </div>
        </div>
      </div>
    );
  }

  // Client is available and session tracking is active!
  return (
    <div className="border-b border-surface-700 bg-surface-900/40 px-8 py-3 transition-all duration-300">
      <div className="mx-auto flex max-w-4xl flex-col gap-2.5 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-3 text-sm">
          {lifecycle === "Idle" && (
            <>
              <div className="h-2 w-2 shrink-0 rounded-full bg-emerald-500"></div>
              <Activity className="h-4 w-4 text-emerald-400" />
              <span className="text-surface-200">
                Connected to League client (
                <span className="font-medium text-emerald-400">Idle</span>)
              </span>
            </>
          )}

          {lifecycle === "ObservingChampSelect" && (
            <>
              <div className="relative flex h-2.5 w-2.5 shrink-0">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-violet-400 opacity-75"></span>
                <span className="relative inline-flex h-2.5 w-2.5 rounded-full bg-violet-500"></span>
              </div>
              <User className="h-4 w-4 text-violet-400" />
              <span className="text-violet-200">
                Champ Select:{" "}
                {observedChampion ? (
                  <>
                    Selected{" "}
                    <span className="font-semibold text-violet-300">
                      {observedChampion.championName}
                    </span>
                  </>
                ) : (
                  <span className="text-surface-400">Selecting champion...</span>
                )}
              </span>
            </>
          )}

          {lifecycle === "PendingActualization" && (
            <>
              <div className="relative flex h-2.5 w-2.5 shrink-0">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-sky-400 opacity-75"></span>
                <span className="relative inline-flex h-2.5 w-2.5 rounded-full bg-sky-500"></span>
              </div>
              <Sparkles className="h-4 w-4 text-sky-400" />
              <span className="animate-pulse font-medium text-sky-200">Pending Game Launch...</span>
            </>
          )}

          {lifecycle === "Actualizing" && (
            <>
              <div className="h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-accent-500 border-t-transparent"></div>
              <span className="animate-pulse font-medium text-accent-300">
                Building active skin remap overlay...
              </span>
            </>
          )}

          {lifecycle === "Patching" && (
            <>
              <div className="relative flex h-2.5 w-2.5 shrink-0">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75"></span>
                <span className="relative inline-flex h-2.5 w-2.5 rounded-full bg-emerald-500"></span>
              </div>
              <Gamepad2 className="h-4 w-4 text-emerald-400" />
              <span className="text-emerald-200">
                In Game:{" "}
                {actualizedChampion ? (
                  <>
                    Active Remap:{" "}
                    <span className="font-semibold text-emerald-300">
                      {actualizedChampion.championName}
                    </span>
                  </>
                ) : (
                  <span className="text-surface-400">No remap active (default champion skin)</span>
                )}
              </span>
            </>
          )}

          {lifecycle === "Clearing" && (
            <>
              <div className="h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-surface-500 border-t-transparent"></div>
              <span className="text-surface-400">Cleaning up game session overlay...</span>
            </>
          )}

          {lifecycle === "Error" && (
            <>
              <ShieldAlert className="h-4 w-4 shrink-0 text-rose-400" />
              <span className="text-rose-300">
                Patcher Error: {lastError ?? "Failed to actualize overlay"}
              </span>
            </>
          )}
        </div>

        {lifecycle === "ObservingChampSelect" && observedChampion && (
          <div className="flex items-center gap-1.5 rounded-full border border-violet-800/35 bg-violet-950/40 px-3 py-1 text-xs text-violet-300">
            <Sparkles className="h-3 w-3" />
            <span>Ready to remap {observedChampion.championName}</span>
          </div>
        )}

        {lifecycle === "Patching" && actualizedChampion && (
          <div className="flex items-center gap-1.5 rounded-full border border-emerald-800/35 bg-emerald-950/40 px-3 py-1 text-xs text-emerald-300">
            <Zap className="h-3 w-3" />
            <span>Remap Actualized</span>
          </div>
        )}
      </div>
    </div>
  );
}
