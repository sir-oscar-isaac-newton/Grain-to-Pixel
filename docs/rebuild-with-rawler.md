# Rebuilding the app with a modified rawler

Grain to Pixel decodes camera NEF files with [`rawler`](https://crates.io/crates/rawler), which is under the GNU Lesser General Public License, version 2.1 (see [third-party-notices.md](third-party-notices.md) and [ADR 0007](../decisions/0007-nef-decoder.md)). The LGPL-2.1 lets you change `rawler` and build the app with your version. The app's own licence ([LICENSE](../LICENSE)) lets you modify the app for your own use and reverse-engineer it to debug such changes. This page says how.

## 1. Get the sources

- The app: clone the repository at the tag of the release you use (`git clone`, then `git checkout vX.Y.Z`). `Cargo.lock` pins the exact `rawler` version.
- `rawler`: every release has the pinned version attached as `rawler-<version>.crate` (a gzipped tar archive; the version is in `Cargo.lock`). Unpack it next to the app, for example:

  ```bash
  tar -xzf rawler-0.8.0.crate     # makes the folder rawler-0.8.0
  ```

  The same archive is at `https://static.crates.io/crates/rawler/rawler-0.8.0.crate`, and the upstream repository is https://github.com/dnglab/dnglab.

## 2. Change it

Edit the files in the `rawler` folder. Keep the version in its `Cargo.toml` as it is, so `Cargo.lock` does not need to change.

## 3. Point the app at your copy

Add a `[patch]` entry to the `Cargo.toml` at the root of the app (the workspace file), with the path to your `rawler` folder:

```toml
[patch.crates-io]
rawler = { path = "../rawler-0.8.0" }
```

Do not commit this entry; it is for your build only.

## 4. Build

Install what the app needs (see the README), then build as usual:

```bash
pnpm install
cargo build --release -p grain-to-pixel-desktop    # or: pnpm build:app
```

`cargo tree -p grain-to-pixel-desktop -i rawler` should show the path of your folder, not the registry, which tells you that your version is the one linked in.

## The same for the camera crates (macOS)

The macOS app also contains two LGPL-2.1 crates, `gphoto2` and `libgphoto2_sys` (the camera backend, see [third-party-notices.md](third-party-notices.md)). They are compiled into the app like `rawler`, so the steps are the same:

- Every release has `gphoto2-<version>.crate` and `libgphoto2_sys-<version>.crate` attached (versions in `Cargo.lock`); unpack them next to the app with `tar -xzf`.
- Add the entries to the workspace `Cargo.toml` (not to be committed):

  ```toml
  [patch.crates-io]
  gphoto2 = { path = "../gphoto2-3.4.1" }
  libgphoto2_sys = { path = "../libgphoto2_sys-1.2.3" }
  ```

- Install libgphoto2 (`brew install libgphoto2 pkgconf`) and build with the feature that switches the camera backend on:

  ```bash
  cargo build --release -p grain-to-pixel-desktop --features tether
  ```

  `cargo tree -p gtp-capture -i gphoto2 --features tether` should show your folder. The libgphoto2 libraries themselves are not part of the crates: see the notices for replacing them in the app.

## If you pass your changes on

A changed `rawler` that you give to anyone else stays under the LGPL-2.1: keep its licence, say what you changed, and make the source available. The licence of Grain to Pixel itself does not allow passing the app on.