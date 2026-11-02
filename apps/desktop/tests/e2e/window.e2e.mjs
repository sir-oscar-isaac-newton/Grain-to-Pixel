// End-to-end test of the real app window through WebDriver. Windows only: Tauri has no WebDriver
// for macOS WebKit (macOS is covered by the smoke test).
//
// The app is a build made with src-tauri/tauri.e2e.conf.json, which opens WebView2's debugging port
// 9222 from the window's own settings. That works on the elevated session of a hosted CI runner,
// where the WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS variable that tauri-driver relies on is ignored
// (docs/convention.md section 8). The test starts that app itself and attaches msedgedriver to the
// port.
//
// The app keeps its settings in the user's own folder, so which view it starts in depends on the
// machine: a clean CI runner has no library and shows the first-run dialog, a developer machine that
// opened a library before starts in the Library view. Each state has its own test; the other one is
// skipped.
//
// Prerequisites: `pnpm build:e2e` and the msedgedriver that matches WebView2
// (tools/scripts/e2e/install-edgedriver.ps1). Run: pnpm test:e2e
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { after, before, describe, it } from 'node:test';
import { Browser, Builder, By, until } from 'selenium-webdriver';
import edge from 'selenium-webdriver/edge.js';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..');
const application =
  process.env.GTP_E2E_APP ?? path.join(root, 'target', 'release', 'grain-to-pixel-desktop.exe');
const edgeDriver =
  process.env.GTP_E2E_EDGEDRIVER ?? path.join(root, '.cache', 'edgedriver', 'msedgedriver.exe');
/** The port in src-tauri/tauri.e2e.conf.json. */
const DEBUG_PORT = 9222;
const UI_TIMEOUT = 20_000;
const START_TIMEOUT = 60_000;
const version = JSON.parse(
  readFileSync(path.join(root, 'apps', 'desktop', 'src-tauri', 'tauri.conf.json'), 'utf8'),
).version;

/** Resolves once WebView2 answers on the debugging port, i.e. the window is up. */
async function waitForDebugPort(app, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (app.exitCode !== null) throw new Error(`the app exited early with code ${app.exitCode}`);
    try {
      const response = await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/version`);
      if (response.ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(
    `WebView2 did not open the debugging port ${DEBUG_PORT}; is this a build made with pnpm build:e2e?`,
  );
}

describe('Grain to Pixel window', { skip: process.platform !== 'win32' }, () => {
  let app;
  let driver;
  /** 'first-run' when the app asks for a library, 'library' when one is already open. */
  let state;

  before(async () => {
    assert.ok(existsSync(application), `app binary not found: ${application}`);
    assert.ok(existsSync(edgeDriver), `msedgedriver not found: ${edgeDriver}`);

    app = spawn(application, [], { stdio: 'inherit' });
    await waitForDebugPort(app, START_TIMEOUT);

    const options = new edge.Options();
    options.debuggerAddress(`127.0.0.1:${DEBUG_PORT}`);
    driver = await new Builder()
      .forBrowser(Browser.EDGE)
      .setEdgeOptions(options)
      .setEdgeService(new edge.ServiceBuilder(edgeDriver))
      .build();

    // The Library view first shows "loading", then either the dialog or the library.
    state = await driver.wait(async () => {
      if ((await driver.findElements(By.css('[role="dialog"]'))).length > 0) return 'first-run';
      // Positive evidence only: the toolbar of an open library ("Library: <name>").
      const text = await driver.findElement(By.css('body')).getText();
      return /Library: \S/.test(text) && 'library';
    }, UI_TIMEOUT);
  });

  after(async () => {
    try {
      await driver?.quit();
    } finally {
      // The driver only detaches from a browser it did not start; end the app and its WebView2.
      if (app?.pid) spawnSync('taskkill', ['/PID', String(app.pid), '/T', '/F']);
    }
  });

  it('has the right title', async () => {
    assert.equal(await driver.getTitle(), 'Grain to Pixel');
  });

  it('starts on the Library view and shows the backend version (UI -> IPC -> Rust)', async () => {
    const heading = await driver.wait(until.elementLocated(By.css('h2')), UI_TIMEOUT);
    assert.equal(await heading.getText(), 'Library');

    const footer = await driver.findElement(By.css('footer'));
    await driver.wait(until.elementTextContains(footer, `Version ${version}`), UI_TIMEOUT);
  });

  it('asks for a library when there is none (clean profile)', async (t) => {
    if (state !== 'first-run') return t.skip('a library is already open on this machine');
    const dialog = await driver.findElement(By.css('[role="dialog"]'));
    await driver.wait(until.elementTextContains(dialog, 'Set up your library'), UI_TIMEOUT);
    assert.equal((await dialog.findElements(By.css('input[type="radio"]'))).length, 2);
    // Nothing can be created before a folder is chosen (the folder picker is a native dialog).
    const create = await dialog.findElement(
      By.xpath(".//button[normalize-space()='Create library']"),
    );
    assert.equal(await create.isEnabled(), false);
  });

  it('switches to the Develop view', async (t) => {
    if (state !== 'library') return t.skip('no library is open, so a dialog covers the tabs');
    const develop = await driver.findElement(By.xpath("//button[normalize-space()='Develop']"));
    await develop.click();
    const heading = await driver.findElement(By.css('h2'));
    await driver.wait(until.elementTextIs(heading, 'Develop'), UI_TIMEOUT);
  });
});
