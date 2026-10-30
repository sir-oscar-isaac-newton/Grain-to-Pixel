# nkscan (LS-50 driver)

Source: https://github.com/activexray/nkscan (crate: https://docs.rs/crate/nkscan/latest). Decision: [ADR 0003](../../decisions/0003-ls-50-via-nkscan.md).

## Desk research (from public docs)
- Open-source, cross-platform driver + CLI for Nikon Coolscan scanners; Rust crate and PyPI package. Version tested: **0.17.0** (MIT OR Apache-2.0).
- Written from Nikon's wire-protocol spec; reads the scanner's advertised capabilities instead of hard-coding models. Its README lists the **Coolscan V (LS-50 ED) as supported and tested on real hardware with SA-21, IA-20/21, MA-20/21, SA-30 and SF-210/200**.
- USB goes through `nusb` (pure Rust, libusb-like). On Windows the device must use the **WinUSB** driver (Zadig); macOS "should just work".
- CLI (0.17): subcommands `list`, `scan`, `dump` (read-only INQUIRY pages), `eject`. `scan` options include `--frames`, `--ir`, `--clean` (IR dust removal), `--lock-ae`, `--samples`, `--dpi`, `--film negative|positive|kodachrome|mono`, `--no-eject`, `--basename`.
- Output: `<basename>_<n>.tiff` and `<basename>_<n>_IR.tiff`, tagged with an ICC profile derived from Nikon Scan 4's (gamma composed into the profile curves). The **pixels themselves are linear** (`src/bin/nkscan/io.rs`: "the samples are linear").
- 0.17.0 made the Rust API `async` (blocking natively via `pollster::block_on`); the CLI did not change.

## Spike results (2026-10-30, Windows 10 x64, the author's LS-50)

Hardware: LS-50 with coolscan-mods firmware and an SA-21 jumpered as SA-30 (whole-roll). Installed with `cargo install nkscan --version 0.17.0 --features cli --locked --root .cache/nkscan` (project-local, git-ignored).

| Check | Result |
|---|---|
| Device on Windows with VueScan's driver (Hamrick Software, `oem53.inf`) | `nkscan list` shows `LS-50 (in use)`; `nkscan dump` fails with **"incompatible driver is installed for this device"**. VueScan was not running. nkscan cannot use VueScan's driver. |
| Device on Windows with WinUSB (Zadig, `oem62.inf`, libwdi) | Listed as **Nikon LS-50 ED**, firmware **2.04**. `dump` reads 17 INQUIRY pages. |
| Holder | Scanner reports **`max_frames: 40`** (the SA-30 roll adapter; the SA-21 strip holder carries 6) and adapter id 49; page 01h names it "36Strip". **The modded adapter is recognised as a roll adapter.** |
| Capabilities | 4000 dpi optical (90 to 4000), 3946-pixel CCD line, 14-bit ADC, RGB plus a 4th component (infrared), autofocus, exposure per channel. |
| Load and roll detection | `scan` loads the film (about 42 s), detects **35 mm (F135, frame length 5959 lines)** and **25 frames** on the test roll. |
| One frame, 4000 dpi, IR | **5959 x 3946 px**, 4.6 min end to end (load, detect, focus, meter, scan). Files: 16-bit RGB TIFF (141 MB, uncompressed) and a separate 16-bit IR TIFF (47 MB). |
| Three consecutive frames (2, 3, 4), 2000 dpi, IR, `--lock-ae` | **Works**: 2979 x 1973 px each, 6.5 min. Each frame refocused (positions 213, 219, 218), the locked exposure was reused identically, and the three images are distinct (grid correlation 0.20 to 0.50 between them, 0.32 to 0.55 against frame 1). nkscan numbers its output files from 1 regardless of the frame numbers asked for. |
| Eject | `nkscan eject`: 12 s. |
| File layout (`gtp-io` `tiff_info`) | RGB: 16 bit x 3, photometric RGB, uncompressed, **no ExtraSamples tag**. IR: 16-bit gray. `gtp-io::read_tiff16` reads both (RGB values 8 to 65535, a few samples clipped at the top; IR 32 to 57563). |
| Driver revert | Device Manager roll back: back to "Nikon LS-50" on `oem53.inf` (Hamrick Software); the libwdi package `oem62.inf` stays in the driver store until removed with `pnputil /delete-driver oem62.inf`. |
| macOS | **Not tested by us yet.** The author reports the LS-50 working on macOS with nkscan through Negpy. |

## Spike checklist
- [x] Windows 10/11: device listed? **Yes.** Driver: **WinUSB via Zadig is required**; VueScan's driver gives "incompatible driver".
- [ ] macOS: device listed? (author-reported working via Negpy; to confirm with the CLI on the Mac)
- [x] Output format and bit depth: 16-bit linear RGB TIFF plus a separate 16-bit IR TIFF per frame.
- [x] Scan time and resolution: 4000 dpi single frame with IR 4.6 min end to end; 2000 dpi about 1.6 min per frame after the first. Multi-sample (`--samples`) not timed.
- [ ] Crate API stable enough, or use CLI: the CLI is stable; the crate API had a breaking change in 0.17 (now async). Decide in H-01.
- [x] Modified whole-roll LS-50: advance to frame N and scan 3+ consecutive frames: **yes** (frames 2 to 4).
- [x] SA-30 holder recognised: **yes** (`max_frames: 40`); `--frames` selects frames.
- [x] Windows driver with VueScan's driver installed: **no**; nkscan needs WinUSB. The firmware flasher needs VueScan's driver, so Windows users switch with Zadig and switch back with Roll Back Driver.

## Result and recommendation
- **Use nkscan for the LS-50.** It recognises the modded roll adapter, advances across consecutive frames, and writes exactly what the library stores (linear 16-bit RGB plus a separate IR plane).
- **Windows needs WinUSB.** The app cannot switch drivers itself; the scanner setup docs must explain Zadig and the roll-back. Users who prefer VueScan's driver can keep importing VueScan or Nikon Scan TIFFs instead.
- **Integration:** start with the pinned CLI (stable interface, progress on stderr); move to the crate when its async API settles. Pin the exact version.
- **For the importer (L/H issues):** nkscan names the IR file `<base>_<n>_IR.tiff` (upper case, `.tiff`); the import pairing rule must accept it, not only `*_ir.tif`.
- **Open:** confirm on macOS with the CLI; time `--samples 2/4`; check how much of the top clipping is the rebate.
