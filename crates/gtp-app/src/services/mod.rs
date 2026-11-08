//! Use-case services over the feature crates. Each is called by a thin Tauri command.

pub mod capture;
pub mod capture_control;
pub mod capture_levels;
pub mod counters;
pub mod develop;
pub mod diagnostics;
pub mod export;
pub mod flat_field;
pub mod import;
pub mod import_triplets;
pub mod library;
pub mod roll_balance;
pub mod roll_frame;
pub mod scan;
pub mod scan_preview;
pub mod settings;
pub mod tags_search;
pub mod thumbnails;
pub mod triplets;
pub mod verify;

#[cfg(test)]
mod testkit;
