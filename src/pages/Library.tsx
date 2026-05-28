import { useState } from "react";
import { useHotkeys } from "react-hotkeys-hook";

import { usePlatformSupport } from "@/hooks";
import {
  DragDropOverlay,
  ImportProgressDialog,
  LibraryContent,
  LibraryToolbar,
  useFilterOptions,
  useInstalledMods,
  useLibraryActions,
  useModFileDrop,
} from "@/modules/library";
import { MigrationBanner, MigrationWizardDialog } from "@/modules/migration";
import { PatcherUnsupported, StatusBar, usePatcherStatus } from "@/modules/patcher";
import { useSaveSettings, useSettings } from "@/modules/settings";

interface LibraryProps {
  folderId?: string;
}

export function Library({ folderId }: LibraryProps = {}) {
  const [searchQuery, setSearchQuery] = useState("");
  const [migrationOpen, setMigrationOpen] = useState(false);

  const { data: platform } = usePlatformSupport();
  const patcherAvailable = platform?.patcherAvailable ?? true;

  const { data: mods = [], isLoading, error } = useInstalledMods();
  const actions = useLibraryActions();
  const isDragOver = useModFileDrop(actions.handleBulkInstallFiles);

  const { data: settings } = useSettings();
  const saveSettings = useSaveSettings();

  const { data: patcherStatus } = usePatcherStatus();
  const isPatcherActive = patcherStatus?.running ?? false;

  const filterOptions = useFilterOptions(mods);

  useHotkeys("ctrl+i", () => actions.handleInstallMod(), {
    preventDefault: true,
    enabled: !isPatcherActive,
  });

  function handleDismissMigration() {
    if (!settings) return;
    saveSettings.mutate({ ...settings, migrationDismissed: true });
  }

  return (
    <div className="relative flex h-full flex-col">
      <DragDropOverlay visible={isDragOver} />
      {settings && !settings.migrationDismissed && (
        <MigrationBanner
          onImport={() => setMigrationOpen(true)}
          onDismiss={handleDismissMigration}
        />
      )}
      {!patcherAvailable && (
        <div className="px-4 pt-3">
          <PatcherUnsupported />
        </div>
      )}
      <LibraryToolbar
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        actions={actions}
        isPatcherActive={isPatcherActive}
        filterOptions={filterOptions}
      />
      <StatusBar />
      <LibraryContent
        mods={mods}
        searchQuery={searchQuery}
        isLoading={isLoading}
        error={error}
        folderId={folderId}
      />
      <ImportProgressDialog
        open={actions.importDialogOpen}
        onClose={actions.handleCloseImportDialog}
        progress={actions.installProgress}
        result={actions.importResult}
      />
      <MigrationWizardDialog open={migrationOpen} onClose={() => setMigrationOpen(false)} />
    </div>
  );
}
