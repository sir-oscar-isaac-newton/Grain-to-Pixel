# apps/desktop

The Tauri 2 desktop application: Rust shell (`src-tauri/`) that depends on `crates/*`, plus a React + TypeScript UI (`src/`) that consumes `packages/ui` and `packages/ipc-types`.

**Status:** not yet scaffolded. Generate it in place:

```bash
cd apps
pnpm create tauri-app desktop --template react-ts --manager pnpm
```

Afterwards: add `"apps/desktop/src-tauri"` to the root `Cargo.toml` workspace members and set the package name to `@gtp/desktop`.

**Rule:** the UI never touches image data directly; it calls Tauri commands that delegate to the crates.
