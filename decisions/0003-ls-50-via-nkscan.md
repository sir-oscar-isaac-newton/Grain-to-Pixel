# 0003. Drive the LS-50 with nkscan

- Status: Proposed (confirm with a hardware spike)
- Date: 2026-10-29

## Context

The Nikon Coolscan LS-50 (Coolscan V) must work on Windows 10/11 and macOS. Nikon Scan 4 does not run on current OSes. [nkscan](https://github.com/activexray/nkscan) is an open-source, cross-platform driver and CLI for Nikon Coolscan scanners, published on crates.io as a Rust crate (and on PyPI). It supports IR capture (`--ir`) and device selection (`--device`, `--list`).

Caveats found in its docs (they change quickly between releases, so re-check the current README):
- The LS-50 was first listed as tested on Linux only; Windows was untested.
- Roll transport, multi-sample readout and metering are described as inferred, not proven. `--lock-wb` is unavailable on the LS-50.
- macOS binaries are unsigned (Gatekeeper); build from source or clear the quarantine flag.

## Decision

`gtp-ingest` gets a `ScannerSource` trait with an nkscan-backed implementation. Prefer depending on the `nkscan` crate directly (same language as the core); fall back to spawning the CLI if the library API is unstable. Pin an exact version.

## Consequences

- No driver work of our own; the unknown shrinks to "does nkscan work on this LS-50 on Win/mac".
- First task is a hardware spike: list the device, scan a strip with `--ir`, confirm 16-bit RGB + IR output reaches `gtp-io` intact. Record results in `docs/research/nkscan.md`.
- The author's LS-50 runs [coolscan-mods](https://github.com/kosma/coolscan-mods) firmware and an SA-21 jumpered as an SA-30, so it presents a roll adapter (up to 40 frames); validated in Nikon Scan 4, VueScan and Negpy. nkscan's docs call roll transport inferred and assume a strip holder, so the spike must prove SA-30 recognition and multi-frame advance on this hardware. If it fails, either contribute SA-30 support to nkscan or rely on importing TIFFs from Nikon Scan 4 / VueScan.
- The firmware flasher needs VueScan's USB drivers on Windows (x64). The spike must find out whether nkscan can use the scanner with that driver installed, or needs WinUSB (Zadig).
- Windows ARM is out of scope for the MVP (no test device).
- If nkscan falls short, the trait lets us swap in another backend (e.g. SANE `coolscan3`) or accept pre-scanned TIFFs via the file-import adapter.
