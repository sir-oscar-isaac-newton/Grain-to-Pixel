# 0004. Guided RGB capture: Scanlight serial control and two camera backends

- Status: Proposed (confirm with the capture spikes in M0)
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
- New Rust dependencies: `serialport`, `notify`, a libgphoto2 binding (LGPL, dynamic).
