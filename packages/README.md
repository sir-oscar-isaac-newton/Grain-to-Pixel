# packages

Shared TypeScript packages (pnpm workspace, scope `@gtp/*`).

| Package | Purpose |
|---|---|
| `@gtp/ipc-types` | TypeScript types for Tauri commands/events. Generated from Rust (e.g. `specta` / `ts-rs`); do not hand-edit once generation is wired up. |
| `@gtp/ui` | Reusable React components (histogram, curve editor, filmstrip, tag picker). |
