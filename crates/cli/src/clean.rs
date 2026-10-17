//! `wfc rules`, `wfc clean`, and `wfc recover`.

use std::collections::HashSet;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use serde::Serialize;

use cleaner_core::cleaner::{self, CleanPlan, Context, ExecOptions, JunkScan, RuleStatus};
use cleaner_core::format::{format_size, unix_to_utc};
use cleaner_core::progress::CancelToken;
use cleaner_core::recovery::{Conflict, RecoveryStore};
use cleaner_core::rules::{self, Rule, RuleError, RuleSet, Tier};
use cleaner_core::settings::{Exclusions, Settings};

use crate::{CliResult, files, group, truncate};

#[derive(Subcommand, Debug)]
pub enum RulesCommand {
    /// List the cleaning rules (built-in and your own).
    List {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Check rule files for mistakes without using them.
    Check {
        /// Rule files (.toml) to check.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

/// The highest tier `wfc clean` selects when no rule is named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TierArg {
    /// Safe items only.
    Green,
    /// Safe and optional items.
    Yellow,
}

#[derive(clap::Args, Debug)]
pub struct CleanArgs {
    /// Clean only these rules (see `wfc rules list`). May be repeated.
    #[arg(long = "rule", value_name = "ID")]
    rules: Vec<String>,
    /// Which rules to clean when no --rule is given. Review (red) items are
    /// never cleaned automatically.
    #[arg(long, value_enum, default_value_t = TierArg::Green)]
    tier: TierArg,
    /// Show exactly what would be cleaned, and change nothing.
    #[arg(long)]
    dry_run: bool,
    /// With --dry-run, list every file.
    #[arg(long, requires = "dry_run")]
    list: bool,
    /// Clean without asking first.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Use only the rules in this folder (no built-in rules). Handy for
    /// trying out your own rules.
    #[arg(long, value_name = "DIR")]
    rules_dir: Option<PathBuf>,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ConflictArg {
    /// Leave the existing file and keep the item in the bin.
    Skip,
    /// Restore under a new name, such as "report (restored).pdf".
    KeepBoth,
    /// Move the existing file to the bin, then restore.
    Overwrite,
}

#[derive(Subcommand, Debug)]
pub enum RecoverCommand {
    /// List batches in the recovery bin, newest first.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show the files in one batch.
    Show {
        batch: String,
        #[arg(long)]
        json: bool,
    },
    /// Put files from a batch back where they were.
    Restore {
        batch: String,
        /// Restore only these item numbers (see `wfc recover show`).
        #[arg(long = "item", value_name = "N")]
        items: Vec<u32>,
        /// What to do when a file with the same name is already there.
        #[arg(long, value_enum, default_value_t = ConflictArg::Skip)]
        on_conflict: ConflictArg,
    },
    /// Permanently delete one batch.
    Delete {
        batch: String,
        /// Don't ask first.
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Permanently delete everything in the recovery bin.
    Empty {
        /// Don't ask first.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

fn data_dir() -> Result<PathBuf, String> {
    cleaner_core::paths::default_data_dir()
        .ok_or_else(|| "cannot find the user's data folder (LOCALAPPDATA is not set)".to_string())
}

fn context(settings: &Settings) -> Context {
    Context::system(Exclusions::new(&settings.exclusions))
}

fn load_rules(ctx: &Context, data_dir: &Path, rules_dir: Option<&Path>) -> RuleSet {
    match rules_dir {
        Some(dir) => rules::load_dir(dir, &ctx.vars, &ctx.guard),
        None => rules::load(
            Some(&cleaner_core::paths::user_rules_dir(data_dir)),
            &ctx.vars,
            &ctx.guard,
        ),
    }
}

fn print_rule_errors(errors: &[RuleError]) {
    for e in errors {
        match &e.rule {
            Some(id) => eprintln!("warning: {} (rule {id}): {}", e.source, e.message),
            None => eprintln!("warning: {}: {}", e.source, e.message),
        }
    }
}

fn tier_label(t: Tier) -> &'static str {
    t.label()
}

// ---------------------------------------------------------------- rules

pub fn rules_cmd(cmd: RulesCommand) -> CliResult {
    match cmd {
        RulesCommand::List { json } => {
            let data = data_dir()?;
            let settings = Settings::load(&data);
            let ctx = context(&settings);
            let set = load_rules(&ctx, &data, None);
            if json {
                println!("{}", serde_json::to_string_pretty(&set)?);
                return Ok(());
            }
            print_rule_errors(&set.errors);
            println!(
                "{:<9} {:<34} {:<16} {:<6} NAME",
                "TIER", "ID", "GROUP", "ADMIN"
            );
            for r in &set.rules {
                println!(
                    "{:<9} {:<34} {:<16} {:<6} {}",
                    tier_label(r.tier),
                    r.id,
                    truncate(&r.group, 16),
                    if r.requires_admin { "yes" } else { "" },
                    r.name
                );
            }
            Ok(())
        }
        RulesCommand::Check { files } => {
            let ctx = context(&Settings::default());
            let mut bad = 0usize;
            for f in &files {
                let set = rules::load_file(f, &ctx.vars, &ctx.guard);
                if set.errors.is_empty() {
                    println!("{}: OK ({} rules)", f.display(), set.rules.len());
                } else {
                    bad += 1;
                    for e in &set.errors {
                        match &e.rule {
                            Some(id) => println!("{}: rule {id}: {}", f.display(), e.message),
                            None => println!("{}: {}", f.display(), e.message),
                        }
                    }
                }
            }
            if bad > 0 {
                return Err(format!("{bad} of {} rule files have problems", files.len()).into());
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- clean

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuleLine<'a> {
    id: &'a str,
    name: &'a str,
    tier: Tier,
    status: &'a RuleStatus,
    files: usize,
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DryRunOutput<'a> {
    dry_run: bool,
    rules: Vec<RuleLine<'a>>,
    plan: &'a CleanPlan,
}

fn selected_rules(set: &RuleSet, args: &CleanArgs) -> Result<Vec<Rule>, String> {
    if args.rules.is_empty() {
        let max = match args.tier {
            TierArg::Green => Tier::Green,
            TierArg::Yellow => Tier::Yellow,
        };
        return Ok(set
            .rules
            .iter()
            .filter(|r| r.tier <= max)
            .cloned()
            .collect());
    }
    let mut out = Vec::new();
    for id in &args.rules {
        match set.rules.iter().find(|r| r.id.eq_ignore_ascii_case(id)) {
            Some(r) => {
                if !out.iter().any(|o: &Rule| o.id == r.id) {
                    out.push(r.clone());
                }
            }
            None => return Err(format!("no rule named `{id}` (see `wfc rules list`)")),
        }
    }
    Ok(out)
}

fn status_text(s: &RuleStatus) -> String {
    match s {
        RuleStatus::Ready => String::new(),
        RuleStatus::NeedsAdmin => "Needs administrator rights (skipped)".into(),
        RuleStatus::AppRunning(apps) => format!("Close {} first (skipped)", apps.join(", ")),
        RuleStatus::NothingFound => "Nothing found".into(),
    }
}

fn print_scan(scan: &JunkScan) {
    println!(
        "{:<9} {:<36} {:>9} {:>10}  STATUS",
        "TIER", "RULE", "FILES", "SIZE"
    );
    for r in &scan.rules {
        println!(
            "{:<9} {:<36} {:>9} {:>10}  {}",
            tier_label(r.rule.tier),
            truncate(&r.rule.name, 36),
            group(r.items.len() as u64),
            format_size(r.size),
            status_text(&r.status)
        );
    }
}

pub fn clean_cmd(args: CleanArgs) -> CliResult {
    let data = data_dir()?;
    let settings = Settings::load(&data);
    let mut ctx = context(&settings);
    let set = load_rules(&ctx, &data, args.rules_dir.as_deref());
    if !args.json {
        print_rule_errors(&set.errors);
    }
    let selected = selected_rules(&set, &args)?;
    if selected.is_empty() {
        return Err("no rules selected".into());
    }
    let ids: Vec<String> = selected.iter().map(|r| r.id.clone()).collect();
    let scan = cleaner::scan(&selected, &ctx, &CancelToken::new());
    let plan = cleaner::plan_rules(&scan, &ids, &HashSet::new());

    if args.dry_run {
        if args.json {
            let out = DryRunOutput {
                dry_run: true,
                rules: scan
                    .rules
                    .iter()
                    .map(|r| RuleLine {
                        id: &r.rule.id,
                        name: &r.rule.name,
                        tier: r.rule.tier,
                        status: &r.status,
                        files: r.items.len(),
                        size: r.size,
                    })
                    .collect(),
                plan: &plan,
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }
        println!("Dry run: nothing will be changed.\n");
        print_scan(&scan);
        if args.list {
            println!();
            for item in &plan.items {
                println!(
                    "{:>10}  {:<8}  {}",
                    format_size(item.size),
                    tier_label(item.tier),
                    item.path.display()
                );
            }
        }
        println!(
            "\nWould clean {} ({}).",
            format_size(plan.total_size),
            files(plan.count() as u64)
        );
        print_where_files_go(&plan, settings.green_to_bin);
        println!("Run again without --dry-run to clean.");
        return Ok(());
    }

    if !args.json {
        print_scan(&scan);
        println!();
    }
    if plan.items.is_empty() {
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&cleaner::CleanReport::default())?
            );
        } else {
            println!("Nothing to clean.");
        }
        return Ok(());
    }
    if !args.yes {
        let question = format!(
            "Clean {} ({})?",
            format_size(plan.total_size),
            files(plan.count() as u64)
        );
        if !confirm(&question)? {
            println!("Nothing was changed.");
            return Ok(());
        }
    }

    // Programs may have started while the user was reading the preview.
    ctx.running = cleaner_core::process::running_programs();
    ctx.now = cleaner_core::scan::now_unix();
    let store = RecoveryStore::open(&data, settings.bin);
    let opts = ExecOptions {
        green_to_bin: settings.green_to_bin,
        label: "Cleaner".into(),
    };
    let report = cleaner::execute(
        &plan,
        &ctx,
        &store,
        &data,
        &opts,
        &|_, _| {},
        &CancelToken::new(),
    );
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    println!(
        "Deleted {} ({}).",
        files(report.deleted_count),
        format_size(report.deleted_bytes)
    );
    if report.binned_count > 0 {
        println!(
            "Moved {} ({}) to the recovery bin. Undo with: wfc recover restore {}",
            files(report.binned_count),
            format_size(report.binned_bytes),
            report.batch_id.as_deref().unwrap_or("?")
        );
    }
    if report.skipped_count > 0 {
        println!("Skipped {}:", files(report.skipped_count));
        for s in report.skipped.iter().take(10) {
            println!("  {}: {}", s.path.display(), s.reason);
        }
        if report.skipped_count > 10 {
            println!("  … the full list is in the cleanup log.");
        }
    }
    if report.evicted.batches > 0 {
        println!(
            "Permanently deleted {} old recovery batches ({}) to stay within the bin's limits.",
            report.evicted.batches,
            format_size(report.evicted.bytes)
        );
    }
    println!("Disk space freed: {}.", format_size(report.freed_bytes));
    Ok(())
}

fn print_where_files_go(plan: &CleanPlan, green_to_bin: bool) {
    let to_bin = plan
        .items
        .iter()
        .filter(|i| i.tier != Tier::Green || green_to_bin)
        .count();
    let deleted = plan.items.len() - to_bin;
    if deleted > 0 {
        println!(
            "{} safe would be deleted (they regenerate on their own).",
            files(deleted as u64)
        );
    }
    if to_bin > 0 {
        println!(
            "{} would be moved to the recovery bin, so they can be restored.",
            files(to_bin as u64)
        );
    }
}

/// Asks a yes/no question on the terminal. Refuses to guess when there is no
/// terminal to ask on.
fn confirm(question: &str) -> Result<bool, String> {
    if !std::io::stdin().is_terminal() {
        return Err("not asking for confirmation without a terminal; pass --yes to proceed".into());
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

// ---------------------------------------------------------------- recover

fn date(secs: i64) -> String {
    let (y, mo, d, h, mi, _) = unix_to_utc(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02} UTC")
}

pub fn recover_cmd(cmd: RecoverCommand) -> CliResult {
    let data = data_dir()?;
    let settings = Settings::load(&data);
    let store = RecoveryStore::open(&data, settings.bin);
    match cmd {
        RecoverCommand::List { json } => {
            let list = store.list()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
                return Ok(());
            }
            if list.is_empty() {
                println!("The recovery bin is empty.");
                return Ok(());
            }
            println!(
                "{:<26} {:<21} {:>7} {:>10}  LABEL",
                "BATCH", "DATE", "FILES", "SIZE"
            );
            for b in &list {
                println!(
                    "{:<26} {:<21} {:>7} {:>10}  {}",
                    b.id,
                    date(b.created_at),
                    group(b.items as u64),
                    format_size(b.size),
                    b.label
                );
            }
            Ok(())
        }
        RecoverCommand::Show { batch, json } => {
            let b = store.batch(&batch)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&b)?);
                return Ok(());
            }
            println!("{} ({}, {})", b.id, b.label, date(b.created_at));
            println!(
                "{:>5} {:>10} {:<9} ORIGINAL LOCATION",
                "ITEM", "SIZE", "TIER"
            );
            for i in &b.items {
                println!(
                    "{:>5} {:>10} {:<9} {}",
                    i.id,
                    format_size(i.size),
                    tier_label(i.tier),
                    i.original.display()
                );
            }
            Ok(())
        }
        RecoverCommand::Restore {
            batch,
            items,
            on_conflict,
        } => {
            let guard = cleaner_core::safety::Guard::system();
            let conflict = match on_conflict {
                ConflictArg::Skip => Conflict::Skip,
                ConflictArg::KeepBoth => Conflict::KeepBoth,
                ConflictArg::Overwrite => Conflict::Overwrite,
            };
            let ids = (!items.is_empty()).then_some(items.as_slice());
            let r = store.restore(&batch, ids, conflict, &guard)?;
            println!(
                "Restored {} ({}).",
                files(r.restored),
                format_size(r.restored_bytes)
            );
            for p in &r.renamed {
                println!("  Restored as {}", p.display());
            }
            if !r.skipped.is_empty() {
                println!("Not restored:");
                for (p, why) in &r.skipped {
                    println!("  {}: {why}", p.display());
                }
            }
            Ok(())
        }
        RecoverCommand::Delete { batch, yes } => {
            let b = store.batch(&batch)?;
            if !yes
                && !confirm(&format!(
                    "Permanently delete {} ({}) from the recovery bin?",
                    files(b.items.len() as u64),
                    format_size(b.size())
                ))?
            {
                println!("Nothing was changed.");
                return Ok(());
            }
            let r = store.delete_batch(&batch)?;
            println!(
                "Permanently deleted {} ({}).",
                files(r.files),
                format_size(r.bytes)
            );
            if r.failed > 0 {
                println!(
                    "{} could not be deleted and stay in the bin.",
                    files(r.failed)
                );
            }
            Ok(())
        }
        RecoverCommand::Empty { yes } => {
            let list = store.list()?;
            if list.is_empty() {
                println!("The recovery bin is already empty.");
                return Ok(());
            }
            let count: usize = list.iter().map(|b| b.items).sum();
            let size: u64 = list.iter().map(|b| b.size).sum();
            if !yes
                && !confirm(&format!(
                    "Permanently delete everything in the recovery bin ({}, {})?",
                    files(count as u64),
                    format_size(size)
                ))?
            {
                println!("Nothing was changed.");
                return Ok(());
            }
            let r = store.empty()?;
            println!(
                "Permanently deleted {} ({}).",
                files(r.files),
                format_size(r.bytes)
            );
            if r.failed > 0 {
                println!(
                    "{} could not be deleted and stay in the bin.",
                    files(r.failed)
                );
            }
            Ok(())
        }
    }
}
