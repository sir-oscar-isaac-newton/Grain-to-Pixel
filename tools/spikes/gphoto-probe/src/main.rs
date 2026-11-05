//! R-03: can the `gphoto2` crate find the D600 and take a picture, the way the `gphoto2` command line
//! does? Needs libgphoto2 (macOS: `brew install libgphoto2 pkgconf`), the camera on USB, and the camera
//! free of macOS's own camera helper (see docs/research/scanlight-capture.md).
//!
//! ```text
//! cargo run --release -- list                       the cameras libgphoto2 sees
//! cargo run --release -- capture <file> [--keep]    take a picture, download it to <file>, delete it
//!                                                   from the camera unless --keep
//! ```

use std::error::Error;
use std::path::Path;
use std::time::Instant;

use gphoto2::Context;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => list(),
        Some("capture") => {
            let file = args.get(1).ok_or("usage: capture <file> [--keep]")?;
            capture(Path::new(file), args.iter().any(|a| a == "--keep"))
        }
        _ => Err("usage: list | capture <file> [--keep]".into()),
    }
}

fn list() -> Result<()> {
    let context = Context::new()?;
    let mut found = 0;
    for camera in context.list_cameras().wait()? {
        println!("{} on {}", camera.model, camera.port);
        found += 1;
    }
    println!("{found} camera entries");
    Ok(())
}

fn capture(file: &Path, keep: bool) -> Result<()> {
    let context = Context::new()?;
    let started = Instant::now();
    let camera = context.autodetect_camera().wait()?;
    println!(
        "camera opened after {} ms: {}",
        started.elapsed().as_millis(),
        camera.abilities().model()
    );

    let trigger = Instant::now();
    let remote = camera.capture_image().wait()?;
    let captured = trigger.elapsed();
    println!("picture taken after {} ms: {}/{}", captured.as_millis(), remote.folder(), remote.name());

    let fs = camera.fs();
    fs.download_to(&remote.folder(), &remote.name(), file).wait()?;
    println!(
        "downloaded to {} after {} ms in all",
        file.display(),
        trigger.elapsed().as_millis()
    );
    println!("size: {} bytes", std::fs::metadata(file)?.len());

    if !keep {
        fs.delete_file(&remote.folder(), &remote.name()).wait()?;
        println!("deleted from the camera");
    }
    Ok(())
}
