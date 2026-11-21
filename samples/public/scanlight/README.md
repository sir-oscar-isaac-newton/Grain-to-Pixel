# Scanlight packet captures (R-02)

Serial traffic between the `scanlight-probe` spike (`tools/spikes/scanlight-probe`) and a Scanlight V4 rev A (PCB 26a901a, hardware ID 1, firmware ID 0) on macOS, recorded on 2026-11-15. They are test vectors for the packet code: real byte streams, split across reads as the USB stack delivered them.

Each line is one JSON object: `t_ms` (milliseconds since the run started), `dir` (`tx` sent by the probe, `rx` received) and `hex` (the bytes).

| File | Pulses sent | D600 fired (counted by the owner) |
|---|---|---|
| `shutter-mono-adapter-20-to-500ms-run1.jsonl` | 20, 50, 100, 200, 300, 500 ms | all but 20 ms |
| `shutter-mono-adapter-5x100ms.jsonl` | 5 x 100 ms | 5 of 5 |
| `shutter-mono-adapter-20-to-500ms-run2.jsonl` | 20, 50, 100, 200, 300, 500 ms | all but 20 ms |
| `shutter-mono-adapter-10x50ms.jsonl` | 10 x 50 ms | 10 of 10 |
| `shutter-mono-adapter-10x100ms.jsonl` | 10 x 100 ms | 10 of 10 |

The release cord went to the light through a 3.5 mm adapter with a mono male plug; the camera was not on USB. What the packets are and what the runs showed: [docs/research/scanlight-capture.md](../../../docs/research/scanlight-capture.md), section "Retests of 2026-11-15".
