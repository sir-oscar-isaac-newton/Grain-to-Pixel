//! Prints the layout of a TIFF file (size, bits, channels, compression, extra samples) without
//! decoding its pixels. Use it to see how other software writes its scans:
//!
//! ```text
//! cargo run -p gtp-io --example tiff_info -- path/to/scan.tif
//! ```

use std::error::Error;
use std::io::Write;
use std::path::PathBuf;

use gtp_io::inspect_tiff;

fn main() -> Result<(), Box<dyn Error>> {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        return Err("usage: tiff_info <file.tif>".into());
    };
    let layout = inspect_tiff(&path).map_err(|err| {
        format!(
            "{:?}: {} {}",
            err.code,
            err.message,
            err.details.unwrap_or_default()
        )
    })?;
    writeln!(std::io::stdout(), "{layout}")?;
    Ok(())
}
