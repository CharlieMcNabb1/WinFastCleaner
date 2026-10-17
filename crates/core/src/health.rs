//! Health check: a read-only summary of what needs attention, each check
//! green, yellow, or red, with a link to where it can be fixed. Nothing here
//! changes anything.
//!
//! [`evaluate`] is pure: it turns already-gathered facts into checks, so
//! the thresholds are easy to test. The facts come from the drives, the last
//! junk scan and update check (cached in the data folder), the startup
//! manager, the recovery bin, and Windows' own drive health reports.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::format::format_size;
use crate::platform::{DriveInfo, DriveKind};
use crate::recovery::DriveUsage;

const GIB: u64 = 1024 * 1024 * 1024;

/// When each check turns yellow or red. Configurable in Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Thresholds {
    /// Yellow below this much free space, in percent of the drive.
    pub disk_free_yellow_pct: u8,
    pub disk_free_red_pct: u8,
    /// Yellow above this much cleanable junk, in GB.
    pub junk_yellow_gb: u32,
    pub junk_red_gb: u32,
    /// Yellow above this many apps starting at sign-in.
    pub startup_yellow: u32,
    pub startup_red: u32,
    /// Yellow when the recovery bin is this full, in percent of its limit.
    pub bin_yellow_pct: u8,
    /// Yellow when an SSD has used this much of its rated life, in percent.
    pub ssd_wear_yellow_pct: u8,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            disk_free_yellow_pct: 15,
            disk_free_red_pct: 5,
            junk_yellow_gb: 5,
            junk_red_gb: 20,
            startup_yellow: 10,
            startup_red: 20,
            bin_yellow_pct: 90,
            ssd_wear_yellow_pct: 80,
        }
    }
}

impl Thresholds {
    pub fn validate(&self) -> Result<(), String> {
        let t = self;
        if !(1..=99).contains(&t.disk_free_yellow_pct)
            || t.disk_free_red_pct == 0
            || t.disk_free_red_pct >= t.disk_free_yellow_pct
        {
            return Err("free space: red must be below yellow, both between 1% and 99%".into());
        }
        if t.junk_yellow_gb == 0 || t.junk_red_gb <= t.junk_yellow_gb || t.junk_red_gb > 100_000 {
            return Err("junk: red must be above yellow, and yellow at least 1 GB".into());
        }
        if t.startup_yellow == 0 || t.startup_red <= t.startup_yellow || t.startup_red > 1000 {
            return Err("startup apps: red must be above yellow, and yellow at least 1".into());
        }
        if !(50..=100).contains(&t.bin_yellow_pct) {
            return Err("recovery bin: between 50% and 100%".into());
        }
        if !(1..=100).contains(&t.ssd_wear_yellow_pct) {
            return Err("SSD wear: between 1% and 100%".into());
        }
        Ok(())
    }
}

/// A check's result. Always shown with a label, not color alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Not known (not checked yet, or not available).
    Unknown,
    Green,
    Yellow,
    Red,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckId {
    DiskSpace,
    Junk,
    Startup,
    DriveHealth,
    Temperatures,
    Updates,
    RecoveryBin,
}

/// Where the fix is (a tab in the app).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Fix {
    Disk,
    Cleaner,
    Startup,
    Updates,
    Recover,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: CheckId,
    pub title: String,
    pub status: Status,
    pub summary: String,
    pub details: Vec<String>,
    pub fix: Option<Fix>,
    /// When the facts were gathered, if they came from a cache (Unix
    /// seconds).
    pub as_of: Option<i64>,
}

/// The last junk scan, cached for the health check and the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JunkSummary {
    pub scanned_at: i64,
    /// Everything the rules found.
    pub bytes: u64,
    pub files: u64,
    /// What "Clean safe items" would remove (green, ready).
    pub safe_bytes: u64,
    pub safe_files: u64,
}

impl JunkSummary {
    pub fn from_scan(scan: &crate::cleaner::JunkScan) -> JunkSummary {
        let (safe_bytes, safe_files) = scan.safe_total();
        JunkSummary {
            scanned_at: scan.scanned_at,
            bytes: scan.rules.iter().map(|r| r.size).sum(),
            files: scan.rules.iter().map(|r| r.items.len() as u64).sum(),
            safe_bytes,
            safe_files: safe_files as u64,
        }
    }
}

/// The last update check, cached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesSummary {
    pub checked_at: i64,
    pub count: u32,
}

const JUNK_FILE: &str = "junk-summary.json";
const UPDATES_FILE: &str = "updates-summary.json";

fn save<T: Serialize>(data_dir: &Path, name: &str, value: &T) -> std::io::Result<()> {
    let json = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    crate::paths::write_atomic(&data_dir.join(name), &json)
}

fn load<T: for<'de> Deserialize<'de>>(data_dir: &Path, name: &str) -> Option<T> {
    let bytes = std::fs::read(data_dir.join(name)).ok()?;
    if bytes.len() > 64 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

pub fn save_junk(data_dir: &Path, s: &JunkSummary) -> std::io::Result<()> {
    save(data_dir, JUNK_FILE, s)
}

pub fn load_junk(data_dir: &Path) -> Option<JunkSummary> {
    load(data_dir, JUNK_FILE)
}

pub fn save_updates(data_dir: &Path, s: &UpdatesSummary) -> std::io::Result<()> {
    save(data_dir, UPDATES_FILE, s)
}

pub fn load_updates(data_dir: &Path) -> Option<UpdatesSummary> {
    load(data_dir, UPDATES_FILE)
}

// ------------------------------------------------------------ drive health

/// What Windows reports about a physical drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskStatus {
    Healthy,
    Warning,
    Unhealthy,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskHealth {
    pub name: String,
    /// "SSD", "HDD", or empty when unknown.
    pub media: String,
    pub status: DiskStatus,
    /// Percent of rated life used (SSDs), when Windows reports it.
    pub wear_pct: Option<u8>,
    /// Raw SMART counters, when readable (needs administrator rights).
    pub reallocated: Option<u64>,
    pub pending: Option<u64>,
    pub uncorrectable: Option<u64>,
}

/// Reallocated, pending, and uncorrectable sector counts.
pub type SmartCounts = (Option<u64>, Option<u64>, Option<u64>);

/// Reallocated, pending, and uncorrectable sector counts from a raw SMART
/// attribute table (`MSStorageDriver_FailurePredictData.VendorSpecific`):
/// a 2-byte version, then 30 entries of 12 bytes (id, flags, current,
/// worst, 6-byte raw value, reserved).
pub fn smart_counts(data: &[u8]) -> SmartCounts {
    let (mut realloc, mut pending, mut uncorrectable) = (None, None, None);
    for entry in data.get(2..).unwrap_or_default().chunks_exact(12).take(30) {
        let mut raw = [0u8; 8];
        raw[..6].copy_from_slice(&entry[5..11]);
        let value = u64::from_le_bytes(raw);
        match entry[0] {
            5 => realloc = Some(value),
            197 => pending = Some(value),
            198 => uncorrectable = Some(value),
            _ => {}
        }
    }
    (realloc, pending, uncorrectable)
}

/// Parses a list of bytes as WMI prints it (`1; 2; 3` or `1,2,3`).
pub fn parse_byte_list(s: &str) -> Vec<u8> {
    s.split([';', ','])
        .filter_map(|p| p.trim().parse::<u8>().ok())
        .collect()
}

/// Reads drive health from Windows (Storage Spaces' view of each physical
/// drive, plus reliability counters and raw SMART data where readable).
/// `None` when Windows can't be asked at all.
#[cfg(windows)]
pub fn read_disks() -> Option<Vec<DiskHealth>> {
    use std::collections::HashMap;

    use crate::wincom::wmi_query;

    let storage = r"ROOT\Microsoft\Windows\Storage";
    let disks = wmi_query(
        storage,
        "SELECT DeviceId, FriendlyName, MediaType, HealthStatus FROM MSFT_PhysicalDisk",
        &["DeviceId", "FriendlyName", "MediaType", "HealthStatus"],
    )
    .ok()?;
    let num = |m: &HashMap<String, Option<String>>, k: &str| {
        m.get(k)
            .cloned()
            .flatten()
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    let text = |m: &HashMap<String, Option<String>>, k: &str| {
        m.get(k)
            .cloned()
            .flatten()
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    // Reliability counters (wear), by DeviceId. Usually needs admin.
    let wear: HashMap<String, u8> = wmi_query(
        storage,
        "SELECT DeviceId, Wear FROM MSFT_StorageReliabilityCounter",
        &["DeviceId", "Wear"],
    )
    .unwrap_or_default()
    .iter()
    .filter_map(|m| Some((text(m, "DeviceId"), num(m, "Wear")?.min(100) as u8)))
    .collect();

    // Raw SMART data (ATA drives only, needs admin), matched to the disk
    // number through Win32_DiskDrive.
    let index_by_pnp: HashMap<String, String> = wmi_query(
        r"ROOT\CIMV2",
        "SELECT Index, PNPDeviceID FROM Win32_DiskDrive",
        &["Index", "PNPDeviceID"],
    )
    .unwrap_or_default()
    .iter()
    .map(|m| {
        (
            text(m, "PNPDeviceID").to_ascii_lowercase(),
            text(m, "Index"),
        )
    })
    .collect();
    let smart: HashMap<String, SmartCounts> = wmi_query(
        r"ROOT\WMI",
        "SELECT InstanceName, VendorSpecific FROM MSStorageDriver_FailurePredictData",
        &["InstanceName", "VendorSpecific"],
    )
    .unwrap_or_default()
    .iter()
    .filter_map(|m| {
        let instance = text(m, "InstanceName").to_ascii_lowercase();
        let pnp = instance
            .rsplit_once('_')
            .map(|(p, _)| p)
            .unwrap_or(&instance);
        let index = index_by_pnp.get(pnp)?.clone();
        Some((
            index,
            smart_counts(&parse_byte_list(&text(m, "VendorSpecific"))),
        ))
    })
    .collect();

    Some(
        disks
            .iter()
            .map(|m| {
                let id = text(m, "DeviceId");
                let (reallocated, pending, uncorrectable) =
                    smart.get(&id).copied().unwrap_or((None, None, None));
                DiskHealth {
                    name: Some(text(m, "FriendlyName"))
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| format!("Disk {id}")),
                    media: match num(m, "MediaType") {
                        Some(3) => "HDD".into(),
                        Some(4) => "SSD".into(),
                        _ => String::new(),
                    },
                    status: match num(m, "HealthStatus") {
                        Some(0) => DiskStatus::Healthy,
                        Some(1) => DiskStatus::Warning,
                        Some(2) => DiskStatus::Unhealthy,
                        _ => DiskStatus::Unknown,
                    },
                    wear_pct: wear.get(&id).copied(),
                    reallocated,
                    pending,
                    uncorrectable,
                }
            })
            .collect(),
    )
}

#[cfg(not(windows))]
pub fn read_disks() -> Option<Vec<DiskHealth>> {
    None
}

// ------------------------------------------------------------ evaluate

/// Everything the checks look at.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub drives: Vec<DriveInfo>,
    pub junk: Option<JunkSummary>,
    /// Apps that start at sign-in and are turned on.
    pub startup_enabled: Option<usize>,
    pub disks: Option<Vec<DiskHealth>>,
    pub winget_installed: bool,
    pub updates: Option<UpdatesSummary>,
    pub bin: Option<Vec<DriveUsage>>,
}

/// Runs every check. Pure: the same inputs give the same results.
pub fn evaluate(i: &Inputs, t: &Thresholds) -> Vec<Check> {
    vec![
        disk_space(&i.drives, t),
        junk(i.junk.as_ref(), t),
        startup(i.startup_enabled, t),
        drive_health(i.disks.as_deref(), t),
        temperatures(),
        updates(i.winget_installed, i.updates.as_ref()),
        recovery_bin(i.bin.as_deref(), t),
    ]
}

/// The worst status, counting only checks that ran.
pub fn overall(checks: &[Check]) -> Status {
    checks
        .iter()
        .map(|c| c.status)
        .max()
        .unwrap_or(Status::Unknown)
}

fn check(id: CheckId, title: &str, status: Status, summary: String) -> Check {
    Check {
        id,
        title: title.into(),
        status,
        summary,
        details: Vec::new(),
        fix: None,
        as_of: None,
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn disk_space(drives: &[DriveInfo], t: &Thresholds) -> Check {
    let title = "Disk space";
    let mut rows: Vec<(Status, f64, &DriveInfo)> = drives
        .iter()
        .filter(|d| d.kind == DriveKind::Fixed && d.total > 0)
        .map(|d| {
            let pct = d.free as f64 * 100.0 / d.total as f64;
            let status = if pct < f64::from(t.disk_free_red_pct) {
                Status::Red
            } else if pct < f64::from(t.disk_free_yellow_pct) {
                Status::Yellow
            } else {
                Status::Green
            };
            (status, pct, d)
        })
        .collect();
    if rows.is_empty() {
        return check(
            CheckId::DiskSpace,
            title,
            Status::Unknown,
            "No drives to check.".into(),
        );
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
    let describe = |(_, pct, d): &(Status, f64, &DriveInfo)| {
        format!(
            "{} has {:.0}% free ({} of {}).",
            d.root.trim_end_matches('\\'),
            pct.floor(),
            format_size(d.free),
            format_size(d.total)
        )
    };
    let worst = &rows[0];
    let mut c = if worst.0 == Status::Green {
        check(
            CheckId::DiskSpace,
            title,
            Status::Green,
            format!("Every drive has at least {}% free.", t.disk_free_yellow_pct),
        )
    } else {
        let mut c = check(CheckId::DiskSpace, title, worst.0, describe(worst));
        c.fix = Some(Fix::Cleaner);
        c
    };
    c.details = rows.iter().map(describe).collect();
    c
}

fn junk(j: Option<&JunkSummary>, t: &Thresholds) -> Check {
    let title = "Junk";
    let Some(j) = j else {
        let mut c = check(
            CheckId::Junk,
            title,
            Status::Unknown,
            "Not scanned yet.".into(),
        );
        c.fix = Some(Fix::Cleaner);
        return c;
    };
    let status = if j.bytes > u64::from(t.junk_red_gb) * GIB {
        Status::Red
    } else if j.bytes > u64::from(t.junk_yellow_gb) * GIB {
        Status::Yellow
    } else {
        Status::Green
    };
    let mut c = check(
        CheckId::Junk,
        title,
        status,
        format!(
            "{} can be cleaned ({}).",
            format_size(j.bytes),
            plural(j.files, "file", "files")
        ),
    );
    if j.safe_bytes > 0 {
        c.details.push(format!(
            "{} of it is safe to clean without a second thought.",
            format_size(j.safe_bytes)
        ));
    }
    c.fix = (status != Status::Green).then_some(Fix::Cleaner);
    c.as_of = Some(j.scanned_at);
    c
}

fn startup(n: Option<usize>, t: &Thresholds) -> Check {
    let title = "Startup apps";
    let Some(n) = n else {
        return check(
            CheckId::Startup,
            title,
            Status::Unknown,
            "The startup list couldn't be read.".into(),
        );
    };
    let status = if n > t.startup_red as usize {
        Status::Red
    } else if n > t.startup_yellow as usize {
        Status::Yellow
    } else {
        Status::Green
    };
    let mut c = check(
        CheckId::Startup,
        title,
        status,
        format!(
            "{} when you sign in.",
            plural(n as u64, "app starts", "apps start")
        ),
    );
    if status != Status::Green {
        c.details.push(
            "Each one slows down signing in. Turn off the ones you don't need right away.".into(),
        );
        c.fix = Some(Fix::Startup);
    }
    c
}

fn drive_health(disks: Option<&[DiskHealth]>, t: &Thresholds) -> Check {
    let title = "Drive health";
    let Some(disks) = disks else {
        return check(
            CheckId::DriveHealth,
            title,
            Status::Unknown,
            "Windows didn't report drive health.".into(),
        );
    };
    if disks.is_empty() {
        return check(
            CheckId::DriveHealth,
            title,
            Status::Unknown,
            "No drives reported their health.".into(),
        );
    }
    let mut worst: Option<(Status, String)> = None;
    let mut details = Vec::new();
    for d in disks {
        let name = if d.media.is_empty() {
            d.name.clone()
        } else {
            format!("{} ({})", d.name, d.media)
        };
        let mut problems = Vec::new();
        let mut status = match d.status {
            DiskStatus::Healthy => Status::Green,
            DiskStatus::Warning => {
                problems.push("Windows reports a warning".to_string());
                Status::Yellow
            }
            DiskStatus::Unhealthy => {
                problems.push("Windows reports it is failing".to_string());
                Status::Red
            }
            DiskStatus::Unknown => Status::Unknown,
        };
        if let Some(w) = d.wear_pct.filter(|&w| w >= t.ssd_wear_yellow_pct) {
            problems.push(format!("{w}% of its rated life is used"));
            status = status.max(Status::Yellow);
        }
        for (count, what) in [
            (d.reallocated, "reallocated sectors"),
            (d.pending, "sectors waiting to be reallocated"),
            (d.uncorrectable, "sectors that can't be read"),
        ] {
            if let Some(n) = count.filter(|&n| n > 0) {
                problems.push(format!("{n} {what}"));
                status = status.max(Status::Yellow);
            }
        }
        let line = if problems.is_empty() {
            let mut s = format!(
                "{name}: {}",
                match d.status {
                    DiskStatus::Healthy => "healthy",
                    _ => "health not reported",
                }
            );
            if let Some(w) = d.wear_pct {
                s.push_str(&format!(", {w}% of rated life used"));
            }
            s
        } else {
            format!("{name}: {}", problems.join(", "))
        };
        details.push(line.clone());
        if worst.as_ref().is_none_or(|(s, _)| status > *s) {
            worst = Some((status, line));
        }
    }
    let (status, line) = worst.unwrap_or((Status::Unknown, String::new()));
    let summary = match status {
        Status::Red => format!("{line}. Back up your files now."),
        Status::Yellow => format!("{line}. Keep your backups up to date."),
        Status::Green => {
            let healthy = disks
                .iter()
                .filter(|d| d.status == DiskStatus::Healthy)
                .count();
            match (healthy, disks.len()) {
                (1, 1) => "Your drive reports it is healthy.".to_string(),
                (h, n) if h == n => format!("All {n} drives report they are healthy."),
                (h, n) => {
                    format!("{h} of {n} drives report they are healthy; the others didn't say.")
                }
            }
        }
        Status::Unknown => "Your drives didn't report their health.".into(),
    };
    let mut c = check(CheckId::DriveHealth, title, status, summary);
    c.details = details;
    c
}

fn temperatures() -> Check {
    let mut c = check(
        CheckId::Temperatures,
        "Temperatures",
        Status::Unknown,
        "Not available yet.".into(),
    );
    c.details.push(
        "Reading CPU and GPU temperatures needs a hardware sensor driver, which WinFastCleaner \
         doesn't install."
            .into(),
    );
    c
}

fn updates(installed: bool, u: Option<&UpdatesSummary>) -> Check {
    let title = "App updates";
    if !installed {
        return check(
            CheckId::Updates,
            title,
            Status::Unknown,
            "winget isn't installed, so updates can't be checked.".into(),
        );
    }
    let Some(u) = u else {
        let mut c = check(
            CheckId::Updates,
            title,
            Status::Unknown,
            "Not checked yet.".into(),
        );
        c.fix = Some(Fix::Updates);
        return c;
    };
    let mut c = if u.count > 0 {
        let mut c = check(
            CheckId::Updates,
            title,
            Status::Yellow,
            format!(
                "{} available.",
                plural(u64::from(u.count), "app update", "app updates")
            ),
        );
        c.fix = Some(Fix::Updates);
        c
    } else {
        check(
            CheckId::Updates,
            title,
            Status::Green,
            "Your apps are up to date.".into(),
        )
    };
    c.as_of = Some(u.checked_at);
    c
}

fn recovery_bin(bin: Option<&[DriveUsage]>, t: &Thresholds) -> Check {
    let title = "Recovery bin";
    let Some(bin) = bin else {
        return check(
            CheckId::RecoveryBin,
            title,
            Status::Unknown,
            "The recovery bin couldn't be read.".into(),
        );
    };
    let total: u64 = bin.iter().map(|d| d.bytes).sum();
    let full = bin
        .iter()
        .filter(|d| d.limit > 0 && d.limit != u64::MAX)
        .map(|d| (d, d.bytes.saturating_mul(100) / d.limit))
        .filter(|(_, pct)| *pct >= u64::from(t.bin_yellow_pct))
        .max_by_key(|(_, pct)| *pct);
    let mut c = match full {
        Some((d, pct)) => {
            let mut c = check(
                CheckId::RecoveryBin,
                title,
                Status::Yellow,
                format!(
                    "The recovery bin is {pct}% full on {} ({} of {}).",
                    d.drive.trim_end_matches('\\'),
                    format_size(d.bytes),
                    format_size(d.limit)
                ),
            );
            c.details.push(
                "When it fills up, the oldest batches are deleted for good to make room.".into(),
            );
            c.fix = Some(Fix::Recover);
            c
        }
        None if total == 0 => check(
            CheckId::RecoveryBin,
            title,
            Status::Green,
            "The recovery bin is empty.".into(),
        ),
        None => check(
            CheckId::RecoveryBin,
            title,
            Status::Green,
            format!("{} in the recovery bin.", format_size(total)),
        ),
    };
    if full.is_none() && total > 0 {
        c.fix = Some(Fix::Recover);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(root: &str, total_gb: u64, free_gb: u64) -> DriveInfo {
        DriveInfo {
            root: root.into(),
            label: String::new(),
            file_system: "NTFS".into(),
            kind: DriveKind::Fixed,
            total: total_gb * GIB,
            free: free_gb * GIB,
            is_system: root == r"C:\",
        }
    }

    fn get(checks: &[Check], id: CheckId) -> &Check {
        checks.iter().find(|c| c.id == id).unwrap()
    }

    fn healthy_disk() -> DiskHealth {
        DiskHealth {
            name: "Disk".into(),
            media: "SSD".into(),
            status: DiskStatus::Healthy,
            wear_pct: Some(3),
            reallocated: None,
            pending: None,
            uncorrectable: None,
        }
    }

    #[test]
    fn disk_space_uses_percent_free() {
        let t = Thresholds::default();
        let c = disk_space(&[drive(r"C:\", 100, 20), drive(r"D:\", 100, 50)], &t);
        assert_eq!(c.status, Status::Green);
        assert_eq!(c.fix, None);
        let c = disk_space(&[drive(r"C:\", 100, 20), drive(r"D:\", 100, 14)], &t);
        assert_eq!(c.status, Status::Yellow);
        assert!(c.summary.starts_with("D: has 14% free"), "{}", c.summary);
        assert_eq!(c.fix, Some(Fix::Cleaner));
        let c = disk_space(&[drive(r"C:\", 100, 4), drive(r"D:\", 100, 14)], &t);
        assert_eq!(c.status, Status::Red);
        assert!(c.summary.starts_with("C: has 4% free"), "{}", c.summary);
        assert_eq!(c.details.len(), 2);
        // Exactly at the line is not below it.
        assert_eq!(
            disk_space(&[drive(r"C:\", 100, 15)], &t).status,
            Status::Green
        );
        assert_eq!(
            disk_space(&[drive(r"C:\", 100, 5)], &t).status,
            Status::Yellow
        );
        // Removable and network drives don't count.
        let mut usb = drive(r"E:\", 100, 1);
        usb.kind = DriveKind::Removable;
        assert_eq!(disk_space(&[usb], &t).status, Status::Unknown);
    }

    #[test]
    fn junk_startup_and_updates_thresholds() {
        let t = Thresholds::default();
        let j = |gb: u64| JunkSummary {
            scanned_at: 1,
            bytes: gb * GIB,
            files: 10,
            safe_bytes: GIB,
            safe_files: 2,
        };
        assert_eq!(junk(None, &t).status, Status::Unknown);
        assert_eq!(junk(Some(&j(5)), &t).status, Status::Green);
        assert_eq!(junk(Some(&j(6)), &t).status, Status::Yellow);
        assert_eq!(junk(Some(&j(21)), &t).status, Status::Red);
        assert_eq!(junk(Some(&j(21)), &t).as_of, Some(1));

        assert_eq!(startup(Some(10), &t).status, Status::Green);
        assert_eq!(startup(Some(11), &t).status, Status::Yellow);
        assert_eq!(startup(Some(21), &t).status, Status::Red);
        assert_eq!(
            startup(Some(1), &t).summary,
            "1 app starts when you sign in."
        );
        assert_eq!(startup(None, &t).status, Status::Unknown);

        assert_eq!(updates(false, None).status, Status::Unknown);
        assert_eq!(updates(true, None).fix, Some(Fix::Updates));
        let u = |count| UpdatesSummary {
            checked_at: 5,
            count,
        };
        assert_eq!(updates(true, Some(&u(0))).status, Status::Green);
        let c = updates(true, Some(&u(3)));
        assert_eq!(c.status, Status::Yellow);
        assert_eq!(c.summary, "3 app updates available.");
        assert_eq!(
            updates(true, Some(&u(1))).summary,
            "1 app update available."
        );
    }

    #[test]
    fn drive_health_flags_warnings_and_failures() {
        let t = Thresholds::default();
        assert_eq!(drive_health(None, &t).status, Status::Unknown);
        assert_eq!(drive_health(Some(&[]), &t).status, Status::Unknown);
        let ok = healthy_disk();
        assert_eq!(
            drive_health(Some(std::slice::from_ref(&ok)), &t).status,
            Status::Green
        );

        let worn = DiskHealth {
            wear_pct: Some(85),
            ..ok.clone()
        };
        let c = drive_health(Some(&[ok.clone(), worn]), &t);
        assert_eq!(c.status, Status::Yellow);
        assert!(c.summary.contains("85% of its rated life"), "{}", c.summary);

        let realloc = DiskHealth {
            reallocated: Some(8),
            ..ok.clone()
        };
        assert_eq!(drive_health(Some(&[realloc]), &t).status, Status::Yellow);

        let failing = DiskHealth {
            status: DiskStatus::Unhealthy,
            ..ok.clone()
        };
        let c = drive_health(Some(&[ok.clone(), failing]), &t);
        assert_eq!(c.status, Status::Red);
        assert!(c.summary.contains("Back up"), "{}", c.summary);

        let unknown = DiskHealth {
            status: DiskStatus::Unknown,
            wear_pct: None,
            ..ok.clone()
        };
        assert_eq!(
            drive_health(Some(std::slice::from_ref(&unknown)), &t).status,
            Status::Unknown
        );
        assert_eq!(drive_health(Some(&[unknown, ok]), &t).status, Status::Green);
    }

    #[test]
    fn recovery_bin_near_its_cap() {
        let t = Thresholds::default();
        let u = |bytes, limit| DriveUsage {
            drive: r"C:\".into(),
            bytes,
            limit,
        };
        assert_eq!(
            recovery_bin(Some(&[]), &t).summary,
            "The recovery bin is empty."
        );
        assert_eq!(recovery_bin(Some(&[u(50, 100)]), &t).status, Status::Green);
        let c = recovery_bin(Some(&[u(95, 100)]), &t);
        assert_eq!(c.status, Status::Yellow);
        assert!(c.summary.contains("95% full on C:"), "{}", c.summary);
        assert_eq!(
            recovery_bin(Some(&[u(95, u64::MAX)]), &t).status,
            Status::Green
        );
        assert_eq!(recovery_bin(None, &t).status, Status::Unknown);
    }

    #[test]
    fn every_check_is_reported_and_temperatures_are_not_guessed() {
        let checks = evaluate(&Inputs::default(), &Thresholds::default());
        assert_eq!(checks.len(), 7);
        assert_eq!(get(&checks, CheckId::Temperatures).status, Status::Unknown);
        assert_eq!(overall(&checks), Status::Unknown);
        let checks = evaluate(
            &Inputs {
                drives: vec![drive(r"C:\", 100, 3)],
                ..Inputs::default()
            },
            &Thresholds::default(),
        );
        assert_eq!(overall(&checks), Status::Red);
    }

    #[test]
    fn thresholds_are_validated() {
        assert!(Thresholds::default().validate().is_ok());
        let bad = [
            Thresholds {
                disk_free_red_pct: 15,
                ..Thresholds::default()
            },
            Thresholds {
                junk_red_gb: 5,
                ..Thresholds::default()
            },
            Thresholds {
                startup_yellow: 0,
                ..Thresholds::default()
            },
            Thresholds {
                bin_yellow_pct: 10,
                ..Thresholds::default()
            },
            Thresholds {
                ssd_wear_yellow_pct: 0,
                ..Thresholds::default()
            },
        ];
        for b in bad {
            assert!(b.validate().is_err(), "{b:?}");
        }
    }

    #[test]
    fn smart_table_is_parsed() {
        let mut data = vec![0x10, 0x00];
        let mut entry = |id: u8, raw: u64| {
            let mut e = vec![id, 0x33, 0, 100, 100];
            e.extend_from_slice(&raw.to_le_bytes()[..6]);
            e.push(0);
            data.extend(e);
        };
        entry(9, 12345);
        entry(5, 7);
        entry(197, 0);
        entry(198, 0x0102_0304_0506);
        assert_eq!(
            smart_counts(&data),
            (Some(7), Some(0), Some(0x0102_0304_0506))
        );
        assert_eq!(smart_counts(&[]), (None, None, None));
        assert_eq!(parse_byte_list("16; 0; 5"), vec![16, 0, 5]);
        assert_eq!(parse_byte_list("1,2,x,300"), vec![1, 2]);
    }

    #[test]
    fn caches_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load_junk(tmp.path()), None);
        let j = JunkSummary {
            scanned_at: 9,
            bytes: 1,
            files: 2,
            safe_bytes: 3,
            safe_files: 4,
        };
        save_junk(tmp.path(), &j).unwrap();
        assert_eq!(load_junk(tmp.path()), Some(j));
        let u = UpdatesSummary {
            checked_at: 7,
            count: 2,
        };
        save_updates(tmp.path(), &u).unwrap();
        assert_eq!(load_updates(tmp.path()), Some(u));
        std::fs::write(tmp.path().join(UPDATES_FILE), "not json").unwrap();
        assert_eq!(load_updates(tmp.path()), None);
    }

    #[cfg(windows)]
    #[test]
    fn real_drive_health_can_be_read() {
        // CI machines have virtual disks; this only checks the queries work.
        let disks = read_disks().expect("Storage WMI");
        for d in &disks {
            assert!(!d.name.is_empty());
        }
    }
}
