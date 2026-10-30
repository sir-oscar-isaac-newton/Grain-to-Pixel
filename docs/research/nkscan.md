# nkscan (LS-50 driver)

Source: https://github.com/activexray/nkscan (crate: https://docs.rs/crate/nkscan/latest). Decision: [ADR 0003](../../decisions/0003-ls-50-via-nkscan.md).

## Desk research (from public docs; unverified on our hardware)
- Open-source, cross-platform driver + CLI for Nikon Coolscan scanners; Rust crate and PyPI package.
- CLI: `nkscan --list`, `nkscan scan --ir --samples N --lock-ae --format <n> --device <id>`.
- LS-50: tested on Linux in early releases; Windows/macOS status to be confirmed. Strip holder assumed.
- macOS: unsigned binaries; build from source or `xattr -d com.apple.quarantine <binary>`.

## Spike checklist (to fill in)
- [ ] Windows 10/11: device listed? driver/USB setup needed (WinUSB/Zadig)?
- [ ] macOS: device listed?
- [ ] Output format and bit depth; is the IR plane delivered separately?
- [ ] Scan time, resolution options (up to 4000 dpi), multi-sample
- [ ] Crate API stable enough, or use CLI
- [ ] Modified whole-roll LS-50: can nkscan advance to frame N and scan 3+ consecutive frames?
- [ ] SA-30 holder: is the roll adapter (coolscan-mods firmware) recognised by nkscan, and which command advances to frame N?
- [ ] Windows driver: can nkscan reach the scanner with VueScan's driver installed (needed for the firmware flasher), or does it need WinUSB via Zadig?
- Result / recommendation:
