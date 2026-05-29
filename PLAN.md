# Skin Companion Suffix Generation Plan

## Goal

Replace the inline companion suffix list in `src-tauri/src/skin_remap.rs` with a checked-in generated data file, and add an explicit `xtask` command that refreshes that file by scanning the local League install.

Normal builds must stay reproducible and must not require League to be installed.

## Target Design

### Checked-In Data

Add a generated TOML file:

```text
src-tauri/data/skin_companion_suffixes.toml
```

Suggested shape:

```toml
direct = [
  "cougar",
  "tibbers",
  "dragon",
]

[[alias]]
suffix = "ghoulmelee"
trigger = "ghoul"

[[alias]]
suffix = "spiderling"
trigger = "spider"
```

Rules:

- `direct` means: if a skin bin string contains `suffix`, generate `champion + suffix`.
- `alias` means: if a skin bin string contains `trigger`, generate `champion + suffix`.
- Keep the file deterministic: sorted direct suffixes, sorted aliases, stable formatting.
- Do not include non-prefix companions like `heimertblue` in this file. Those are handled by direct path references.

### Runtime Loading

Update `skin_remap.rs` to load the TOML with `include_str!`.

Use a small parsed struct and cache it with `std::sync::OnceLock`:

```rust
static COMPANION_RULES: OnceLock<CompanionRules> = OnceLock::new();
```

Then `collect_companion_skin_id_candidates_from_terms` should iterate:

- `rules.direct`
- `rules.aliases`

This removes the large inline static list from code while keeping runtime behavior deterministic.

### Tests

Add focused Rust tests that:

- Parse the checked-in TOML successfully.
- Verify representative known cases:
  - `Nidalee` + `cougar` -> `nidaleecougar`
  - `Annie` + `tibbers` -> `annietibbers`
  - `Zyra` + `thornplant` -> `zyrathornplant`
  - `Yorick` + `ghoul` alias -> `yorickghoulmelee` and `yorickbigghoul`
  - `Elise` + `spider` alias -> `elisespiderling`
- Keep the existing Kittalee/Nidalee cougar regression test.

No `build.rs` validation is needed. Tests are enough and less surprising.

## Xtask

Add an `xtask` workspace crate:

```text
xtask/Cargo.toml
xtask/src/main.rs
```

Update root `Cargo.toml`:

```toml
[workspace]
members = ["src-tauri", "xtask"]
resolver = "2"
```

Command:

```bash
cargo run -p xtask -- generate-skin-companions
```

Optional shorthand later:

```toml
[alias]
xtask = "run -p xtask --"
```

Then:

```bash
cargo xtask generate-skin-companions
```

## Generator Behavior

The `generate-skin-companions` command should:

1. Resolve the League game dir:
   - Use `--game-dir <path>` if provided.
   - Otherwise use `/Applications/League of Legends.app/Contents/LoL/Game` on macOS.
2. Scan `DATA/FINAL/Champions/*.wad.client`.
3. Read champion skin numbers from LTK Manager's `game-champions.json` cache when available.
4. Mount each champion WAD and inspect known `data/characters/{id}/skins/skinN.bin` chunks.
5. Collect strings from parsed `ltk_meta::Bin` objects.
6. Detect companion character IDs by:
   - Direct path references to another `assets/characters/{character}/skins/...`.
   - Term-based candidates where `champion + suffix` has a matching `skinN.bin` chunk.
7. Emit:
   - `direct` suffixes for same-suffix trigger matches.
   - `alias` entries for suffixes that need a broader trigger, such as `ghoulmelee` from `ghoul`.
8. Write deterministic TOML to `src-tauri/data/skin_companion_suffixes.toml`.

The command should print a short summary:

```text
Generated 72 direct suffixes and 4 aliases
Wrote src-tauri/data/skin_companion_suffixes.toml
```

## Performance

Initial implementation can stay serial if it is simple and correct.

If refresh time remains around 30 seconds and feels painful, add Rayon later:

- Parallelize per champion WAD.
- Keep each WAD mounted inside one worker.
- Merge results into sorted sets after scanning.

Avoid running the scanner during `pnpm tauri build`, `cargo build`, or `build.rs`.

## Cleanup

After the `xtask` exists:

- Move the current scanner logic out of `src-tauri/examples/scan_skin_companions.rs`.
- Either delete that example or keep it as a thin wrapper only if useful.
- Ensure `pnpm check` and `cargo test -p ltk-manager` pass.
