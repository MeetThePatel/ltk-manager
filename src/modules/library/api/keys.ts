export const libraryKeys = {
  all: ["library"] as const,
  mods: () => [...libraryKeys.all, "mods"] as const,
  mod: (id: string) => [...libraryKeys.mods(), id] as const,
  thumbnail: (modId: string) => [...libraryKeys.mod(modId), "thumbnail"] as const,
  profiles: () => [...libraryKeys.all, "profiles"] as const,
  activeProfile: () => [...libraryKeys.profiles(), "active"] as const,
  skinRemaps: (profileId?: string | null) =>
    [...libraryKeys.profiles(), profileId ?? "active", "skinRemaps"] as const,
  leagueFontSettings: (profileId?: string | null) =>
    [...libraryKeys.profiles(), profileId ?? "active", "leagueFontSettings"] as const,
  folders: () => [...libraryKeys.all, "folders"] as const,
  folderOrder: () => [...libraryKeys.all, "folderOrder"] as const,
  wadReports: () => [...libraryKeys.all, "wadReport"] as const,
  wadReport: (modId: string) => [...libraryKeys.wadReports(), modId] as const,
};
