//! Writes the TypeScript bindings without opening a window.
//! Usage: `cargo run -p grain-to-pixel-desktop --example gen_ipc -- <output.ts>` (see `pnpm gen:ipc`).

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: gen_ipc <output.ts>")?;
    grain_to_pixel_desktop_lib::export_bindings(&out)
}
