//! Merges a real triplet and renders it to a JPEG so the author can judge whether the positive is
//! plausible (H-05). Not part of the app. Run in release mode:
//!
//!   cargo run --release -p gtp-app --example triplet_preview -- r.NEF g.NEF b.NEF out.jpg
//!
//! `r.NEF` is the exposure lit red, `g.NEF` green, `b.NEF` blue. It prints the warnings and the
//! time each step took. The recipe is the automatic one: crop, film base and levels.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gtp_app::{analyze, render, PipelineInput, RenderTarget};
use gtp_core::recipe::OutputSpace;
use gtp_ingest::merge_triplet;
use gtp_io::{encode_jpeg, Limits, RawlerDecoder};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [r, g, b, out] = args.as_slice() else {
        eprintln!("usage: triplet_preview <red.NEF> <green.NEF> <blue.NEF> <out.jpg>");
        std::process::exit(2);
    };

    let started = Instant::now();
    let merged = merge_triplet(&RawlerDecoder, r, g, b, &Limits::default())?;
    println!("merged in {:?}", started.elapsed());
    for warning in &merged.warnings {
        println!("warning: {warning:?}");
    }
    if merged.warnings.is_empty() {
        println!("no warnings");
    }

    let input = PipelineInput {
        rgb: Arc::new(merged.image),
        ir: None,
        film_base: None,
    };
    let started = Instant::now();
    let analysis = analyze(&input)?;
    println!(
        "analysed in {:?}: frame confidence {:.2}, film base {:?} (confidence {:.2})",
        started.elapsed(),
        analysis.frame.confidence,
        analysis.base.rgb,
        analysis.base.confidence
    );
    let started = Instant::now();
    let positive = render(&input, &analysis.auto_recipe, RenderTarget::Full)?;
    println!("rendered in {:?}", started.elapsed());
    encode_jpeg(&positive, OutputSpace::Srgb, 90, out)?;
    println!("wrote {}", out.display());
    Ok(())
}
