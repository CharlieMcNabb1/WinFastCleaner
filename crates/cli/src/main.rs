//! `wfc`: command-line interface over `cleaner-core`.
//!
//! `drives` and `scan` are read-only. `clean` always shows what it will do
//! first (`--dry-run` stops there) and asks before changing anything.

mod clean;
mod files;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;

use cleaner_core::format::{format_size, percent};
use cleaner_core::platform;
use cleaner_core::progress::{CancelToken, ScanPhase, ScanProgress};
use cleaner_core::scan::{Scan, ScanOptions, ScanSummary, scan};
use cleaner_core::snapshot;
use cleaner_core::tree::{NodeId, Tree, flags};

#[derive(Parser, Debug)]
#[command(
    name = "wfc",
    version,
    about = "WinFastCleaner command-line tool",
    long_about = "WinFastCleaner command-line tool.\n\nScanning never changes anything. \
                  Cleaning shows exactly what it will do and asks first; protected \
                  Windows files are never touched. No data ever leaves this machine."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List drives with their size and free space.
    Drives {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Scan a drive or folder and show what takes up space.
    Scan {
        /// Drive or folder to scan, for example C:\ or D:\Projects.
        path: PathBuf,
        /// Number of entries to show per folder.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=10_000))]
        top: u32,
        /// How many folder levels to show.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=32))]
        depth: u32,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Never use the fast MFT reader; always walk folders.
        #[arg(long)]
        no_mft: bool,
        /// Don't save a scan snapshot (used later by "What grew").
        #[arg(long)]
        no_snapshot: bool,
        /// Don't show progress.
        #[arg(long, short)]
        quiet: bool,
    },
    /// List or check cleaning rules.
    #[command(subcommand)]
    Rules(clean::RulesCommand),
    /// Clean junk found by the rules (safe items by default).
    Clean(clean::CleanArgs),
    /// Look into, restore from, or empty the recovery bin.
    #[command(subcommand)]
    Recover(clean::RecoverCommand),
    /// List large or old files (read-only).
    #[command(subcommand)]
    Find(files::FindCommand),
    /// List files with identical contents (read-only).
    Dupes(files::DupesArgs),
    /// Show which folders grew or shrank between two scans.
    Grew(files::GrewArgs),
    /// List saved scan snapshots.
    Snapshots {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Drives { json } => drives(json),
        Command::Scan {
            path,
            top,
            depth,
            json,
            no_mft,
            no_snapshot,
            quiet,
        } => run_scan(ScanArgs {
            path,
            top: top as usize,
            depth: depth as usize,
            json,
            allow_mft: !no_mft,
            snapshot: !no_snapshot,
            quiet,
        }),
        Command::Rules(cmd) => clean::rules_cmd(cmd),
        Command::Clean(args) => clean::clean_cmd(args),
        Command::Recover(cmd) => clean::recover_cmd(cmd),
        Command::Find(cmd) => files::find_cmd(cmd),
        Command::Dupes(args) => files::dupes_cmd(args),
        Command::Grew(args) => files::grew_cmd(args),
        Command::Snapshots { json } => files::snapshots_cmd(json),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) type CliResult = Result<(), Box<dyn std::error::Error>>;

fn drives(json: bool) -> CliResult {
    let drives = platform::drives();
    if json {
        println!("{}", serde_json::to_string_pretty(&drives)?);
        return Ok(());
    }
    println!(
        "{:<6} {:<20} {:<6} {:>10} {:>10} {:>6}",
        "DRIVE", "LABEL", "FS", "SIZE", "FREE", "FREE%"
    );
    for d in drives {
        println!(
            "{:<6} {:<20} {:<6} {:>10} {:>10} {:>5.1}%{}",
            d.root,
            truncate(&d.label, 20),
            d.file_system,
            format_size(d.total),
            format_size(d.free),
            percent(d.free, d.total),
            if d.is_system { "  (system)" } else { "" }
        );
    }
    Ok(())
}

struct ScanArgs {
    path: PathBuf,
    top: usize,
    depth: usize,
    json: bool,
    allow_mft: bool,
    snapshot: bool,
    quiet: bool,
}

fn run_scan(args: ScanArgs) -> CliResult {
    let show_progress = !args.quiet && std::io::stderr().is_terminal();
    let progress = |p: &ScanProgress| {
        if show_progress {
            let what = match p.phase {
                ScanPhase::ReadingMft if p.total > 0 => {
                    format!("Reading MFT {:.0}%", percent(p.done, p.total))
                }
                ScanPhase::Building => "Building tree".to_string(),
                _ => "Scanning".to_string(),
            };
            let mut err = std::io::stderr().lock();
            let _ = write!(
                err,
                "\r\x1b[2K{what}: {} files, {}",
                group(p.files),
                format_size(p.bytes)
            );
            let _ = err.flush();
        }
    };
    let opts = ScanOptions {
        allow_mft: args.allow_mft,
    };
    let result = scan(&args.path, &opts, &progress, &CancelToken::new());
    if show_progress {
        eprint!("\r\x1b[2K");
    }
    let scan = result?;

    if args.snapshot {
        save_snapshot(&scan, args.quiet);
    }

    if args.json {
        print_json(&scan, args.top, args.depth)?;
    } else {
        print_text(&scan, args.top, args.depth);
    }
    Ok(())
}

pub(crate) fn save_snapshot(scan: &Scan, quiet: bool) {
    let Some(data) = cleaner_core::paths::default_data_dir() else {
        return;
    };
    let days = cleaner_core::settings::Settings::load(&data).snapshot_days;
    match snapshot::record(&data, scan, days) {
        Ok(path) => {
            if !quiet {
                eprintln!("Snapshot saved: {}", path.display());
            }
        }
        Err(e) => eprintln!("warning: could not save snapshot: {e}"),
    }
}

fn print_text(scan: &Scan, top: usize, depth: usize) {
    let s = &scan.summary;
    let method = match s.method {
        cleaner_core::scan::ScanMethod::Mft => "fast MFT scan",
        cleaner_core::scan::ScanMethod::Walk => "folder walk",
    };
    println!(
        "Scanned {} in {:.1} s ({method})",
        s.root,
        s.duration_ms as f64 / 1000.0
    );
    if let Some(reason) = &s.mft_unavailable
        && s.method == cleaner_core::scan::ScanMethod::Walk
    {
        println!("  Note: {reason}.");
    }
    println!(
        "Total: {} ({} on disk), {} files, {} folders",
        format_size(s.size),
        format_size(s.allocated),
        group(s.files),
        group(s.dirs)
    );
    if let Some(v) = s.volume {
        println!(
            "Drive: {} total, {} free ({:.1}%)",
            format_size(v.total),
            format_size(v.free),
            percent(v.free, v.total)
        );
    }
    if s.skipped > 0 {
        println!(
            "Skipped {} entries that could not be read:",
            group(s.skipped)
        );
        for sk in s.skipped_details.iter().take(5) {
            println!("  {}: {}", sk.path, sk.reason);
        }
    }
    println!();
    println!(
        "{:>10} {:>10} {:>8} {:>11}  NAME",
        "SIZE", "ON DISK", "%PARENT", "FILES"
    );
    print_children(&scan.tree, scan.tree.root(), top, depth, 0);
}

fn print_children(tree: &Tree, id: NodeId, top: usize, depth: usize, level: usize) {
    let parent_size = tree.node(id).size();
    let kids = tree.children(id);
    for &c in kids.iter().take(top) {
        let n = tree.node(c);
        let mut name = tree.name(c).into_owned();
        if n.is_dir() && !n.has(flags::LINK) {
            name.push(std::path::MAIN_SEPARATOR);
        }
        println!(
            "{:>10} {:>10} {:>7.1}% {:>11}  {}{}{}",
            format_size(n.size()),
            format_size(n.allocated()),
            percent(n.size(), parent_size),
            if n.is_dir() {
                group(n.files())
            } else {
                String::new()
            },
            "  ".repeat(level),
            name,
            markers(n.flags())
        );
        if n.is_dir() && level + 1 < depth {
            print_children(tree, c, top, depth, level + 1);
        }
    }
    if kids.len() > top {
        let rest: u64 = kids[top..].iter().map(|&c| tree.node(c).size()).sum();
        println!(
            "{:>10} {:>10} {:>7.1}% {:>11}  {}({} more)",
            format_size(rest),
            "",
            percent(rest, parent_size),
            "",
            "  ".repeat(level),
            group((kids.len() - top) as u64)
        );
    }
}

fn markers(f: u16) -> String {
    let mut m = String::new();
    for (bit, label) in [
        (flags::LINK, "link, not followed"),
        (flags::HARDLINK_DUP, "hard link, counted elsewhere"),
        (flags::HAS_HARDLINKS, "has hard links, counted once"),
        (flags::UNREADABLE, "could not be read"),
        (flags::HAS_UNREADABLE, "some contents could not be read"),
        (flags::METAFILE, "NTFS metadata"),
        (flags::CLOUD, "cloud file"),
    ] {
        if f & bit != 0 {
            m.push_str(if m.is_empty() { "  [" } else { ", " });
            m.push_str(label);
        }
    }
    if !m.is_empty() {
        m.push(']');
    }
    m
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    allocated: u64,
    files: u64,
    dirs: u64,
    modified: i64,
    flags: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<JsonEntry>,
}

#[derive(Serialize)]
struct JsonOutput<'a> {
    summary: &'a ScanSummary,
    root: JsonEntry,
}

fn json_entry(tree: &Tree, id: NodeId, top: usize, depth: usize) -> JsonEntry {
    let n = tree.node(id);
    let children = if depth > 0 {
        tree.children(id)
            .iter()
            .take(top)
            .map(|&c| json_entry(tree, c, top, depth - 1))
            .collect()
    } else {
        Vec::new()
    };
    let flag_names = [
        (flags::LINK, "link"),
        (flags::HARDLINK, "hardLink"),
        (flags::HARDLINK_DUP, "hardLinkCountedElsewhere"),
        (flags::HAS_HARDLINKS, "hasHardLinks"),
        (flags::UNREADABLE, "unreadable"),
        (flags::HAS_UNREADABLE, "hasUnreadable"),
        (flags::METAFILE, "ntfsMetadata"),
        (flags::CLOUD, "cloud"),
        (flags::HIDDEN, "hidden"),
        (flags::SYSTEM, "system"),
    ];
    JsonEntry {
        name: tree.name(id).into_owned(),
        path: tree.path(id).display().to_string(),
        is_dir: n.is_dir(),
        size: n.size(),
        allocated: n.allocated(),
        files: n.files(),
        dirs: n.dirs(),
        modified: n.modified(),
        flags: flag_names
            .iter()
            .filter(|(bit, _)| n.has(*bit))
            .map(|(_, name)| *name)
            .collect(),
        children,
    }
}

fn print_json(scan: &Scan, top: usize, depth: usize) -> CliResult {
    let out = JsonOutput {
        summary: &scan.summary,
        root: json_entry(&scan.tree, scan.tree.root(), top, depth),
    };
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, &out)?;
    println!();
    Ok(())
}

/// 1 -> "1 file", 1234 -> "1,234 files"
pub(crate) fn files(n: u64) -> String {
    format!("{} file{}", group(n), if n == 1 { "" } else { "s" })
}

/// 1234567 -> "1,234,567"
pub(crate) fn group(n: u64) -> String {
    cleaner_core::format::format_count(n)
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max - 1).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1000), "1,000");
        assert_eq!(group(1_234_567), "1,234,567");
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
