# nef-probe

The measurement behind `docs/research/nef-decode.md` (R-05). It decodes NEF files with `rawler` and
with LibRaw, compares what they return, and times a bilinear demosaic and the storing of a plane as
a 16-bit TIFF. It is **outside the Cargo workspace** on purpose: it pulls in an LGPL decoder and a
C++ build that must not become part of any build or CI run.

```bash
cd tools/spikes/nef-probe
cargo run --release -- "../../../samples/private/Mamiya 645 Kodak Gold 200 120 Trichrome RGB/DSC_4652.NEF"
```

Always use `--release`; the numbers mean nothing otherwise. On Windows (MSVC) LibRaw only compiles
when the C++ define is set first, in PowerShell:

```powershell
$env:CXXFLAGS = '/DLIBRAW_NODLL'
```