//! Core library for WinFastCleaner: scanning, the NTFS MFT reader, scan
//! snapshots, the safety guard, the rules engine, deletion, and the recovery
//! bin.
//!
//! This crate has no UI code. The CLI and the desktop app are thin layers on
//! top of it, so every safety rule lives here and applies to all of them.

pub mod cleaner;
pub mod error;
pub mod format;
pub mod fsops;
pub mod glob;
pub mod log;
pub mod mft;
pub mod paths;
pub mod platform;
pub mod process;
pub mod progress;
pub mod recovery;
pub mod rules;
pub mod safety;
pub mod scan;
pub mod settings;
pub mod snapshot;
pub mod tree;
pub mod walk;
#[cfg(windows)]
pub mod winsec;

pub use error::{Error, Result};

/// Display name of the app. Kept in one place so it is easy to rename.
pub const APP_NAME: &str = "WinFastCleaner";

/// Folder name used under `%LOCALAPPDATA%` for settings, snapshots, and logs.
pub const APP_DIR_NAME: &str = "WinFastCleaner";
