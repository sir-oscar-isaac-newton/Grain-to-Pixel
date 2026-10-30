# apps/desktop

The Tauri 2 desktop application: a thin Rust shell (`src-tauri/`, crate `grain-to-pixel-desktop`) over `gtp-app`, plus a React + TypeScript UI (`src/`, package `@gtp/desktop`) that will consume `packages/ui` and `packages/ipc-types`.

**Status (D-01):** the skeleton runs. It has three placeholder views (Library, Develop, Import), a Zustand view store, i18n (`src/shared/i18n/en.json`), a strict CSP, minimal capabilities, and one stub command, `library_info`.

## Run

```bash
pnpm install
pnpm dev          # from the repo root: starts Vite on 127.0.0.1:1420 and the Tauri window
pnpm --filter @gtp/desktop test        # Vitest component tests
cargo test -p grain-to-pixel-desktop   # Rust unit tests
pnpm --filter @gtp/desktop exec tauri build --no-bundle   # release binary at target/release/
```

Prerequisites: Windows needs the WebView2 runtime and Visual Studio Build Tools ("Desktop development with C++"); macOS needs the Xcode command line tools.

## Layout

```
src/                 React UI: main.tsx, App.tsx, app/store.ts, features/{library,develop,import}, shared/{ipc.ts,i18n,styles}
src-tauri/           Rust shell: lib.rs (run), state.rs, commands/system.rs, tauri.conf.json, capabilities/default.json, icons/
```

- The UI calls the backend only through `src/shared/ipc.ts` (replaced by generated types in D-02).
- Commands are thin: validate, call a `gtp-app` service, map errors. No business logic in `src-tauri`.
- `capabilities/default.json` grants only `core:default`; add permissions together with the command that needs them. There is no shell plugin.
- CSP (production): `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data: blob:`. The `connect-src` entry is what Tauri's IPC needs. `devCsp` is looser (inline styles and the Vite websocket) and applies only to `pnpm dev`.
- `src-tauri/icons/source.png` is a generated placeholder (`tools/scripts/icons/make-placeholder-icon.mjs`); the other icons come from `tauri icon`. Replace with real artwork later.

**Rule:** the UI never touches image data directly; it calls Tauri commands that delegate to the crates.
