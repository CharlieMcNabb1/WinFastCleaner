//! "What grew" tab: snapshot list and comparisons.

use std::path::PathBuf;

use serde::Serialize;

use cleaner_core::grew::{self, Change};
use cleaner_core::rules;
use cleaner_core::safety::Guard;
use cleaner_core::snapshot;

use crate::cleaning::{context, data_dir};

type CmdResult<T> = Result<T, String>;

const SUFFIX: &str = ".snapshot.json";
const LIMIT: usize = 50;
const MIN_CHANGE: u64 = 10 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    /// The snapshot's file name, used to pick it.
    key: String,
    root: String,
    taken_at: i64,
    size: u64,
}

/// Saved snapshots, newest first.
#[tauri::command(async)]
pub fn list_snapshots() -> CmdResult<Vec<SnapshotView>> {
    let list = snapshot::list(&snapshot::dir(&data_dir()?)).map_err(|e| e.to_string())?;
    Ok(list
        .into_iter()
        .filter_map(|s| {
            Some(SnapshotView {
                key: s.file.file_name()?.to_string_lossy().into_owned(),
                root: s.root,
                taken_at: s.taken_at,
                size: s.size,
            })
        })
        .collect())
}

/// A snapshot file name from the UI: a plain name in the snapshots folder.
fn snapshot_path(key: &str) -> CmdResult<PathBuf> {
    let ok = !key.is_empty()
        && key.len() <= 200
        && key.ends_with(SUFFIX)
        && !key.contains(['/', '\\', ':', '\0'])
        && !key.starts_with('.');
    if !ok {
        return Err("Unknown snapshot.".into());
    }
    let path = snapshot::dir(&data_dir()?).join(key);
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
        return Err("That snapshot no longer exists.".into());
    }
    Ok(path)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRef {
    id: String,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    #[serde(flatten)]
    change: Change,
    /// A cleaning rule that covers this folder (or part of it).
    rule: Option<RuleRef>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonView {
    root: String,
    old_taken_at: i64,
    new_taken_at: i64,
    total_delta: i64,
    threshold: u64,
    grew: Vec<ChangeView>,
    shrank: Vec<ChangeView>,
}

/// Compares two snapshots of the same root.
#[tauri::command(async)]
pub fn compare_snapshots(older: String, newer: String) -> CmdResult<ComparisonView> {
    let old = snapshot::load(&snapshot_path(&older)?).map_err(|e| e.to_string())?;
    let new = snapshot::load(&snapshot_path(&newer)?).map_err(|e| e.to_string())?;
    if !grew::same_root(&old.root, &new.root) {
        return Err("Those snapshots are of different drives or folders.".into());
    }
    let (old, new) = if old.taken_at <= new.taken_at {
        (old, new)
    } else {
        (new, old)
    };
    let c = grew::compare(&old, &new, LIMIT, MIN_CHANGE);

    // Rule locations on this system, to link changes to the cleaner.
    let (data, _, ctx) = context()?;
    let set = rules::load(
        Some(&cleaner_core::paths::user_rules_dir(&data)),
        &ctx.vars,
        &ctx.guard,
    );
    let roots: Vec<(PathBuf, RuleRefSrc)> = set
        .rules
        .iter()
        .flat_map(|r| {
            r.paths
                .iter()
                .flat_map(|p| rules::expand_path(p, &ctx.vars))
                .map(move |root| {
                    (
                        root,
                        RuleRefSrc {
                            id: r.id.clone(),
                            name: r.name.clone(),
                        },
                    )
                })
        })
        .collect();
    let with_rule = |change: Change| {
        let path = PathBuf::from(&change.path);
        let rule = roots
            .iter()
            .find(|(root, _)| Guard::is_within(root, &path) || Guard::is_within(&path, root))
            .map(|(_, r)| RuleRef {
                id: r.id.clone(),
                name: r.name.clone(),
            });
        ChangeView { change, rule }
    };
    Ok(ComparisonView {
        root: c.root,
        old_taken_at: c.old_taken_at,
        new_taken_at: c.new_taken_at,
        total_delta: c.total_delta,
        threshold: c.threshold,
        grew: c.grew.into_iter().map(with_rule).collect(),
        shrank: c.shrank.into_iter().map(with_rule).collect(),
    })
}

struct RuleRefSrc {
    id: String,
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_keys_are_plain_names() {
        for bad in [
            "",
            "../x.snapshot.json",
            r"..\x.snapshot.json",
            "C:x.snapshot.json",
            "x.json",
            ".hidden.snapshot.json",
        ] {
            assert!(snapshot_path(bad).is_err(), "{bad}");
        }
    }
}
