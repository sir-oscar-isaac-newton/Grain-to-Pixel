// End-to-end test of the real app window through tauri-driver (WebDriver). Windows only: Tauri has
// no WebDriver for macOS WebKit (macOS is covered by the smoke test).
//
// Prerequisites: a release build (pnpm --filter @gtp/desktop exec tauri build --no-bundle),
// `tauri-driver` on PATH (cargo install tauri-driver --locked) and the matching msedgedriver
// (tools/scripts/e2e/install-edgedriver.ps1). Run: pnpm test:e2e
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { after, before, describe, it } from 'node:test';
import { Builder, By, Capabilities, until } from 'selenium-webdriver';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..');
const application =
  process.env.GTP_E2E_APP ?? path.join(root, 'target', 'release', 'grain-to-pixel-desktop.exe');
const edgeDriver =
  process.env.GTP_E2E_EDGEDRIVER ?? path.join(root, '.cache', 'edgedriver', 'msedgedriver.exe');
const PORT = 4444;
const UI_TIMEOUT = 20_000;

function waitForPort(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.connect(port, '127.0.0.1');
      socket.once('connect', () => {
        socket.destroy();
        resolve();
      });
      socket.once('error', () => {
        socket.destroy();
        if (Date.now() > deadline) reject(new Error(`tauri-driver did not listen on ${port}`));
        else setTimeout(attempt, 250);
      });
    };
    attempt();
  });
}

describe('Grain to Pixel window', { skip: process.platform !== 'win32' }, () => {
  let tauriDriver;
  let driver;

  before(async () => {
    assert.ok(existsSync(application), `app binary not found: ${application}`);
    assert.ok(existsSync(edgeDriver), `msedgedriver not found: ${edgeDriver}`);

    tauriDriver = spawn('tauri-driver', ['--native-driver', edgeDriver], { stdio: 'inherit' });
    tauriDriver.on('error', (err) => {
      throw new Error(`could not start tauri-driver (is it on PATH?): ${err.message}`);
    });
    await waitForPort(PORT, 20_000);

    const capabilities = new Capabilities();
    capabilities.set('tauri:options', { application });
    capabilities.setBrowserName('wry');
    driver = await new Builder()
      .withCapabilities(capabilities)
      .usingServer(`http://127.0.0.1:${PORT}/`)
      .build();
  });

  after(async () => {
    try {
      await driver?.quit();
    } finally {
      tauriDriver?.kill();
    }
  });

  it('has the right title', async () => {
    assert.equal(await driver.getTitle(), 'Grain to Pixel');
  });

  it('starts on the Library view and shows the backend version (UI -> IPC -> Rust)', async () => {
    const heading = await driver.wait(until.elementLocated(By.css('h2')), UI_TIMEOUT);
    assert.equal(await heading.getText(), 'Library');

    const footer = await driver.findElement(By.css('footer'));
    await driver.wait(until.elementTextContains(footer, 'Version 0.0.0'), UI_TIMEOUT);
  });

  it('switches to the Develop view', async () => {
    const develop = await driver.findElement(By.xpath("//button[normalize-space()='Develop']"));
    await develop.click();
    const heading = await driver.findElement(By.css('h2'));
    await driver.wait(until.elementTextIs(heading, 'Develop'), UI_TIMEOUT);
  });
});
