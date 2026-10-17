//! User settings and the exclusions list, stored as JSON in the data folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::glob::Glob;
use crate::safety::Guard;
use crate::{Error, Result};

const FILE_NAME: &str = "settings.json";
const MAX_EXCLUSIONS: usize = 1000;

/// What an exclusion matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExclusionKind {
    /// A folder and everything inside it.
    Folder,
    /// One file.
    File,
    /// A pattern such as `**/*.psd` or `D:/Projects/**`.
    Pattern,
}

/// One "never touch" entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exclusion {
    pub kind: ExclusionKind,
    pub value: String,
}

/// Recovery bin limits. The oldest batches are deleted first when a new
/// batch pushes the bin over any of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BinLimits {
    /// Per drive, in bytes (the smaller of this and `max_percent` applies).
    pub max_bytes: u64,
    /// Per drive, as a percentage of the drive's size.
    pub max_percent: u8,
    pub max_batches: u32,
    pub max_age_days: u32,
}

impl Default for BinLimits {
    fn default() -> Self {
        Self {
            max_bytes: 5 * 1024 * 1024 * 1024,
            max_percent: 10,
            max_batches: 5,
            max_age_days: 14,
        }
    }
}

/// All user settings. Unknown or missing fields fall back to defaults, so
/// older and newer versions can share a settings file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub exclusions: Vec<Exclusion>,
    /// Send green items to the recovery bin too (off: they are deleted
    /// directly, which frees space immediately).
    pub green_to_bin: bool,
    pub bin: BinLimits,
}

impl Settings {
    /// Loads settings; a missing or unreadable file gives the defaults.
    pub fn load(data_dir: &Path) -> Settings {
        std::fs::read(data_dir.join(FILE_NAME))
            .ok()
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .map(Settings::sanitized)
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        self.validate()?;
        let json =
            serde_json::to_vec_pretty(self).map_err(|e| Error::InvalidInput(e.to_string()))?;
        let path = data_dir.join(FILE_NAME);
        crate::paths::write_atomic(&path, &json).map_err(|e| Error::io(path, e))
    }

    /// Checks values coming from the UI or a hand-edited file.
    pub fn validate(&self) -> Result<()> {
        if self.exclusions.len() > MAX_EXCLUSIONS {
            return Err(Error::InvalidInput(format!(
                "at most {MAX_EXCLUSIONS} exclusions are supported"
            )));
        }
        for e in &self.exclusions {
            validate_exclusion(e)?;
        }
        let b = &self.bin;
        if !(1..=50).contains(&b.max_percent)
            || b.max_batches == 0
            || b.max_batches > 100
            || b.max_age_days > 365
        {
            return Err(Error::InvalidInput(
                "recovery bin limits are out of range".into(),
            ));
        }
        Ok(())
    }

    fn sanitized(mut self) -> Settings {
        self.exclusions.retain(|e| validate_exclusion(e).is_ok());
        self.exclusions.truncate(MAX_EXCLUSIONS);
        if self.validate().is_err() {
            self.bin = BinLimits::default();
        }
        self
    }
}

/// Checks one exclusion entry.
pub fn validate_exclusion(e: &Exclusion) -> Result<()> {
    if e.value.trim().is_empty() || e.value.len() > 1024 || e.value.contains('\0') {
        return Err(Error::InvalidInput("exclusion is empty or too long".into()));
    }
    match e.kind {
        ExclusionKind::Folder | ExclusionKind::File => {
            crate::safety::normalize(Path::new(&e.value))
                .map(|_| ())
                .map_err(|b| {
                    Error::InvalidInput(format!("`{}` is not a usable path: {b}", e.value))
                })
        }
        ExclusionKind::Pattern => Glob::new(&e.value).map(|_| ()).map_err(Error::InvalidInput),
    }
}

/// Compiled exclusions: the "never touch" list. Exclusions always win over
/// every rule and selection.
#[derive(Clone, Debug, Default)]
pub struct Exclusions {
    folders: Vec<PathBuf>,
    files: Vec<PathBuf>,
    patterns: Vec<Glob>,
}

impl Exclusions {
    pub fn new(list: &[Exclusion]) -> Exclusions {
        let mut x = Exclusions::default();
        for e in list {
            match e.kind {
                ExclusionKind::Folder => x.folders.push(PathBuf::from(&e.value)),
                ExclusionKind::File => x.files.push(PathBuf::from(&e.value)),
                ExclusionKind::Pattern => {
                    // Like .gitignore: a pattern without a separator matches
                    // a file or folder name at any depth.
                    let full = if e.value.contains(['/', '\\']) {
                        e.value.clone()
                    } else {
                        format!("**/{}/**", e.value)
                    };
                    if let Ok(g) = Glob::new(&full) {
                        x.patterns.push(g);
                    }
                }
            }
        }
        x
    }

    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.files.is_empty() && self.patterns.is_empty()
    }

    /// Whether `path` (or a folder containing it) is excluded.
    pub fn is_excluded(&self, path: &Path) -> bool {
        if self.folders.iter().any(|f| Guard::is_within(path, f)) {
            return true;
        }
        if self.files.iter().any(|f| Guard::same_path(path, f)) {
            return true;
        }
        if !self.patterns.is_empty() {
            let s = path.to_string_lossy();
            let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
            return self.patterns.iter().any(|g| g.matches_path(s));
        }
        false
    }

    /// Whether removing folder `dir` could remove something excluded
    /// (an excluded folder or file inside it, or `dir` itself).
    pub fn protects_folder(&self, dir: &Path) -> bool {
        self.is_excluded(dir)
            || self.folders.iter().any(|f| Guard::is_within(f, dir))
            || self.files.iter().any(|f| Guard::is_within(f, dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(kind: ExclusionKind, value: &str) -> Exclusion {
        Exclusion {
            kind,
            value: value.into(),
        }
    }

    #[test]
    fn exclusions_match_folders_files_and_patterns() {
        let x = Exclusions::new(&[
            ex(ExclusionKind::Folder, r"C:\Users\test-user\Keep"),
            ex(
                ExclusionKind::File,
                r"C:\Users\test-user\Downloads\important.iso",
            ),
            ex(ExclusionKind::Pattern, "**/*.psd"),
        ]);
        assert!(x.is_excluded(Path::new(r"C:\Users\test-user\Keep\a\b.txt")));
        assert!(x.is_excluded(Path::new(r"c:\users\TEST-USER\keep")));
        assert!(!x.is_excluded(Path::new(r"C:\Users\test-user\Keeper\b.txt")));
        assert!(x.is_excluded(Path::new(r"\\?\C:\Users\test-user\Downloads\IMPORTANT.iso")));
        assert!(x.is_excluded(Path::new(r"D:\art\cover.PSD")));
        assert!(!x.is_excluded(Path::new(r"D:\art\cover.png")));
        assert!(x.protects_folder(Path::new(r"C:\Users\test-user")));
        assert!(!x.protects_folder(Path::new(r"C:\Users\test-user\Temp")));

        // Names without a separator match at any depth, files and folders.
        let x = Exclusions::new(&[
            ex(ExclusionKind::Pattern, "*.psd"),
            ex(ExclusionKind::Pattern, "keep-me"),
        ]);
        assert!(x.is_excluded(Path::new(r"D:\art\deep\cover.PSD")));
        assert!(x.is_excluded(Path::new(r"C:\a\keep-me\b\c.tmp")));
        assert!(x.is_excluded(Path::new(r"C:\a\keep-me")));
        assert!(!x.is_excluded(Path::new(r"C:\a\keep-me-not\c.tmp")));
    }

    #[test]
    fn settings_round_trip_and_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Settings::load(tmp.path());
        assert_eq!(s, Settings::default());
        s.green_to_bin = true;
        s.exclusions.push(ex(ExclusionKind::Pattern, "**/*.psd"));
        s.save(tmp.path()).unwrap();
        assert_eq!(Settings::load(tmp.path()), s);

        let mut bad = s.clone();
        bad.exclusions
            .push(ex(ExclusionKind::Folder, "relative/path"));
        assert!(bad.save(tmp.path()).is_err());
        let mut bad = s.clone();
        bad.bin.max_percent = 0;
        assert!(bad.validate().is_err());

        // A hand-edited file with junk keeps the valid parts.
        std::fs::write(
            tmp.path().join("settings.json"),
            r#"{"greenToBin": true, "exclusions": [{"kind":"pattern","value":"../x"},{"kind":"file","value":"C:\\a.txt"}], "future": 1}"#,
        )
        .unwrap();
        let loaded = Settings::load(tmp.path());
        assert!(loaded.green_to_bin);
        assert_eq!(loaded.exclusions.len(), 1);
    }
}
