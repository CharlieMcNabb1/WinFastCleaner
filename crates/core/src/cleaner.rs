//! The cleaner: find junk with rules, build an exact plan, then execute
//! exactly that plan.
//!
//! - **Scan** finds files matched by rules. Anything the safety guard
//!   blocks, anything excluded, and any link, system, or cloud file is
//!   filtered out here, so it never appears in results.
//! - **Plan** turns the user's selection into an exact list of files and
//!   sizes (this is also the `--dry-run` output).
//! - **Execute** acts on that list only. Each file is re-checked through a
//!   link-safe handle right before it is deleted or moved, and skipped (with
//!   a reason) if anything changed.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;
use serde::Serialize;

use crate::fsops;
use crate::log::{self, Action};
use crate::progress::CancelToken;
use crate::recovery::{NewItem, PurgeReport, RecoveryStore};
use crate::rules::{Rule, RuleError, Tier, Vars};
use crate::safety::Guard;
use crate::settings::Exclusions;

/// Everything the cleaner needs to know about the system and the user.
#[derive(Clone, Debug)]
pub struct Context {
    pub guard: Guard,
    pub vars: Vars,
    pub exclusions: Exclusions,
    pub elevated: bool,
    /// Lowercase names of running programs.
    pub running: HashSet<String>,
    pub now: i64,
}

impl Context {
    /// The context of the running system with the user's exclusions.
    pub fn system(exclusions: Exclusions) -> Context {
        Context {
            guard: Guard::system(),
            vars: Vars::system(),
            exclusions,
            elevated: crate::platform::is_elevated(),
            running: crate::process::running_programs(),
            now: crate::scan::now_unix(),
        }
    }
}

/// A file the cleaner found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkItem {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
}

/// Whether a rule can be cleaned right now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "detail")]
pub enum RuleStatus {
    Ready,
    /// Needs administrator rights.
    NeedsAdmin,
    /// These programs must be closed first.
    AppRunning(Vec<String>),
    /// Nothing to clean (the folders don't exist).
    NothingFound,
}

/// Results for one rule.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleScan {
    pub rule: Rule,
    pub status: RuleStatus,
    pub items: Vec<JunkItem>,
    pub size: u64,
    /// The folders the rule's paths expanded to.
    pub roots: Vec<PathBuf>,
    /// Matches left out because they are protected, excluded, links, system
    /// files, or cloud files. They are never shown or cleaned.
    pub hidden: u64,
}

/// Results of a junk scan.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkScan {
    pub rules: Vec<RuleScan>,
    pub errors: Vec<RuleError>,
    pub scanned_at: i64,
}

impl JunkScan {
    /// Total of ready green rules: what "Clean safe items" would remove.
    pub fn safe_total(&self) -> (u64, usize) {
        self.rules
            .iter()
            .filter(|r| r.rule.tier == Tier::Green && r.status == RuleStatus::Ready)
            .fold((0, 0), |(s, c), r| (s + r.size, c + r.items.len()))
    }
}

/// Finds junk for `rules`. Rules are scanned in parallel.
pub fn scan(rules: &[Rule], ctx: &Context, cancel: &CancelToken) -> JunkScan {
    let results: Vec<RuleScan> = rules
        .par_iter()
        .map(|rule| scan_rule(rule, ctx, cancel))
        .collect();
    let mut results = results;
    results.sort_by(|a, b| a.rule.group.cmp(&b.rule.group).then(b.size.cmp(&a.size)));
    JunkScan {
        rules: results,
        errors: Vec::new(),
        scanned_at: ctx.now,
    }
}

fn scan_rule(rule: &Rule, ctx: &Context, cancel: &CancelToken) -> RuleScan {
    let mut roots: Vec<PathBuf> = rule
        .paths
        .iter()
        .flat_map(|p| crate::rules::expand_path(p, &ctx.vars))
        .collect();
    roots.sort();
    roots.dedup();
    let mut items = Vec::new();
    let mut hidden = 0u64;
    let mut real_roots = Vec::new();
    for root in &roots {
        if cancel.is_cancelled() {
            break;
        }
        // The final location of the root (ancestors may be links).
        let Ok(real) = std::fs::canonicalize(root) else {
            continue;
        };
        let real = crate::platform::simplify_path(real);
        if ctx.guard.check(&real).is_err() {
            continue;
        }
        let Ok(md) = std::fs::symlink_metadata(&real) else {
            continue;
        };
        if md.is_file() {
            // A rule path that names a single file.
            match candidate(&real, &md, rule, ctx) {
                Some(item) => items.push(item),
                None => hidden += 1,
            }
            real_roots.push(real);
            continue;
        }
        if !md.is_dir() || is_special(&md) {
            continue;
        }
        walk_rule_root(&real, rule, ctx, cancel, &mut items, &mut hidden);
        real_roots.push(real);
    }
    items.sort_by(|a, b| b.size.cmp(&a.size).then(a.path.cmp(&b.path)));
    items.dedup_by(|a, b| a.path == b.path);
    let size = items.iter().map(|i| i.size).sum();
    let status = if rule.requires_admin && !ctx.elevated {
        RuleStatus::NeedsAdmin
    } else if let Some(running) = running_apps(rule, ctx) {
        RuleStatus::AppRunning(running)
    } else if items.is_empty() {
        RuleStatus::NothingFound
    } else {
        RuleStatus::Ready
    };
    RuleScan {
        rule: rule.clone(),
        status,
        items,
        size,
        roots: real_roots,
        hidden,
    }
}

fn running_apps(rule: &Rule, ctx: &Context) -> Option<Vec<String>> {
    let running: Vec<String> = rule
        .close_apps
        .iter()
        .filter(|a| ctx.running.contains(a.as_str()))
        .cloned()
        .collect();
    (!running.is_empty()).then_some(running)
}

fn walk_rule_root(
    root: &Path,
    rule: &Rule,
    ctx: &Context,
    cancel: &CancelToken,
    items: &mut Vec<JunkItem>,
    hidden: &mut u64,
) {
    let mut stack: Vec<(PathBuf, Vec<String>)> = vec![(root.to_path_buf(), Vec::new())];
    while let Some((dir, rel)) = stack.pop() {
        if cancel.is_cancelled() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(md) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            let mut child_rel = rel.clone();
            child_rel.push(name);
            if md.is_dir() && !is_special(&md) {
                stack.push((entry.path(), child_rel));
                continue;
            }
            let included = rule.include.iter().any(|g| g.matches(&child_rel))
                && !rule.exclude.iter().any(|g| g.matches(&child_rel));
            if !included {
                continue;
            }
            match candidate(&entry.path(), &md, rule, ctx) {
                Some(item) => items.push(item),
                None => *hidden += 1,
            }
        }
    }
}

/// Applies every per-file filter. `None` means the file is never shown.
fn candidate(path: &Path, md: &std::fs::Metadata, rule: &Rule, ctx: &Context) -> Option<JunkItem> {
    if !md.is_file() || is_special(md) || is_system_or_cloud(md) {
        return None;
    }
    let modified = modified_secs(md);
    if let Some(days) = rule.min_age_days
        && ctx.now - modified < i64::from(days) * 86_400
    {
        return None;
    }
    if ctx.guard.check(path).is_err() || ctx.exclusions.is_excluded(path) {
        return None;
    }
    Some(JunkItem {
        path: path.to_path_buf(),
        size: md.len(),
        modified,
    })
}

fn modified_secs(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn is_special(md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    md.file_type().is_symlink() || md.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_special(md: &std::fs::Metadata) -> bool {
    md.file_type().is_symlink()
}

#[cfg(windows)]
fn is_system_or_cloud(md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const SYSTEM: u32 = 0x4;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    md.file_attributes() & (SYSTEM | OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

/// On Linux, another account's file (in shared folders such as `/tmp`) is
/// never ours to clean.
#[cfg(not(windows))]
fn is_system_or_cloud(md: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and cannot fail.
    md.uid() != unsafe { libc::geteuid() }
}

/// One file in a plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
    pub tier: Tier,
    pub rule: Option<String>,
    /// Empty folders between the file and this root are removed afterwards.
    /// The root itself is removed only when `remove_root` is set (manual
    /// folder selections).
    #[serde(skip)]
    pub cleanup: Option<(PathBuf, bool)>,
    /// Programs that must not be running when this file is cleaned
    /// (checked again right before acting).
    #[serde(skip)]
    pub close_apps: Vec<String>,
}

/// The exact list of files a clean will act on.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanPlan {
    pub items: Vec<PlanItem>,
    pub total_size: u64,
    /// Rules that were selected but cannot be cleaned now, with the reason.
    pub not_ready: Vec<(String, RuleStatus)>,
}

impl CleanPlan {
    fn push(&mut self, item: PlanItem) {
        self.total_size += item.size;
        self.items.push(item);
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Adds another plan's items (skipping files already planned).
    pub fn merge(&mut self, other: CleanPlan) {
        let mut seen: HashSet<PathBuf> = self.items.iter().map(|i| i.path.clone()).collect();
        for item in other.items {
            if seen.insert(item.path.clone()) {
                self.push(item);
            }
        }
        self.not_ready.extend(other.not_ready);
    }
}

/// Plans the selected rules. Only rules that are ready are included.
/// `deselected` removes single files the user unticked in a preview.
pub fn plan_rules(
    scan: &JunkScan,
    rule_ids: &[String],
    deselected: &HashSet<PathBuf>,
) -> CleanPlan {
    let mut plan = CleanPlan::default();
    for r in &scan.rules {
        if !rule_ids.contains(&r.rule.id) {
            continue;
        }
        if r.status != RuleStatus::Ready {
            if r.status != RuleStatus::NothingFound {
                plan.not_ready.push((r.rule.id.clone(), r.status.clone()));
            }
            continue;
        }
        for item in &r.items {
            if deselected.contains(&item.path) {
                continue;
            }
            let cleanup = r
                .rule
                .remove_empty_folders
                .then(|| {
                    r.roots
                        .iter()
                        .find(|root| Guard::is_within(&item.path, root))
                        .cloned()
                })
                .flatten()
                .map(|root| (root, false));
            plan.push(PlanItem {
                path: item.path.clone(),
                size: item.size,
                modified: item.modified,
                tier: r.rule.tier,
                rule: Some(r.rule.id.clone()),
                cleanup,
                close_apps: r.rule.close_apps.clone(),
            });
        }
    }
    plan
}

/// A path the user picked by hand that cannot be cleaned, and why.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    pub path: PathBuf,
    pub reason: String,
}

/// Plans files and folders the user picked by hand. These are always red
/// tier. Folders are expanded to the files inside (never through links);
/// protected, excluded, system, and cloud files are left out.
pub fn plan_manual(paths: &[PathBuf], ctx: &Context) -> (CleanPlan, Vec<Rejected>) {
    let mut plan = CleanPlan::default();
    let mut rejected = Vec::new();
    for p in paths {
        let real = match std::fs::canonicalize(p.parent().unwrap_or(p)) {
            Ok(parent) => match p.file_name() {
                Some(name) => crate::platform::simplify_path(parent).join(name),
                None => p.clone(),
            },
            Err(e) => {
                rejected.push(Rejected {
                    path: p.clone(),
                    reason: fsops::describe_error(&e),
                });
                continue;
            }
        };
        if let Err(b) = ctx.guard.check(&real) {
            rejected.push(Rejected {
                path: p.clone(),
                reason: format!("Protected: {b}"),
            });
            continue;
        }
        if ctx.exclusions.is_excluded(&real) {
            rejected.push(Rejected {
                path: p.clone(),
                reason: "On your exclusions list".into(),
            });
            continue;
        }
        let md = match std::fs::symlink_metadata(&real) {
            Ok(m) => m,
            Err(e) => {
                rejected.push(Rejected {
                    path: p.clone(),
                    reason: fsops::describe_error(&e),
                });
                continue;
            }
        };
        if is_special(&md) {
            rejected.push(Rejected {
                path: p.clone(),
                reason: "Links are never cleaned".into(),
            });
            continue;
        }
        let manual = Rule {
            id: "manual".into(),
            name: String::new(),
            group: String::new(),
            tier: Tier::Red,
            description: String::new(),
            lose: String::new(),
            paths: Vec::new(),
            include: vec![crate::glob::Glob::new("**").expect("valid")],
            exclude: Vec::new(),
            min_age_days: None,
            requires_admin: false,
            close_apps: Vec::new(),
            remove_empty_folders: true,
            source: crate::rules::RuleSource::BuiltIn("manual".into()),
        };
        if md.is_file() {
            match candidate(&real, &md, &manual, ctx) {
                Some(item) => plan.push(PlanItem {
                    path: item.path,
                    size: item.size,
                    modified: item.modified,
                    tier: Tier::Red,
                    rule: None,
                    cleanup: None,
                    close_apps: Vec::new(),
                }),
                None => rejected.push(Rejected {
                    path: p.clone(),
                    reason: "System and cloud files are not cleaned".into(),
                }),
            }
            continue;
        }
        let mut items = Vec::new();
        let mut hidden = 0;
        walk_rule_root(
            &real,
            &manual,
            ctx,
            &CancelToken::new(),
            &mut items,
            &mut hidden,
        );
        for item in items {
            plan.push(PlanItem {
                path: item.path,
                size: item.size,
                modified: item.modified,
                tier: Tier::Red,
                rule: None,
                cleanup: Some((real.clone(), true)),
                close_apps: Vec::new(),
            });
        }
    }
    (plan, rejected)
}

/// How to execute a plan.
#[derive(Clone, Debug)]
pub struct ExecOptions {
    /// Send green items to the recovery bin too.
    pub green_to_bin: bool,
    /// Label for the recovery batch, such as "Cleaner".
    pub label: String,
    /// Secure delete: overwrite green and yellow files before deleting
    /// them (they skip the recovery bin). Red items always go to the bin.
    pub secure: bool,
}

/// What happened to one file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "reason")]
pub enum Outcome {
    Deleted,
    /// Overwritten, then deleted (secure delete).
    Wiped,
    MovedToBin,
    Skipped(String),
}

/// A skipped file and why.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedItem {
    pub path: PathBuf,
    pub reason: String,
}

/// Result of a clean.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    pub deleted_count: u64,
    pub deleted_bytes: u64,
    /// Of the deleted files, how many were overwritten first.
    pub wiped_count: u64,
    pub binned_count: u64,
    pub binned_bytes: u64,
    pub skipped_count: u64,
    /// The first skipped files with reasons (all of them are in the log).
    pub skipped: Vec<SkippedItem>,
    /// Free space gained on the affected drives, measured before and after.
    pub freed_bytes: u64,
    pub batch_id: Option<String>,
    /// Old recovery batches deleted to stay within the bin's limits.
    pub evicted: PurgeReport,
    pub folders_removed: u64,
}

const MAX_SKIPPED_DETAILS: usize = 500;

/// Executes exactly the files in `plan`.
pub fn execute(
    plan: &CleanPlan,
    ctx: &Context,
    store: &RecoveryStore,
    data_dir: &Path,
    opts: &ExecOptions,
    progress: &(dyn Fn(u64, u64) + Sync),
    cancel: &CancelToken,
) -> CleanReport {
    let total = plan.items.len() as u64;
    let to_bin: Vec<bool> = plan
        .items
        .iter()
        .map(|i| {
            // Red items always go to the bin, even with secure delete.
            i.tier == Tier::Red || (!opts.secure && (i.tier != Tier::Green || opts.green_to_bin))
        })
        .collect();

    // Record the recovery batch before anything moves.
    let bin_items: Vec<NewItem> = plan
        .items
        .iter()
        .zip(&to_bin)
        .filter(|(_, b)| **b)
        .map(|(i, _)| NewItem {
            original: i.path.clone(),
            size: i.size,
            tier: i.tier,
            rule: i.rule.clone(),
        })
        .collect();
    let prepared = if bin_items.is_empty() {
        None
    } else {
        match store.prepare_batch(&opts.label, &bin_items, ctx.now) {
            Ok(p) => Some(p),
            Err(e) => {
                // No bin: those files are skipped, never deleted instead.
                let reason = format!("Recovery bin unavailable: {e}");
                return skip_all(
                    plan, &to_bin, ctx, store, data_dir, opts, progress, cancel, &reason,
                );
            }
        }
    };
    let mut bin_slot = Vec::with_capacity(plan.items.len());
    let mut next = 0usize;
    for b in &to_bin {
        if *b {
            bin_slot.push(Some(next));
            next += 1;
        } else {
            bin_slot.push(None);
        }
    }

    let drives = affected_drives(plan);
    let free_before = free_space(&drives);
    let done = AtomicU64::new(0);

    let outcomes: Vec<Outcome> = plan
        .items
        .par_iter()
        .zip(bin_slot.par_iter())
        .map(|(item, slot)| {
            let outcome = if cancel.is_cancelled() {
                Outcome::Skipped("Cancelled".into())
            } else {
                match slot {
                    None => act(item, ctx, None, opts.secure),
                    Some(i) => {
                        let p = prepared.as_ref().expect("batch prepared");
                        match (&p.stored[*i], &p.unavailable[*i]) {
                            (Some(dest), _) => act(item, ctx, Some(dest), false),
                            (None, why) => Outcome::Skipped(
                                why.clone()
                                    .unwrap_or_else(|| "Recovery bin unavailable".into()),
                            ),
                        }
                    }
                }
            };
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(256) || n == total {
                progress(n, total);
            }
            outcome
        })
        .collect();

    let mut report = CleanReport::default();
    if let Some(p) = &prepared {
        let moved: Vec<bool> = plan
            .items
            .iter()
            .zip(&outcomes)
            .zip(&to_bin)
            .filter(|(_, b)| **b)
            .map(|((_, o), _)| *o == Outcome::MovedToBin)
            .collect();
        let _ = store.finish_batch(&p.id, &moved);
        if moved.iter().any(|m| *m) {
            report.batch_id = Some(p.id.clone());
            report.evicted = store.enforce_limits(ctx.now).unwrap_or_default();
        }
    }

    for (item, outcome) in plan.items.iter().zip(&outcomes) {
        match outcome {
            Outcome::Deleted => {
                report.deleted_count += 1;
                report.deleted_bytes += item.size;
            }
            Outcome::Wiped => {
                report.deleted_count += 1;
                report.deleted_bytes += item.size;
                report.wiped_count += 1;
            }
            Outcome::MovedToBin => {
                report.binned_count += 1;
                report.binned_bytes += item.size;
            }
            Outcome::Skipped(reason) => {
                report.skipped_count += 1;
                if report.skipped.len() < MAX_SKIPPED_DETAILS {
                    report.skipped.push(SkippedItem {
                        path: item.path.clone(),
                        reason: reason.clone(),
                    });
                }
            }
        }
    }
    report.folders_removed = remove_empty_folders(plan, &outcomes, ctx);
    write_log(
        plan,
        &outcomes,
        report.batch_id.as_deref(),
        ctx.now,
        data_dir,
    );
    let free_after = free_space(&drives);
    report.freed_bytes = free_after.saturating_sub(free_before);
    report
}

#[allow(clippy::too_many_arguments)]
fn skip_all(
    plan: &CleanPlan,
    to_bin: &[bool],
    ctx: &Context,
    store: &RecoveryStore,
    data_dir: &Path,
    opts: &ExecOptions,
    progress: &(dyn Fn(u64, u64) + Sync),
    cancel: &CancelToken,
    reason: &str,
) -> CleanReport {
    // Run the direct deletes as normal; skip everything meant for the bin.
    let direct = CleanPlan {
        items: plan
            .items
            .iter()
            .zip(to_bin)
            .filter(|(_, b)| !**b)
            .map(|(i, _)| i.clone())
            .collect(),
        ..Default::default()
    };
    let mut report = if direct.items.is_empty() {
        CleanReport::default()
    } else {
        execute(&direct, ctx, store, data_dir, opts, progress, cancel)
    };
    for (item, b) in plan.items.iter().zip(to_bin) {
        if *b {
            report.skipped_count += 1;
            if report.skipped.len() < MAX_SKIPPED_DETAILS {
                report.skipped.push(SkippedItem {
                    path: item.path.clone(),
                    reason: reason.to_string(),
                });
            }
        }
    }
    report
}

/// Re-checks one file through a link-safe handle, then deletes it or moves
/// it to `bin_dest`.
fn act(item: &PlanItem, ctx: &Context, bin_dest: Option<&PathBuf>, secure: bool) -> Outcome {
    let skip = |why: &str| Outcome::Skipped(why.to_string());
    if let Some(app) = item
        .close_apps
        .iter()
        .find(|a| ctx.running.contains(a.as_str()))
    {
        return Outcome::Skipped(format!("{app} is running"));
    }
    let opened = if secure {
        fsops::open_for_wipe(&item.path)
    } else {
        fsops::open(&item.path)
    };
    let h = match opened {
        Ok(h) => h,
        Err(e) => return Outcome::Skipped(fsops::describe_error(&e)),
    };
    let info = match h.info() {
        Ok(i) => i,
        Err(e) => return Outcome::Skipped(fsops::describe_error(&e)),
    };
    if info.is_dir {
        return skip("Is a folder");
    }
    if info.is_special {
        return skip("Links and special files are never cleaned");
    }
    if info.is_system {
        return skip("Windows system file");
    }
    if info.is_cloud {
        return skip("Cloud file (its contents are not on this PC)");
    }
    if info.size != item.size || info.modified != item.modified {
        return skip("Changed since the preview");
    }
    let fin = match h.final_path() {
        Ok(f) => f,
        Err(e) => return Outcome::Skipped(fsops::describe_error(&e)),
    };
    if !Guard::same_path(&fin, &item.path) {
        return skip("Its location changed since the preview");
    }
    if let Err(b) = ctx.guard.check(&fin) {
        return Outcome::Skipped(format!("Protected: {b}"));
    }
    if ctx.exclusions.is_excluded(&fin) {
        return skip("On your exclusions list");
    }
    match bin_dest {
        None if secure => match h.wipe().and_then(|()| h.delete()) {
            Ok(()) => Outcome::Wiped,
            Err(e) => Outcome::Skipped(fsops::describe_error(&e)),
        },
        None => match h.delete() {
            Ok(()) => Outcome::Deleted,
            Err(e) => Outcome::Skipped(fsops::describe_error(&e)),
        },
        Some(dest) => match h.rename_no_replace(dest) {
            Ok(()) => Outcome::MovedToBin,
            Err(e) => Outcome::Skipped(fsops::describe_error(&e)),
        },
    }
}

/// Removes folders left empty, deepest first, never above each item's
/// cleanup root (and the root itself only for manual folder selections).
fn remove_empty_folders(plan: &CleanPlan, outcomes: &[Outcome], ctx: &Context) -> u64 {
    let mut candidates: BTreeSet<(usize, PathBuf)> = BTreeSet::new();
    for (item, outcome) in plan.items.iter().zip(outcomes) {
        if matches!(outcome, Outcome::Skipped(_)) {
            continue;
        }
        let Some((root, remove_root)) = &item.cleanup else {
            continue;
        };
        let mut dir = item.path.parent();
        while let Some(d) = dir {
            if !Guard::is_within(d, root) {
                break;
            }
            let is_root = Guard::same_path(d, root);
            if !is_root || *remove_root {
                candidates.insert((usize::MAX - d.components().count(), d.to_path_buf()));
            }
            if is_root {
                break;
            }
            dir = d.parent();
        }
    }
    let mut removed = 0;
    for (_, dir) in candidates {
        if ctx.guard.check_dir_removal(&dir).is_err() || ctx.exclusions.protects_folder(&dir) {
            continue;
        }
        let Ok(h) = fsops::open(&dir) else { continue };
        let ok = h.info().is_ok_and(|i| i.is_dir && !i.is_special)
            && h.final_path().is_ok_and(|f| Guard::same_path(&f, &dir));
        // Deleting a folder handle only succeeds when it is empty.
        if ok && h.delete().is_ok() {
            removed += 1;
        }
    }
    removed
}

fn write_log(
    plan: &CleanPlan,
    outcomes: &[Outcome],
    batch: Option<&str>,
    now: i64,
    data_dir: &Path,
) {
    let entries: Vec<log::Entry<'_>> = plan
        .items
        .iter()
        .zip(outcomes)
        .map(|(item, outcome)| {
            let (action, detail) = match outcome {
                Outcome::Deleted => (Action::Deleted, None),
                Outcome::Wiped => (Action::Wiped, None),
                Outcome::MovedToBin => (Action::MovedToBin, None),
                Outcome::Skipped(r) => (Action::Skipped, Some(r.as_str())),
            };
            log::Entry {
                time: now,
                batch: if action == Action::MovedToBin {
                    batch
                } else {
                    None
                },
                action,
                tier: Some(item.tier),
                path: &item.path,
                size: item.size,
                detail,
            }
        })
        .collect();
    let _ = log::append(data_dir, &entries);
}

fn affected_drives(plan: &CleanPlan) -> Vec<PathBuf> {
    let mut drives: Vec<PathBuf> = plan
        .items
        .iter()
        .filter_map(|i| crate::recovery::drive_root(&i.path))
        .collect();
    drives.sort();
    drives.dedup();
    drives
}

fn free_space(drives: &[PathBuf]) -> u64 {
    drives
        .iter()
        .filter_map(|d| crate::platform::volume_space(d))
        .map(|s| s.free)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{RuleSet, RuleSource, parse_into};
    use crate::settings::{BinLimits, Exclusion, ExclusionKind};

    struct Env {
        tmp: tempfile::TempDir,
        junk: PathBuf,
        data: PathBuf,
        store: RecoveryStore,
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        // Long, final paths (temp folders may have 8.3 short names).
        let base = crate::platform::simplify_path(std::fs::canonicalize(tmp.path()).unwrap());
        let junk = base.join("Local");
        let data = base.join("data");
        let bin = base.join("bin");
        for d in [&junk, &data, &bin] {
            std::fs::create_dir_all(d).unwrap();
        }
        let store = RecoveryStore::with_root(&data, &bin, BinLimits::default());
        Env {
            tmp,
            junk,
            data,
            store,
        }
    }

    fn ctx(e: &Env, exclusions: &[Exclusion]) -> Context {
        Context {
            guard: Guard::system(),
            vars: Vars::default()
                .with("LOCALAPPDATA", &e.junk)
                .with("TEMP", e.tmp.path().join("nothing")),
            exclusions: Exclusions::new(exclusions),
            elevated: false,
            running: HashSet::new(),
            now: crate::scan::now_unix() + 10 * 86_400,
        }
    }

    fn rules(text: &str, c: &Context) -> Vec<Rule> {
        let mut set = RuleSet::default();
        parse_into(
            text,
            RuleSource::BuiltIn("t".into()),
            &c.vars,
            &c.guard,
            &mut set,
        );
        assert!(set.errors.is_empty(), "{:?}", set.errors);
        set.rules
    }

    const RULES: &str = r#"
        [[rule]]
        id = "test.cache"
        name = "App cache"
        group = "Test"
        tier = "green"
        description = "d"
        lose = "l"
        paths = ['%LOCALAPPDATA%\App\Cache']
        exclude = ['keep/**']

        [[rule]]
        id = "test.logs"
        name = "App logs"
        group = "Test"
        tier = "yellow"
        description = "d"
        lose = "l"
        paths = ['%LOCALAPPDATA%\App\Logs']
        include = ['*.log']
        close_apps = ["app.exe"]
    "#;

    fn write(p: &Path, len: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'x'; len]).unwrap();
    }

    fn setup(e: &Env) {
        let cache = e.junk.join("App/Cache");
        write(&cache.join("a.bin"), 1000);
        write(&cache.join("sub/deep/b.bin"), 2000);
        write(&cache.join("keep/c.bin"), 50);
        let logs = e.junk.join("App/Logs");
        write(&logs.join("x.log"), 300);
        write(&logs.join("x.txt"), 300);
    }

    #[test]
    fn scan_plan_execute_round_trip() {
        let e = env();
        setup(&e);
        let c = ctx(&e, &[]);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let cache = scan
            .rules
            .iter()
            .find(|r| r.rule.id == "test.cache")
            .unwrap();
        assert_eq!(cache.status, RuleStatus::Ready);
        assert_eq!(cache.items.len(), 2, "excluded pattern left out");
        assert_eq!(cache.size, 3000);
        let logs = scan
            .rules
            .iter()
            .find(|r| r.rule.id == "test.logs")
            .unwrap();
        assert_eq!(logs.items.len(), 1, "include pattern applied");

        let plan = plan_rules(
            &scan,
            &["test.cache".into(), "test.logs".into()],
            &HashSet::new(),
        );
        assert_eq!(plan.count(), 3);
        assert_eq!(plan.total_size, 3300);

        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert_eq!((report.deleted_count, report.deleted_bytes), (2, 3000));
        assert_eq!((report.binned_count, report.binned_bytes), (1, 300));
        assert_eq!(report.skipped_count, 0, "{:?}", report.skipped);
        assert!(!e.junk.join("App/Cache/a.bin").exists());
        assert!(e.junk.join("App/Cache/keep/c.bin").exists());
        assert!(e.junk.join("App/Logs/x.txt").exists());
        // Empty folders removed below the root; the root itself stays.
        assert!(!e.junk.join("App/Cache/sub").exists());
        assert!(e.junk.join("App/Cache").exists());

        // The yellow file is restorable from the bin.
        let batch = report.batch_id.unwrap();
        let r = e
            .store
            .restore(&batch, None, crate::recovery::Conflict::Skip, &c.guard)
            .unwrap();
        assert_eq!(r.restored, 1);
        assert_eq!(
            std::fs::read(e.junk.join("App/Logs/x.log")).unwrap().len(),
            300
        );

        // The log recorded every action.
        let log = std::fs::read_to_string(e.data.join("logs/cleanup.log")).unwrap();
        assert_eq!(log.lines().filter(|l| l.contains("\"deleted\"")).count(), 2);
        assert!(log.contains("moved-to-bin"));
        assert!(log.contains("restored"));
    }

    #[test]
    fn only_the_plan_is_deleted() {
        let e = env();
        setup(&e);
        let c = ctx(&e, &[]);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let plan = plan_rules(&scan, &["test.cache".into()], &HashSet::new());
        // Files that appear after planning, or change, are not touched.
        write(&e.junk.join("App/Cache/new.bin"), 10);
        write(&e.junk.join("App/Cache/a.bin"), 1001);
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert!(e.junk.join("App/Cache/new.bin").exists());
        assert!(e.junk.join("App/Cache/a.bin").exists());
        assert_eq!(report.deleted_count, 1);
        assert_eq!(report.skipped[0].reason, "Changed since the preview");
    }

    #[test]
    fn exclusions_always_win() {
        let e = env();
        setup(&e);
        let excl = [Exclusion {
            kind: ExclusionKind::Folder,
            value: e.junk.join("App/Cache/sub").display().to_string(),
        }];
        let c = ctx(&e, &excl);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let cache = scan
            .rules
            .iter()
            .find(|r| r.rule.id == "test.cache")
            .unwrap();
        assert_eq!(cache.items.len(), 1);
        assert_eq!(cache.hidden, 1);

        // Even if an excluded file is forced into a plan, it is skipped.
        let mut plan = plan_rules(&scan, &["test.cache".into()], &HashSet::new());
        let b = e.junk.join("App/Cache/sub/deep/b.bin");
        plan.items.push(PlanItem {
            path: b.clone(),
            size: 2000,
            modified: modified_secs(&std::fs::metadata(&b).unwrap()),
            tier: Tier::Green,
            rule: None,
            cleanup: None,
            close_apps: Vec::new(),
        });
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert!(b.exists());
        assert!(
            report
                .skipped
                .iter()
                .any(|s| s.reason == "On your exclusions list")
        );
    }

    #[test]
    fn protected_paths_are_never_deleted_even_if_planned() {
        let e = env();
        let mut c = ctx(&e, &[]);
        // A guard whose protected system folder lives inside the temp
        // folder, so nothing outside it is ever at stake.
        let base = crate::platform::simplify_path(std::fs::canonicalize(e.tmp.path()).unwrap());
        let protected = if cfg!(windows) {
            c.guard = Guard::windows(&crate::safety::WindowsLayout {
                system_root: base.join("Windows"),
                program_files: vec![base.join("Program Files")],
                program_data: base.join("ProgramData"),
                users_dir: base.join("Users"),
                profiles: Vec::new(),
                app_dirs: Vec::new(),
            });
            base.join(r"Windows\explorer.exe")
        } else {
            c.guard = Guard::unix(&[base.join("app")]);
            base.join("app/settings.json")
        };
        write(&protected, 100);
        assert!(c.guard.check(&protected).is_err());
        let md = std::fs::metadata(&protected).unwrap();
        let plan = CleanPlan {
            items: vec![PlanItem {
                path: protected.clone(),
                size: md.len(),
                modified: modified_secs(&md),
                tier: Tier::Green,
                rule: None,
                cleanup: None,
                close_apps: Vec::new(),
            }],
            ..Default::default()
        };
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert!(protected.exists());
        assert_eq!(report.deleted_count, 0);
        assert_eq!(report.skipped_count, 1);

        // And manual selections refuse it up front.
        let (plan, rejected) = plan_manual(&[protected], &c);
        assert!(plan.items.is_empty());
        assert!(rejected[0].reason.starts_with("Protected"));
    }

    #[test]
    fn running_apps_and_admin_rules_are_not_ready() {
        let e = env();
        setup(&e);
        let mut c = ctx(&e, &[]);
        c.running.insert("app.exe".into());
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let logs = scan
            .rules
            .iter()
            .find(|r| r.rule.id == "test.logs")
            .unwrap();
        assert_eq!(logs.status, RuleStatus::AppRunning(vec!["app.exe".into()]));
        let plan = plan_rules(&scan, &["test.logs".into()], &HashSet::new());
        assert!(plan.items.is_empty());
        assert_eq!(plan.not_ready.len(), 1);
        assert!(e.junk.join("App/Logs/x.log").exists());

        // An app started between the preview and the clean: skipped.
        let idle = ctx(&e, &[]);
        let scan = super::scan(&rules(RULES, &idle), &idle, &CancelToken::new());
        let plan = plan_rules(&scan, &["test.logs".into()], &HashSet::new());
        assert_eq!(plan.count(), 1);
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert_eq!(report.skipped_count, 1);
        assert_eq!(report.skipped[0].reason, "app.exe is running");
        assert!(e.junk.join("App/Logs/x.log").exists());
    }

    #[test]
    fn secure_delete_overwrites_green_and_yellow_but_red_still_goes_to_the_bin() {
        let e = env();
        setup(&e);
        let c = ctx(&e, &[]);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let mut plan = plan_rules(
            &scan,
            &["test.cache".into(), "test.logs".into()],
            &HashSet::new(),
        );
        let mine = e.junk.join("mine.doc");
        write(&mine, 10);
        let (manual, _) = plan_manual(std::slice::from_ref(&mine), &c);
        plan.merge(manual);
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: true,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert_eq!(report.skipped_count, 0, "{:?}", report.skipped);
        assert_eq!(report.wiped_count, 3, "two green files and one yellow");
        assert_eq!(report.binned_count, 1, "the red file still goes to the bin");
        assert!(!e.junk.join("App/Logs/x.log").exists());
        let batch = e.store.batch(report.batch_id.as_deref().unwrap()).unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].original, mine);
        let log = std::fs::read_to_string(e.data.join("logs/cleanup.log")).unwrap();
        assert_eq!(log.matches("\"wiped\"").count(), 3, "{log}");
    }

    #[test]
    fn min_age_keeps_recent_files() {
        let e = env();
        setup(&e);
        let mut c = ctx(&e, &[]);
        c.now = crate::scan::now_unix();
        let aged = RULES.replace(
            "exclude = ['keep/**']",
            "exclude = ['keep/**']\nmin_age_days = 2",
        );
        let scan = scan(&rules(&aged, &c), &c, &CancelToken::new());
        let cache = scan
            .rules
            .iter()
            .find(|r| r.rule.id == "test.cache")
            .unwrap();
        assert!(cache.items.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn links_are_never_followed_or_cleaned() {
        let e = env();
        setup(&e);
        let outside = e.tmp.path().join("outside");
        write(&outside.join("precious.doc"), 100);
        std::os::unix::fs::symlink(&outside, e.junk.join("App/Cache/link-dir")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("precious.doc"),
            e.junk.join("App/Cache/link-file"),
        )
        .unwrap();
        let c = ctx(&e, &[]);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let cache = scan
            .rules
            .iter()
            .find(|r| r.rule.id == "test.cache")
            .unwrap();
        assert!(
            cache
                .items
                .iter()
                .all(|i| !i.path.to_string_lossy().contains("link"))
        );
        let plan = plan_rules(&scan, &["test.cache".into()], &HashSet::new());
        execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert!(outside.join("precious.doc").exists());
        assert!(e.junk.join("App/Cache/link-dir").symlink_metadata().is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_never_followed_or_cleaned() {
        let e = env();
        setup(&e);
        let outside = e.tmp.path().join("outside");
        write(&outside.join("precious.doc"), 100);
        // Backslashes only: cmd reads "/Cache" as a switch.
        let link = e.junk.join("App").join("Cache").join("junction");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let c = ctx(&e, &[]);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let plan = plan_rules(&scan, &["test.cache".into()], &HashSet::new());
        assert!(
            plan.items
                .iter()
                .all(|i| !i.path.to_string_lossy().contains("junction"))
        );
        execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert!(outside.join("precious.doc").exists());
        assert!(
            link.symlink_metadata().is_ok(),
            "the junction itself is left alone"
        );
    }

    #[cfg(windows)]
    #[test]
    fn locked_files_are_skipped_and_reported() {
        use std::os::windows::fs::OpenOptionsExt;
        let e = env();
        setup(&e);
        let c = ctx(&e, &[]);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let plan = plan_rules(&scan, &["test.cache".into()], &HashSet::new());
        let locked = e.junk.join("App/Cache/a.bin");
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap();
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert_eq!(
            report.deleted_count, 1,
            "one failure never aborts the batch"
        );
        assert_eq!(report.skipped[0].reason, "In use by another program");
        assert!(locked.exists());
    }

    #[test]
    fn manual_selections_are_red_and_go_to_the_bin() {
        let e = env();
        let folder = e.junk.join("Downloads/old-project");
        write(&folder.join("a.zip"), 500);
        write(&folder.join("nested/b.iso"), 700);
        let c = ctx(&e, &[]);
        let (plan, rejected) = plan_manual(std::slice::from_ref(&folder), &c);
        assert!(rejected.is_empty());
        assert_eq!(plan.count(), 2);
        assert!(plan.items.iter().all(|i| i.tier == Tier::Red));
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Your selections".into(),
                secure: false,
            },
            &|_, _| {},
            &CancelToken::new(),
        );
        assert_eq!(report.binned_count, 2);
        assert!(!folder.exists(), "the emptied folder is removed too");
        assert_eq!(e.store.list().unwrap()[0].items, 2);
    }

    #[test]
    fn cancelled_cleans_skip_remaining_items() {
        let e = env();
        setup(&e);
        let c = ctx(&e, &[]);
        let scan = scan(&rules(RULES, &c), &c, &CancelToken::new());
        let plan = plan_rules(&scan, &["test.cache".into()], &HashSet::new());
        let cancel = CancelToken::new();
        cancel.cancel();
        let report = execute(
            &plan,
            &c,
            &e.store,
            &e.data,
            &ExecOptions {
                green_to_bin: false,
                label: "Cleaner".into(),
                secure: false,
            },
            &|_, _| {},
            &cancel,
        );
        assert_eq!(report.deleted_count, 0);
        assert!(e.junk.join("App/Cache/a.bin").exists());
    }
}
