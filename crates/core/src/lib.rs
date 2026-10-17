//! Core library for WinFastCleaner: scanning, the NTFS MFT reader, scan
//! snapshots, and (from Phase 2) the rules engine, deletion, and recovery bin.
//!
//! This crate has no UI code. The CLI and the desktop app are thin layers on
//! top of it, so every safety rule lives here and applies to all of them.

pub mod error;
pub mod format;
pub mod mft;
pub mod paths;
pub mod platform;
pub mod progress;
pub mod scan;
pub mod snapshot;
pub mod tree;
pub mod walk;

pub use error::{Error, Result};

/// Display name of the app. Kept in one place so it is easy to rename.
pub const APP_NAME: &str = "WinFastCleaner";

/// Folder name used under `%LOCALAPPDATA%` for settings, snapshots, and logs.
pub const APP_DIR_NAME: &str = "WinFastCleaner";
