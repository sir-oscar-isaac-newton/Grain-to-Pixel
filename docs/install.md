# Installing Grain to Pixel

The installers are not signed with a paid certificate. Windows and macOS both warn about that the first time; this page shows what to click. Nothing on this page sends anything anywhere: the app works on your computer only.

## Windows (x64)

File: `Grain to Pixel_<version>_x64-setup.exe` (an NSIS installer).

1. Double-click the file.
2. Windows SmartScreen may show "Windows protected your PC". Choose **More info**, then **Run anyway**. The warning is about the missing paid signature, not about something found in the file.
3. The installer installs for **your user only**. It needs no administrator rights and asks for none. The app appears in the Start menu.
4. To remove it, use **Settings > Apps > Installed apps > Grain to Pixel > Uninstall**. Your library folder and your originals are never part of the uninstall.

On a Windows on ARM computer the x64 installer works through Windows' emulation. There is no native ARM build yet.

## macOS (Apple silicon)

File: `Grain to Pixel_<version>_aarch64.dmg`. Intel Macs are not supported.

1. Open the dmg and drag **Grain to Pixel** into **Applications**.
2. Open it from Applications. macOS says it cannot verify the app. That is Gatekeeper, because the app is only ad-hoc signed (`signingIdentity: "-"`), not notarized. Click **Done**, not "Move to Trash".
3. Open **System Settings > Privacy & Security**, scroll to the **Security** section and click **Open Anyway** next to "Grain to Pixel", then confirm with your password or Touch ID. This is needed once; later launches open normally.
4. Alternative in the Terminal, which removes the download mark from the app:

```bash
xattr -dr com.apple.quarantine "/Applications/Grain to Pixel.app"
```

The macOS build has the tethered-capture feature (`tether`) switched on; the Windows build has it switched off.

## Updates

Grain to Pixel asks GitHub Releases whether a newer version exists when it starts, at most once a day, and shows a notice with what is new. It installs nothing until you press **Install**, and checks the signature of the download first; a download that does not match is thrown away. It sends nothing about you or your library. **Settings > Check for updates now** asks at once. To stop the check, switch off **Check for updates** in the feature flags in Settings: the app then makes no network request at all.

On Windows the installer closes the app and starts the new version. On macOS the app asks you to restart after the update is in place. The ad-hoc signed app opens after an update without a new Gatekeeper warning (tried on a Mac with two test releases), so the Gatekeeper steps for the first start are needed only once.

## First start

The app asks for a library folder. Choose or create an empty folder on a disk with room for your scans; originals are copied there and never changed.

## For the person building the installers

CI builds both installers in the `Bundle` workflow (`.github/workflows/bundle.yml`) and keeps them as artifacts for 14 days. The Windows installer must be at most 40 MB; the workflow fails above that. The dmg size is only reported. Locally:

```bash
pnpm --filter @gtp/desktop exec tauri build --bundles nsis
```

```bash
pnpm --filter @gtp/desktop exec tauri build --bundles dmg --features tether
```

`pnpm check:bundle` checks that the installer settings in `tauri.conf.json` have not drifted.
