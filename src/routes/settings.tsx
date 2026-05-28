import { createFileRoute } from "@tanstack/react-router";

import { Settings } from "../pages/Settings";

interface SettingsSearch {
  firstRun?: boolean;
  tab?: string;
}

export const Route = createFileRoute("/settings")({
  validateSearch: (search: Record<string, unknown>): SettingsSearch => {
    return {
      firstRun: search.firstRun === true || search.firstRun === "true",
      tab: typeof search.tab === "string" ? search.tab : undefined,
    };
  },
  component: Settings,
});
