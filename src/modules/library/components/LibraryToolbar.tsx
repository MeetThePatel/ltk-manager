import { Grid3X3, List, Plus, Search } from "lucide-react";

import { Button, IconButton, Kbd, Tooltip } from "@/components";
import type { FilterOptions } from "@/modules/library/api";
import type { useLibraryActions } from "@/modules/library/api";
import { useLibraryViewMode } from "@/modules/library/api";

import { ActiveFilterChips } from "./ActiveFilterChips";
import { FilterPopover } from "./FilterPopover";
import { SortDropdown } from "./SortDropdown";

interface LibraryToolbarProps {
  searchQuery: string;
  onSearchChange: (query: string) => void;
  actions: ReturnType<typeof useLibraryActions>;
  isPatcherActive: boolean;
  filterOptions: FilterOptions;
}

export function LibraryToolbar({
  searchQuery,
  onSearchChange,
  actions,
  isPatcherActive,
  filterOptions,
}: LibraryToolbarProps) {
  const { viewMode, setViewMode } = useLibraryViewMode();

  return (
    <div className="border-b border-surface-600 bg-surface-800/50 px-4 py-3" data-tauri-drag-region>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-3">
        {/* Search */}
        <div className="relative min-w-[180px] flex-1">
          <Search className="absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-surface-500" />
          <input
            type="text"
            placeholder="Search mods..."
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            className="w-full rounded-lg border border-surface-600 bg-surface-800 py-2 pr-4 pl-10 text-surface-100 transition-colors duration-150 placeholder:text-surface-500 focus-visible:border-accent-500 focus-visible:ring-2 focus-visible:ring-accent-500 focus-visible:ring-offset-0 focus-visible:outline-none"
          />
        </div>

        <FilterPopover filterOptions={filterOptions} />

        <SortDropdown />

        {/* View toggle */}
        <div className="flex items-center gap-1">
          <Tooltip content="Grid view">
            <IconButton
              icon={<Grid3X3 className="h-4 w-4" />}
              variant={viewMode === "grid" ? "default" : "ghost"}
              size="sm"
              onClick={() => setViewMode("grid")}
            />
          </Tooltip>
          <Tooltip content="List view">
            <IconButton
              icon={<List className="h-4 w-4" />}
              variant={viewMode === "list" ? "default" : "ghost"}
              size="sm"
              onClick={() => setViewMode("list")}
            />
          </Tooltip>
        </div>

        {/* Actions */}
        <Tooltip
          content={
            <>
              Add mod <Kbd shortcut="Ctrl+I" />
            </>
          }
        >
          <Button
            variant="filled"
            size="sm"
            onClick={actions.handleInstallMod}
            loading={actions.installMod.isPending || actions.bulkInstallMods.isPending}
            disabled={isPatcherActive}
            left={<Plus className="h-4 w-4" />}
          >
            {actions.installMod.isPending || actions.bulkInstallMods.isPending
              ? "Installing..."
              : "Add Mod"}
          </Button>
        </Tooltip>
      </div>
      <ActiveFilterChips />
    </div>
  );
}
