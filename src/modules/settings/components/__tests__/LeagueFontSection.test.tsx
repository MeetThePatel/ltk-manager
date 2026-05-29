/* eslint-disable @typescript-eslint/no-explicit-any */
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useToast } from "@/components";
import {
  useActiveProfile,
  useAllModWadReports,
  useInstalledMods,
  useLeagueFontSettings,
  useSetLeagueFontSettings,
  useSystemFonts,
} from "@/modules/library/api";
import { usePatcherStatus } from "@/modules/patcher/api/usePatcherStatus";

import { LeagueFontSection } from "../LeagueFontSection";

// Mock the modules/hooks
vi.mock("@/modules/library/api", () => ({
  useActiveProfile: vi.fn(),
  useLeagueFontSettings: vi.fn(),
  useSystemFonts: vi.fn(),
  useSetLeagueFontSettings: vi.fn(),
  useInstalledMods: vi.fn(),
  useAllModWadReports: vi.fn(),
}));

vi.mock("@/modules/patcher/api/usePatcherStatus", () => ({
  usePatcherStatus: vi.fn(),
}));

vi.mock("@/components", async (importOriginal) => {
  const original = await importOriginal<any>();
  return {
    ...original,
    useToast: vi.fn(),
  };
});

describe("LeagueFontSection", () => {
  const mockToast = {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
  };

  const mockMutateAsync = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useToast).mockReturnValue(mockToast as any);

    // Default mock hook returns
    vi.mocked(usePatcherStatus).mockReturnValue({
      data: { running: false, configPath: null, phase: "Idle" as any },
    } as any);

    vi.mocked(useActiveProfile).mockReturnValue({
      data: { id: "p1", name: "Default" },
    } as any);

    vi.mocked(useLeagueFontSettings).mockReturnValue({
      data: { enabled: false, singleFont: null },
      isLoading: false,
    } as any);

    vi.mocked(useSystemFonts).mockReturnValue({
      data: [
        {
          family: "Noto Sans",
          fullName: "Noto Sans Regular",
          style: "Regular",
          weight: 400,
          path: "/fonts/notosans.ttf",
          faceIndex: 0,
          isValid: true,
          issues: [],
        },
        {
          family: "Corrupted Font",
          fullName: "Corrupted Regular",
          style: "Regular",
          weight: 400,
          path: "/fonts/corrupt.ttf",
          faceIndex: 0,
          isValid: false,
          issues: [{ severity: "error" as any, message: "Corrupted bytes" }],
        },
      ],
      isLoading: false,
    } as any);

    vi.mocked(useSetLeagueFontSettings).mockReturnValue({
      mutateAsync: mockMutateAsync,
      isPending: false,
    } as any);

    vi.mocked(useInstalledMods).mockReturnValue({
      data: [],
    } as any);

    vi.mocked(useAllModWadReports).mockReturnValue({
      data: {},
    } as any);
  });

  it("renders the toggle switch in disabled (off) state by default", () => {
    render(<LeagueFontSection />);
    expect(screen.getByText("Enable Font Overrides")).toBeInTheDocument();

    const toggle = screen.getByRole("switch");
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("disables modification elements when the patcher is running", () => {
    vi.mocked(usePatcherStatus).mockReturnValue({
      data: { running: true, configPath: null, phase: "Patching" as any },
    } as any);

    render(<LeagueFontSection />);

    expect(
      screen.getByText(
        "Font settings are locked while the patcher is running. Stop the patcher to modify or enable overrides.",
      ),
    ).toBeInTheDocument();
    const toggle = screen.getByRole("switch");
    expect(toggle).toHaveAttribute("data-disabled");
  });

  it("attempts to save with a default valid font when enabling overrides without prior choice", async () => {
    render(<LeagueFontSection />);

    const toggle = screen.getByRole("switch");
    fireEvent.click(toggle);

    expect(mockMutateAsync).toHaveBeenCalledTimes(1);
    expect(mockMutateAsync.mock.calls[0][0]).toEqual({
      profileId: "p1",
      fontSettings: {
        enabled: true,
        singleFont: {
          family: "Noto Sans",
          fullName: "Noto Sans Regular",
          style: "Regular",
          weight: 400,
          path: "/fonts/notosans.ttf",
          faceIndex: 0,
        },
      },
    });
  });

  it("displays conflict warning when an active mod also overrides the same assets", () => {
    // Setup enabled overrides
    vi.mocked(useLeagueFontSettings).mockReturnValue({
      data: {
        enabled: true,
        singleFont: {
          family: "Noto Sans",
          fullName: "Noto Sans Regular",
          style: "Regular",
          weight: 400,
          path: "/fonts/notosans.ttf",
          faceIndex: 0,
        },
      },
      isLoading: false,
    } as any);

    // Setup an enabled mod that looks like a font mod
    vi.mocked(useInstalledMods).mockReturnValue({
      data: [
        {
          id: "m1",
          name: "korean-font-pack",
          displayName: "Korean Font Pack",
          enabled: true,
          tags: ["font"],
        },
      ],
    } as any);

    vi.mocked(useAllModWadReports).mockReturnValue({
      data: {
        m1: {
          modId: "m1",
          affectedWads: ["Bootstrap.windows.wad.client"],
        },
      },
    } as any);

    render(<LeagueFontSection />);

    expect(screen.getByText("Potential Font Mod Conflicts Detected")).toBeInTheDocument();
    expect(screen.getByText("Korean Font Pack")).toBeInTheDocument();
  });
});
