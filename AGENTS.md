# AGENTS.md

Tauri v2 desktop app — Rust backend, React/TypeScript frontend.

## Setup

```bash
pnpm install
```

## Dev commands

```bash
pnpm dev          # Frontend only (fastest)
pnpm tauri dev    # Full app with Rust hot reload
pnpm check        # typecheck + lint + format:check
pnpm tauri build  # Production build
```

## Architecture

Backend (`src-tauri/src/`) is organized by domain: `mods/`, `overlay/`, `patcher/`, `legacy_patcher/`. Each domain has business logic in its own module, with thin `#[tauri::command]` wrappers in `commands/`. State is accessed via `State<T>`.

Frontend (`src/`) is organized by feature modules under `src/modules/`. Each module has `api/` (TanStack Query hooks), `components/`, and a barrel `index.ts`. Import from barrel files only (`@/components`, `@/modules/library`) — never from subdirectories.

Tauri IPC is the bridge between them — Rust commands return `IpcResult<T>`, frontend calls them via `src/lib/tauri.ts`.

## Code conventions

- **UI:** Use wrapped components from `@/components`, not raw `@base-ui/react`. Icons from `lucide-react`.
- **Forms:** `@tanstack/react-form` with Zod via `useAppForm()`.
- **Styling:** Tailwind v4 via `@tailwindcss/vite`. Design tokens are CSS custom properties (`--space-{NNN}`, `--surface-{50..950}`). No `@apply` in `global.css`.
- **State:** Server state → TanStack Query. Client-only state → Zustand. Consume directly in the component that needs it — no prop drilling.
- **Comments:** Only for non-obvious business logic, workarounds, or "why" decisions. No redundant narration.
- **JSX:** Early returns and `{condition && <Component />}` — no ternaries in JSX.

## Adding a new feature

1. Business logic in `src-tauri/src/{domain}/`
2. Command wrapper in `src-tauri/src/commands/{domain}.rs`, register in `main.rs`
3. TS types + binding in `src/lib/tauri.ts`
4. Hook in `src/modules/{domain}/api/useX.ts`, export through barrel files

## Log files

- macOS/Linux: `~/.local/share/dev.leaguetoolkit.manager/logs/ltk-manager.log`
- Windows: `%APPDATA%\dev.leaguetoolkit.manager\logs\ltk-manager.log`
