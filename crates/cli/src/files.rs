//! `wfc find`, `wfc dupes`, `wfc grew`, and `wfc snapshots`. All read-only:
//! they list files and changes; cleaning them is done in the app, where
//! every pick goes to the recovery bin.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;

use cleaner_core::cleaner::Context;
use cleaner_core::dupes::{self, DupeOptions, DupePhase, DupeProgress};
use cleaner_core::filetypes::FileKind;
use cleaner_core::finder::{self, FinderOptions};
use cleaner_core::format::{format_size, unix_to_utc};
use cleaner_core::grew;
use cleaner_core::progress::{CancelToken, ScanProgress};
use cleaner_core::scan::{Scan, ScanOptions, scan};
use cleaner_core::settings::{Exclusions, Settings};
use cleaner_core::snapshot::{self, SnapshotInfo};

use crate::{CliResult, files, group, save_snapshot};

/// Options shared by the commands that scan first.
#[derive(Args, Debug)]
pub struct ScanFlags {
    /// Never use the fast MFT reader; always walk folders.
    #[arg(long)]
    no_mft: bool,
    /// Don't save a scan snapshot.
    #[arg(long)]
    no_snapshot: bool,
    /// Only these file types: video, audio, image, archive, installer,
    /// disk-image, document, other. May be repeated.
    #[arg(long = "kind", value_name = "TYPE", value_parser = parse_kind)]
    kinds: Vec<FileKind>,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug)]
pub enum FindCommand {
    /// The largest files in a drive or folder.
    Large {
        path: PathBuf,
        /// Only files at least this big, such as 100MB or 2GB.
        #[arg(long, default_value = "100MB", value_parser = parse_size)]
        min_size: u64,
        /// How many files to list.
        #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..=100_000))]
        top: u32,
        #[command(flatten)]
        flags: ScanFlags,
    },
    /// Files not modified or opened for a long time.
    Old {
        path: PathBuf,
        /// Unused for at least this many days.
        #[arg(long, default_value_t = 365, value_parser = clap::value_parser!(u32).range(1..=36_500))]
        days: u32,
        /// Only files at least this big.
        #[arg(long, default_value = "1MB", value_parser = parse_size)]
        min_size: u64,
        /// How many files to list.
        #[arg(long, default_value_t = 500, value_parser = clap::value_parser!(u32).range(1..=100_000))]
        top: u32,
        #[command(flatten)]
        flags: ScanFlags,
    },
}

#[derive(Args, Debug)]
pub struct DupesArgs {
    path: PathBuf,
    /// Skip files smaller than this.
    #[arg(long, default_value = "1MB", value_parser = parse_size)]
    min_size: u64,
    #[command(flatten)]
    flags: ScanFlags,
}

#[derive(Args, Debug)]
pub struct GrewArgs {
    /// The scanned drive or folder (default: the most recently scanned).
    root: Option<String>,
    /// Compare against this many snapshots back (1 = the previous one).
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    back: u32,
    /// How many folders to list each way.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=1_000))]
    top: u32,
    /// Ignore changes smaller than this.
    #[arg(long, default_value = "10MB", value_parser = parse_size)]
    min_change: u64,
    #[arg(long)]
    json: bool,
}

/// `100MB`, `1.5 GB`, `500k`, or plain bytes. Units are binary (1 KB = 1024).
pub fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim().to_ascii_lowercase().replace(' ', "");
    let digits = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(digits);
    let n: f64 = num.parse().map_err(|_| format!("`{s}` is not a size"))?;
    let mult: u64 = match unit {
        "" | "b" => 1,
        "k" | "kb" => 1 << 10,
        "m" | "mb" => 1 << 20,
        "g" | "gb" => 1 << 30,
        "t" | "tb" => 1 << 40,
        _ => return Err(format!("`{s}`: unknown unit (use KB, MB, GB, or TB)")),
    };
    let v = n * mult as f64;
    if !v.is_finite() || v < 0.0 || v > u64::MAX as f64 / 2.0 {
        return Err(format!("`{s}` is out of range"));
    }
    Ok(v as u64)
}

fn parse_kind(s: &str) -> Result<FileKind, String> {
    FileKind::parse(s).ok_or_else(|| format!("unknown file type `{s}`"))
}

fn context() -> Context {
    let settings = cleaner_core::paths::default_data_dir()
        .map(|d| Settings::load(&d))
        .unwrap_or_default();
    Context::system(Exclusions::new(&settings.exclusions))
}

fn run_scan(path: &Path, flags: &ScanFlags) -> Result<Scan, Box<dyn std::error::Error>> {
    let show = !flags.json && std::io::stderr().is_terminal();
    let progress = |p: &ScanProgress| {
        if show {
            let mut err = std::io::stderr().lock();
            let _ = write!(err, "\r\x1b[2KScanning: {} files", group(p.files));
            let _ = err.flush();
        }
    };
    let opts = ScanOptions {
        allow_mft: !flags.no_mft,
    };
    let result = scan(path, &opts, &progress, &CancelToken::new());
    if show {
        eprint!("\r\x1b[2K");
    }
    let scan = result?;
    if !flags.no_snapshot {
        save_snapshot(&scan, true);
    }
    Ok(scan)
}

fn date(secs: i64) -> String {
    let (y, mo, d, _, _, _) = unix_to_utc(secs);
    format!("{y:04}-{mo:02}-{d:02}")
}

pub fn find_cmd(cmd: FindCommand) -> CliResult {
    let (path, opts, flags) = match cmd {
        FindCommand::Large {
            path,
            min_size,
            top,
            flags,
        } => (
            path,
            FinderOptions {
                min_size,
                older_than_days: None,
                kinds: flags.kinds.clone(),
                limit: top as usize,
            },
            flags,
        ),
        FindCommand::Old {
            path,
            days,
            min_size,
            top,
            flags,
        } => (
            path,
            FinderOptions {
                min_size,
                older_than_days: Some(days),
                kinds: flags.kinds.clone(),
                limit: top as usize,
            },
            flags,
        ),
    };
    let scan = run_scan(&path, &flags)?;
    let result = finder::find(&scan.tree, &opts, &context(), &CancelToken::new());
    if flags.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }
    if result.files.is_empty() {
        println!("No matching files.");
    } else {
        println!(
            "{:>10}  {:<11} {:<11} {:<12} PATH",
            "SIZE", "MODIFIED", "LAST USED", "TYPE"
        );
        for f in &result.files {
            println!(
                "{:>10}  {:<11} {:<11} {:<12} {}",
                format_size(f.size),
                date(f.modified),
                date(f.last_used),
                f.kind.label(),
                f.path.display()
            );
        }
        let total: u64 = result.files.iter().map(|f| f.size).sum();
        println!(
            "\n{} listed, {} in total.",
            files(result.files.len() as u64),
            format_size(total)
        );
    }
    if result.more > 0 {
        println!("About {} more not shown (raise --top).", files(result.more));
    }
    if result.hidden > 0 {
        println!(
            "{} in protected or excluded locations left out.",
            files(result.hidden)
        );
    }
    if result.recently_opened > 0 {
        println!(
            "{} left out because they were opened recently.",
            files(result.recently_opened)
        );
    }
    Ok(())
}

pub fn dupes_cmd(args: DupesArgs) -> CliResult {
    let scan = run_scan(&args.path, &args.flags)?;
    let show = !args.flags.json && std::io::stderr().is_terminal();
    let progress = |p: DupeProgress| {
        if !show {
            return;
        }
        let text = match p.phase {
            DupePhase::Sizes => "Comparing sizes".to_string(),
            DupePhase::Sampling => format!("Checking {} of {} candidates", p.done, p.total),
            DupePhase::Hashing => format!(
                "Comparing contents: {} of {}",
                format_size(p.done),
                format_size(p.total)
            ),
        };
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r\x1b[2K{text}");
        let _ = err.flush();
    };
    let opts = DupeOptions {
        min_size: args.min_size,
        kinds: args.flags.kinds.clone(),
    };
    let result = dupes::find(
        &scan.tree,
        &opts,
        &context(),
        &progress,
        &CancelToken::new(),
    );
    if show {
        eprint!("\r\x1b[2K");
    }
    let result = result?;
    if args.flags.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }
    if result.groups.is_empty() {
        println!("No duplicate files found.");
    }
    for g in &result.groups {
        println!(
            "{} copies of {} ({} extra)  blake3 {}",
            g.files.len(),
            format_size(g.size),
            format_size(g.wasted()),
            &g.hash[..16]
        );
        for f in &g.files {
            println!("    {}", f.path.display());
        }
    }
    println!(
        "\n{} groups, {} taken by extra copies. Compared the full contents of {} ({}).",
        group(result.groups.len() as u64),
        format_size(result.wasted),
        files(result.files_hashed),
        format_size(result.bytes_hashed)
    );
    if result.unreadable > 0 {
        println!(
            "{} could not be read and were skipped.",
            files(result.unreadable)
        );
    }
    if result.hidden > 0 {
        println!(
            "{} in protected or excluded locations left out.",
            files(result.hidden)
        );
    }
    Ok(())
}

fn all_snapshots() -> Result<Vec<SnapshotInfo>, Box<dyn std::error::Error>> {
    let data = cleaner_core::paths::default_data_dir()
        .ok_or("cannot find the user's data folder (LOCALAPPDATA is not set)")?;
    Ok(snapshot::list(&snapshot::dir(&data))?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotLine<'a> {
    root: &'a str,
    taken_at: i64,
    size: u64,
}

pub fn snapshots_cmd(json: bool) -> CliResult {
    let list = all_snapshots()?;
    if json {
        let lines: Vec<SnapshotLine> = list
            .iter()
            .map(|s| SnapshotLine {
                root: &s.root,
                taken_at: s.taken_at,
                size: s.size,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&lines)?);
        return Ok(());
    }
    if list.is_empty() {
        println!("No snapshots yet. Each `wfc scan` (and each scan in the app) saves one.");
        return Ok(());
    }
    println!("{:<12} {:>10}  ROOT", "DATE", "SIZE");
    for s in &list {
        println!(
            "{:<12} {:>10}  {}",
            date(s.taken_at),
            format_size(s.size),
            s.root
        );
    }
    Ok(())
}

pub fn grew_cmd(args: GrewArgs) -> CliResult {
    let list = all_snapshots()?;
    let root = match &args.root {
        Some(r) => r.clone(),
        None => list
            .first()
            .map(|s| s.root.clone())
            .ok_or("No snapshots yet. Scan a drive first.")?,
    };
    let same: Vec<&SnapshotInfo> = list
        .iter()
        .filter(|s| grew::same_root(&s.root, &root))
        .collect();
    if same.len() < 2 {
        return Err(format!(
            "need at least two snapshots of {root} to compare (found {}); scan it again later",
            same.len()
        )
        .into());
    }
    let newer = snapshot::load(&same[0].file)?;
    let older_info = same
        .get(args.back as usize)
        .ok_or_else(|| format!("only {} older snapshots of {root}", same.len() - 1))?;
    let older = snapshot::load(&older_info.file)?;
    let c = grew::compare(&older, &newer, args.top as usize, args.min_change);
    if args.json {
        println!("{}", serde_json::to_string_pretty(&c)?);
        return Ok(());
    }
    let sign = |d: i64| {
        if d >= 0 {
            format!("+{}", format_size(d as u64))
        } else {
            format!("-{}", format_size(d.unsigned_abs()))
        }
    };
    println!(
        "{}: {} since {} (now {}).",
        c.root,
        sign(c.total_delta),
        date(c.old_taken_at),
        date(c.new_taken_at)
    );
    for (title, list) in [("Grew", &c.grew), ("Shrank", &c.shrank)] {
        if list.is_empty() {
            continue;
        }
        println!("\n{title}:");
        for ch in list {
            let note = match (ch.old_size, ch.new_size) {
                (None, _) => "  (new, or was under the snapshot threshold)",
                (_, None) => "  (gone, or now under the snapshot threshold)",
                _ => "",
            };
            println!("{:>11}  {}{note}", sign(ch.delta), ch.path);
        }
    }
    if c.grew.is_empty() && c.shrank.is_empty() {
        println!(
            "No folder changed by more than {}.",
            format_size(args.min_change)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("100MB"), Ok(100 << 20));
        assert_eq!(parse_size("1.5 GB"), Ok(3 << 29));
        assert_eq!(parse_size("500k"), Ok(500 << 10));
        assert_eq!(parse_size("42"), Ok(42));
        assert!(parse_size("ten").is_err());
        assert!(parse_size("5 PB").is_err());
        assert!(parse_size("-1MB").is_err());
    }
}
