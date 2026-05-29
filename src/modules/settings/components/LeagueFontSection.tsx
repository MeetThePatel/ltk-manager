import { Loader2, Type } from "lucide-react";
import { useMemo, useState } from "react";

import { AlertBox, Button, SectionCard, SelectField, Switch, useToast } from "@/components";
import {
  useActiveProfile,
  useAllModWadReports,
  useInstalledMods,
  useLeagueFontSettings,
  useSetLeagueFontSettings,
  useSystemFonts,
} from "@/modules/library/api";
import { usePatcherStatus } from "@/modules/patcher/api/usePatcherStatus";

function getWeightName(weight: number): string {
  if (weight <= 150) return "Thin";
  if (weight <= 250) return "Extra Light";
  if (weight <= 350) return "Light";
  if (weight <= 450) return "Regular";
  if (weight <= 550) return "Medium";
  if (weight <= 650) return "Semi Bold";
  if (weight <= 750) return "Bold";
  if (weight <= 850) return "Extra Bold";
  return "Black";
}

export function LeagueFontSection() {
  const toast = useToast();

  // Queries & Mutations
  const { data: patcherStatus } = usePatcherStatus();
  const { data: activeProfile } = useActiveProfile();
  const { data: fontSettings, isLoading: isLoadingSettings } = useLeagueFontSettings(
    activeProfile?.id ?? null,
  );
  const { data: systemFonts, isLoading: isLoadingFonts } = useSystemFonts();
  const { mutateAsync: setFontSettings, isPending: isSaving } = useSetLeagueFontSettings();

  // Conflict Queries
  const { data: installedMods } = useInstalledMods();
  const { data: wadReports } = useAllModWadReports();

  const isPatcherRunning = !!patcherStatus?.running;

  // Find a good platform default valid font (prioritizing Normal / Regular styles)
  const defaultFont = useMemo(() => {
    if (!systemFonts) return null;
    const isMac = typeof window !== "undefined" && navigator.userAgent.includes("Mac");
    const priorityList = isMac
      ? ["SF Pro", "Helvetica Neue", "Helvetica", "Arial"]
      : ["Segoe UI", "Calibri", "Arial", "Microsoft YaHei", "Malgun Gothic"];

    // Find the prioritized font matching standard style first
    for (const name of priorityList) {
      const fontsWithFamily = systemFonts.filter(
        (f) => f.family.toLowerCase() === name.toLowerCase() && f.isValid,
      );
      if (fontsWithFamily.length > 0) {
        const normalFont = fontsWithFamily.find((f) => {
          const styleLower = f.style.toLowerCase();
          return (
            styleLower === "normal" ||
            styleLower === "regular" ||
            styleLower === "plain" ||
            styleLower === "book"
          );
        });
        if (normalFont) return normalFont;
        return fontsWithFamily[0];
      }
    }

    // Generic fallback
    const validFonts = systemFonts.filter((f) => f.isValid);
    if (validFonts.length > 0) {
      const normalFont = validFonts.find((f) => {
        const styleLower = f.style.toLowerCase();
        return (
          styleLower === "normal" ||
          styleLower === "regular" ||
          styleLower === "plain" ||
          styleLower === "book"
        );
      });
      return normalFont || validFonts[0];
    }

    return null;
  }, [systemFonts]);

  const [selectedFamily, setSelectedFamily] = useState<string>(
    () => fontSettings?.singleFont?.family ?? defaultFont?.family ?? "",
  );
  const [selectedSlant, setSelectedSlant] = useState<string>(
    () => fontSettings?.singleFont?.style ?? defaultFont?.style ?? "",
  );

  // Retrieve current font details for the preview line/validation badge
  const selectedFontDetails = useMemo(() => {
    if (!fontSettings?.singleFont) return null;

    // Find matching font in discovered system fonts
    return (
      systemFonts?.find(
        (f) =>
          f.path === fontSettings.singleFont?.path &&
          f.faceIndex === fontSettings.singleFont?.faceIndex,
      ) || null
    );
  }, [fontSettings, systemFonts]);

  // Sync selectedFamily and selectedSlant with saved font settings, falling back to defaultFont if none is saved
  const [prevFontSettings, setPrevFontSettings] = useState(fontSettings);
  const [prevDefaultFont, setPrevDefaultFont] = useState(defaultFont);

  if (fontSettings !== prevFontSettings || defaultFont !== prevDefaultFont) {
    setPrevFontSettings(fontSettings);
    setPrevDefaultFont(defaultFont);
    if (fontSettings?.singleFont?.family) {
      setSelectedFamily(fontSettings.singleFont.family);
      setSelectedSlant(fontSettings.singleFont.style);
    } else if (defaultFont) {
      setSelectedFamily(defaultFont.family);
      setSelectedSlant(defaultFont.style);
    } else {
      setSelectedFamily("");
      setSelectedSlant("");
    }
  }

  // Unique valid font families
  const familyOptions = useMemo(() => {
    if (!systemFonts) return [];
    const families = new Set<string>();
    for (const font of systemFonts) {
      if (font.isValid) {
        families.add(font.family);
      }
    }
    return Array.from(families)
      .sort((a, b) => a.localeCompare(b))
      .map((family) => ({
        label: family,
        value: family,
      }));
  }, [systemFonts]);

  // All system fonts matching the selected family
  const fontsForFamily = useMemo(() => {
    if (!systemFonts || !selectedFamily) return [];
    return systemFonts.filter((f) => f.family === selectedFamily);
  }, [systemFonts, selectedFamily]);

  // Unique styles/slants for the selected family
  const slantOptions = useMemo(() => {
    const slants = new Set<string>();
    for (const font of fontsForFamily) {
      if (font.isValid && font.style && font.style.trim() !== "") {
        slants.add(font.style.trim());
      }
    }
    return Array.from(slants)
      .sort((a, b) => {
        const aLower = a.toLowerCase();
        const bLower = b.toLowerCase();
        const aIsRegular = aLower === "normal" || aLower === "regular" || aLower === "plain";
        const bIsRegular = bLower === "normal" || bLower === "regular" || bLower === "plain";
        if (aIsRegular && !bIsRegular) return -1;
        if (!aIsRegular && bIsRegular) return 1;
        return a.localeCompare(b);
      })
      .map((slant) => ({
        label: slant === "Normal" ? "Regular" : slant,
        value: slant,
      }));
  }, [fontsForFamily]);

  // Weights available for family + slant
  const weightOptions = useMemo(() => {
    if (!selectedSlant) return [];
    const matchingFonts = fontsForFamily.filter(
      (font) => font.style === selectedSlant && font.isValid,
    );

    // Deduplicate by weight
    const uniqueWeights = new Map<number, (typeof matchingFonts)[0]>();
    for (const font of matchingFonts) {
      if (!uniqueWeights.has(font.weight)) {
        uniqueWeights.set(font.weight, font);
      }
    }

    return Array.from(uniqueWeights.values())
      .sort((a, b) => {
        // Place weight 400 (Regular) first
        if (a.weight === 400 && b.weight !== 400) return -1;
        if (a.weight !== 400 && b.weight === 400) return 1;
        return a.weight - b.weight;
      })
      .map((font) => {
        const issuesCount = font.issues?.filter((i) => i.severity === "error").length ?? 0;
        const weightName = getWeightName(font.weight);
        const label = `${weightName} ${font.isValid ? "" : `[Invalid - ${issuesCount} Errors]`}`;

        const val = JSON.stringify({
          family: font.family,
          fullName: font.fullName,
          style: font.style,
          weight: font.weight,
          path: font.path,
          faceIndex: font.faceIndex,
        });

        return {
          label,
          value: val,
          disabled: !font.isValid,
        };
      });
  }, [fontsForFamily, selectedSlant]);

  // Selected value string for combobox
  const selectedValueString = useMemo(() => {
    const font = fontSettings?.singleFont || defaultFont;
    if (!font) return "";

    return JSON.stringify({
      family: font.family,
      fullName: font.fullName,
      style: font.style,
      weight: font.weight,
      path: font.path,
      faceIndex: font.faceIndex,
    });
  }, [fontSettings, defaultFont]);

  // Conflicting mods check
  const conflictingMods = useMemo(() => {
    if (!fontSettings?.enabled || !installedMods || !wadReports) return [];
    return installedMods.filter((mod) => {
      if (!mod.enabled) return false;

      const nameLower = (mod.displayName || mod.name || "").toLowerCase();
      const hasFontKeywords =
        nameLower.includes("font") || nameLower.includes("korean") || nameLower.includes("uttum");

      const hasFontTags = mod.tags?.some(
        (tag) => tag.toLowerCase().includes("font") || tag.toLowerCase().includes("ui"),
      );

      const report = wadReports[mod.id];
      const affectsFontWads = report?.affectedWads?.some(
        (wad) => wad.toLowerCase().includes("bootstrap") || wad.toLowerCase().includes("ui"),
      );

      return hasFontKeywords || (hasFontTags && affectsFontWads) || affectsFontWads;
    });
  }, [fontSettings, installedMods, wadReports]);

  // Save changes wrapper with patcher block error handling
  async function handleToggle(enabled: boolean) {
    if (isPatcherRunning) {
      toast.error("Blocked", "Cannot modify font settings while patcher is running");
      return;
    }

    const currentFont = fontSettings?.singleFont || null;
    if (enabled && !currentFont) {
      if (defaultFont) {
        const defaultSelection = {
          family: defaultFont.family,
          fullName: defaultFont.fullName,
          style: defaultFont.style,
          weight: defaultFont.weight,
          path: defaultFont.path,
          faceIndex: defaultFont.faceIndex,
        };
        try {
          await setFontSettings({
            profileId: activeProfile?.id ?? null,
            fontSettings: { enabled: true, singleFont: defaultSelection },
          });
          toast.success("Font Overrides Enabled", `Defaulted to ${defaultFont.family}`);
        } catch (err) {
          const errMsg = err instanceof Error ? err.message : String(err);
          toast.error("Failed to enable font overrides", errMsg);
        }
      } else {
        toast.error("No valid fonts found", "Please install a valid TTF/OTF font first.");
      }
      return;
    }

    try {
      await setFontSettings({
        profileId: activeProfile?.id ?? null,
        fontSettings: {
          enabled,
          singleFont: currentFont,
        },
      });
      toast.success(
        enabled ? "Font Overrides Enabled" : "Font Overrides Disabled",
        enabled ? "League will now load your custom font." : "League will load default fonts.",
      );
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      toast.error("Failed to update font settings", errMsg);
    }
  }

  async function handleFamilyChange(family: string | null) {
    if (isPatcherRunning) {
      toast.error("Blocked", "Cannot modify font settings while patcher is running");
      return;
    }

    if (!family) {
      setSelectedFamily("");
      setSelectedSlant("");
      return;
    }

    setSelectedFamily(family);

    if (!systemFonts) return;
    const familyFonts = systemFonts.filter((f) => f.family === family && f.isValid);
    if (familyFonts.length === 0) return;

    // Pick best slant (Normal/Regular preferred)
    const slants = familyFonts.map((f) => f.style);
    const bestSlant =
      slants.find((s) => {
        const sLower = s.toLowerCase();
        return sLower === "normal" || sLower === "regular" || sLower === "plain";
      }) || slants[0];

    setSelectedSlant(bestSlant);

    // Pick best weight (400 preferred)
    const fontsWithSlant = familyFonts.filter((f) => f.style === bestSlant);
    const bestFont = fontsWithSlant.find((f) => f.weight === 400) || fontsWithSlant[0];

    const selection = {
      family: bestFont.family,
      fullName: bestFont.fullName,
      style: bestFont.style,
      weight: bestFont.weight,
      path: bestFont.path,
      faceIndex: bestFont.faceIndex,
    };

    try {
      await setFontSettings({
        profileId: activeProfile?.id ?? null,
        fontSettings: {
          enabled: fontSettings?.enabled ?? false,
          singleFont: selection,
        },
      });
      toast.success("Font family set", `Selected ${selection.family} ${selection.style}`);
    } catch (err) {
      if (fontSettings?.singleFont?.family) {
        setSelectedFamily(fontSettings.singleFont.family);
        setSelectedSlant(fontSettings.singleFont.style);
      } else if (defaultFont) {
        setSelectedFamily(defaultFont.family);
        setSelectedSlant(defaultFont.style);
      } else {
        setSelectedFamily("");
        setSelectedSlant("");
      }
      const errMsg = err instanceof Error ? err.message : String(err);
      toast.error("Failed to set font family", errMsg);
    }
  }

  async function handleSlantChange(slant: string | null) {
    if (isPatcherRunning) {
      toast.error("Blocked", "Cannot modify font settings while patcher is running");
      return;
    }

    if (!slant || !selectedFamily) return;

    setSelectedSlant(slant);

    if (!systemFonts) return;
    const matchingFonts = systemFonts.filter(
      (f) => f.family === selectedFamily && f.style === slant && f.isValid,
    );
    if (matchingFonts.length === 0) return;

    const bestFont = matchingFonts.find((f) => f.weight === 400) || matchingFonts[0];

    const selection = {
      family: bestFont.family,
      fullName: bestFont.fullName,
      style: bestFont.style,
      weight: bestFont.weight,
      path: bestFont.path,
      faceIndex: bestFont.faceIndex,
    };

    try {
      await setFontSettings({
        profileId: activeProfile?.id ?? null,
        fontSettings: {
          enabled: fontSettings?.enabled ?? false,
          singleFont: selection,
        },
      });
      toast.success("Font style set", `Selected ${selection.family} ${selection.style}`);
    } catch (err) {
      if (fontSettings?.singleFont?.family) {
        setSelectedFamily(fontSettings.singleFont.family);
        setSelectedSlant(fontSettings.singleFont.style);
      } else if (defaultFont) {
        setSelectedFamily(defaultFont.family);
        setSelectedSlant(defaultFont.style);
      } else {
        setSelectedFamily("");
        setSelectedSlant("");
      }
      const errMsg = err instanceof Error ? err.message : String(err);
      toast.error("Failed to set style slant", errMsg);
    }
  }

  async function handleFontChange(valueStr: string | null) {
    if (isPatcherRunning) {
      toast.error("Blocked", "Cannot modify font settings while patcher is running");
      return;
    }

    if (!valueStr) return;

    try {
      const selection = JSON.parse(valueStr);
      await setFontSettings({
        profileId: activeProfile?.id ?? null,
        fontSettings: {
          enabled: fontSettings?.enabled ?? false,
          singleFont: selection,
        },
      });
      toast.success("Font updated", `League font set to ${selection.family} ${selection.style}`);
    } catch (err) {
      if (fontSettings?.singleFont?.family) {
        setSelectedFamily(fontSettings.singleFont.family);
        setSelectedSlant(fontSettings.singleFont.style);
      }
      const errMsg = err instanceof Error ? err.message : String(err);
      toast.error("Failed to set font", errMsg);
    }
  }

  async function handleReset() {
    if (isPatcherRunning) {
      toast.error("Blocked", "Cannot reset font settings while patcher is running");
      return;
    }

    try {
      await setFontSettings({
        profileId: activeProfile?.id ?? null,
        fontSettings: {
          enabled: false,
          singleFont: null,
        },
      });
      toast.success("Font settings reset");
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      toast.error("Failed to reset", errMsg);
    }
  }

  if (isLoadingSettings || isLoadingFonts) {
    return (
      <SectionCard title="Generated League Font" icon={<Type className="h-5 w-5" />}>
        <div className="flex items-center justify-center py-6">
          <Loader2 className="h-6 w-6 animate-spin text-accent-500" />
          <span className="ml-2 text-sm text-surface-400">Loading system fonts...</span>
        </div>
      </SectionCard>
    );
  }

  return (
    <SectionCard title="Generated League Font" icon={<Type className="h-5 w-5" />}>
      <div className="space-y-4">
        <p className="text-sm text-surface-400">
          Apply a selected local font directly to League of Legends by dynamically generating WAD
          overrides at patch time. This eliminates the need for separate installed font archives.
        </p>

        {isPatcherRunning && (
          <AlertBox variant="warning" title="Patcher is Running">
            Font settings are locked while the patcher is running. Stop the patcher to modify or
            enable overrides.
          </AlertBox>
        )}

        <div className="flex items-center justify-between rounded-lg bg-surface-800 p-4">
          <div className="space-y-1">
            <span className="block text-sm font-semibold text-surface-100">
              Enable Font Overrides
            </span>
            <span className="block text-xs text-surface-400">
              Replace standard League text fonts with your custom selected font.
            </span>
          </div>
          <Switch
            checked={fontSettings?.enabled ?? false}
            onCheckedChange={(checked) => handleToggle(checked)}
            disabled={isPatcherRunning || isSaving}
          />
        </div>

        {fontSettings?.enabled && (
          <div className="space-y-4 rounded-lg border border-surface-700 bg-surface-900/50 p-4">
            <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
              {/* Font Family Dropdown */}
              <SelectField
                label="Font Family"
                description="Select a font family."
                placeholder="Select font family..."
                options={familyOptions}
                value={selectedFamily}
                onValueChange={handleFamilyChange}
                disabled={isPatcherRunning || isSaving}
              />

              {/* Style Dropdown */}
              <SelectField
                label="Style"
                description="Choose normal or italic style."
                placeholder={selectedFamily ? "Select style..." : "Choose family first..."}
                options={slantOptions}
                value={selectedSlant}
                onValueChange={handleSlantChange}
                disabled={isPatcherRunning || isSaving || !selectedFamily}
              />

              {/* Weight Dropdown */}
              <SelectField
                label="Weight"
                description="Choose the weight of the font."
                placeholder={selectedSlant ? "Select weight..." : "Choose style first..."}
                options={weightOptions}
                value={selectedValueString}
                onValueChange={handleFontChange}
                disabled={isPatcherRunning || isSaving || !selectedSlant}
              />
            </div>

            {/* Validation Reports / Issues */}
            {selectedFontDetails &&
              selectedFontDetails.issues &&
              selectedFontDetails.issues.length > 0 && (
                <AlertBox
                  variant={selectedFontDetails.isValid ? "warning" : "error"}
                  title={
                    selectedFontDetails.isValid
                      ? "Font Validation Warning"
                      : "Font Validation Error"
                  }
                >
                  <ul className="list-disc space-y-1 pl-4 text-xs font-medium">
                    {selectedFontDetails.issues.map((issue, idx) => (
                      <li key={idx}>
                        <strong>[{issue.severity.toUpperCase()}]:</strong> {issue.message}
                      </li>
                    ))}
                  </ul>
                </AlertBox>
              )}

            {/* Conflicting mods warning */}
            {conflictingMods.length > 0 && (
              <AlertBox variant="warning" title="Potential Font Mod Conflicts Detected">
                <div className="space-y-2">
                  <p className="text-xs">
                    The following enabled mods likely override {"League's"} fonts. Since generated
                    fonts are prioritized at the top of the overlay, they will win, but we highly
                    recommend disabling or uninstalling these mods to avoid confusion:
                  </p>
                  <ul className="list-disc space-y-0.5 pl-4 text-xs font-medium">
                    {conflictingMods.map((mod) => (
                      <li key={mod.id}>{mod.displayName || mod.name}</li>
                    ))}
                  </ul>
                </div>
              </AlertBox>
            )}

            {/* Reset button */}
            <div className="flex justify-end border-t border-surface-700/50 pt-2">
              <Button
                variant="transparent"
                size="sm"
                onClick={handleReset}
                disabled={isPatcherRunning || isSaving}
                className="font-medium text-red-400 hover:text-red-300"
              >
                Reset Settings
              </Button>
            </div>
          </div>
        )}
      </div>
    </SectionCard>
  );
}
