# packages

Shared TypeScript packages (pnpm workspace, scope `@gtp/*`).

| Package | Purpose |
|---|---|
| `@gtp/ipc-types` | Typed Tauri commands, events and the `AppError` shape, **generated from Rust** by `tauri-specta`. `src/bindings.ts` is generated and committed; never hand-edit it. |
| `@gtp/ui` | Reusable React components (histogram, curve editor, filmstrip, tag picker). |

## Working with `@gtp/ipc-types`

```bash
pnpm gen:ipc     # regenerate packages/ipc-types/src/bindings.ts from the Rust sources
pnpm check:ipc   # fails if the committed bindings are stale (also run by CI)
```

- The command list lives in `apps/desktop/src-tauri/src/ipc.rs` (`collect_commands!`, `collect_events!`). A command needs `#[tauri::command]` and `#[specta::specta]`; types that cross the boundary derive `specta::Type` (in `gtp-core` behind the `specta` feature).
- The UI never imports `bindings.ts` directly. It goes through `apps/desktop/src/shared/ipc.ts`, which turns `{ status: 'error' }` results into thrown `IpcError`s.
- Commands return `Result<T, AppError>`; `AppError` is `{ code, message, details? }` ([specification section 6](../docs/specification.md)).
- The `tauri-specta` stack is still pre-release, so `specta`, `specta-typescript` and `tauri-specta` are pinned to one exact compatible set in `Cargo.toml`; upgrade them together.
