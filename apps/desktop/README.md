# apps/desktop

The Tauri 2 desktop application: a thin Rust shell (`src-tauri/`, crate `grain-to-pixel-desktop`) over `gtp-app`, plus a React + TypeScript UI (`src/`, package `@gtp/desktop`) that will consume `packages/ui` and `packages/ipc-types`.

**Status (D-01):** the skeleton runs. It has three placeholder views (Library, Develop, Import), a Zustand view store, i18n (`src/shared/i18n/en.json`), a strict CSP, minimal capabilities, and one stub command, `library_info`.

## Run

```bash
pnpm install
pnpm dev          # from the repo root: starts Vite on 127.0.0.1:1420 and the Tauri window
pnpm --filter @gtp/desktop test        # Vitest component tests
cargo test -p grain-to-pixel-desktop   # Rust unit tests
pnpm build:app    # release binary at target/release/ (no installer)
```

Prerequisites: Windows needs the WebView2 runtime and Visual Studio Build Tools ("Desktop development with C++"); macOS needs the Xcode command line tools.

## Smoke and end-to-end tests (CI gate 7)

Both need a release build first (`pnpm build:app`).

```bash
pnpm smoke        # starts the built app and waits for it to load its page (macOS and Windows)
pnpm test:e2e     # Windows only: drives the real window through tauri-driver (WebDriver)
```

- **Smoke** (`tools/scripts/smoke/smoke.mjs`): runs the binary with `GTP_READY_FILE` set; the app writes that file when its page has finished loading (`src-tauri/src/lib.rs`). The test fails if the app exits early or is not ready within 90 s. Its own tests use fake binaries (crash, hang, ready).
- **End-to-end** (`apps/desktop/tests/e2e/window.e2e.mjs`): Selenium WebDriver against `tauri-driver`. It checks the window title, that the footer shows the backend version (UI, IPC and the Rust command), and the view switch. One-time setup on Windows:

  ```bash
  cargo install tauri-driver --locked --version 2.1.0
  powershell -File tools/scripts/e2e/install-edgedriver.ps1   # downloads the msedgedriver that matches WebView2 into .cache/
  ```

  Tauri has no WebDriver for macOS WebKit, so macOS is covered by the smoke test only.

## Logging and crash notes (D-03)

- **Log file:** JSON lines in the OS log directory (`app_log_dir()`: `%LOCALAPPDATA%\io.github.sir-oscar-isaac-newton.graintopixel\logs` on Windows, `~/Library/Logs/io.github.sir-oscar-isaac-newton.graintopixel` on macOS), file `gtp.<date>.log`. Rotated daily, 7 files kept, total size capped at 50 MB: the oldest files are deleted at startup and then hourly (the newest file is never deleted). Default level `info`; set `RUST_LOG` to change it. Debug builds also print to stderr.
- **Panics:** a panic writes `crash-<unix ms>-<pid>.txt` next to the log. On the next start the notes are logged at `warn`, renamed to `.reported`, and the UI shows a dismissible notice ("The app closed unexpectedly last time"). The command is `crash_notes_take`; "export diagnostics" arrives with `diagnostics_export`.
- **UI errors:** the React `ErrorBoundary` and the `window` `error` / `unhandledrejection` handlers (`src/shared/errors/`) call `log_client_event(level, message)` through `ipc.ts`. The user sees a recoverable screen with no technical text; the details are in the log.
- Code: `src-tauri/src/{logging.rs, crash.rs}` and `commands/system.rs`. Use `tracing` macros only (`println!` and `console.log` fail lint).
## Layout

```
src/                 React UI: main.tsx, App.tsx, app/store.ts, features/{library,develop,import}, shared/{ipc.ts,i18n,styles}
src-tauri/           Rust shell: lib.rs (run), logging.rs, crash.rs, state.rs, commands/system.rs, tauri.conf.json, capabilities/default.json, icons/
```

- The UI calls the backend only through `src/shared/ipc.ts`, which wraps the commands generated from Rust (`@gtp/ipc-types`, regenerate with `pnpm gen:ipc`; see [packages/README.md](../../packages/README.md)).
- `src-tauri/build.rs` embeds a Windows application manifest (Common Controls v6) into every target, so unit tests and the `gen_ipc` example can start; without it they crash with `STATUS_ENTRYPOINT_NOT_FOUND`.
- Commands are thin: validate, call a `gtp-app` service, map errors. No business logic in `src-tauri`.
- `capabilities/default.json` grants only `core:default`; add permissions together with the command that needs them. There is no shell plugin.
- CSP (production): `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data: blob:`. The `connect-src` entry is what Tauri's IPC needs. `devCsp` is looser (inline styles and the Vite websocket) and applies only to `pnpm dev`.
- `src-tauri/icons/source.png` is a generated placeholder (`tools/scripts/icons/make-placeholder-icon.mjs`); the other icons come from `tauri icon`. Replace with real artwork later.

**Rule:** the UI never touches image data directly; it calls Tauri commands that delegate to the crates.
