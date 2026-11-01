# Releasing Grain to Pixel

A release is a tag. Pushing `vX.Y.Z` runs `.github/workflows/release.yml`, which checks the tag, runs the gates of `ci.yml`, builds the Windows installer and the macOS dmg with updater signatures, and creates a **draft** GitHub Release with the installers and `latest.json`. Nothing is public until you publish the draft. The app's updater reads `latest.json` from the latest published release, so only a published, non-prerelease release is offered to users.

## Once: the updater signing key

The key pair signs the update packages; the app holds the public half and refuses any package that does not match it. Make it on your own computer (the private key never goes into chat or the repository):

```bash
pnpm --filter @gtp/desktop exec tauri signer generate -w "$HOME/.tauri/gtp-updater.key"
```

1. Keep a backup of the key file and its password. If either is lost, installed copies can never be updated and everyone has to install again by hand.
2. In the repository settings (Secrets and variables > Actions) add `TAURI_SIGNING_PRIVATE_KEY` (the whole contents of the key file) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Put the contents of the `.pub` file in `plugins.updater.pubkey` in `apps/desktop/src-tauri/tauri.conf.json`, replacing `REPLACE_WITH_UPDATER_PUBLIC_KEY`. The release workflow refuses to run while the placeholder is there.

## Each release

1. Set the same version in `apps/desktop/src-tauri/tauri.conf.json` (`version`) and in `Cargo.toml` (`[workspace.package]`), for example `0.1.0`. Every `version = "0.0.0"` on a path dependency in the crates' `Cargo.toml` files (`crates/*`, `apps/desktop/src-tauri`, `tools/benchmark`) must change too, then run `cargo update --workspace` so `Cargo.lock` follows. The updater compares this number with the one in `latest.json`, so a build that keeps the old number is never offered the new one.
2. Merge that change, then tag the merge commit and push the tag:

```bash
git tag v0.1.0
```

```bash
git push origin v0.1.0
```

3. Wait for the `Release` workflow. Its `check the tag` job fails at once if the tag, the two versions or the public key do not agree.
4. Open the draft release on GitHub. It should hold the Windows `-setup.exe` with its `.sig`, the macOS `.dmg`, the `.app.tar.gz` with its `.sig`, and `latest.json`. Read the notes the app will show, and install the draft's installers once on each system.
5. Publish the draft. A tag with a hyphen (`v0.1.0-rc.1`) is published as a prerelease, which the updater does not offer.

## Dry run on a prerelease tag

To test the pipeline without touching `develop`, make a branch, set both versions to `0.0.1-rc.1`, and push the tag `v0.0.1-rc.1` from that branch. The workflow creates a draft prerelease. Check the assets and `latest.json` as in step 4, then delete the draft release and the tag:

```bash
git push origin --delete v0.0.1-rc.1
```

## Trying the updater by hand

The real update needs two releases: install the first, publish a second with a higher version, and start the first. The app should offer the new version (once a day at launch, or at once with **Settings > Check for updates now**), install it only after you press Install, and then restart. On a Mac, check that the updated app opens without a new Gatekeeper warning and write what you saw in `docs/install.md`.