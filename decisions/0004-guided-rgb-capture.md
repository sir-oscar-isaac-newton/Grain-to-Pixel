# 0004. Guided RGB capture: Scanlight serial control and two camera backends

- Status: Accepted for macOS after the spikes R-02 and R-03 (2026-11-04); the Windows capture path is still to be confirmed (R-04, and the Scanlight's serial port on Windows)
- Date: 2026-10-29

## Context

RGB triplets are shot with a Nikon D600 (NEF) under Jack's Scanlight V4 narrowband R/G/B light, in R, G, B order with equal exposure times. The Scanlight is normally driven by its own web app, which also fires the shutter. The project wants one app that does the whole capture and conversion, including tethering the camera.

Facts from the Scanlight repository (https://github.com/jackw01/scanlight, `automation/bsl_control_interface.md`, firmware 1.2; MIT-licensed software and firmware):
- USB CDC serial, binary packets: `0xFE`, header, length, data.
- Host commands: `SET_COLOR` (R, G, B, W, IR plus a save flag), `GET_FW_VERSION`, `SHUTTER_PULSE` (10 ms units), `SET_FOCUS` (PCB 26a902b only), `DFU_MODE`.
- Device packets: `LED_TEMP` and `VBUS` every 200 ms. No acknowledgements, no VID/PID defined, no sequence command: sequencing is the host's job.
- Camera trigger is a 3.5 mm output that needs a user-supplied camera-specific cable; Nikon may need the focus signal.

libgphoto2 gives USB (PTP) tethering on macOS; Windows support is poor. D600 support is not confirmed by what we found.

## Decision

- New crate `gtp-capture` with traits `LightSource` and `CameraSource`, an orchestrator that runs the per-frame R, G, B sequence, and fakes for tests. See specification §6.1.
- `ScanlightV4` implements `LightSource` over the `serialport` crate. SET_COLOR is always sent with save flag 0; no trim, no presets; light off on every exit path; temperature limits.
- Two camera backends behind one trait: `GphotoCamera` (libgphoto2, macOS, Cargo feature `tether`; **the primary, developed first**) and `WatchedFolder` (Scanlight shutter pulse through the author's D600 remote cable, plus a folder that digiCamControl writes to on Windows; also the macOS fallback).
- The Scanlight is PCB rev B (26a902b), so `SET_FOCUS` is available.
- The app does not control exposure or focus on the camera.

## Consequences

- One app covers capture, merge, convert and archive; the Scanlight's web app is only needed for firmware updates and presets.
- Open-loop light control means correctness is verified by the files arriving, with timeouts and clear errors.
- Two backends double the capture test matrix; the fakes keep that manageable. If libgphoto2 proves hard to bundle or the D600 is unsupported, ship `WatchedFolder` only (the M6 cut line).
- The Windows path depends on digiCamControl (third-party; D600 support to be confirmed) and the remote cable.
- New Rust dependencies: `serialport`, `notify`, a libgphoto2 binding (LGPL, compiled into the macOS app; the libgphoto2 libraries it loads are dynamic).

## Amendments from the spikes (R-02, R-03; 2026-11-03 and 04)

What the spikes measured on the author's hardware, and what changes in the decision above. Details and numbers: [docs/research/scanlight-capture.md](../docs/research/scanlight-capture.md).

1. **The light is a V4 rev A, PCB 26a901a** (read off the board; hardware ID 1), not rev B. It has **no `SET_FOCUS`**, and its 3.5 mm jack is **not isolated from USB** (the V4 notes say not to tether the camera and the light to one computer through the release cable). The line "The Scanlight is PCB rev B (26a902b), so `SET_FOCUS` is available" is withdrawn.
2. **The camera is triggered over USB, not by the Scanlight's shutter pulse.** With the author's MC-DC2-to-3.5 mm cord in the D600's DC2/GPS port (the cord does fire the camera from the author's own remote), neither `SHUTTER_PULSE` nor the Scanlight's own web app fired the shutter. The author decided to trigger over USB. **`GphotoCamera` (macOS, libgphoto2) is verified**: the D600 is found, and a picture is taken and its 26 MB NEF downloaded in 1.8 to 2.1 s. **`WatchedFolder` on Windows changes** from "the Scanlight pulse fires the camera" to "digiCamControl fires it and writes the NEF to the folder" (R-04 tests it); the release cable stays unplugged, and the Scanlight's shutter output is not used.
3. **Wire facts.** The 16 and 32 bit fields are **big-endian** (a VBUS packet `00 00 14 27` is 5159 mV). The two halves of `FW_VERSION` are the **hardware ID first, then the firmware ID**, the other way round from the control interface document. The device answers `GET_FW_VERSION` in about 4 ms and sends `LED_TEMP` and `VBUS` every 200 ms by itself.
4. **Limits (settings, with these defaults).** Warn at 45 °C and abort at 55 °C: at full power, one colour at a time for 4.5 minutes, the light levelled off at about 30 °C (24 °C room), so these are safety nets. A `VBUS` below about 4.75 V warns that the supply is failing (it sagged from 5160 to 5086 mV at full power).
5. **Settle time and levels.** A **0.5 s** wait between switching the colour on and firing the camera gives the same pictures as 1.5 s (within one step in 255): the default is 0.5 s, as a setting. The three LEDs need **very different levels for one exposure** (red 44, green 22, blue 2 of 255 at the author's camera settings, with the blue preview still clipped at the lowest level): the capture profile has a level per colour, not one level.
6. **Cadence and the importer.** A shot takes about 3 s (settle 0.5 s, trigger to file about 2 s), a triplet about 10 s. The file importer starts a new triplet when two shots are more than 5 s apart, so the capture sequence stores the three shots **directly**, where the channels are known, not through the file grouping.
7. **macOS camera claim.** libgphoto2 cannot claim the camera while macOS's own helper (`ptpcamerad`) holds it, which happened because **a second logged-in user account** had Photos open; with that account logged out no workaround was needed. The app recognises the error (`Could not claim the USB device`) and says in words what to do (log other accounts out, quit Photos and Image Capture); it does not ship a kill loop.
8. **Bundling libgphoto2 on macOS: go.** A copy of the gphoto probe with its seven libraries (libgphoto2, libgphoto2_port, libltdl, libexif, libusb, libjpeg, libintl; 2.2 MB with the drivers), `install_name_tool` rewrites to `@loader_path` and ad-hoc signing, and **only two drivers**, the camera driver `ptp2` and the USB port driver `usb1` (about 0.9 MB), took a picture on its own with no path left into Homebrew. libgphoto2 finds its drivers through the `CAMLIBS` and `IOLIBS` environment variables, so the app sets both, before the first context, to folders inside the bundle. The M6 cut line stays as written; its macOS side is cleared.
9. **Licences.** libgphoto2, libgphoto2_port, libexif, libltdl and libusb are LGPL-2.1 as dynamic libraries, which keeps replacing them simple. The bundle also ships libjpeg (IJG, Zlib and BSD-3-Clause, not LGPL) and libintl (LGPL-2.1). The `gphoto2` and `libgphoto2_sys` crates are LGPL-2.1-only and compiled statically into the macOS app, so they are treated as `rawler` is: the source of the pinned versions is attached to each release, and `docs/rebuild-with-rawler.md` gives a rebuild path with a Cargo `[patch]` entry. `pnpm check:notices` covers Rust crates and npm packages, not dylibs, so the bundled libraries need a manual entry in `docs/third-party-notices.md` and a source offer, as rawler has.
10. **Not settled.** On the author's Windows PC the Scanlight's serial port opens but stays silent (the official web app fails there too, on a USB 2.0 port as well); it works on the Mac. The Windows capture path needs a Scanlight that answers on Windows before H-07 can be finished there.
