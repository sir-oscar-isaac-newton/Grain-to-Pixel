# 0006. The e2e test reaches WebView2 through a debugging port set in the window config

- Status: Accepted (approved by the author with T-07 / GitHub #69, 2026-11-01)
- Date: 2026-11-01

## Context

The WebDriver test of the window passed on a desktop session but could not start a session on GitHub-hosted Windows runners, so it ran by hand or weekly and was not a gate. `tauri-driver` makes msedgedriver reach WebView2 through the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` environment variable. A spike on the runner (elevated session, High integrity, WebView2 156) showed that WebView2 does not apply that variable there: the browser process had no `--remote-debugging-port` and nothing listened. The same flag given in the window's own settings (`additionalBrowserArgs`) was applied and port 9222 answered. A throwaway standard user did not help: the app ran, but under a different data folder the port still did not answer.

## Decision

- The e2e job builds the app with `apps/desktop/src-tauri/tauri.e2e.conf.json`, which restates the main window with `additionalBrowserArgs` including `--remote-debugging-port=9222`. It is a build-time config file, not a Cargo feature, so a normal build cannot carry it.
- The test starts the app itself and attaches msedgedriver with `debuggerAddress` (`apps/desktop/tests/e2e/window.e2e.mjs`). `tauri-driver` is no longer used.
- Chromium's port listens on 127.0.0.1 only. The port exists only in the e2e build.
- `pnpm check:bundle` fails if `tauri.conf.json`, `tauri.release.conf.json`, `release.yml` or `bundle.yml` mention the e2e config or a remote debugging port.
- `e2e-windows` is a job of `ci.yml` and a required check of `develop`.

## Consequences

- The window test runs on every pull request.
- The e2e build replaces `target/release/grain-to-pixel-desktop.exe`; run `pnpm build:app` again before `pnpm smoke`.
- Port 9222 must be free on the machine that runs the test.
- If WebView2 ever stops honouring the window setting, the job fails loudly (the test waits for the port and says what to check).