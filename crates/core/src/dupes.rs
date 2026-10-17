//! Duplicate finder: files with identical contents.
//!
//! Candidates come from a finished scan's tree and are narrowed in three
//! steps, each only on files that still have a possible twin:
//!
//! 1. same size,
//! 2. same BLAKE3 hash of the first and last 16 KB,
//! 3. same BLAKE3 hash of the whole file.
//!
//! Only step 3 decides: two files are reported as duplicates only when their
//! full contents hash the same. Names of one hard-linked file are the same
//! file, not copies, so they are never reported. Results are red tier, and
//! [`plan_removal`] refuses any plan that would remove every copy.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rayon::prelude::*;
use serde::Serialize;

use crate::cleaner::{self, CleanPlan, Context, Rejected};
use crate::filetypes::{FileKind, kind_of};
use crate::finder::NEVER_LISTED;
use crate::fsops::{self, FileId};
use crate::progress::CancelToken;
use crate::tree::{NodeId, Tree};
use crate::{Error, Result};

const SAMPLE: u64 = 16 * 1024;
const BUFFER: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DupeOptions {
    /// Smaller files are skipped (default 1 MB).
    pub min_size: u64,
    /// Only these types (all types when empty).
    pub kinds: Vec<FileKind>,
}

impl Default for DupeOptions {
    fn default() -> Self {
        DupeOptions {
            min_size: 1024 * 1024,
            kinds: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeFile {
    pub path: PathBuf,
    pub modified: i64,
}

/// Files whose contents are identical.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeGroup {
    pub size: u64,
    /// BLAKE3 hash of the contents (hex).
    pub hash: String,
    /// Sorted by path.
    pub files: Vec<DupeFile>,
}

impl DupeGroup {
    /// Space used by the extra copies.
    pub fn wasted(&self) -> u64 {
        self.size * (self.files.len() as u64).saturating_sub(1)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeResult {
    /// Largest waste first.
    pub groups: Vec<DupeGroup>,
    pub wasted: u64,
    /// Files whose whole contents were hashed.
    pub files_hashed: u64,
    pub bytes_hashed: u64,
    /// Files that could not be read (in use, denied, changed while reading).
    pub unreadable: u64,
    /// Same-size candidates left out because they are protected or excluded.
    pub hidden: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DupePhase {
    /// Comparing sizes.
    Sizes,
    /// Hashing the start and end of each candidate.
    Sampling,
    /// Hashing whole files.
    Hashing,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeProgress {
    pub phase: DupePhase,
    /// Files (sampling) or bytes (hashing) done so far.
    pub done: u64,
    pub total: u64,
}

struct Candidate {
    path: PathBuf,
    size: u64,
    modified: i64,
}

/// Finds files with identical contents in a scan.
pub fn find(
    tree: &Tree,
    opts: &DupeOptions,
    ctx: &Context,
    progress: &(dyn Fn(DupeProgress) + Sync),
    cancel: &CancelToken,
) -> Result<DupeResult> {
    let mut result = DupeResult::default();
    progress(DupeProgress {
        phase: DupePhase::Sizes,
        done: 0,
        total: 0,
    });

    // 1. Same size.
    let mut by_size: HashMap<u64, Vec<NodeId>> = HashMap::new();
    for id in 0..tree.len() as NodeId {
        let n = tree.node(id);
        if n.is_dir() || n.flags() & NEVER_LISTED != 0 || n.size() < opts.min_size {
            continue;
        }
        if !opts.kinds.is_empty() && !opts.kinds.contains(&kind_of(&tree.name(id))) {
            continue;
        }
        by_size.entry(n.size()).or_default().push(id);
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    for (size, ids) in by_size {
        if ids.len() < 2 {
            continue;
        }
        for id in ids {
            let path = tree.path(id);
            if ctx.guard.check(&path).is_err() || ctx.exclusions.is_excluded(&path) {
                result.hidden += 1;
                continue;
            }
            candidates.push(Candidate {
                path,
                size,
                modified: tree.node(id).modified(),
            });
        }
    }
    let candidates = keep_groups(candidates, |c| c.size);
    check_cancel(cancel)?;

    // 2. Same start and end (files of up to 32 KB are hashed whole here).
    let total = candidates.len() as u64;
    let done = AtomicU64::new(0);
    let sampled: Vec<Option<(Candidate, FileId, String)>> = candidates
        .into_par_iter()
        .map(|c| {
            if cancel.is_cancelled() {
                return None;
            }
            let r = sample(&c).ok().map(|(id, h)| (c, id, h));
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(64) || n == total {
                progress(DupeProgress {
                    phase: DupePhase::Sampling,
                    done: n,
                    total,
                });
            }
            r
        })
        .collect();
    check_cancel(cancel)?;
    result.unreadable += sampled.iter().filter(|s| s.is_none()).count() as u64;
    let sampled = distinct_files(sampled.into_iter().flatten().collect());
    let sampled = keep_groups(sampled, |(c, _, h)| (c.size, h.clone()));

    // 3. Same whole contents.
    let (small, large): (Vec<_>, Vec<_>) = sampled
        .into_iter()
        .partition(|(c, _, _)| c.size <= 2 * SAMPLE);
    let total_bytes: u64 = large.iter().map(|(c, _, _)| c.size).sum();
    let hashed_bytes = AtomicU64::new(0);
    let hashed: Vec<Option<(Candidate, String)>> = large
        .into_par_iter()
        .map(|(c, _, _)| {
            if cancel.is_cancelled() {
                return None;
            }
            let r = full_hash(&c, cancel).ok().map(|h| (c, h));
            let before =
                hashed_bytes.fetch_add(r.as_ref().map_or(0, |(c, _)| c.size), Ordering::Relaxed);
            progress(DupeProgress {
                phase: DupePhase::Hashing,
                done: before + r.as_ref().map_or(0, |(c, _)| c.size),
                total: total_bytes,
            });
            r
        })
        .collect();
    check_cancel(cancel)?;
    result.unreadable += hashed.iter().filter(|h| h.is_none()).count() as u64;
    let mut hashed: Vec<(Candidate, String)> = hashed.into_iter().flatten().collect();
    result.files_hashed = (hashed.len() + small.len()) as u64;
    result.bytes_hashed =
        hashed_bytes.into_inner() + small.iter().map(|(c, _, _)| c.size).sum::<u64>();
    hashed.extend(small.into_iter().map(|(c, _, h)| (c, h)));

    let mut groups: BTreeMap<(u64, String), Vec<DupeFile>> = BTreeMap::new();
    for (c, h) in hashed {
        groups.entry((c.size, h)).or_default().push(DupeFile {
            path: c.path,
            modified: c.modified,
        });
    }
    result.groups = groups
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|((size, hash), mut files)| {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            DupeGroup { size, hash, files }
        })
        .collect();
    result
        .groups
        .sort_by(|a, b| b.wasted().cmp(&a.wasted()).then(a.hash.cmp(&b.hash)));
    result.wasted = result.groups.iter().map(DupeGroup::wasted).sum();
    Ok(result)
}

fn check_cancel(cancel: &CancelToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

/// Keeps only items that share their key with at least one other item.
fn keep_groups<T, K: std::hash::Hash + Eq>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    let mut counts: HashMap<K, usize> = HashMap::new();
    for item in &items {
        *counts.entry(key(item)).or_default() += 1;
    }
    items
        .into_iter()
        .filter(|item| counts.get(&key(item)).is_some_and(|n| *n > 1))
        .collect()
}

/// Several names of one hard-linked file are one file: keep the first name.
fn distinct_files(mut items: Vec<(Candidate, FileId, String)>) -> Vec<(Candidate, FileId, String)> {
    items.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    let mut seen = std::collections::HashSet::new();
    items.retain(|(_, id, _)| seen.insert(*id));
    items
}

/// Hashes the first and last 16 KB (or the whole file when it is small).
fn sample(c: &Candidate) -> std::io::Result<(FileId, String)> {
    let (mut f, id) = fsops::open_read(&c.path)?;
    if f.metadata()?.len() != c.size {
        return Err(std::io::Error::other("changed since the scan"));
    }
    let mut hasher = blake3::Hasher::new();
    if c.size <= 2 * SAMPLE {
        let mut buf = Vec::with_capacity(c.size as usize);
        f.read_to_end(&mut buf)?;
        if buf.len() as u64 != c.size {
            return Err(std::io::Error::other("changed while reading"));
        }
        hasher.update(&buf);
    } else {
        let mut buf = vec![0u8; SAMPLE as usize];
        f.read_exact(&mut buf)?;
        hasher.update(&buf);
        f.seek(SeekFrom::Start(c.size - SAMPLE))?;
        f.read_exact(&mut buf)?;
        hasher.update(&buf);
    }
    Ok((id, hasher.finalize().to_hex().to_string()))
}

/// Hashes the whole file.
fn full_hash(c: &Candidate, cancel: &CancelToken) -> std::io::Result<String> {
    let (mut f, _) = fsops::open_read(&c.path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; BUFFER];
    let mut total = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(std::io::Error::other("cancelled"));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buf[..n]);
    }
    if total != c.size {
        return Err(std::io::Error::other("changed while reading"));
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Plans moving the chosen copies (`(group, file)` indexes) to the recovery
/// bin. Every group must keep at least one copy, and a kept copy must still
/// be there; otherwise nothing is planned.
pub fn plan_removal(
    groups: &[DupeGroup],
    remove: &[(usize, usize)],
    ctx: &Context,
) -> std::result::Result<(CleanPlan, Vec<Rejected>), String> {
    let mut chosen: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for &(g, f) in remove {
        let group = groups.get(g).ok_or("Unknown duplicate group.")?;
        if f >= group.files.len() {
            return Err("Unknown file.".into());
        }
        chosen.entry(g).or_default().insert(f);
    }
    let mut paths = Vec::new();
    for (g, files) in &chosen {
        let group = &groups[*g];
        let name = group.files[0]
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if files.len() >= group.files.len() {
            return Err(format!(
                "Keep at least one copy of \"{name}\". Every copy was selected."
            ));
        }
        let kept_exists = group.files.iter().enumerate().any(|(i, f)| {
            !files.contains(&i)
                && std::fs::symlink_metadata(&f.path)
                    .is_ok_and(|m| m.is_file() && m.len() == group.size)
        });
        if !kept_exists {
            return Err(format!(
                "The copy of \"{name}\" you're keeping is no longer there. Scan again."
            ));
        }
        paths.extend(files.iter().map(|&i| group.files[i].path.clone()));
    }
    Ok(cleaner::plan_manual(&paths, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Vars;
    use crate::safety::Guard;
    use crate::scan::{ScanOptions, scan};
    use crate::settings::{Exclusion, ExclusionKind, Exclusions};

    fn ctx(exclusions: &[Exclusion]) -> Context {
        Context {
            guard: Guard::system(),
            vars: Vars::default(),
            exclusions: Exclusions::new(exclusions),
            elevated: false,
            running: Default::default(),
            now: crate::scan::now_unix(),
        }
    }

    fn write(root: &std::path::Path, rel: &str, body: &[u8]) -> PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        std::fs::canonicalize(p).unwrap()
    }

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn run(root: &std::path::Path, opts: &DupeOptions, c: &Context) -> DupeResult {
        let tree = scan(
            root,
            &ScanOptions { allow_mft: false },
            &|_| {},
            &CancelToken::new(),
        )
        .unwrap()
        .tree;
        find(&tree, opts, c, &|_| {}, &CancelToken::new()).unwrap()
    }

    #[test]
    fn finds_duplicates_by_full_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let big = pattern(200_000, 1);
        let a = write(root, "a/movie.bin", &big);
        let b = write(root, "b/movie copy.bin", &big);
        let c = write(root, "c/movie.bin", &big);
        // Same size, same first and last 16 KB, different middle: not a
        // duplicate (only the full hash decides).
        let mut tricky = big.clone();
        tricky[100_000] ^= 0xFF;
        write(root, "d/almost.bin", &tricky);
        // Same size, different start.
        write(root, "e/other.bin", &pattern(200_000, 2));
        // Small identical files (hashed whole in the sampling step).
        let s1 = write(root, "s/one.txt", &pattern(3000, 7));
        let s2 = write(root, "s/two.txt", &pattern(3000, 7));
        // A unique file.
        write(root, "u/unique.bin", &pattern(5000, 9));

        let opts = DupeOptions {
            min_size: 1,
            kinds: Vec::new(),
        };
        let r = run(root, &opts, &ctx(&[]));
        assert_eq!(r.groups.len(), 2, "{:#?}", r.groups);
        let g = &r.groups[0];
        assert_eq!(g.size, 200_000);
        assert_eq!(
            g.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(),
            vec![a.clone(), b.clone(), c.clone()]
        );
        assert_eq!(g.hash, blake3::hash(&big).to_hex().to_string());
        assert_eq!(g.wasted(), 400_000);
        assert_eq!(
            r.groups[1]
                .files
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            vec![s1, s2]
        );
        assert_eq!(r.wasted, 400_000 + 3000);

        // The minimum size and exclusions apply.
        let r = run(root, &DupeOptions::default(), &ctx(&[]));
        assert!(r.groups.is_empty(), "all files are under 1 MB");
        let x = ctx(&[Exclusion {
            kind: ExclusionKind::File,
            value: c.display().to_string(),
        }]);
        let r = run(root, &opts, &x);
        assert_eq!(r.groups[0].files.len(), 2);
        assert!(r.hidden >= 1);
    }

    #[test]
    fn hard_links_are_not_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let body = pattern(50_000, 3);
        let a = write(root, "a.bin", &body);
        std::fs::hard_link(&a, root.join("a-link.bin")).unwrap();
        let r = run(
            root,
            &DupeOptions {
                min_size: 1,
                kinds: Vec::new(),
            },
            &ctx(&[]),
        );
        assert!(r.groups.is_empty(), "{:#?}", r.groups);
    }

    #[test]
    fn removal_always_keeps_a_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let body = pattern(40_000, 5);
        let paths: Vec<PathBuf> = (0..3)
            .map(|i| write(root, &format!("d{i}/f.bin"), &body))
            .collect();
        let groups = vec![DupeGroup {
            size: 40_000,
            hash: blake3::hash(&body).to_hex().to_string(),
            files: paths
                .iter()
                .map(|p| DupeFile {
                    path: p.clone(),
                    modified: 0,
                })
                .collect(),
        }];
        let c = ctx(&[]);
        let err = plan_removal(&groups, &[(0, 0), (0, 1), (0, 2)], &c).unwrap_err();
        assert!(err.contains("Keep at least one copy"), "{err}");
        assert!(plan_removal(&groups, &[(0, 9)], &c).is_err());
        assert!(plan_removal(&groups, &[(5, 0)], &c).is_err());

        // The kept copy vanished: refuse.
        std::fs::remove_file(&paths[2]).unwrap();
        let err = plan_removal(&groups, &[(0, 0), (0, 1)], &c).unwrap_err();
        assert!(err.contains("no longer there"), "{err}");

        let (plan, rejected) = plan_removal(&groups, &[(0, 0)], &c).unwrap();
        assert!(rejected.is_empty());
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].path, paths[0]);
        assert_eq!(plan.items[0].tier, crate::rules::Tier::Red);
    }
}
