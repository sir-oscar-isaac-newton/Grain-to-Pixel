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

`tools/spikes/scanlight-probe` (outside the workspace, see its README) talks to the Scanlight from the desk research above. The packets, from the control interface document of firmware 1.2 (the byte order of the 16 and 32 bit fields is not given there; the probe prints both readings of the first packets and the recorded packets settle it):

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

**Seen so far.** On the author's Windows machine the device appeared as a "USB Serial Device" on COM5 (vendor ID 2e8a, product ID 000a, which is the Raspberry Pi Pico's USB serial; the probe's first try, without DTR, timed out writing to it with error 121). Whether COM5 is the Scanlight is for `ports` to say.

Hardware checklist for R-02 (Windows; macOS for the first two lines). Each line is ticked in the list at the top by the person who ran it.

1. `ports`: the port that answers, and the firmware and hardware IDs (rev B expected). Paste the output.
2. `colors --level 64`: does each of red, green and blue light alone, and nothing else? Note anything odd.
3. `temp --level 64 --cycles 5 --seconds 10`: paste the summary line; the CSV and the jsonl are written to `probe-out/`.
4. `shutter` and then `shutter --focus`, with the D600 and the remote cable on the 3.5 mm output: which pulses took a picture?
5. `settle --pulse <the shortest length that fired the camera> --colour red`, then `levels` on the five NEFs (copy them to `samples\private`): paste the table.

| Run | Result |
|---|---|
| ports (Windows) | |
| ports (macOS) | |
| colors | |
| temp | |
| shutter | |
| shutter --focus | |
| settle and levels | |
