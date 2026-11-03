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
| macOS: device (macOS 27.0.1, Apple M1 Pro, 2026-10-31) | **Listed with no driver setup** (`usb:01-1.4`). `dump` matches Windows: firmware 2.04, 17 INQUIRY pages, `max_frames: 40`, adapter 49, page 01h "36Strip", 4000 dpi, 3946-pixel CCD, 14-bit, focus range 0 to 323. The extra probe of the unlisted page E3h is refused by the scanner, which is expected. |
| macOS: one frame, 2000 dpi, IR | **Works**: 2979 x 1973 px, `frame_1.tiff` plus `frame_1_IR.tiff`, 3 min 18 s end to end with ejection (load and detect 41 s, focus 27 s, metering 1 min 55 s, scan and write under 1 s after metering). F135, 25 frames detected, as on Windows. Two runs metered within 0.4 % of each other (red 208151 vs 208274). nkscan **does not create the output folder**: the first run failed with "No such file or directory" until it was made with `mkdir -p`. The scanner ejects after a scan unless `--no-eject` is given. |

## Spike checklist
- [x] Windows 10/11: device listed? **Yes.** Driver: **WinUSB via Zadig is required**; VueScan's driver gives "incompatible driver".
- [x] macOS: device listed? **Yes**, with no driver setup; a 2000 dpi frame with IR scans to the same RGB plus IR TIFF pair (2026-10-31, macOS 27.0.1, Apple M1 Pro).
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
- **macOS needs nothing installed** beyond nkscan itself: the scanner is used straight away.
- **For H-02 (the nkscan adapter):** create the output folder before calling `scan` (nkscan does not), and pass `--no-eject` when more frames follow.
- **Open:** time `--samples 2/4`; check how much of the top clipping is the rebate.

## The adapter (H-02)

`gtp-ingest::NkscanSource` runs the pinned CLI (`nkscan 0.17.0`; any other version is refused as
`ScannerUnavailable`) and reads what it writes. The choices follow the spike:

- **CLI, not the crate:** the crate's API went async in 0.17. The `ScannerSource` port hides the
  difference, so a later move to the crate changes one file. The program is `GTP_NKSCAN` or `nkscan`
  on `PATH`.
- **One process per scan,** `scan <device> --frames 2,3,4 --dpi N --samples N --lock-ae --no-eject
  [--ir] --basename <folder>/frame`, in a folder the adapter makes (nkscan does not) and removes.
  `--lock-ae` keeps one exposure for the whole range; `--no-eject` leaves the film in, so a roll can be
  scanned in parts. `NkscanSource::eject()` runs `nkscan eject`.
- **Frame by frame:** the folder is watched; when `frame_<n>.tiff` (and `frame_<n>_IR.tiff`) are
  whole and have stopped growing, the frame is read, handed to `on_frame` and its files deleted.
  nkscan numbers files from 1 whatever frames were asked for, so file `n` is the `n`-th frame of the
  range. Cancel kills the process; an error on frame k keeps frames 1 to k-1.
- **Errors:** the program missing, another version, no scanner, a scanner "in use" or "incompatible
  driver" (Windows without WinUSB) are `ScannerUnavailable`; any other failure is `ScanFailed` with
  the last lines nkscan said.
- **Not possible:** `ScanParams.autofocus` has no switch in the CLI; nkscan always focuses each
  frame. There is no way to preview the roll before scanning in what the spike found.

**Tested without the scanner** against `examples/fake_nkscan.rs`, which writes TIFFs the way nkscan
names them. **Tested on the scanner on Windows (2026-11-02)**, see the results below; macOS is still to
do. `nkscan list` prints `Nikon LS-50 ED       usb:PCIROOT(0)#PCI(0102)#PCI(0000)#USBROOT(0)-7.2`
(the name, spaces, then an id with no spaces); `parse_list` splits it at the `usb:` token, and the id
is accepted by `scan`.

### Hardware checklist for H-02 (Windows with WinUSB, and macOS)

1. `cargo run --release -p gtp-ingest --example nkscan_roll -- 1 3 2000` prints the scanner, three
   frames with size, colour range and infrared range, and the time. Paste the output into the table
   below. If the scanner is not found or the device id is refused, paste `nkscan list` too.
2. `nkscan list` output with the scanner attached (the lines under "Attached scanners").
3. Thumbnails, for a possible roll preview: `nkscan scan --thumbnail --frames 1,2,3 --basename t/frame
   --no-eject` in an existing folder `t`: are `frame_<n>_thumbnail.tiff` files written, how big, and
   how long before the first one appears?
4. `nkscan eject` after step 1.

| OS | Frames | dpi | Result | Time |
|---|---|---|---|---|
| Windows 10 | 1 to 3 | 2000 | Through `NkscanSource` (example `nkscan_roll`): three frames of 2979 x 1973, colour 0 to 65535, infrared 92 to 65131, handed over after 175 s, 251 s and 327 s (76 s per frame after the first). Progress 0, 33, 67, 100 %. Exit 0. | 5 min 27 s |
| Windows 10, thumbnails | 1 to 3 | 1000 | `nkscan scan --thumbnail`: three frames of 1489 x 986 (9.0 MB each) in 275 s (65 to 70 s per frame after the first). **One** thumbnail, `frame_1_thumbnail.tiff`, 5153 x 96, 16-bit RGB, 3.0 MB, written 59 s after the start, right after the film is loaded and the frames are detected (36 on this roll), before focusing and metering. | 4 min 35 s |
| macOS | | | | |

**Thumbnails and a roll preview.** nkscan writes one framing thumbnail per scan run (not one per
frame), a 5153 x 96 strip, about a minute after the film is loaded and before any frame is metered.
Whether that strip shows the whole roll or only the holder's window was not looked at yet; if it
shows the roll, a preview of the frames costs about a minute and needs no scan. Otherwise a preview
has to be a low-resolution scan (about 65 s a frame at 1000 dpi, so about 40 minutes for 36 frames).

Two things seen in the log: nkscan reports `detected frames frames=36` as soon as the film is
loaded (the frame count of the roll for free), and a frame is refocused every time (positions 212 to
215).

## The roll preview (H-12)

`ScannerSource::preview(device, cancel)` returns the scanner's framing thumbnail of the whole roll
and the number of frames it found, without scanning a frame. `NkscanSource` runs
`scan --thumbnail --frames 1 --dpi 1000 --lock-ae --no-eject` in a folder of its own, reads the frame
count from the log line `detected frames frames=N`, waits for `frame_1_thumbnail.tiff` to stop
growing, and stops the process; the film stays in. It gives up after 180 s (`PREVIEW_TIMEOUT`) and a
cancel stops it at once.

`gtp-app::services::scan_preview` turns the strip into a positive: the pipeline's automatic base and
levels over the whole strip (no crop; the strip is the roll, rebate and all), rendered at full width
and encoded as a JPEG. The strip comes back whole with the pitch of evenly spaced frames (strip
width divided by the frame count; 143 px on the author's roll), so the contact sheet (H-13) can lay
a grid over it and move the grid without asking the scanner again. The command is
`scan_preview(device_id)`, with `scan_preview_cancel()`; one preview runs at a time.

Run on the author's thumbnail (36 frames, 5149 x 96) the strip is a recognisable positive with a
green cast: it has no colour balance of its own (automatic colour balance is not built yet), which
is enough to see which frames are exposed and how they lie, not to judge colour.

### Hardware checklist for H-12 (Windows and macOS)

1. Run a preview from the app code on the LS-50 with a roll loaded (the scan-preview command, or
   `nkscan scan --thumbnail --frames 1 --dpi 1000 --no-eject --basename t/frame` stopped once
   `t/frame_1_thumbnail.tiff` exists): the frame count and the strip match the roll.
2. **Right after a preview, run a normal scan** (`nkscan_roll 1 3 2000`). nkscan is killed while the
   scanner may be in the middle of focusing; this checks that the next scan still works. Record the
   result below, and whether the scanner needed to be switched off and on.
3. Time from the start to the strip.

| OS | Preview | Scan right after | Time |
|---|---|---|---|
| Windows 10 (2026-11-02) | Through `NkscanSource::preview` (`nkscan_roll preview`): 36 frames, strip 5144 x 96. | **Works**: frame 1 at 1000 dpi (1489 x 986, with infrared) scanned normally right after, 184 s with the film load. Straight after the kill, `nkscan list` showed the scanner as `LS-50 (no answer)` once; the next listing was normal. | 58 s |
| macOS | | | |

Seen once, cause not known: some twenty minutes after a preview that had been stopped by killing
nkscan, the next preview found "load a film strip" (the scanner reported no film) and waited for one
until the timeout; the film had to be put back by hand. A scan started straight after a kill is not
affected (above). If the film is gone after a longer pause, the preview now times out with that
message in its details instead of hanging.
