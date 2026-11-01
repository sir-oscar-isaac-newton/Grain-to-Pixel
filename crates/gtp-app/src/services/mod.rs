//! Use-case services over the feature crates. Each is called by a thin Tauri command.

pub mod counters;
pub mod develop;
pub mod diagnostics;
pub mod export;
pub mod import;
pub mod library;
pub mod roll_frame;
pub mod settings;
pub mod tags_search;
pub mod thumbnails;
pub mod verify;

#[cfg(test)]
mod testkit;
