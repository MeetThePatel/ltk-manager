import { RotateCcw, Search } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";

import { Button, Select, Spinner, useToast } from "@/components";
import type { GameChampion, SkinRemap } from "@/lib/tauri";
import { LeagueSessionStatusBand } from "@/modules/league-session";
import {
  useActiveProfile,
  useRemoveSkinRemap,
  useSetSkinRemap,
  useSkinRemaps,
} from "@/modules/library";
import { useGameChampions } from "@/modules/settings";

export function SkinRemaps() {
  const toast = useToast();
  const [searchQuery, setSearchQuery] = useState("");

  const { data: activeProfile } = useActiveProfile();
  const { data: champions = [], isLoading, error } = useGameChampions();
  const { data: remaps = [] } = useSkinRemaps(activeProfile?.id);
  const setSkinRemap = useSetSkinRemap();
  const removeSkinRemap = useRemoveSkinRemap();

  const remapByChampion = useMemo(() => {
    return new Map(remaps.map((remap) => [remap.championId, remap]));
  }, [remaps]);

  const filteredChampions = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    if (!query) return champions;
    return champions.filter((champion) => champion.championName.toLowerCase().includes(query));
  }, [champions, searchQuery]);

  function saveSkinRemap(
    champion: GameChampion,
    skinNumber: number,
    chromaId: number | null = null,
  ) {
    const skin = champion.skins.find((candidate) => candidate.skinNumber === skinNumber);
    if (!skin || !activeProfile) return;
    const chroma =
      chromaId == null
        ? null
        : (skin.chromas.find((candidate) => candidate.chromaId === chromaId) ?? null);

    const remap: SkinRemap = {
      championId: champion.championId,
      championName: champion.championName,
      targetSkinNumber: skin.skinNumber,
      targetSkinName: skin.skinName,
      targetChromaId: chroma?.chromaId ?? null,
      targetChromaName: chroma?.chromaName ?? null,
    };

    setSkinRemap.mutate(
      { profileId: activeProfile.id, remap },
      {
        onError: (error) => {
          toast.error("Failed to save remap", error.message);
        },
      },
    );
  }

  function handleTargetSkinChange(champion: GameChampion, value: string | null) {
    if (value === "default") {
      handleRemoveRemap(champion);
      return;
    }

    const skinNumber = Number(value);
    saveSkinRemap(champion, skinNumber);
  }

  function handleTargetChromaChange(
    champion: GameChampion,
    remap: SkinRemap | undefined,
    value: string | null,
  ) {
    if (!remap) return;
    const chromaId = value === "none" || value == null ? null : Number(value);
    saveSkinRemap(champion, remap.targetSkinNumber, chromaId);
  }

  function handleRemoveRemap(champion: GameChampion) {
    if (!activeProfile) return;

    removeSkinRemap.mutate(
      { profileId: activeProfile.id, championId: champion.championId },
      {
        onError: (error) => {
          toast.error("Failed to remove remap", error.message);
        },
      },
    );
  }

  async function handleResetAll() {
    if (!activeProfile || remaps.length === 0) return;

    const results = await Promise.allSettled(
      remaps.map((remap) =>
        removeSkinRemap.mutateAsync({
          profileId: activeProfile.id,
          championId: remap.championId,
        }),
      ),
    );

    if (results.some((result) => result.status === "rejected")) {
      toast.error("Failed to reset all remaps");
    }
  }

  return (
    <div className="flex h-full flex-col">
      <LeagueSessionStatusBand />
      <div className="border-b border-surface-600 bg-surface-800/50 px-8 py-3 lg:px-10">
        <div className="mx-auto flex max-w-4xl items-center gap-3">
          <div className="relative min-w-0 flex-1">
            <Search className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-surface-500" />
            <input
              type="text"
              placeholder="Search champions..."
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              className="w-full rounded-lg border border-surface-600 bg-surface-800 py-2 pr-4 pl-10 text-surface-100 transition-colors duration-150 placeholder:text-surface-500 focus-visible:border-accent-500 focus-visible:ring-2 focus-visible:ring-accent-500 focus-visible:ring-offset-0 focus-visible:outline-none"
            />
          </div>
          <Button
            variant="outline"
            size="sm"
            left={<RotateCcw className="h-4 w-4" />}
            loading={removeSkinRemap.isPending}
            disabled={!activeProfile || remaps.length === 0}
            onClick={handleResetAll}
            className="shrink-0"
          >
            Reset all
          </Button>
        </div>
      </div>

      {isLoading ? (
        <CenteredState>
          <Spinner size="lg" />
        </CenteredState>
      ) : error ? (
        <CenteredState>
          <p className="text-sm text-red-300">{error.message}</p>
        </CenteredState>
      ) : champions.length === 0 ? (
        <CenteredState>
          <p className="text-sm text-surface-400">No champion data found</p>
        </CenteredState>
      ) : (
        <ChampionGrid
          champions={filteredChampions}
          remapByChampion={remapByChampion}
          disabled={!activeProfile}
          saving={setSkinRemap.isPending}
          removing={removeSkinRemap.isPending}
          onTargetSkinChange={handleTargetSkinChange}
          onTargetChromaChange={handleTargetChromaChange}
          onReset={handleRemoveRemap}
        />
      )}
    </div>
  );
}

function CenteredState({ children }: { children?: ReactNode }) {
  return <div className="flex flex-1 items-center justify-center p-6">{children}</div>;
}

interface ChampionGridProps {
  champions: GameChampion[];
  remapByChampion: Map<string, SkinRemap>;
  disabled: boolean;
  saving: boolean;
  removing: boolean;
  onTargetSkinChange: (champion: GameChampion, value: string | null) => void;
  onTargetChromaChange: (
    champion: GameChampion,
    remap: SkinRemap | undefined,
    value: string | null,
  ) => void;
  onReset: (champion: GameChampion) => void;
}

function ChampionGrid({
  champions,
  remapByChampion,
  disabled,
  saving,
  removing,
  onTargetSkinChange,
  onTargetChromaChange,
  onReset,
}: ChampionGridProps) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-8 py-4 lg:px-10">
      <div className="grid grid-cols-1 gap-x-10 gap-y-2.5 lg:grid-cols-2">
        {champions.map((champion) => {
          const remap = remapByChampion.get(champion.championId);
          const selectedValue = remap?.targetSkinNumber.toString() ?? "default";
          const skins = champion.skins.filter((skin) => skin.skinNumber !== 0);
          const selectedSkin = skins.find((skin) => skin.skinNumber.toString() === selectedValue);
          const chromas = selectedSkin?.chromas ?? [];
          const selectedChromaValue = remap?.targetChromaId?.toString() ?? "none";
          const selectedLabel = remap?.targetSkinName ?? selectedSkin?.skinName ?? "Default";
          const selectedChromaLabel =
            chromas.length === 0
              ? "No chromas"
              : (remap?.targetChromaName ??
                chromas.find((chroma) => chroma.chromaId.toString() === selectedChromaValue)
                  ?.chromaName ??
                "None");
          const modified = !!remap;

          return (
            <div
              key={champion.championId}
              className="grid min-w-0 grid-cols-[minmax(7rem,9rem)_minmax(0,11rem)_minmax(0,10rem)_1.75rem] items-center gap-2.5"
            >
              <div className="flex min-w-0 items-center gap-2">
                <span
                  className="truncate text-sm font-medium text-surface-100"
                  title={champion.championName}
                >
                  {champion.championName}
                </span>
                {modified && (
                  <span
                    className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent-400"
                    title="Modified"
                    aria-label="Modified"
                  />
                )}
              </div>
              <Select.Root
                value={selectedValue}
                onValueChange={(value) => onTargetSkinChange(champion, value)}
              >
                <Select.Trigger
                  disabled={disabled || skins.length === 0 || saving || removing}
                  className="h-8 rounded-md px-2.5 py-1 text-xs"
                >
                  <Select.Value className="min-w-0 truncate" placeholder="Select skin">
                    {() => selectedLabel}
                  </Select.Value>
                  <Select.Icon />
                </Select.Trigger>
                <Select.Portal>
                  <Select.Positioner>
                    <Select.Popup className="w-[var(--anchor-width)]">
                      <Select.Item value="default" className="px-2 py-1 text-xs">
                        Default
                      </Select.Item>
                      {skins.map((skin) => (
                        <Select.Item
                          key={skin.skinNumber}
                          value={skin.skinNumber.toString()}
                          className="px-2 py-1 text-xs"
                        >
                          {skin.skinName}
                        </Select.Item>
                      ))}
                    </Select.Popup>
                  </Select.Positioner>
                </Select.Portal>
              </Select.Root>
              <Select.Root
                value={selectedChromaValue}
                onValueChange={(value) => onTargetChromaChange(champion, remap, value)}
              >
                <Select.Trigger
                  disabled={disabled || !remap || chromas.length === 0 || saving || removing}
                  className="h-8 rounded-md px-2.5 py-1 text-xs"
                >
                  <Select.Value className="min-w-0 truncate" placeholder="Chroma">
                    {() => selectedChromaLabel}
                  </Select.Value>
                  <Select.Icon />
                </Select.Trigger>
                <Select.Portal>
                  <Select.Positioner>
                    <Select.Popup className="w-[var(--anchor-width)]">
                      <Select.Item value="none" className="px-2 py-1 text-xs">
                        None
                      </Select.Item>
                      {chromas.map((chroma) => (
                        <Select.Item
                          key={chroma.chromaId}
                          value={chroma.chromaId.toString()}
                          className="px-2 py-1 text-xs"
                        >
                          {chroma.chromaName}
                        </Select.Item>
                      ))}
                    </Select.Popup>
                  </Select.Positioner>
                </Select.Portal>
              </Select.Root>
              <button
                type="button"
                className="inline-flex h-7 w-7 items-center justify-center rounded-md text-surface-500 transition-colors hover:bg-surface-700 hover:text-surface-100 disabled:pointer-events-none disabled:opacity-25"
                title="Reset"
                aria-label={`Reset ${champion.championName}`}
                disabled={disabled || !modified || removing}
                onClick={() => onReset(champion)}
              >
                <RotateCcw className="h-3.5 w-3.5" />
              </button>
            </div>
          );
        })}
      </div>
      {champions.length === 0 && (
        <div className="flex h-full min-h-48 items-center justify-center">
          <p className="text-sm text-surface-400">No champions match your search</p>
        </div>
      )}
    </div>
  );
}
