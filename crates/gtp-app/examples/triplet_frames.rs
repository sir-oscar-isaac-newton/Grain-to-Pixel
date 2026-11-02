//! Checks frame detection on a folder of real camera triplets (P-18): for each triplet it prints the
//! confidence and crop without and with the format's frame window, and writes the hinted renders as
//! JPEGs. Not part of the app. Run in release mode:
//!
//!   cargo run --release -p gtp-app --example triplet_frames -- <folder> <out folder> [base r,g,b]
//!
//! The optional base is the roll's film base (the app takes it from the first frame of the roll).
//! The frame size is learned from the frames the way the app does it, after the first confident
//! measurement; with none, the format's aspect ratio (645) stands in.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use gtp_app::services::triplets::triplet_group;
use gtp_app::{analyze, analyze_with, render, PipelineInput, RenderTarget};
use gtp_core::recipe::OutputSpace;
use gtp_core::FilmFormat;
use gtp_ingest::merge_triplet;
use gtp_io::{encode_jpeg, Limits, RawlerDecoder};
use gtp_negative::FrameHint;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(folder), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("usage: triplet_frames <folder> <out folder> [r,g,b]");
        std::process::exit(2);
    };
    let base: Option<[f32; 3]> = args.get(2).and_then(|text| {
        let v: Vec<f32> = text
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        <[f32; 3]>::try_from(v).ok()
    });
    std::fs::create_dir_all(out)?;

    let mut files: Vec<String> = std::fs::read_dir(folder)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nef")))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    files.sort();
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let candidates = triplet_group(&paths)?;

    let hint = FrameHint {
        aspect: FilmFormat::Mf645.aspect(),
        size: None,
    };
    let mut confident = 0;
    for (n, candidate) in candidates.iter().enumerate() {
        let [r, g, b] = candidate.files.as_slice() else {
            println!("{n:02}: skipped, {} files", candidate.files.len());
            continue;
        };
        let merged = merge_triplet(
            &RawlerDecoder,
            std::path::Path::new(&r.path),
            std::path::Path::new(&g.path),
            std::path::Path::new(&b.path),
            &Limits::default(),
        )?;
        let input = PipelineInput {
            rgb: Arc::new(merged.image),
            ir: None,
            film_base: base,
        };
        let plain = analyze(&input)?;
        let hinted = analyze_with(&input, Some(hint))?;
        let c = hinted.frame.rect;
        if hinted.frame.confidence >= 0.5 {
            confident += 1;
        }
        println!(
            "{n:02}: plain {:.2} | window {:.2} crop x {:.3} y {:.3} w {:.3} h {:.3} | base {:.3?} sure {:.2}",
            plain.frame.confidence, hinted.frame.confidence, c.x, c.y, c.w, c.h, hinted.base.rgb, hinted.base.confidence
        );
        let positive = render(&input, &hinted.auto_recipe, RenderTarget::Full)?;
        encode_jpeg(
            &positive,
            OutputSpace::Srgb,
            90,
            &PathBuf::from(out).join(format!("{n:02}.jpg")),
        )?;
    }
    println!(
        "{confident} of {} confident with the window",
        candidates.len()
    );
    Ok(())
}
