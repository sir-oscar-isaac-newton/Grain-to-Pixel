# scanlight-probe (R-02)

Measures the Scanlight V4 over USB serial: which port it is, its firmware, that each colour lights alone, how warm it gets, and which shutter pulse fires the D600. The packets are in [docs/research/scanlight-capture.md](../../../docs/research/scanlight-capture.md), section "The R-02 probe"; this tool only talks to the device, and the hardware items of the checklist there are ticked by the person who ran them.

Outside the workspace on purpose, like `nef-probe` and `nkscan-probe`: it is a throwaway tool and the app does not depend on it. The packet module (`src/packet.rs`) has unit tests and is meant to be reused by the real driver (H-07).

**Safety.** The save flag of SET_COLOR is always 0 (there is no way to ask for 1: the levels are never written to the device's memory). The default level is 64 of 255. Every command ends with all channels off, also on Ctrl-C and on errors, and `temp` stops by itself above `--abort-c` (60 °C).

```bash
cd tools/spikes/scanlight-probe
cargo run --release -- ports                     # which port answers, firmware and hardware IDs
cargo run --release -- colors --level 64         # each of red, green, blue alone for 3 s
cargo run --release -- temp --level 64 --cycles 5 --seconds 10
cargo run --release -- shutter                   # pulses of 20 to 500 ms, 3 s apart: which fire the camera?
cargo run --release -- shutter --focus           # the same with the focus signal (rev 26a902b only)
cargo run --release -- settle --pulse 10 --colour red --delays 0,50,100,200,500
cargo run --release -- levels shot1.NEF shot2.NEF shot3.NEF shot4.NEF shot5.NEF
```

`--port COM5` names the port; without it the probe uses the first that answers GET_FW_VERSION. Every run that talks to the device writes the packets it sent and received, with times, to `probe-out/scanlight-<command>-<time>.jsonl` (and `temp` a CSV): paste or copy them back, and the good ones become the test vectors in `samples/public/scanlight/`.

On Windows the device needs no driver (USB CDC). The probe holds DTR while it talks, because a USB CDC device often accepts data only then.