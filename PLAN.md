# Regular Skin Support Plan

## Goal

Add support for redirecting one League of Legends skin slot to load a different skin slot, without requiring custom mod content. This allows users to remap their selected base skin to load a different skin, even with no mods installed.

The player flow should be:

1. The player selects a base skin in the League client.
2. The player selects a target skin in LTK Manager.
3. LTK Manager patches the game so the base skin loads the target skin's assets.

This is separate from mod loading - it works with 0 mods installed.

## Current Architecture

The existing launch path already supports replacing League files through an overlay:

1. `start_patcher` receives the launch config.
2. `ModLibrary::ensure_overlay` builds the active profile overlay from enabled mods.
3. The overlay builder writes patched WAD/client content under the profile overlay directory.
4. Windows uses the legacy DLL patcher.
5. macOS Apple Silicon uses `ltk_macos_process_patcher` to redirect `.client` file reads into the overlay.

Regular skin support builds on this path. Skin remapping is additive to mod loading - both use the same overlay infrastructure. When the overlay is built, it includes:
1. Custom mod content (existing)
2. Skin slot remapping (new - redirects base skin to target skin)

A user can use skin remapping alone (0 mods), mods alone (0 remaps), or both together.

**Precedence**: If a champion has both a custom mod AND a skin remap configured, the mod takes precedence and the remap is ignored for that champion. This avoids conflicts and keeps behavior predictable. The remap feature is intended for users without custom mods for that champion.

## Terminology

- **Base skin**: Always `skin0` (the default/base skin). The user must have this selected in League.
- **Target skin**: The regular Riot skin slot selected in LTK Manager to remap to.
- **Skin slot**: Riot's numeric skin slot for a champion, for example `skin0`, `skin1`, `skin27`.
- **Skin remapping**: The process of redirecting skin0 to load a different skin's assets.

## User Flow

1. User has skin0 selected in the League client.
2. User opens Skin Remaps tab in LTK Manager.
3. User selects a target skin for a champion.
4. User clicks Run in LTK Manager.
5. Overlay remaps skin0 → target skin.
6. Platform patcher starts as it does today.

## Data Model

Store skin remapping configuration in app settings (not per-mod).

Suggested TypeScript shape:

```ts
export interface SkinRemap {
  championId: string;
  championName: string;
  targetSkinNumber: number;
  targetSkinName?: string;
}
```

Suggested serialized shape:

```json
{
  "skinRemaps": [
    {
      "championId": "ahri",
      "championName": "Ahri",
      "targetSkinNumber": 27,
      "targetSkinName": "Star Guardian Ahri"
    }
  ]
}
```

Field meanings:

- `championId`: Stable lowercase champion identifier used by content paths.
- `championName`: Display name for UI.
- `targetSkinNumber`: The skin slot to redirect skin0 to.
- `targetSkinName`: Display name from local game data.

## Skin Slot Remapping

The overlay needs to redirect the base skin slot to the target skin slot. This works with or without custom mod content.

Example:

```text
Base skin selected in League: Ahri skin0
Target skin in LTK Manager: Ahri skin27
Overlay remaps: skin0 → skin27
```

Implementation:

1. When building the overlay, get enabled mods and skin remap configuration.
2. For each champion with a custom mod enabled, use the mod content (remaps ignored for that champion).
3. For champions without custom mods but with a skin remap, add path transforms:
   - `assets/characters/ahri/skins/skin0/*` → `assets/characters/ahri/skins/skin27/*`
4. The remapping is conservative - only rewrite champion skin paths, not unrelated assets.



## Backend Launch Validation

Add skin remapping to the overlay build process alongside mod content.

Suggested launch order:

```text
start_patcher_inner
  -> snapshot settings
  -> get enabled mods
  -> get skin remap configuration
  -> build overlay (includes both mods AND skin remapping)
  -> start platform patcher
```

## Frontend UX

Add "Skin Remaps" as a top-level tab within each profile (next to Mods and Workshop).

UI layout:

1. **Champion list view** (first tab content)
   - List of all champions with remap status indicator
   - Modified champions show indicator (e.g., "✓ → skin27")
   - Click champion to edit that champion's remap
   - Base is always skin0 (default) - no selection needed

2. **Champion detail view** (when clicking a champion)
   - Shows "skin0 (Default)" as base (always fixed)
   - Target skin selector (what skin to redirect to)
   - Shows skin name and number
   - Remove Remap button

The UI uses local game data to populate champion and skin options (no network required). Remaps are stored per-profile.

## Tests

Backend tests:

1. Remap skin paths correctly in overlay.
2. Do not remap unrelated paths.
3. Handle multiple remaps for different champions.

Frontend tests:

1. Skin remap editor works correctly.
2. Remaps persist in settings.

Manual validation:

1. Configure skin remap with 0 mods and verify it works.
2. Configure skin remap with mods and verify both work.
3. Verify macOS Apple Silicon still uses the existing process patcher path.
4. Verify Windows still uses the existing DLL patcher path.

## Implementation Phases

### Phase 1: Data Model & Storage

- Add skin remap configuration to app settings (per-profile).
- Add parsing of local game data to get champion/skin list.

### Phase 2: Frontend UI

- Add Skin Remaps tab to profile.
- Add champion list view with remap indicators.
- Add target skin selector (base is always skin0).
- Persist remaps to profile.

### Phase 3: Backend Overlay Integration

- Add skin remapping to overlay build process.
- Implement path transforms for each remap.
- Add diagnostics for transformed paths.

### Phase 4: End-to-End Testing

- Test with skin remap only.
- Test with skin remap + mods.
- Test macOS Apple Silicon patching.
- Test Windows patching.
- Run `pnpm check` and relevant Rust tests.

## Acceptance Criteria

- A user can remap their base skin to a different skin slot, even with no mods installed.
- Overlay remapping targets the selected skin slot.
- Existing base-skin mod behavior continues to work.
- macOS Apple Silicon and Windows patcher paths remain unchanged except for the overlay content they receive.
