//! The rolling recovery bin.
//!
//! Yellow and red items (and green ones if the user asks) are **moved**, not
//! copied, into a folder on the same drive, so the move is an instant
//! rename:
//!
//! - on the drive of the app's data folder (normally C:), the bin is inside
//!   that private folder (`%LOCALAPPDATA%\WinFastCleaner\recovery\bin`);
//! - on other drives it is `X:\.winfastcleaner-recovery\<user>`, a hidden
//!   folder whose per-user subfolder only that user, SYSTEM, and
//!   administrators can open. A folder owned by anyone else is refused.
//!
//! An index in the app's data folder records each file's original path,
//! size, and batch.
//!
//! Crash safety: a batch is written to the index (as "pending") *before*
//! any file moves, and reconciled on the next load. A failure on one file
//! never leaves the index inconsistent.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::fsops;
use crate::log::{self, Action};
use crate::rules::Tier;
use crate::safety::Guard;
use crate::settings::BinLimits;
use crate::{Error, Result};

/// Name of the hidden folder at the root of each drive.
pub const BIN_DIR_NAME: &str = ".winfastcleaner-recovery";

const INDEX_VERSION: u32 = 1;

/// Serializes index access between threads of this process (the file lock
/// below covers other processes, such as a scheduled cleanup).
static PROCESS_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    /// Recorded before the move; reconciled on the next load.
    Pending,
    InBin,
}

/// One file in the bin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinItem {
    pub id: u32,
    pub original: PathBuf,
    pub stored: PathBuf,
    pub size: u64,
    pub tier: Tier,
    pub rule: Option<String>,
    pub state: ItemState,
}

/// One cleanup's worth of files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: String,
    pub created_at: i64,
    pub label: String,
    pub items: Vec<BinItem>,
}

impl Batch {
    pub fn size(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    version: u32,
    batches: Vec<Batch>,
}

/// Summary of a batch for lists.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSummary {
    pub id: String,
    pub created_at: i64,
    pub label: String,
    pub items: usize,
    pub size: u64,
}

/// Bin usage on one drive.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveUsage {
    pub drive: String,
    pub bytes: u64,
    pub limit: u64,
}

/// What to do when a restored file's original path is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    /// Leave both files where they are and skip this one.
    Skip,
    /// Restore under a new name such as `report (restored).pdf`.
    KeepBoth,
    /// Move the existing file into the bin, then restore.
    Overwrite,
}

/// Result of a restore.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub restored: u64,
    pub restored_bytes: u64,
    /// Restored under a new name because the original path was taken.
    pub renamed: Vec<PathBuf>,
    pub skipped: Vec<(PathBuf, String)>,
}

/// Result of permanently deleting batches.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeReport {
    pub batches: u64,
    pub files: u64,
    pub bytes: u64,
    pub failed: u64,
}

/// A batch that has been recorded and is ready for files to be moved in.
#[derive(Debug)]
pub struct PreparedBatch {
    pub id: String,
    /// Where each item goes, in the order the items were given.
    pub stored: Vec<Option<PathBuf>>,
    /// Why an item could not get a place in the bin.
    pub unavailable: Vec<Option<String>>,
}

/// Input for [`RecoveryStore::prepare_batch`].
#[derive(Clone, Debug)]
pub struct NewItem {
    pub original: PathBuf,
    pub size: u64,
    pub tier: Tier,
    pub rule: Option<String>,
}

/// The recovery bin of the current user.
#[derive(Clone, Debug)]
pub struct RecoveryStore {
    data_dir: PathBuf,
    /// Tests keep every bin inside one folder instead of drive roots.
    root_override: Option<PathBuf>,
    user_key: String,
    limits: BinLimits,
}

impl RecoveryStore {
    pub fn open(data_dir: &Path, limits: BinLimits) -> RecoveryStore {
        RecoveryStore {
            data_dir: data_dir.to_path_buf(),
            root_override: None,
            user_key: user_key(),
            limits,
        }
    }

    /// A store whose bins all live under `root` (used by tests).
    pub fn with_root(data_dir: &Path, root: &Path, limits: BinLimits) -> RecoveryStore {
        RecoveryStore {
            data_dir: data_dir.to_path_buf(),
            root_override: Some(root.to_path_buf()),
            user_key: "test".into(),
            limits,
        }
    }

    fn index_path(&self) -> PathBuf {
        self.data_dir.join("recovery").join("index.json")
    }

    /// Runs `f` on the index while holding both locks, then saves it.
    fn with_index<T>(&self, f: impl FnOnce(&mut Index) -> Result<T>) -> Result<T> {
        let _guard = PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.data_dir.join("recovery");
        crate::paths::ensure_private_dir(&dir).map_err(|e| Error::io(&dir, e))?;
        let lock_path = dir.join("index.lock");
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| Error::io(&lock_path, e))?;
        lock.lock().map_err(|e| Error::io(&lock_path, e))?;
        let mut index = self.read_index()?;
        let out = f(&mut index)?;
        self.write_index(&index)?;
        Ok(out)
    }

    fn read_index(&self) -> Result<Index> {
        let path = self.index_path();
        let mut index = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Index>(&bytes)
                .map_err(|e| Error::Snapshot(format!("recovery index is damaged: {e}")))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Index {
                version: INDEX_VERSION,
                batches: Vec::new(),
            },
            Err(e) => return Err(Error::io(path, e)),
        };
        reconcile(&mut index);
        Ok(index)
    }

    fn write_index(&self, index: &Index) -> Result<()> {
        let json = serde_json::to_vec(index).map_err(|e| Error::Snapshot(e.to_string()))?;
        let path = self.index_path();
        crate::paths::write_atomic(&path, &json).map_err(|e| Error::io(path, e))
    }

    /// The bin folder of the data folder's drive.
    fn home_bin(&self) -> PathBuf {
        self.data_dir.join("recovery").join("bin")
    }

    /// The bin folder for files from the drive of `original`, created if
    /// needed. Refuses a bin folder that is a link or belongs to another
    /// user.
    fn bin_dir_for(&self, original: &Path) -> Result<PathBuf, String> {
        if let Some(r) = &self.root_override {
            let dir = r.join(&self.user_key);
            for d in [r, &dir] {
                ensure_real_dir(d, || std::fs::create_dir(d))?;
            }
            return Ok(dir);
        }
        let drive = drive_root(original).ok_or("not on a local drive")?;
        let on_home_drive =
            drive_root(&self.data_dir).is_some_and(|h| Guard::same_path(&h, &drive));
        if on_home_drive || !cfg!(windows) {
            let dir = self.home_bin();
            ensure_real_dir(&dir, || crate::paths::ensure_private_dir(&dir))?;
            return Ok(dir);
        }
        let root = drive.join(BIN_DIR_NAME);
        let dir = root.join(&self.user_key);
        shared_bin::prepare(&drive, &root, &dir)?;
        Ok(dir)
    }

    /// Every bin folder of this user that exists.
    fn existing_bins(&self) -> Vec<PathBuf> {
        let mut bins = match &self.root_override {
            Some(r) => vec![r.join(&self.user_key)],
            None => {
                let mut v = vec![self.home_bin()];
                if cfg!(windows) {
                    v.extend(crate::platform::drives().into_iter().map(|d| {
                        PathBuf::from(d.root)
                            .join(BIN_DIR_NAME)
                            .join(&self.user_key)
                    }));
                }
                v
            }
        };
        bins.retain(|b| std::fs::symlink_metadata(b).is_ok_and(|m| m.is_dir() && !is_reparse(&m)));
        bins
    }

    /// Records a new batch before any file moves. Items whose drive has no
    /// usable bin get `None` and a reason.
    pub fn prepare_batch(&self, label: &str, items: &[NewItem], now: i64) -> Result<PreparedBatch> {
        let id = batch_id(now);
        let mut stored = Vec::with_capacity(items.len());
        let mut unavailable = Vec::with_capacity(items.len());
        let mut records = Vec::new();
        for (i, item) in items.iter().enumerate() {
            match self.bin_dir_for(&item.original) {
                Ok(dir) => {
                    let batch_dir = dir.join(&id);
                    if !batch_dir.exists()
                        && let Err(e) = std::fs::create_dir(&batch_dir)
                    {
                        stored.push(None);
                        unavailable.push(Some(format!("cannot create the recovery bin: {e}")));
                        continue;
                    }
                    let path = batch_dir.join(i.to_string());
                    records.push(BinItem {
                        id: i as u32,
                        original: item.original.clone(),
                        stored: path.clone(),
                        size: item.size,
                        tier: item.tier,
                        rule: item.rule.clone(),
                        state: ItemState::Pending,
                    });
                    stored.push(Some(path));
                    unavailable.push(None);
                }
                Err(why) => {
                    stored.push(None);
                    unavailable.push(Some(why));
                }
            }
        }
        if !records.is_empty() {
            let batch = Batch {
                id: id.clone(),
                created_at: now,
                label: label.to_string(),
                items: records,
            };
            self.with_index(|index| {
                index.batches.push(batch);
                Ok(())
            })?;
        }
        Ok(PreparedBatch {
            id,
            stored,
            unavailable,
        })
    }

    /// Marks which items made it into the bin; drops the rest.
    pub fn finish_batch(&self, batch_id: &str, moved: &[bool]) -> Result<()> {
        self.with_index(|index| {
            if let Some(b) = index.batches.iter_mut().find(|b| b.id == batch_id) {
                b.items.retain_mut(|item| {
                    let ok = moved.get(item.id as usize).copied().unwrap_or(false);
                    if ok {
                        item.state = ItemState::InBin;
                    }
                    ok
                });
            }
            index.batches.retain(|b| !b.items.is_empty());
            Ok(())
        })?;
        self.remove_empty_batch_dirs(batch_id);
        Ok(())
    }

    /// Batches, newest first.
    pub fn list(&self) -> Result<Vec<BatchSummary>> {
        let batches = self.with_index(|i| Ok(i.batches.clone()))?;
        let mut out: Vec<BatchSummary> = batches
            .iter()
            .map(|b| BatchSummary {
                id: b.id.clone(),
                created_at: b.created_at,
                label: b.label.clone(),
                items: b.items.len(),
                size: b.size(),
            })
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(out)
    }

    /// Items of one batch.
    pub fn batch(&self, batch_id: &str) -> Result<Batch> {
        self.with_index(|i| {
            i.batches
                .iter()
                .find(|b| b.id == batch_id)
                .cloned()
                .ok_or_else(|| Error::InvalidInput("that recovery batch no longer exists".into()))
        })
    }

    /// Bin usage per drive with each drive's limit.
    pub fn usage(&self) -> Result<Vec<DriveUsage>> {
        let batches = self.with_index(|i| Ok(i.batches.clone()))?;
        let mut per: Vec<DriveUsage> = Vec::new();
        for b in &batches {
            for item in &b.items {
                let drive = self.drive_key(&item.stored);
                match per.iter_mut().find(|d| d.drive == drive) {
                    Some(d) => d.bytes += item.size,
                    None => per.push(DriveUsage {
                        limit: self.size_limit(&drive),
                        drive,
                        bytes: item.size,
                    }),
                }
            }
        }
        Ok(per)
    }

    fn drive_key(&self, stored: &Path) -> String {
        match &self.root_override {
            Some(r) => r.display().to_string(),
            None => drive_root(stored)
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
        }
    }

    fn size_limit(&self, drive: &str) -> u64 {
        let percent = crate::platform::volume_space(Path::new(drive))
            .map(|s| s.total / 100 * u64::from(self.limits.max_percent))
            .unwrap_or(u64::MAX);
        self.limits.max_bytes.min(percent)
    }

    /// Permanently deletes the oldest batches until the bin is within its
    /// limits. The newest batch is always kept.
    pub fn enforce_limits(&self, now: i64) -> Result<PurgeReport> {
        let batches = self.with_index(|i| Ok(i.batches.clone()))?;
        let mut order: Vec<&Batch> = batches.iter().collect();
        order.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let newest = order.last().map(|b| b.id.clone());
        let max_age = i64::from(self.limits.max_age_days) * 86_400;

        let mut usage: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for b in &order {
            for item in &b.items {
                *usage.entry(self.drive_key(&item.stored)).or_default() += item.size;
            }
        }
        let mut remaining = order.len();
        let mut evict = Vec::new();
        for b in &order {
            if Some(&b.id) == newest.as_ref() {
                break;
            }
            let too_many = remaining > self.limits.max_batches as usize;
            let too_old = now - b.created_at > max_age;
            let too_big = usage
                .iter()
                .any(|(drive, bytes)| *bytes > self.size_limit(drive));
            if !(too_many || too_old || too_big) {
                continue;
            }
            for item in &b.items {
                if let Some(u) = usage.get_mut(&self.drive_key(&item.stored)) {
                    *u = u.saturating_sub(item.size);
                }
            }
            remaining -= 1;
            evict.push(b.id.clone());
        }
        let mut report = PurgeReport::default();
        for id in evict {
            let r = self.delete_batch(&id)?;
            report.batches += r.batches;
            report.files += r.files;
            report.bytes += r.bytes;
            report.failed += r.failed;
        }
        Ok(report)
    }

    /// Permanently deletes one batch.
    pub fn delete_batch(&self, batch_id: &str) -> Result<PurgeReport> {
        let batch =
            match self.with_index(|i| Ok(i.batches.iter().find(|b| b.id == batch_id).cloned()))? {
                Some(b) => b,
                None => return Ok(PurgeReport::default()),
            };
        let mut report = PurgeReport {
            batches: 1,
            ..Default::default()
        };
        let mut kept = Vec::new();
        let mut log_entries = Vec::new();
        let now = crate::scan::now_unix();
        for item in &batch.items {
            match self.purge_file(&item.stored) {
                Ok(()) => {
                    report.files += 1;
                    report.bytes += item.size;
                    log_entries.push((item.original.clone(), item.size, item.tier));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    report.failed += 1;
                    kept.push(item.clone());
                }
            }
        }
        self.with_index(|i| {
            if let Some(b) = i.batches.iter_mut().find(|b| b.id == batch_id) {
                b.items = kept;
            }
            i.batches.retain(|b| !b.items.is_empty());
            Ok(())
        })?;
        self.remove_empty_batch_dirs(batch_id);
        let entries: Vec<log::Entry<'_>> = log_entries
            .iter()
            .map(|(p, size, tier)| log::Entry {
                time: now,
                batch: Some(batch_id),
                action: Action::Purged,
                tier: Some(*tier),
                path: p,
                size: *size,
                detail: None,
            })
            .collect();
        let _ = log::append(&self.data_dir, &entries);
        Ok(report)
    }

    /// Permanently deletes everything in the bin.
    pub fn empty(&self) -> Result<PurgeReport> {
        let ids: Vec<String> =
            self.with_index(|i| Ok(i.batches.iter().map(|b| b.id.clone()).collect()))?;
        let mut report = PurgeReport::default();
        for id in ids {
            let r = self.delete_batch(&id)?;
            report.batches += r.batches;
            report.files += r.files;
            report.bytes += r.bytes;
            report.failed += r.failed;
        }
        Ok(report)
    }

    /// Deletes a file inside the bin: never follows links and never acts
    /// outside the bin.
    fn purge_file(&self, stored: &Path) -> std::io::Result<()> {
        let h = fsops::open(stored)?;
        let info = h.info()?;
        let fin = h.final_path()?;
        let bin = self.bin_dir_for(stored).map_err(std::io::Error::other)?;
        let bin = std::fs::canonicalize(&bin).unwrap_or(bin);
        if info.is_dir || info.is_special || !Guard::is_within(&fin, &bin) {
            return Err(std::io::Error::other("not a file in the recovery bin"));
        }
        h.delete()
    }

    fn remove_empty_batch_dirs(&self, batch_id: &str) {
        for bin in self.existing_bins() {
            let dir = bin.join(batch_id);
            if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir() && !is_reparse(&m)) {
                let _ = std::fs::remove_dir(&dir);
            }
        }
    }

    /// Restores items of a batch (all of them when `item_ids` is `None`) to
    /// their original locations.
    pub fn restore(
        &self,
        batch_id: &str,
        item_ids: Option<&[u32]>,
        conflict: Conflict,
        guard: &Guard,
    ) -> Result<RestoreReport> {
        let batch = self.batch(batch_id)?;
        let selected: Vec<BinItem> = batch
            .items
            .iter()
            .filter(|i| item_ids.is_none_or(|ids| ids.contains(&i.id)))
            .cloned()
            .collect();
        let mut report = RestoreReport::default();
        let mut done_ids = Vec::new();
        let mut to_replace = Vec::new();
        let now = crate::scan::now_unix();

        if conflict == Conflict::Overwrite {
            for item in &selected {
                if std::fs::symlink_metadata(&item.original).is_ok() {
                    to_replace.push(item.original.clone());
                }
            }
            if !to_replace.is_empty() {
                // Existing files go into the bin first, so "overwrite" can be
                // undone too.
                let new: Vec<NewItem> = to_replace
                    .iter()
                    .map(|p| NewItem {
                        original: p.clone(),
                        size: std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0),
                        tier: Tier::Red,
                        rule: None,
                    })
                    .collect();
                let prepared = self.prepare_batch("Replaced while restoring", &new, now + 1)?;
                let mut moved = Vec::new();
                for (item, dest) in new.iter().zip(&prepared.stored) {
                    let ok = match dest {
                        Some(dest) => move_checked(&item.original, dest, guard, None).is_ok(),
                        None => false,
                    };
                    moved.push(ok);
                }
                self.finish_batch(&prepared.id, &moved)?;
            }
        }

        let mut log_entries = Vec::new();
        for item in &selected {
            let mut target = item.original.clone();
            if std::fs::symlink_metadata(&target).is_ok() {
                match conflict {
                    Conflict::Skip | Conflict::Overwrite => {
                        report.skipped.push((
                            item.original.clone(),
                            "A file with that name already exists".into(),
                        ));
                        continue;
                    }
                    Conflict::KeepBoth => match unique_name(&target) {
                        Some(t) => {
                            target = t;
                            report.renamed.push(target.clone());
                        }
                        None => {
                            report
                                .skipped
                                .push((item.original.clone(), "No free name to restore to".into()));
                            continue;
                        }
                    },
                }
            }
            if let Err(why) = prepare_parent(&target, guard) {
                report.skipped.push((item.original.clone(), why));
                continue;
            }
            let bin = self.bin_dir_for(&item.stored).ok();
            match move_checked(&item.stored, &target, guard, bin.as_deref()) {
                Ok(()) => {
                    report.restored += 1;
                    report.restored_bytes += item.size;
                    done_ids.push(item.id);
                    log_entries.push((target, item.size, item.tier));
                }
                Err(why) => report.skipped.push((item.original.clone(), why)),
            }
        }
        self.with_index(|i| {
            if let Some(b) = i.batches.iter_mut().find(|b| b.id == batch_id) {
                b.items.retain(|it| !done_ids.contains(&it.id));
            }
            i.batches.retain(|b| !b.items.is_empty());
            Ok(())
        })?;
        self.remove_empty_batch_dirs(batch_id);
        let entries: Vec<log::Entry<'_>> = log_entries
            .iter()
            .map(|(p, size, tier)| log::Entry {
                time: now,
                batch: Some(batch_id),
                action: Action::Restored,
                tier: Some(*tier),
                path: p,
                size: *size,
                detail: None,
            })
            .collect();
        let _ = log::append(&self.data_dir, &entries);
        Ok(report)
    }
}

/// Moves `from` to `to` through a link-safe handle. When `must_be_in` is
/// given, the source must be inside that folder (the bin); otherwise the
/// source must pass the guard.
fn move_checked(
    from: &Path,
    to: &Path,
    guard: &Guard,
    must_be_in: Option<&Path>,
) -> Result<(), String> {
    let h = fsops::open(from).map_err(|e| fsops::describe_error(&e))?;
    let info = h.info().map_err(|e| fsops::describe_error(&e))?;
    if info.is_dir || info.is_special {
        return Err("Links, folders, and special files are not moved".into());
    }
    let fin = h.final_path().map_err(|e| fsops::describe_error(&e))?;
    match must_be_in {
        Some(bin) => {
            let bin = std::fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
            if !Guard::is_within(&fin, &bin) {
                return Err("The file is no longer in the recovery bin".into());
            }
        }
        None => guard.check(&fin).map_err(|b| b.to_string())?,
    }
    guard.check(to).map_err(|b| b.to_string())?;
    h.rename_no_replace(to)
        .map_err(|e| fsops::describe_error(&e))
}

/// Creates the restore target's parent folder if needed and makes sure it
/// is not reached through a link.
fn prepare_parent(target: &Path, guard: &Guard) -> Result<(), String> {
    guard.check(target).map_err(|b| b.to_string())?;
    let parent = target.parent().ok_or("No parent folder")?;
    if !parent.exists() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot recreate the folder: {e}"))?;
    }
    let real = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
    if !Guard::same_path(&real, parent) {
        return Err("The original folder is now a link to somewhere else".into());
    }
    Ok(())
}

/// `report.pdf` → `report (restored).pdf`, `report (restored 2).pdf`, …
fn unique_name(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..1000 {
        let suffix = if n == 1 {
            " (restored)".to_string()
        } else {
            format!(" (restored {n})")
        };
        let candidate = parent.join(format!("{stem}{suffix}{ext}"));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return Some(candidate);
        }
    }
    None
}

/// Fixes up pending items left by an interrupted cleanup.
fn reconcile(index: &mut Index) {
    for b in &mut index.batches {
        b.items.retain_mut(|item| {
            if item.state == ItemState::Pending {
                if std::fs::symlink_metadata(&item.stored).is_ok() {
                    item.state = ItemState::InBin;
                    return true;
                }
                // Never moved (or already gone): nothing to recover.
                return false;
            }
            true
        });
    }
    index.batches.retain(|b| !b.items.is_empty());
}

fn batch_id(now: i64) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let (y, mo, d, h, mi, s) = crate::format::unix_to_utc(now);
    format!(
        "{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z-{:04x}{:02x}",
        std::process::id() & 0xFFFF,
        SEQ.fetch_add(1, Ordering::Relaxed) & 0xFF
    )
}

/// A short, stable key for the current user, so users sharing a drive keep
/// separate bins.
fn user_key() -> String {
    let who = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default()
        .to_lowercase();
    let mut hash: u32 = 0x811c_9dc5;
    for b in who.bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("u{hash:08x}")
}

/// `C:\` for any path on drive C.
pub fn drive_root(path: &Path) -> Option<PathBuf> {
    let s = path.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let b = s.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some(PathBuf::from(format!(
            "{}:\\",
            (b[0] as char).to_ascii_uppercase()
        )));
    }
    if !cfg!(windows) && s.starts_with('/') {
        return Some(PathBuf::from("/"));
    }
    None
}

#[cfg(windows)]
fn is_reparse(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse(m: &std::fs::Metadata) -> bool {
    m.file_type().is_symlink()
}

/// Makes sure `dir` is a real folder (not a link), creating it with
/// `create` when it is missing.
fn ensure_real_dir(dir: &Path, create: impl FnOnce() -> std::io::Result<()>) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() && !is_reparse(&m) => Ok(()),
        Ok(_) => Err("the recovery bin folder is not a real folder".into()),
        Err(_) => {
            create().map_err(|e| format!("cannot create the recovery bin: {e}"))?;
            match std::fs::symlink_metadata(dir) {
                Ok(m) if m.is_dir() && !is_reparse(&m) => Ok(()),
                _ => Err("the recovery bin folder is not a real folder".into()),
            }
        }
    }
}

/// Bin folders at the root of drives other than the data folder's.
#[cfg(windows)]
mod shared_bin {
    use std::path::Path;

    use super::{ensure_real_dir, set_hidden};
    use crate::winsec;

    /// Creates or checks `root` (shared by all users of the drive) and `dir`
    /// (private to this user).
    pub fn prepare(drive: &Path, root: &Path, dir: &Path) -> Result<(), String> {
        // FAT and exFAT drives keep no owners or permissions.
        let has_acls = matches!(
            crate::platform::file_system(drive).as_deref(),
            Some("NTFS" | "ReFS")
        );
        ensure_real_dir(root, || {
            if has_acls {
                winsec::create_dir(root, winsec::DirAccess::SharedRoot)?;
            } else {
                std::fs::create_dir(root)?;
            }
            set_hidden(root);
            Ok(())
        })?;
        ensure_real_dir(dir, || {
            if has_acls {
                winsec::create_dir(dir, winsec::DirAccess::Private)
            } else {
                std::fs::create_dir(dir)
            }
        })?;
        if has_acls {
            match winsec::owned_by_user_or_admins(dir) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(
                        "the recovery bin folder on this drive belongs to another user".into(),
                    );
                }
                Err(e) => return Err(format!("cannot check the recovery bin folder: {e}")),
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod shared_bin {
    use std::path::Path;

    pub fn prepare(_drive: &Path, _root: &Path, _dir: &Path) -> Result<(), String> {
        Err("drive bins are only used on Windows".into())
    }
}

#[cfg(windows)]
fn set_hidden(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
    let w: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `w` is NUL-terminated.
    unsafe { SetFileAttributesW(w.as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) struct Fixture {
        pub tmp: tempfile::TempDir,
        pub store: RecoveryStore,
        pub guard: Guard,
    }

    pub(crate) fn fixture(limits: BinLimits) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let store = RecoveryStore::with_root(&data, &bin, limits);
        Fixture {
            store,
            guard: Guard::system(),
            tmp,
        }
    }

    fn put(f: &Fixture, name: &str, body: &[u8]) -> PathBuf {
        let p = f.tmp.path().join("files").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        std::fs::canonicalize(p).unwrap()
    }

    /// Moves files into a new batch the way the cleaner does.
    fn bin_files(f: &Fixture, files: &[PathBuf], now: i64) -> String {
        let items: Vec<NewItem> = files
            .iter()
            .map(|p| NewItem {
                original: p.clone(),
                size: std::fs::metadata(p).unwrap().len(),
                tier: Tier::Yellow,
                rule: None,
            })
            .collect();
        let prepared = f.store.prepare_batch("Cleaner", &items, now).unwrap();
        let moved: Vec<bool> = items
            .iter()
            .zip(&prepared.stored)
            .map(|(i, s)| move_checked(&i.original, s.as_ref().unwrap(), &f.guard, None).is_ok())
            .collect();
        f.store.finish_batch(&prepared.id, &moved).unwrap();
        prepared.id
    }

    #[test]
    fn move_in_and_restore_round_trip() {
        let f = fixture(BinLimits::default());
        let a = put(&f, "docs/a.txt", b"alpha");
        let b = put(&f, "docs/deep/b.bin", &[7u8; 5000]);
        let id = bin_files(&f, &[a.clone(), b.clone()], 1_000);
        assert!(!a.exists() && !b.exists());
        let list = f.store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].items, 2);
        assert_eq!(list[0].size, 5005);

        // Restore recreates missing folders.
        std::fs::remove_dir(b.parent().unwrap()).unwrap();
        let r = f
            .store
            .restore(&id, None, Conflict::Skip, &f.guard)
            .unwrap();
        assert_eq!(r.restored, 2, "{:?}", r.skipped);
        assert_eq!(std::fs::read(&a).unwrap(), b"alpha");
        assert_eq!(std::fs::read(&b).unwrap().len(), 5000);
        assert!(f.store.list().unwrap().is_empty());
    }

    /// A normally opened store keeps the bin for the data folder's drive
    /// inside the (private) data folder, never at the drive root.
    #[test]
    fn bin_for_the_data_drive_is_inside_the_data_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let store = RecoveryStore::open(&data, BinLimits::default());
        let f = Fixture {
            store,
            guard: Guard::system(),
            tmp,
        };
        let a = put(&f, "a.txt", b"alpha");
        let id = bin_files(&f, std::slice::from_ref(&a), 1_000);
        let batch = f.store.batch(&id).unwrap();
        assert!(
            Guard::is_within(&batch.items[0].stored, &data.join("recovery").join("bin")),
            "{:?}",
            batch.items[0].stored
        );
        let r = f
            .store
            .restore(&id, None, Conflict::Skip, &f.guard)
            .unwrap();
        assert_eq!(r.restored, 1, "{:?}", r.skipped);
        assert_eq!(std::fs::read(&a).unwrap(), b"alpha");
    }

    #[test]
    fn restore_conflicts() {
        let f = fixture(BinLimits::default());
        let a = put(&f, "a.txt", b"old");
        let id = bin_files(&f, std::slice::from_ref(&a), 1_000);

        std::fs::write(&a, b"new").unwrap();
        let skip = f
            .store
            .restore(&id, None, Conflict::Skip, &f.guard)
            .unwrap();
        assert_eq!((skip.restored, skip.skipped.len()), (0, 1));
        assert_eq!(std::fs::read(&a).unwrap(), b"new");

        let both = f
            .store
            .restore(&id, None, Conflict::KeepBoth, &f.guard)
            .unwrap();
        assert_eq!(both.restored, 1);
        assert_eq!(std::fs::read(&both.renamed[0]).unwrap(), b"old");
        assert!(
            both.renamed[0]
                .to_string_lossy()
                .ends_with("a (restored).txt")
        );

        // Overwrite moves the current file into the bin first.
        let id2 = bin_files(&f, &[both.renamed[0].clone()], 2_000);
        let _ = id2;
        let c = put(&f, "c.txt", b"from bin");
        let id3 = bin_files(&f, std::slice::from_ref(&c), 3_000);
        std::fs::write(&c, b"current").unwrap();
        let over = f
            .store
            .restore(&id3, None, Conflict::Overwrite, &f.guard)
            .unwrap();
        assert_eq!(over.restored, 1);
        assert_eq!(std::fs::read(&c).unwrap(), b"from bin");
        let replaced = f.store.list().unwrap();
        assert!(
            replaced
                .iter()
                .any(|b| b.label == "Replaced while restoring")
        );
    }

    #[test]
    fn rolling_eviction_by_count_age_and_size() {
        let limits = BinLimits {
            max_bytes: 10_000,
            max_percent: 50,
            max_batches: 2,
            max_age_days: 14,
        };
        let f = fixture(limits);
        let mut ids = Vec::new();
        for (i, now) in [1_000, 2_000, 3_000].iter().enumerate() {
            let p = put(&f, &format!("f{i}.bin"), &[1u8; 100]);
            ids.push(bin_files(&f, &[p], *now));
        }
        let r = f.store.enforce_limits(3_000).unwrap();
        assert_eq!(r.batches, 1, "count limit drops the oldest");
        let left: Vec<String> = f.store.list().unwrap().into_iter().map(|b| b.id).collect();
        assert!(!left.contains(&ids[0]));

        // Age: everything but the newest is older than 14 days.
        let r = f.store.enforce_limits(3_000 + 15 * 86_400).unwrap();
        assert_eq!(r.batches, 1);
        assert_eq!(
            f.store.list().unwrap().len(),
            1,
            "newest batch is always kept"
        );

        // Size: a big new batch pushes the older one out.
        let big = put(&f, "big.bin", &vec![0u8; 9_950]);
        bin_files(&f, &[big], 4_000 + 15 * 86_400);
        let r = f.store.enforce_limits(4_000 + 15 * 86_400).unwrap();
        assert_eq!(r.batches, 1);
        assert_eq!(f.store.list().unwrap().len(), 1);
    }

    #[test]
    fn interrupted_cleanups_are_reconciled() {
        let f = fixture(BinLimits::default());
        let a = put(&f, "a.txt", b"a");
        let b = put(&f, "b.txt", b"b");
        let items: Vec<NewItem> = [&a, &b]
            .iter()
            .map(|p| NewItem {
                original: (*p).clone(),
                size: 1,
                tier: Tier::Yellow,
                rule: None,
            })
            .collect();
        let prepared = f.store.prepare_batch("Cleaner", &items, 1_000).unwrap();
        // Simulate a crash after moving only the first file.
        move_checked(&a, prepared.stored[0].as_ref().unwrap(), &f.guard, None).unwrap();
        let batch = f.store.batch(&prepared.id).unwrap();
        assert_eq!(batch.items.len(), 1, "unmoved pending item dropped");
        assert_eq!(batch.items[0].state, ItemState::InBin);
        assert!(b.exists());
    }

    #[test]
    fn purge_and_empty() {
        let f = fixture(BinLimits::default());
        let a = put(&f, "a.txt", b"aaaa");
        let id = bin_files(&f, &[a], 1_000);
        let b = put(&f, "b.txt", b"bb");
        bin_files(&f, &[b], 2_000);
        let r = f.store.delete_batch(&id).unwrap();
        assert_eq!((r.files, r.bytes), (1, 4));
        let r = f.store.empty().unwrap();
        assert_eq!(r.files, 1);
        assert!(f.store.list().unwrap().is_empty());
        let leftovers = std::fs::read_dir(f.tmp.path().join("bin/test"))
            .unwrap()
            .count();
        assert_eq!(leftovers, 0, "batch folders removed");
    }

    #[cfg(unix)]
    #[test]
    fn purge_never_follows_links_out_of_the_bin() {
        let f = fixture(BinLimits::default());
        let a = put(&f, "a.txt", b"aaaa");
        let id = bin_files(&f, &[a], 1_000);
        // Replace the stored file with a link to a file outside the bin.
        let outside = put(&f, "outside.txt", b"keep me");
        let stored = f.store.batch(&id).unwrap().items[0].stored.clone();
        std::fs::remove_file(&stored).unwrap();
        std::os::unix::fs::symlink(&outside, &stored).unwrap();
        let r = f.store.delete_batch(&id).unwrap();
        assert_eq!(r.failed, 1);
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep me");
    }

    #[test]
    fn unique_names() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("r.pdf");
        std::fs::write(&p, b"").unwrap();
        let n1 = unique_name(&p).unwrap();
        assert!(n1.ends_with("r (restored).pdf"));
        std::fs::write(&n1, b"").unwrap();
        assert!(unique_name(&p).unwrap().ends_with("r (restored 2).pdf"));
    }

    #[test]
    fn drive_roots() {
        assert_eq!(
            drive_root(Path::new(r"c:\x\y")),
            Some(PathBuf::from(r"C:\"))
        );
        assert_eq!(
            drive_root(Path::new(r"\\?\D:\x")),
            Some(PathBuf::from(r"D:\"))
        );
        assert_eq!(drive_root(Path::new(r"\\server\share")), None);
    }
}
