# Scanlight V4 and camera capture

Decision: [ADR 0004](../../decisions/0004-guided-rgb-capture.md). Spec: [§6.1](../specification.md).

## Desk research (public docs, unverified on our hardware)
- Scanlight: https://github.com/jackw01/scanlight, control interface doc `automation/bsl_control_interface.md` (firmware 1.2). USB CDC serial, binary protocol (`0xFE`, header, length, data). Commands: SET_COLOR (0), GET_FW_VERSION (2), SHUTTER_PULSE (3), SET_FOCUS (7, PCB 26a902b only). Streams LED_TEMP and VBUS every 200 ms. No acks, no VID/PID.
- Shutter output is a 3.5 mm jack; needs a camera-specific cable; Nikon may need the focus signal. Pulse length is set per command.
- libgphoto2 tethering: D600 support not confirmed; macOS may claim the camera (Image Capture / PTP daemon).

## Spike checklist (to fill in)
- [ ] Scanlight PCB revision and firmware ID (`GET_FW_VERSION`)
- [ ] Serial port appears and opens on macOS and Windows 10/11 without extra drivers
- [ ] Each colour lights alone with `SET_COLOR` (save flag 0); switch latency; settle time needed
- [ ] `LED_TEMP` readings during a 3-colour sequence; sensible warn/abort limits
- [ ] D600 remote cable (already owned) on the 3.5 mm output: does it fire the shutter; is `SET_FOCUS` needed (rev B supports it)
- [ ] `SHUTTER_PULSE` length that reliably fires the D600 (default 100 ms)
- [ ] libgphoto2 on Apple Silicon: D600 listed? capture + download NEF? does macOS block USB access (how to release it)?
- [ ] Bundling libgphoto2 and its camera/port driver libraries inside the .app; env paths for driver dirs
- [ ] Windows: digiCamControl sees the D600, downloads NEFs fired by the Scanlight pulse into a session folder (without triggering captures itself); file-complete detection
- [ ] EXIF identical across the three shots; histogram/clipping check works on `rawler` output
- Result / recommendation:

## The R-02 probe

`tools/spikes/scanlight-probe` (outside the workspace, see its README) talks to the Scanlight from the desk research above. The packets, from the control interface document of firmware 1.2 (the byte order of the 16 and 32 bit fields is not given there; **it is big-endian**, measured on a Mac: a VBUS packet `fe 02 04 00 00 14 27` is 5159 mV, which read little-endian would be 655,622,144):

| | Header | Data |
|---|---|---|
| SET_COLOR | 0 | red, green, blue, white, IR (0 to 255 each), save flag. The device limits power on a weak supply, will not run white or IR with RGB, and ignores IR on a v4. **The probe always sends save = 0.** |
| GET_FW_VERSION | 2 | none; the device answers with FW_VERSION |
| SHUTTER_PULSE | 3 | the length in units of 10 ms |
| SET_FOCUS | 7 | the focus signal on or off (PCB rev 26a902b only) |
| LED_TEMP (from the device, every 200 ms) | 1 | 32-bit signed, millidegrees Celsius |
| VBUS (from the device, every 200 ms) | 2 | 32-bit signed, millivolts |
| FW_VERSION (from the device) | 3 | firmware ID (16 bits), hardware ID (16 bits) |

Every packet is `0xFE`, the header, the length of the data, then the data. There is no checksum and no acknowledgement.

**Seen so far.**
- **macOS: works.** `ports` finds the Scanlight and decodes its packets; the byte order is big-endian (above). The official web app also connects. The light reports hardware ID 1, firmware ID 0 (see the results table below); an early reading of hardware ID 0 had the two halves the wrong way round.
- **Windows 10 (this PC): the port opens and stays silent.** The device appears as a "USB Serial Device" on COM5 (vendor ID 2e8a, product ID 000a; the device calls itself "Pico", with the serial interface "Board CDC" and a "Reset" interface: the stock names of the Raspberry Pi Pico SDK, which the V4's RP2040 firmware uses). `listen` for 3 s with DTR held gets 0 bytes, the probe's first try (no DTR) timed out writing (error 121), a later write blocked in the driver until the cable was pulled, and the official automation web app says "Connection Failed" too. So the fault is on the Windows side of this PC, not the probe or the light. Still to try: another USB port (a USB 2.0 one), another cable.

Hardware checklist for R-02 (Windows; macOS for the first two lines). Each line is ticked in the list at the top by the person who ran it.

1. `ports`: the port that answers, and the firmware and hardware IDs (rev B expected). Paste the output.
2. `colors --level 64`: does each of red, green and blue light alone, and nothing else? Note anything odd.
3. `temp --level 64 --cycles 5 --seconds 10`: paste the summary line; the CSV and the jsonl are written to `probe-out/`.
4. `shutter` and then `shutter --focus`, with the D600 and the remote cable on the 3.5 mm output: which pulses took a picture?
5. `settle --pulse <the shortest length that fired the camera> --colour red`, then `levels` on the five NEFs (copy them to `samples\private`): paste the table.

| Run | Result |
|---|---|
| ports (Windows) | The port opens and stays silent (0 bytes in 3 s with DTR held); the official automation web app fails to connect too. Not understood yet; another USB port and cable still to try. |
| ports (macOS) | **Works** (2026-11-03). `/dev/cu.usbmodem1101` (and `tty.`), vendor 2e8a, product 0x000a, manufacturer "Raspberry Pi", product "Pico". FW_VERSION answers in 4 ms: raw `00 01 00 00`, halves 1 and 0: **hardware ID 1 (the V4 PCB 26a901a, rev A, as printed on the board), firmware ID 0** (the document lists the halves the other way round, see below). LED_TEMP and VBUS every 200 ms (29.7 °C, about 5.15 V). Big-endian words. |
| colors | macOS: each of red, green and blue lights alone and the light is off afterwards (author's report). |
| temp | macOS. Level 64, 5 x 10 s: 28.3 °C start, 28.4 peak, 27.4 end, VBUS 5159 idle to 5134 mV (no heating). **Level 255, 3 x 30 s (4.5 min):** 24.0 °C start, 29.9 peak, 29.6 end (levelled off, +6 °C), VBUS 5086 mV (-75 mV). |
| shutter | macOS: **no pulse fired the D600** (20 to 500 ms, then 1 to 2.55 s, with manual focus, per the author), with an MC-DC2-to-3.5 mm cable in the camera's DC2/GPS port. The light's own web app does not fire it either. |
| shutter --focus | Not run yet (the focus signal may not exist on this light). |
| settle and levels | Not run: it needs the camera fired by a pulse. Replaced, see the decision above. |

**Limits proposed from the temperature runs** (for the author to approve; they become settings): warn at 45 °C, abort at 55 °C. The worst case measured is about 30 °C at full power, one colour at a time, so these are safety nets (a stuck-on LED, a hot room: 35 °C plus the 6 °C rise is 41 °C), not limits normal use should meet. A VBUS below about 4.75 V (the lowest a USB port should give) would mean the supply is failing.

**What the shutter result means, and the decision (2026-11-03).** With this cable (an MC-DC2 cord with a 3.5 mm plug, which does fire the camera from the author's own remote) in the D600's DC2/GPS port, neither the probe nor the Scanlight's own web app fires the camera, so the pulse is not something the capture can rely on. **The author's decision: trigger the camera over USB** (digiCamControl on Windows, libgphoto2 on macOS; R-04 and R-03) and keep the Scanlight for the light only. The acceptance criterion "a pulse length that reliably fires the D600" is therefore not met, and is recorded as such.

What the V4 documentation says, for anyone who wants the pulse later: "some cameras (mainly Nikon) may need the focus signal in order to trigger the shutter"; with "send focus signal" selected the focus is held for as long as the LEDs are on; rev B (26a901b) switches the shutter and the focus with isolated photorelays and its jack is isolated from USB, rev A (26a901a) is not isolated (test the wiring before tethering the camera and the light to one computer); and for Nikon on older units the workaround is a mono-to-stereo 3.5 mm adapter whose male end is a two-terminal plug. **The light is a V4 rev A (PCB 26a901a, read off the board by the author).** It reports `00 01 00 00` as its version. The Scanlight README lists hardware ID 1 as the v4 26a901a, so **the first half is the hardware ID (1) and the second the firmware ID (0), the other way round from the control interface document**, which says firmware first. Consequences for this unit: rev A has no focus signal (rev B, 26a901b, has it), so a Nikon that wants focus before release cannot be fired by it, which fits the result above; and its 3.5 mm jack is **not isolated from USB**, so while the camera is tethered by USB the release cable should stay unplugged.

**Retest planned (the author is ordering the part).** The V4 notes give a workaround for units without the focus signal on Nikon cameras: connect the release cord to the light through a 3.5 mm adapter whose **male end is a mono (two-contact) plug** and whose female end is a stereo socket for the cord. A mono plug in a stereo socket connects the socket's middle contact to ground, so the light would hold the camera's focus line asserted and the pulse alone would release the shutter (a reading of the note, not yet tried). When the adapter arrives: the cord in the adapter in the light's jack, the camera in M mode, single release, manual focus, awake and **not on USB** (rev A's jack is not isolated), then `scanlight-probe shutter` and the web app's trigger. If a pulse fires the camera, record the shortest length; the USB route stays the plan either way.

**What this changes elsewhere** (to be proposed as issue edits, not done here): H-10 and R-04 are written around a camera fired by the Scanlight pulse and would instead fire it over USB; the settle test is replaced by a simple check in the capture work (the light on for a second before a USB-triggered shot, compared with a longer wait), because a USB trigger has a delay of its own that is much longer than any LED settle time.

## libgphoto2 on the Mac (R-03)

Results on the author's M1 Pro MacBook with the D600 on USB (2026-11-03 and 04); the camera in M mode, single release, manual focus, RAW, the release cable unplugged. **Hardware items are ticked by the person who ran them.**

| Check | Result |
|---|---|
| Install | `brew install gphoto2` (command line); the Rust probe needs `brew install libgphoto2 pkgconf`. |
| `gphoto2 --auto-detect` | Lists "Nikon DSC D600" **twice** (`usb:001,001` and `usb:000,001`), the same camera. The capture worked with the default, so no `--port` was needed. |
| First capture | **Failed in 0.04 s**: `Could not claim the USB device ... Could not claim interface 0 (No such file or directory)`. |
| What held the camera | macOS's camera helper `ptpcamerad`, plus `mscamerad-xpc`, `icdd` and an open Photos, **in a second user account** that was still logged in (since 16 Oct). The author's own account ran no `ptpcamerad`, so a plain `pkill` could not reach it. |
| Workaround 1 | `while true; do sudo pkill -9 ptpcamerad; done` in a second terminal, then unplug and replug the camera: the capture worked. |
| Workaround 2 (better) | **Log the other account out**, with no kill loop: the capture worked at once. So a Mac with one user logged in needs no workaround. |
| Capture and download | `gphoto2 --capture-image-and-download` takes a picture into the camera's memory (`/capt0000.nef`), saves it on the Mac and deletes it from the camera (the default): **1.9 s, 2.1 s and 1 to 2 s** from trigger to file in three runs, at 4 to 7 % CPU. |
| A triplet over USB | `tools/spikes/scanlight-probe/triplet.sh`: the Scanlight lights red, green and blue in turn (settle 1.5 s), each shot fired over USB: three NEFs in about 15 s. **The first set was black** (the lens cap was on). With the exposure set right, `levels` (mean red, green, blue of the preview) gave, at light levels 44, 22 and 2 for red, green and blue: **red shot 230, 0, 0; green shot 0, 247, 0; blue shot 5, 54, 255**. The blue LED is far brighter than red and green for the camera, so blue needs a level of 2 (the lowest useful) or a faster shutter for the three to balance, and the preview of the blue shot is still clipped. **Settle time: the same set shot with a 1.5 s and with a 0.5 s wait between switching the light on and firing the camera differs by about one step in 255, so 0.5 s is enough** (the camera has a delay of its own). The importer split the first set into 2 + 1: **shots more than 5 s apart (`MAX_SHOT_GAP_MS`) start a new triplet**, and the first script took 5 s a shot. A triplet at 0.5 s settle and 0.2 s between colours takes about 3 s a shot. **Import in the app (macOS, 2026-11-04): the three files import as one triplet with no clipping warning, blue included**, so the whole chain works: Scanlight light, camera fired over USB, NEFs downloaded, triplet import. |
| Rust binding | `tools/spikes/gphoto-probe` (the `gphoto2` crate 3.4.1, `brew install libgphoto2 pkgconf`): **compiled on the first build on the Mac** (written from the crate's documentation) and **works**: `capture test.nef` opened the D600 in 255 ms, took the picture in 1.8 s, downloaded the 26.4 MB NEF (1.81 s in all) and deleted it from the camera. |
| Bundling in the .app | **Works** (`tools/spikes/gphoto-probe/bundle-test.sh`, 2026-11-04): a copy of the probe with its seven libraries, the camera driver `ptp2` and the port driver `usb1`, rewritten to `@loader_path` and ad-hoc signed, took a picture on its own with no path left into Homebrew. Sizes: the libraries 2.2 MB (`libgphoto2.6`, `libgphoto2_port.12`, `libltdl.7`, `libexif.12`, `libusb-1.0.0`, `libjpeg.8`, `libintl.8`), of which the drivers `camlibs` 868 KB and `iolibs` 56 KB. With the two drivers alone copied to a folder and `CAMLIBS` and `IOLIBS` pointing at it, a capture worked too. |

**What this means for the app** (the Mac side of the decision to trigger over USB):

- **The camera claim is the one real problem on macOS.** Another logged-in account, or Photos or Image Capture open, makes the capture fail with `Could not claim the USB device`. The app should recognise that error and say in words what to do (log the other account out, quit Photos and Image Capture), not show the code. A kill loop is not something to ship.
- **A shot takes about 3 s** (settle 0.5 s, trigger to file about 2 s, a moment to switch colours), so a triplet is about 10 s, with the camera on USB and nothing to install but libgphoto2.
- **Licence.** libgphoto2 and the `gphoto2` crate are LGPL-2.1, like rawler: the same notice and rebuild obligations apply when the crate becomes a dependency (a `deny.toml` exception, an entry in `docs/third-party-notices.md`, which `pnpm check:notices` will ask for). Shipping libgphoto2 as dynamic libraries in the .app keeps replacing it simple.
- **The importer's 5 s grouping gap is a decision for the capture work.** Shots from a USB trigger are about 3 s apart, close to the limit; the capture sequence (H-08) should store the three shots directly, where the channels are known, and not through the file grouping, or the gap should be raised.

### Bundling libgphoto2 in the macOS app: approach and go/no-go

**Go for macOS.** About 3 MB, nothing to install for the user.

1. **What goes in the bundle.** The seven dylibs above (the script prints the list it finds, so a Homebrew update that adds one shows), and the two drivers: `ptp2` (every PTP camera, the D600 included) and `usb1` (USB). Other camera brands need no other driver as long as they speak PTP.
2. **Where libgphoto2 looks.** It reads `CAMLIBS` (camera drivers) and `IOLIBS` (port drivers) when a context is created. The app sets both with `std::env::set_var` at start-up, before the first `Context::new`, to the driver folders inside the bundle (for example `Contents/Resources/gphoto/camlibs`). Tested by hand with copies of the drivers in another folder.
3. **Library paths.** Every dylib is copied next to the app's binary (`Contents/Frameworks`) and each dependency is rewritten with `install_name_tool -change` to `@executable_path/../Frameworks/...` (and `@loader_path` between the libraries and from the drivers). `otool -L` over every file shows only system libraries left.
4. **Signing.** Rewriting breaks a file's signature and Apple Silicon refuses to load unsigned code, so every dylib and driver is signed again with the ad-hoc identity (`codesign --force --sign -`), inside out, and the app after them. The app's bundle already uses the ad-hoc identity (`signingIdentity: "-"`); no notarisation is involved.
5. **How it gets into the Tauri build** (not tried yet): a step of the macOS release job, the same as `bundle-test.sh` but on the built `.app`, before signing; or Tauri's `bundle > macOS > frameworks` and `resources` for the files. The macOS build jobs need `brew install libgphoto2` to compile the `tether` feature.
6. **Licence.** All LGPL-2.1, as dynamic libraries: see the amendments to ADR 0004. The notices check does not see dylibs, so they get a manual entry and a source offer.
7. **Risks left.** The Tauri integration of steps 3 to 5; a Homebrew update changing the dylib set (the script is the check); an older macOS than the author's.

## The driver (H-07)

`crates/gtp-capture` holds the `LightSource` port (`light.rs`, as in architecture §3.4 and specification §6.1), the `ScanlightV4` serial driver (`scanlight/`) and the recording `FakeLight` (`fakes.rs`, public like `FixtureSource`, with a `FakeLightHandle` that stays readable after the light is moved or dropped).

- **Packets (`scanlight/protocol.rs`).** `SET_COLOR` (red, green, blue; white and IR 0; **save flag always 0**), `GET_FW_VERSION`, `SHUTTER_PULSE`. No `SET_TRIM` and no `SET_FOCUS` (the light is a rev A). Levels are `u8`; `clamp_level` turns any `i32` into 0 to 255. The decoder reads big-endian words and the version reply as hardware ID, then firmware ID, joins packets split over reads, drops unknown headers and noise, skips a packet with a wrong length one byte at a time, never panics, and holds at most 19 bytes of an unfinished packet.
- **Port (`scanlight/port.rs`).** A small `LightPort` trait (write, read, clone) with a real `serialport` implementation. Every read and write has a 100 ms timeout; **nothing flushes** (on Windows a flush waits for the device to drain, and the R-02 probe hung there); DTR and RTS are raised after opening, as in the probe.
- **Probe.** `GET_FW_VERSION` is sent (again every 0.5 s) and the port gets 2 s to answer. The **hardware ID** is checked: 1 (V4 rev A) and 2 (rev B) are accepted; any other is accepted with a `tracing` warning and `LightInfo::recognised` false. (The issue and specification say "firmware IDs 1 and 2"; with the halves in the measured order those numbers are hardware IDs, and the measured light is hardware 1, firmware 0.) A port that stays silent gives an `AppError` (`Io`, "The port opened but the light did not answer ...") after the timeout. With no port named, USB serial ports are tried, the Pico's vendor ID `2e8a` first and macOS's `tty.` twins skipped; if none answers the error is `NotFound` with each port's reason in the details.
- **Status.** After a successful probe a reader thread keeps `LightStatus` (`temp_c`, `vbus_mv`, `connected`) current from the 200 ms `LED_TEMP` and `VBUS` packets. The warn and abort limits are not in the driver; the sequence (H-08) applies them.
- **Off on every exit.** Dropping the driver sends an all-zero `SET_COLOR` (best effort, bounded by the write timeout) and stops the reader. `FakeLight` records the same switch-off when dropped.

**Windows.** The driver is proven on macOS only. On the author's Windows PC the port opens and never replies, so `connect` fails there with the "did not answer" error after about 2 s instead of hanging. The cause is not diagnosed or worked around here (ADR 0004, amendment 10).

**Test vectors and where they come from.** `samples/public/scanlight/` does not exist, so the tests use this note. VBUS `fe 02 04 00 00 14 27` is **recorded** from the light. The version data `00 01 00 00` is **recorded**; the `fe 03 04` framing around it is hand-assembled. LED_TEMP bytes (the note gives 29.7 °C, not the bytes), and the packets the host sends, are **hand-derived** from the packet table above and marked so in the tests. `SHUTTER_PULSE` fired no camera, so its effect is unconfirmed.

**Hardware checklist for H-07 (macOS; to be ticked by whoever runs it).**

1. `cargo test -p gtp-capture --test scanlight_hardware -- --ignored` with the light on USB: it connects, reads `LED_TEMP` and `VBUS`, lights red at a low level for half a second and switches off.
2. The reported `LightInfo` is port `/dev/cu.usbmodem...`, hardware ID 1, firmware ID 0, `recognised` true.
3. The driver gets no reply from the camera side: no shutter test is needed (the camera is fired over USB).
4. Unplug the cable while connected: the next `set_levels` returns an error and `status().connected` is false.
5. Windows (when a light that answers there exists): the same test; until then `connect` is expected to fail with "did not answer".

## The sequence (H-08)

`gtp-capture` also holds the `CameraSource` port, `FakeCamera` and `CaptureSequence` (`sequence.rs`); `gtp-app` has the job (`services/capture.rs`).

- **Sequence.** Per channel R, G, B: set only that colour's level, wait the settle time in pauses of at most 50 ms (cancel checked between them), check link and temperature, arm, trigger, wait up to 15 s for the file. A `LightGuard` writes all-zero levels when dropped, so the light is off after success, cancel, error, a camera timeout, a temperature abort and a panic; a failing off command is logged and does not replace the original error. The camera's own wait is one blocking call, so a cancel during it is seen when the call returns.
- **Safety.** No temperature reading within about 1 s of the start refuses to start; a reading that disappears, a lost link, or a temperature above `temp_abort_c` aborts; above `temp_warn_c` or a `VBUS` under 4750 mV only warns, on the stored frame.
- **Levels.** The defaults 44, 22 and 2 were measured on the bare light with no film in the gate (the blue was still clipped); they are starting values, to be rebalanced with a test shot for film.
- **Storing.** The three files are staged in a temporary folder (the merge reads EXIF from a file), merged and stored by the same code as an imported triplet, so the EXIF mismatch and clipping warnings and the roll's flat field apply. A captured frame gets no colour balance.

## GphotoCamera (H-09)

`crates/gtp-capture/src/gphoto/` holds the libgphoto2 backend. **The worker builds and is tested on every platform; the one file that touches libgphoto2 (`session.rs`) exists only with the `tether` feature on macOS**, and was written against the source of `gphoto2` 3.4.1 and the verified R-03 probe on a PC that cannot compile it. Its first compile is the macOS CI job.

- **Worker.** One thread owns the `gphoto2` `Context` and `Camera` (nothing from the binding crosses a thread, so `GphotoCamera: Send` holds whatever the crate's types are). `trigger` sends a capture request and returns; the worker takes the picture, downloads the NEF to a temporary folder, reads it, deletes the file from the camera and replies; `wait_for_image(timeout)` waits on that request's own channel. `arm` opens the camera (found when first needed, so one plugged in after the app started works). `is_connected` reads an atomic and never touches the worker.
- **A hung camera.** libgphoto2's capture call cannot be interrupted. A reply that does not come within its timeout (15 s for a picture, from the sequence; 20 s to open, 10 s to list) marks the camera **unusable**: every later call fails at once with words (switch the camera off and on, replug the cable, refresh the camera list, restart the app if that does not help). The stuck thread is never joined; its late result goes to a channel nobody reads and is dropped, so it can never be the next picture; dropping the camera does not wait for it. `camera_list` (the "refresh") starts a new worker. The binding runs every libgphoto2 call on one global thread, so after a truly hung call a new worker's first call also waits and times out: only a restart helps then.
- **Claim error.** `Could not claim the USB device` (the binding's `IoUsbClaim`, or that text in any error) becomes: log out any other user accounts, quit Photos and Image Capture, unplug and replug the camera. The raw text goes to the error's details. No process is killed.
- **Paths and libraries.** `configure_library_paths()` sets `CAMLIBS` and `IOLIBS` to `Contents/Frameworks/gphoto/{camlibs,iolibs}` when the binary runs from an app bundle that has them, first thing in `run()` before any thread exists (Rust 2021: `set_var` is a safe function; the single-threaded start is what makes it sound). A development build (`cargo run`, `tauri dev`) sets nothing and uses Homebrew's libgphoto2 and its compiled-in driver paths. `tools/scripts/release/bundle-libgphoto2.sh bundle` (Tauri `beforeBundleCommand`, from `tauri.macos.conf.json`) copies the seven libraries and the two drivers, rewrites them with `install_name_tool`, signs them ad hoc and stages them for `bundle.macOS.files`; `verify <App.app>` checks the result. The licences and the source offer are in [third-party-notices.md](../third-party-notices.md).

**Hardware checklist for H-09 (macOS; to be ticked by whoever runs it).** Setup once: `brew install libgphoto2 pkgconf`; the D600 in M mode, manual focus, RAW, lens cap off, switched on, on USB; the Scanlight on USB; no other account logged in, Photos and Image Capture closed.

1. `cargo test -p gtp-capture -p gtp-app --features gtp-app/tether` (this is also what CI runs on macOS): all pass.
2. `cargo test -p gtp-capture --features tether --test gphoto_hardware -- --ignored --nocapture` (optional `GTP_SCANLIGHT_PORT=/dev/cu.usbmodem...`, `GTP_GPHOTO_OUT=<folder>`): the light connects, the camera is named, red, green and blue are lit in turn at the default levels 44, 22 and 2 and three pictures arrive (about ten seconds); the three NEF sizes are printed (about 26 MB each) and written to the folder; **all three LEDs are dark afterwards** (look). Record the three sizes and the time.
3. Claim error: log in a second account, open Photos there (or open Image Capture here), plug the camera in again and run the test of step 2. It must fail at the camera with the words about logging out other accounts and quitting Photos and Image Capture, not with `Could not claim the USB device`. The light must stay dark (the camera is opened before anything is lit). Record the message shown.
4. Absent camera: unplug the camera and run step 2. It must fail with "No camera was found..." and the light stays dark. Plug it in again and run once more without restarting anything else: it works (the camera is found when needed).
5. The bundle: `brew install libgphoto2 pkgconf`, then `pnpm --filter @gtp/desktop exec tauri build --bundles app,dmg --features tether`, then `bash tools/scripts/release/bundle-libgphoto2.sh verify "target/release/bundle/macos/Grain to Pixel.app"`: it ends with "carries libgphoto2 and nothing points into Homebrew" and lists seven libraries. Report its output, and whether the build itself ran the script (the "libraries in" lines, and `Contents/Frameworks/gphoto` inside the app) and signed without a complaint about the nested files. Open the app from the dmg: the log says the drivers are taken from the app bundle.
6. The capture in the app: see "Capture view (U-10)" below.

## Capture view (U-10)

The Capture section of the Import view (shown while the `guided_capture` flag is on) is `apps/desktop/src/features/import/CaptureSection.tsx`. Status badges for the light and the camera, a slider per colour for the LED level (the working levels; since H-15 they are kept only by an explicit save, and they never light the light), a test shot per colour with its histogram and a warning above 0.1 % clipped, and the large Next frame button (also the Space key) that runs `capture_triplet`, shows the steps, the stored frame, the roll's frame counter and warnings (warm light, low supply, EXIF mismatch, clipping). Leaving the view calls `light_off`.

**Hardware checklist for U-10 (macOS tether build, `GTP_FAKE_HW` unset; to be ticked by whoever runs it).**

- [ ] Open the Import view and press Connect light: the badge shows the light connected with its temperature, updating about every second.
- [ ] The camera badge names the D600; unplug it and press Look for camera again: "No camera found"; plug it in again and it is named again.
- [ ] A test shot for each colour shows a histogram in that colour and a clipped percentage; a test shot above 0.1 % clipped shows the warning. The light is dark afterwards.
- [ ] The sliders change the working levels and the next test shot reflects them (saving them is H-15, below).
- [ ] Next frame (button) stores a frame in the chosen roll: steps go through red, green and blue, the thumbnail and "Frame N on this roll" appear, and the frame opens in Develop.
- [ ] Next frame with the Space key (focus not in a button, field or slider) does the same; Space during a capture does nothing.
- [ ] Time from pressing Next frame to the stored frame, excluding exposure time (budget 15 s): ______ s.
- [ ] Cancel during a capture stops it and the light is dark.
- [ ] The light is dark after leaving the Import view (look at it) and after closing the app. Also leave the view in the middle of a capture: the capture is cancelled and the light stays dark.
- [ ] If practical: let the light heat past the abort temperature (or cover it): the badge turns to "too hot and switched off", Next frame is off, and Reconnect light brings it back once it has cooled.
- [ ] Warnings: with a too-bright level, the clipping warning names the colour. (An EXIF mismatch cannot be forced on the rig: the camera's controls are locked while tethered, and auto ISO gave the same ISO to all three shots. The app reads ISO, shutter time and aperture from the D600's NEFs, checked on the private samples, and the mismatch warning has a unit test.)

## Saved levels (H-15)

The levels 44, 22 and 2 of ADR 0004 amendment 5 were measured on the bare light with no film in the gate. Film absorbs the three colours very differently, so each stock needs levels of its own. Automatic calibration is H-17; H-15 is the manual method and where its result is kept.

**Method.** Put the clear film base of the stock in the gate (an unexposed leader or the rebate). Move the sliders and take a test shot per colour until each channel's histogram has its peak just below the right edge, with less than 0.1 % clipped. Then save the levels: **Save for {stock}** when the roll has a film stock, or **Save as default**. A save is possible only after a test shot at the levels as they now stand, and keeps that shot's shutter time, ISO and aperture (from its EXIF) with the levels.

**Resolution.** A roll's levels come from its film stock (the stock text of the roll, compared trimmed and without case), then the global default. A roll with no stock, or a stock with no saved levels, uses the default. The sliders start from these when a roll is chosen; changes stay working levels (the line under the sliders says "Changed, not saved") until saved. They live in `settings.json` (`capture.levels` and `saved_levels`), not in the library, so they follow the lamp and the camera.

**What a capture records.** `frame.capture_info` (JSON, null for imported and scanned frames): `{ "v": 1, "levels": [r, g, b], "source": "stock" | "default" | "manual", "stock": name or null, "exposure": [red, green, blue shots, each with exposure_time, iso, f_number] }`. `source` is `stock` or `default` when the levels used equal the roll's saved levels, `manual` otherwise.

**Exposure warning.** If the levels were saved with camera settings and any of the three shots differs in shutter time, ISO or aperture, the capture warns that the camera settings differ from when the levels were saved. It is not raised for manual levels or when either side lacks a setting.

**Hardware checklist for H-15 (macOS tether build, `GTP_FAKE_HW` unset; to be ticked by whoever runs it).**

- [ ] Save as default: set the sliders, take a test shot, press Save as default; the line under the sliders reads "Default levels".
- [ ] Save for a stock: give a roll a film stock (Library, roll editor), choose it in the Import view, change the sliders, take a test shot and press Save for {stock}; the line reads "Levels saved for {stock}".
- [ ] Capture on a roll with that stock uses its levels, and a roll with another stock (or none) uses the default (look at the sliders after choosing each roll, and at the light colours' brightness).
- [ ] The capture records its levels: with the library closed or from another shell, run `sqlite3 <library folder>/library.db "SELECT id, capture_info FROM frame ORDER BY created_at DESC LIMIT 3;"`; the newest frame shows the levels, `"source":"stock"` (or `"default"`) and the three exposures.
- [ ] Change the ISO or the shutter time on the camera between saving and capturing (the controls are locked while tethered: unplug the camera, change it, plug it in again and press Look for camera again): the capture shows the warning "The camera settings differ from when these levels were saved." Put the settings back: no warning.
- [ ] Restart the app: the saved levels (default and per stock) are the same.

## Camera settings (H-16)

The camera's own dials are locked while it is tethered, so the capture view reads and sets the exposure itself. Three libgphoto2 configuration keys, each read as a menu (`RadioWidget`): `iso` (ISO), `shutterspeed` (shutter speed) and `f-number` (aperture). The values are the camera's own words (`"100"`, `"1/100"`, `"8"`) and the choices are its list in its order; both are bounded (at most 200 choices, each at most 32 characters, control characters dropped) because they come from a device.

**Absent and readonly.** A key the driver lacks (libgphoto2 answers "bad parameters" or "not supported") or one that is not a menu is an absent setting: no value, no choices, not writable, and not an error. The view then says "Set on the lens" for the aperture (a lens without electronic contacts has no aperture to set) and "Not available" for the others. A readonly widget (the D600 only lets shutter speed and aperture change in M mode) keeps its value and choices but is not writable: the select is greyed. A real I/O failure is an error in words, with the usual link-loss handling.

**Setting.** The worker reads the settings, refuses a value that is not one of the choices of a writable setting before anything is written, then sets the menu value and writes that single widget, and answers with the settings read again (a change of one setting can alter what another allows). Reading and changing go through the worker like every other call, with a 10 s timeout that marks the camera unusable like the others, and are refused while a capture is in flight and while the app's busy flag is held (a capture or a test shot).

**Saved levels.** One line under the settings says what the roll's saved levels were taken at ("Levels for {stock} were saved at 1/100 s, ISO 100, f/8"). It is not compared live with the settings; the capture's own warning (H-15) still does that.

**Hardware checklist for H-16 (macOS tether build, `GTP_FAKE_HW` unset; to be ticked by whoever runs it).**

- [ ] Run the ignored test, `cargo test -p gtp-capture --features tether --test gphoto_settings_hardware -- --ignored --nocapture`, and record here the name, value, writable flag and choices of the three settings on the D600: ISO ___, shutter speed ___, aperture ___. With `GTP_GPHOTO_SET_ISO=<one of the ISO choices>` it sets the ISO and prints the settings again.
- [ ] The three selects in the capture view show the D600's values and choices.
- [ ] Change the ISO and the shutter speed from the app, take a test shot and check its exposure line, then capture a frame and check the exposures of its `capture_info` (`sqlite3 <library folder>/library.db "SELECT id, capture_info FROM frame ORDER BY created_at DESC LIMIT 1;"`): all three shots carry the new settings.
- [ ] With the camera out of M mode, the shutter speed and aperture selects are greyed (and show their values).
- [ ] With a manual lens (no electronic contacts), the aperture shows "Set on the lens" (only if such a lens is to hand).
- [ ] A change attempted while a capture runs is refused: the selects are greyed during a capture, and the camera keeps its settings.

## Automatic calibration (H-17)

With clear film base framed, `capture_calibrate` takes test shots, one colour at a time, and adjusts that colour's level until the 99.9th percentile of its linear value is 0.90 to 0.97 of full scale (target 0.93), in at most 6 shots per colour. The step is proportional (the light is linear in its level); a clipped shot (0.99 or more) halves the level, or bisects towards the highest level known to be unclipped. It measures the whole frame for now. It fails in words for a colour clipped at level 1, too dark at level 255, or not settled in 6 shots, and warns for a final level below 20. Nothing is saved: the user saves the result with H-15's buttons. The fake hardware (`GTP_FAKE_HW=1`) makes the lit colour's brightness follow the light's level, so a calibration converges there too.

**Hardware checklist for H-17 (macOS tether build, `GTP_FAKE_HW` unset; to be ticked by whoever runs it).**

- [x] With the camera and the light connected and clear film base framed, run the ignored test: `cargo test -p gtp-app --features tether --test calibration_hardware -- --ignored --nocapture` (optional `GTP_SCANLIGHT_PORT`, `GTP_CALIBRATE_LEVELS=r,g,b`); it prints the levels, reached values and shots per colour and leaves the light off.
- [x] Build with `pnpm --filter @gtp/desktop exec tauri build --bundles app,dmg --features tether`.
- [x] Frame clear film base (for example the leader) so that it fills the frame, and make sure no bare light leaks past the film's edge: it is brighter than the base and would pull the levels down.
- [x] Press Calibrate. Record the levels, the reached values and the shots per colour: red ___ / ___ / ___, green ___ / ___ / ___, blue ___ / ___ / ___. Recorded on the D600 with clear film base: levels red 246, green 168, blue 156, no coarse warning. On the bare light (no film) at the same shutter speed: red 151 (0.919, 3 shots), green 29 (0.915, 3 shots), blue 2 (0.886, 1 shot, coarse).
- [x] Take a test shot per colour at the result: the histogram peak sits just below the right edge and the clipped share is about 0.
- [x] Try a too-long exposure (a slow shutter speed): the calibration says a colour is clipped even at level 1 and that the exposure is too long.
- [x] Try a too-short exposure (a fast shutter speed): the calibration says a colour is too dark even at level 255 and that the exposure is too short.
- [x] Save the result for the film stock and check the settings: the sliders read "Levels saved for {stock}" and `settings.json` holds them after a restart.

## Live preview (H-14)

**The test on the D600 (2026-11-08, gphoto2 command line, Grain to Pixel closed).**

- `gphoto2 --auto-detect` lists the D600 twice (`usb:001,001` and `usb:000,001`); both are the same camera.
- `gphoto2 --capture-preview` writes `thumb_preview.jpg` (gphoto2 adds the `thumb_` prefix), 640 × 424. The first frame was black with the Scanlight off; with the light on, a frame from a movie run shows the film clearly enough to frame it and roughly to focus.
- `gphoto2 --capture-movie=10s` took 330 frames in 10 s, about 33 frames per second, about 7.4 KB per frame.
- The camera's own screen stays dark while live view is read over USB.
- `gphoto2 --capture-image-and-download` straight after live view gave a normal NEF (24.6 MB), so leaving live view before a capture works.

Conclusion: live view over USB is usable for framing and rough focus on the D600; the rest of H-14 goes ahead. Fine focus needs a zoomed view, which is not part of H-14. Windows does not get live view in H-14 (it waits for R-04).

**Hardware checklist for H-14** (the author, on the Mac with the D600 and the Scanlight; none of these is ticked until it was done on the real hardware).

- [x] Build the app: `pnpm --filter @gtp/desktop exec tauri build --bundles app,dmg --features tether`, install it and start it with the camera connected and switched on.
- [x] With the light on through the app's own controls, Start live view shows the film and the picture keeps updating.
- [x] The frame rate the view shows is about 30 frames per second.
- [x] Stop leaves live view (the camera's own screen is back to normal and the camera answers the settings again).
- [x] A test shot straight after live view works, and so does a capture (a whole triplet) straight after live view.
- [x] Leaving the capture view stops live view.
- [x] Live view is refused or stopped while a capture runs, and a capture started while live view is on works without stopping it by hand.
- [x] Choose each light colour in the Live view panel: the film shows in that colour, framing and focusing work (green is usually sharpest), Off turns the light off, and starting a test shot turns it off first.
- [x] The Scanlight is not lit by live view itself: with the light off, starting live view leaves it off.

## Focus aids (H-18)

**The zoom test on the D600 (2026-11-08, gphoto2 command line, Grain to Pixel closed, the light on, a negative in the gate).**

- `gphoto2 --list-config` offers `/main/capturesettings/liveviewimagezoomratio` (radio: 0 Entire Display, 2 25%, 4 50%, 6 100%, 7 200%; 1, 3 and 5 are shown as "Unknown value"), `/main/actions/changeafarea` (text, `XxY`, current `0x0`), `/main/capturesettings/liveviewsize` (QVGA, VGA; VGA is set) and `/main/status/liveviewprohibit` (read-only text).
- The zoom and the area work only while live view is on ("Not in Liveview", "changeafarea works only in LiveView mode" otherwise), so they are set in the same session as the frames.
- At 50 %, 100 % and 200 % the frames are magnified, and the grain is visible at 100 % and 200 %. The camera then reports "In enlarged liveview".
- `changeafarea` moves the zoomed area, set before or during the zoom: `4500x3000`, `1000x700` and `100x100` each show a different part of the film. The units are not confirmed (most likely sensor pixels, 6016 x 4016); the app's checklist confirms them by pointing at a spot.
- Once, live view refused to start with `liveviewprohibit` = "Sequence error" after earlier runs; switching the camera off and on cleared it. The app should show that condition in words.
- gphoto2 note: in one run `--filename` applies to every frame, so a time pattern (`z-%H%M%S.jpg`) is needed to keep them apart.

Conclusion: the zoom and moving the zoomed area go ahead in H-18.

**Hardware checklist for H-18** (the author, on the Mac with the D600 and the Scanlight; none of these is ticked until it was done on the real hardware).

- [x] Build the app: `pnpm --filter @gtp/desktop exec tauri build --bundles app,dmg --features tether`, install it and start it with the camera connected and switched on.
- [x] The zoom steps offered are Whole frame, 25 %, 50 %, 100 % and 200 %.
- [x] At 100 % and 200 % the grain shows.
- [x] Clicking a spot on the whole-frame view and then zooming shows that spot (this confirms the units of the zoomed area).
- [x] Peaking outlines the edges, and its sensitivity changes how much it outlines.
- [x] The sharpness number rises to a peak as the focus ring passes best focus, and the best mark holds that peak.
- [x] A test shot, and a capture, straight after a zoomed live view work, and the next live view starts unzoomed.
- [x] With the camera in a state that prohibits live view, the message names the reason. (Neither Bulb nor a camera without a memory card stops the D600's live view, so no refusal could be produced on demand; the message is covered by the worker tests and by the "Sequence error" seen with gphoto2.)
- [x] Change the ISO, the shutter speed and the aperture while live view runs (also zoomed): live view goes on at the whole frame, the camera takes the new setting, and the camera does not freeze in the next minutes.

**Observation during the H-18 checks (2026-11-08).** The D600 froze several times: it went dark and its power switch did nothing until the battery was taken out (a moment in the charger, then back in). It never happened before live view, the battery was not low, and it happened after **a camera setting (ISO, shutter speed or aperture) was changed while live view was on**; the camera then froze while it sat idle. Fix: the camera worker leaves live view (and its zoom) before it changes a setting, as it already did before a capture, and the next frame starts live view again at the whole frame; the Live view panel goes back to the whole frame with it. A dummy battery on mains power (EP-5B type) is on order for long sessions anyway.

## Focus check and flatness (H-19)

The focus check is one real picture taken with the chosen light colour and level. The app decodes the lit channel at full sensor resolution, shows a 1:1 crop of its middle (512 x 512 pixels) with a sharpness number, and measures a 3 x 3 map of sharpness over the whole frame. The number is the variance of the Laplacian of the linear plane divided by the square of its mean, so it is comparable between shots of the same scene and light, not between scenes. The map says flat, bowed (middle sharp, edges evenly softer), tilted (one side or corner softer) or unclear; a map saved from a flat target is the reference that is divided out of later maps, which removes the lens's own field curvature. The map is fair only on an evenly textured frame (grain in an evenly exposed area, or a flat target such as a printed fine pattern or a graticule slide), and a wide aperture (f/4 to f/5.6) shows the differences best. Thresholds are in `crates/gtp-app/src/services/focus.rs`.

**Hardware checklist for H-19** (the author, on the Mac with the D600 and the Scanlight; none of these is ticked until it was done on the real hardware).

- [x] Build the app: `pnpm --filter @gtp/desktop exec tauri build --bundles app,dmg --features tether`, install it and start it with the camera connected and switched on.
- [x] Focus check with green shows a 1:1 crop with visible grain and a sharpness number, and the light is off afterwards.
- [x] Turning the focus ring changes the number, and the best focus gives the highest.
- [x] With a flat target (a printed fine pattern or a graticule slide) at a wide aperture (f/4 to f/5.6) the map is close to even; then Save as flat reference.
- [x] With film in the holder the map and the hint show the film's shape, against the reference (try a deliberately bowed strip if possible).
- [x] Clearing the reference works: the next map is no longer against it.
- [x] A focus check is refused while a capture runs.

**Measured on the D600 (2026-11-08), after the sharpness fix.** Green, film in the holder: 47.4 in focus, 10 with the focus ring clearly off, so the number follows focus by a factor of about 5. One check takes about 2 seconds, from the press to the result (one shot, the download, the decode and one pass over the 24 MP plane). Before the fix every check read 0.0: the unscaled ratio of a real picture is about 0.0001 to 0.003, and the panel shows one decimal.

## Calibration area (H-20)

The calibration of H-17 measures the whole frame unless it is given an area. The live view at the whole frame lets the user drag a rectangle on the picture: the app keeps it as fractions of the frame (so the live view's 640 x 424 size does not matter), draws it as an outline, and passes it to the calibration, whose result says whether it measured the area or the whole frame. A click without a drag still aims the zoom. The area is kept while the capture view is open and is not saved.

**Hardware checklist for H-20** (the author, on the Mac with the D600 and the Scanlight; none of these is ticked until it was done on the real hardware).

- [ ] Build the app: `pnpm --filter @gtp/desktop exec tauri build --bundles app,dmg --features tether`, install it and start it with the camera connected and switched on.
- [ ] Drag an area on the whole-frame live view: the outline shows, and a click without a drag still aims the zoom.
- [ ] Calibrate with the area over the rebate between two frames: record the levels; they are close to a whole-frame calibration on a leader (record both).
- [ ] Calibrate with the area over image content: the levels differ.
- [ ] Clearing the area returns the calibration to the whole frame.
