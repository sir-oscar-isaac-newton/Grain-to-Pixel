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
- **macOS: works.** `ports` finds the Scanlight and decodes its packets; the byte order is big-endian (above). The official web app also connects. The light reports hardware ID 0 (the big scanlight, per the Scanlight README), not the V4 the issue assumed.
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
| ports (macOS) | **Works** (2026-11-03). `/dev/cu.usbmodem1101` (and `tty.`), vendor 2e8a, product 0x000a, manufacturer "Raspberry Pi", product "Pico". FW_VERSION answers in 4 ms: raw `00 01 00 00`, **firmware ID 1, hardware ID 0**, which the Scanlight README lists as the *big scanlight* (a V4 would be 1 or 3). LED_TEMP and VBUS every 200 ms (29.7 °C, about 5.15 V). Big-endian words. |
| colors | macOS: each of red, green and blue lights alone and the light is off afterwards (author's report). |
| temp | macOS. Level 64, 5 x 10 s: 28.3 °C start, 28.4 peak, 27.4 end, VBUS 5159 idle to 5134 mV (no heating). **Level 255, 3 x 30 s (4.5 min):** 24.0 °C start, 29.9 peak, 29.6 end (levelled off, +6 °C), VBUS 5086 mV (-75 mV). |
| shutter | macOS: **no pulse fired the D600** (20 to 500 ms, then 1 to 2.55 s, with manual focus, per the author), with an MC-DC2-to-3.5 mm cable in the camera's DC2/GPS port. The light's own web app does not fire it either. |
| shutter --focus | Not run yet (the focus signal may not exist on this light). |
| settle and levels | Waits for a pulse that fires the camera. |

**Limits proposed from the temperature runs** (for the author to approve; they become settings): warn at 45 °C, abort at 55 °C. The worst case measured is about 30 °C at full power, one colour at a time, so these are safety nets (a stuck-on LED, a hot room: 35 °C plus the 6 °C rise is 41 °C), not limits normal use should meet. A VBUS below about 4.75 V (the lowest a USB port should give) would mean the supply is failing.

**What the shutter result means.** With this cable and port, neither the probe nor the Scanlight's own app fires the D600, so the Scanlight pulse cannot be assumed to work for H-10. Possible causes, none confirmed: the cable's wiring (a Nikon release cord has a focus contact that closes before the release contact, and a light with no focus signal closes only one), the light (hardware ID 0 is the big scanlight, and its shutter output may differ from the V4's), or the camera. The fall-back is to trigger the shutter over USB (digiCamControl on Windows, libgphoto2 on macOS), which R-03 and R-04 test.
