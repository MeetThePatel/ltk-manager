import { Link } from "@tanstack/react-router";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { LucideIcon } from "lucide-react";
import { FolderOpen, Hammer, Library, Minus, Play, Settings, Shirt, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useHotkeys } from "react-hotkeys-hook";
import { twMerge } from "tailwind-merge";

import { Button, IconButton, Kbd, Separator, Tooltip, useToast } from "@/components";
import { useHddWarning,usePlatformSupport } from "@/hooks";
import { api, type AppInfo, unwrap } from "@/lib/tauri";
import { ProfileSelector,useInstalledMods, useSkinRemaps } from "@/modules/library";
import { usePatcherStatus, useStartPatcher, useStopPatcher } from "@/modules/patcher";
import { useSettings } from "@/modules/settings";

import { NotificationCenter } from "./NotificationCenter";

const navItems = [
  { to: "/", label: "Library", icon: Library, exact: true },
  { to: "/skin-remaps", label: "Skin Remaps", icon: Shirt, exact: true },
  { to: "/workshop", label: "Workshop", icon: Hammer, exact: false },
] as const;

const linkBaseClass =
  "relative flex h-full items-center gap-1.5 px-3 text-sm font-medium transition-colors";
const activeLinkClass = "text-accent-400";
const inactiveLinkClass = "text-surface-400 hover:text-surface-200";

function ActiveIndicator() {
  return <span className="absolute right-0 bottom-0 left-0 h-0.5 bg-accent-500" />;
}

function NavLink({
  to,
  label,
  icon: Icon,
  exact,
}: {
  to: string;
  label: string;
  icon: LucideIcon;
  exact: boolean;
}) {
  return (
    <Link
      to={to}
      activeOptions={{ exact }}
      activeProps={{ className: twMerge(linkBaseClass, activeLinkClass) }}
      inactiveProps={{ className: twMerge(linkBaseClass, inactiveLinkClass) }}
    >
      {({ isActive }) => (
        <>
          <Icon className="h-4 w-4" />
          {label}
          {isActive && <ActiveIndicator />}
        </>
      )}
    </Link>
  );
}

interface TitleBarProps {
  title?: string;
  appInfo?: AppInfo;
}

export function TitleBar({ title = "LTK Manager", appInfo }: TitleBarProps) {
  const { data: platform } = usePlatformSupport();
  const isMacOS = platform?.os === "macos";

  const version = appInfo?.version;
  const [isMaximized, setIsMaximized] = useState(false);
  const appWindow = getCurrentWindow();
  const toast = useToast();

  const { data: settings } = useSettings();
  const { data: mods = [] } = useInstalledMods();
  const { data: skinRemaps = [] } = useSkinRemaps();
  const { data: patcherStatus } = usePatcherStatus();
  const startPatcher = useStartPatcher();
  const stopPatcher = useStopPatcher();
  const maybeShowHddWarning = useHddWarning();

  const isStarting = patcherStatus?.phase === "building";
  const isPatcherActive = patcherStatus?.running ?? false;
  const hasPatcherInputs = mods.some((m) => m.enabled) || skinRemaps.length > 0;
  const patcherAvailable = platform?.patcherAvailable ?? true;

  async function handleStartPatcher() {
    if (!settings?.leaguePath) {
      toast.error("League path not configured", "Configure it in settings first.");
      return;
    }

    await maybeShowHddWarning();

    startPatcher.mutate(
      {},
      {
        onError: (error) => {
          console.error("Failed to start patcher:", error.message);
        },
      },
    );
  }

  function handleStopPatcher() {
    stopPatcher.mutate(undefined, {
      onError: (error) => {
        console.error("Failed to stop patcher:", error.message);
      },
    });
  }

  useHotkeys(
    "ctrl+p",
    () => {
      if (isPatcherActive) {
        handleStopPatcher();
      } else {
        handleStartPatcher();
      }
    },
    { preventDefault: true },
  );

  async function handleOpenStorageDirectory() {
    try {
      const result = await api.getStorageDirectory();
      const path = unwrap(result);
      await api.revealInExplorer(path);
    } catch (error: unknown) {
      toast.error(
        "Failed to open directory",
        error instanceof Error ? error.message : String(error),
      );
    }
  }

  useEffect(() => {
    // Check initial maximized state
    appWindow.isMaximized().then(setIsMaximized);

    // Listen for resize events to update maximized state
    const unlisten = appWindow.onResized(() => {
      appWindow.isMaximized().then(setIsMaximized);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [appWindow]);

  const handleMinimize = () => {
    api.minimizeToTray();
  };
  const handleMaximize = () => appWindow.toggleMaximize();
  const handleClose = () => appWindow.close();

  return (
    <header
      className="title-bar flex h-10 shrink-0 items-center justify-between border-b border-surface-600 bg-surface-950 select-none"
      data-tauri-drag-region
    >
      {/* Left: App icon, title, version, and navigation */}
      <div className="flex h-full items-center" data-tauri-drag-region>
        <div className="flex items-center gap-2 pr-4 pl-3" data-tauri-drag-region>
          <img src="/icon.svg" alt="LTK" className="h-5 w-5" data-tauri-drag-region />
          <span className="text-sm font-medium text-surface-100" data-tauri-drag-region>
            {title}
          </span>
          {version && (
            <span className="text-xs text-surface-500" data-tauri-drag-region>
              v{version}
            </span>
          )}
        </div>

        {/* Navigation tabs */}
        <nav className="flex h-full items-center gap-1">
          {navItems.map((item) => (
            <NavLink key={item.to} {...item} />
          ))}
        </nav>

        <Separator orientation="vertical" />

        <ProfileSelector />
      </div>

      {/* Right: Notifications, Settings, and window controls */}
      <div className={twMerge("flex h-full items-center gap-1.5", isMacOS && "pr-3")}>
        {patcherAvailable && (
          <>
            <Tooltip
              content={
                <>
                  Toggle patcher <Kbd shortcut="Ctrl+P" />
                </>
              }
            >
              {isPatcherActive ? (
                <Button
                  variant="outline"
                  size="xs"
                  onClick={handleStopPatcher}
                  loading={stopPatcher.isPending}
                  left={
                    !stopPatcher.isPending && (
                      <span className="relative flex h-2 w-2">
                        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-green-400 opacity-75" />
                        <span className="relative inline-flex h-2 w-2 rounded-full bg-green-500" />
                      </span>
                    )
                  }
                  className="h-7 rounded-full border-green-500/40 bg-green-500/10 px-2.5 text-xs font-semibold text-green-400 hover:border-green-500/60 hover:bg-green-500/20"
                >
                  {stopPatcher.isPending ? "Stopping..." : "Stop Patcher"}
                </Button>
              ) : (
                <Button
                  variant={hasPatcherInputs ? "filled" : "default"}
                  size="xs"
                  onClick={handleStartPatcher}
                  loading={isStarting}
                  left={!isStarting && <Play className="h-3 w-3 fill-current" />}
                  disabled={!hasPatcherInputs || isStarting || stopPatcher.isPending}
                  className="h-7 rounded-full px-2.5 text-xs font-semibold"
                >
                  {isStarting ? "Building..." : "Start Patcher"}
                </Button>
              )}
            </Tooltip>
            <Separator orientation="vertical" className="h-4" />
          </>
        )}

        <Tooltip content="Open storage directory">
          <IconButton
            icon={<FolderOpen className="h-4 w-4" />}
            variant="ghost"
            size="sm"
            onClick={handleOpenStorageDirectory}
            aria-label="Open storage directory"
            className="text-surface-400 hover:text-surface-200"
          />
        </Tooltip>

        <NotificationCenter />

        <Tooltip content="Settings">
          <Link to="/settings" aria-label="Settings">
            {({ isActive }) => (
              <IconButton
                icon={<Settings className="h-4 w-4" />}
                variant="ghost"
                size="sm"
                className={twMerge(
                  "text-surface-400 hover:text-surface-200",
                  isActive && "bg-surface-700 text-surface-100",
                )}
              />
            )}
          </Link>
        </Tooltip>

        {!isMacOS && (
          <>
            <Separator orientation="vertical" />

            <IconButton
              icon={<Minus className="h-3.5 w-3.5" />}
              variant="ghost"
              size="sm"
              onClick={handleMinimize}
              aria-label="Minimize"
              className="mx-0.5 h-7 w-7 rounded-md text-surface-400 transition-[transform,background-color,color] duration-100 hover:bg-amber-500 hover:text-white active:scale-90 active:opacity-80"
            />
            <IconButton
              icon={
                isMaximized ? (
                  <OverlappingSquares className="h-3 w-3" />
                ) : (
                  <Square className="h-3 w-3" />
                )
              }
              variant="ghost"
              size="sm"
              onClick={handleMaximize}
              aria-label={isMaximized ? "Restore" : "Maximize"}
              className="mx-0.5 h-7 w-7 rounded-md text-surface-400 transition-[transform,background-color,color] duration-100 hover:bg-green-500 hover:text-white active:scale-90 active:opacity-80"
            />
            <IconButton
              icon={<X className="h-3.5 w-3.5" />}
              variant="ghost"
              size="sm"
              onClick={handleClose}
              aria-label="Close"
              className="mx-0.5 mr-2 h-7 w-7 rounded-md text-surface-400 transition-[transform,background-color,color] duration-100 hover:bg-red-500 hover:text-white active:scale-90 active:opacity-80"
            />
          </>
        )}
      </div>
    </header>
  );
}

// Custom icon for restored/unmaximized state (overlapping squares)
function OverlappingSquares({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 14 14"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
    >
      {/* Back square */}
      <rect x="4" y="1" width="9" height="9" rx="1" />
      {/* Front square */}
      <rect x="1" y="4" width="9" height="9" rx="1" fill="currentColor" fillOpacity="0.1" />
      <rect x="1" y="4" width="9" height="9" rx="1" />
    </svg>
  );
}
