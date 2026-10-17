//! Core library for WinFastCleaner: scanning, the NTFS MFT reader, scan
//! snapshots, the safety guard, the rules engine, deletion, the recovery
//! bin, the large/old and duplicate file finders, "What grew", privacy
//! cleaning, the startup manager, app updates through winget, and the
//! health check.
//!
//! This crate has no UI code. The CLI and the desktop app are thin layers on
//! top of it, so every safety rule lives here and applies to all of them.

pub mod cleaner;
pub mod dupes;
pub mod error;
pub mod filetypes;
pub mod finder;
pub mod format;
pub mod fsops;
pub mod glob;
pub mod grew;
pub mod health;
pub mod integration;
pub mod log;
pub mod mft;
pub mod monitor;
pub mod paths;
pub mod platform;
pub mod privacy;
pub mod process;
pub mod progress;
pub mod recovery;
pub mod rules;
pub mod safety;
pub mod scan;
pub mod schedule;
pub mod services;
pub mod settings;
pub mod snapshot;
pub mod startup;
pub mod tree;
pub mod walk;
#[cfg(windows)]
pub mod wincom;
pub mod winget;
#[cfg(windows)]
pub mod winreg;
#[cfg(windows)]
pub mod winsec;

pub use error::{Error, Result};

/// Display name of the app. Kept in one place so it is easy to rename.
pub const APP_NAME: &str = "WinFastCleaner";

/// Folder name used under `%LOCALAPPDATA%` for settings, snapshots, and logs.
pub const APP_DIR_NAME: &str = "WinFastCleaner";
