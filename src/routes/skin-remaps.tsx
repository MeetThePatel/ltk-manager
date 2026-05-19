import { createFileRoute } from "@tanstack/react-router";

import { SkinRemaps } from "@/pages/SkinRemaps";

export const Route = createFileRoute("/skin-remaps")({
  component: SkinRemaps,
});
