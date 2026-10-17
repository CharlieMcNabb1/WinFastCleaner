//! The deletion safety rules (see `docs/safety-model.md`).
//!
//! Every path that could be deleted, moved into the recovery bin, or restored
//! goes through [`Guard::check`] first, using its **final resolved path**
//! (after environment variables, symlinks, and junctions have been
//! resolved). Anything the guard blocks is filtered out before results are
//! shown, so crucial Windows files never appear in the cleaner and can never
//! be selected, not even by hand.
//!
//! The guard compares paths as strings in a platform-independent way, so the
//! Windows rules are unit-tested on every platform.

use std::path::{Path, PathBuf};

/// Why a path is off limits. Shown in logs and, for manual selections, in
/// the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blocked(pub &'static str);

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Folder or file names that are protected wherever they appear.
const PROTECTED_NAMES: &[(&str, &str)] = &[
    ("winsxs", "Windows component store"),
    ("system32", "Windows system folder"),
    ("syswow64", "Windows system folder"),
    ("system volume information", "Windows system folder"),
    ("$recycle.bin", "Windows Recycle Bin"),
    ("$winreagent", "Windows recovery files"),
    ("$windows.~bt", "Windows upgrade files"),
    ("$windows.~ws", "Windows upgrade files"),
    ("$sysreset", "Windows reset files"),
    ("$getcurrent", "Windows upgrade files"),
    ("windowsapps", "Microsoft Store apps"),
    ("wpsystem", "Microsoft Store apps"),
    ("wudownloadcache", "Windows Update files"),
    (crate::recovery::BIN_DIR_NAME, "WinFastCleaner recovery bin"),
];

/// Top-level folders of any drive that belong to Windows.
const PROTECTED_ROOT_DIRS: &[&str] = &[
    "boot",
    "efi",
    "recovery",
    "config.msi",
    "documents and settings",
    "perflogs",
    "windows.old",
    "msocache",
];

/// Files directly at the root of any drive that Windows needs.
const PROTECTED_ROOT_FILES: &[&str] = &[
    "bootmgr",
    "bootnxt",
    "bootsect.bak",
    "pagefile.sys",
    "hiberfil.sys",
    "swapfile.sys",
    "dumpstack.log",
    "dumpstack.log.tmp",
    "boot.ini",
    "ntldr",
    "ntdetect.com",
    "io.sys",
    "msdos.sys",
    "config.sys",
    "autoexec.bat",
];

/// Subtrees of every user profile that hold Windows or security data.
/// (Paths are relative to the profile folder.)
const PROFILE_PROTECTED_DIRS: &[(&str, &str)] = &[
    (
        r"appdata\roaming\microsoft\protect",
        "Windows encryption keys",
    ),
    (
        r"appdata\roaming\microsoft\crypto",
        "Windows encryption keys",
    ),
    (
        r"appdata\roaming\microsoft\credentials",
        "saved Windows credentials",
    ),
    (
        r"appdata\local\microsoft\credentials",
        "saved Windows credentials",
    ),
    (
        r"appdata\roaming\microsoft\systemcertificates",
        "certificates",
    ),
    (
        r"appdata\local\microsoft\vault",
        "saved Windows credentials",
    ),
    (r"appdata\local\packages", "Microsoft Store app data"),
    (
        r"appdata\local\microsoft\windowsapps",
        "Microsoft Store apps",
    ),
];

/// File name prefixes in a profile root (registry hives) and in
/// `AppData\Local\Microsoft\Windows`.
const PROFILE_ROOT_FILE_PREFIXES: &[&str] = &["ntuser."];
const USRCLASS_DIR: &str = r"appdata\local\microsoft\windows";
const USRCLASS_PREFIX: &str = "usrclass.dat";

/// A path split into a lowercase drive (if any) and lowercase components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormPath {
    /// Uppercase drive letter for Windows-style paths, `None` for Unix paths.
    drive: Option<char>,
    comps: Vec<String>,
}

impl NormPath {
    fn starts_with(&self, prefix: &NormPath) -> bool {
        self.drive == prefix.drive
            && self.comps.len() >= prefix.comps.len()
            && self.comps[..prefix.comps.len()] == prefix.comps[..]
    }

    /// Path relative to `prefix` (which must be a prefix).
    fn rel<'a>(&'a self, prefix: &NormPath) -> &'a [String] {
        &self.comps[prefix.comps.len()..]
    }
}

/// Normalizes a path for comparison and rejects forms that could alias a
/// protected location or that the app never acts on.
pub fn normalize(path: &Path) -> Result<NormPath, Blocked> {
    let raw = path
        .to_str()
        .ok_or(Blocked("name contains invalid characters"))?;
    let mut s = raw;
    if let Some(rest) = s.strip_prefix(r"\\?\").or_else(|| s.strip_prefix("//?/")) {
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
            return Err(Blocked("network locations are not cleaned"));
        }
        s = rest;
    }
    if s.starts_with(r"\\") || s.starts_with("//") || s.starts_with(r"\??\") {
        return Err(Blocked("network and device paths are not cleaned"));
    }
    let bytes = s.as_bytes();
    let (drive, rest) = if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        (Some(bytes[0].to_ascii_uppercase() as char), &s[3..])
    } else if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        (Some(bytes[0].to_ascii_uppercase() as char), "")
    } else if !cfg!(windows) && s.starts_with('/') {
        (None, &s[1..])
    } else {
        return Err(Blocked("not a full local path"));
    };

    let windows_style = drive.is_some();
    let mut comps = Vec::new();
    for comp in rest.split(['\\', '/']) {
        if comp.is_empty() {
            continue;
        }
        if comp == "." || comp == ".." {
            return Err(Blocked("path contains . or .."));
        }
        if windows_style {
            if comp.contains(':') {
                return Err(Blocked("alternate data streams are not cleaned"));
            }
            if comp.ends_with('.') || comp.ends_with(' ') {
                return Err(Blocked("name ends with a dot or space"));
            }
            if looks_like_short_name(comp) {
                return Err(Blocked("short (8.3) names must be resolved first"));
            }
        }
        comps.push(comp.to_lowercase());
    }
    Ok(NormPath { drive, comps })
}

/// `PROGRA~1`, `ABCDEF~12.TXT`: Windows' legacy short names, which can alias
/// any long name, so they are never trusted.
fn looks_like_short_name(comp: &str) -> bool {
    let (stem, ext) = match comp.rsplit_once('.') {
        Some((s, e)) => (s, Some(e)),
        None => (comp, None),
    };
    let Some((head, tail)) = stem.split_once('~') else {
        return false;
    };
    !head.is_empty()
        && head.len() <= 6
        && !tail.is_empty()
        && tail.chars().all(|c| c.is_ascii_digit())
        && head.len() + 1 + tail.len() <= 8
        && ext.is_none_or(|e| e.len() <= 3)
}

/// The locations a [`Guard`] protects on Windows. Built from the running
/// system by [`Guard::system`], or by hand in tests.
#[derive(Clone, Debug, Default)]
pub struct WindowsLayout {
    pub system_root: PathBuf,
    pub program_files: Vec<PathBuf>,
    pub program_data: PathBuf,
    pub users_dir: PathBuf,
    /// Every user profile folder (`C:\Users\name`).
    pub profiles: Vec<PathBuf>,
    /// The app's own install and data folders, and its program file.
    pub app_dirs: Vec<PathBuf>,
}

/// Decides whether a path may be deleted, moved, or restored.
#[derive(Clone, Debug)]
pub struct Guard {
    /// Whole subtrees that are off limits.
    protected: Vec<(NormPath, &'static str)>,
    /// Exact folders that must never be removed themselves (their contents
    /// may still be cleanable).
    exact: Vec<(NormPath, &'static str)>,
    /// Junk folders inside protected areas that rules may clean.
    carve_outs: Vec<NormPath>,
    /// Exact files inside protected areas that rules may clean.
    carve_out_files: Vec<NormPath>,
    profiles: Vec<NormPath>,
    unix: bool,
}

fn norm(p: &Path) -> Option<NormPath> {
    normalize(p).ok()
}

impl Guard {
    /// The guard for the running system.
    pub fn system() -> Guard {
        #[cfg(windows)]
        {
            Guard::windows(&WindowsLayout::detect())
        }
        #[cfg(not(windows))]
        {
            Guard::unix(
                &crate::paths::default_data_dir()
                    .into_iter()
                    .collect::<Vec<_>>(),
            )
        }
    }

    /// A guard for a Windows system with the given layout.
    pub fn windows(layout: &WindowsLayout) -> Guard {
        let mut g = Guard {
            protected: Vec::new(),
            exact: Vec::new(),
            carve_outs: Vec::new(),
            carve_out_files: Vec::new(),
            profiles: Vec::new(),
            unix: false,
        };
        let mut protect = |p: &Path, why: &'static str| {
            if let Some(n) = norm(p) {
                g.protected.push((n, why));
            }
        };
        protect(&layout.system_root, "Windows folder");
        for pf in &layout.program_files {
            protect(pf, "installed programs");
        }
        protect(&layout.program_data, "shared app data (ProgramData)");
        for d in &layout.app_dirs {
            protect(d, "WinFastCleaner's own files");
        }
        let users = layout.users_dir.clone();
        for special in ["Default", "Default User", "All Users"] {
            protect(&users.join(special), "Windows default user profile");
        }

        for p in [&layout.users_dir] {
            if let Some(n) = norm(p) {
                g.exact.push((n, "the Users folder"));
            }
        }
        if let Some(n) = norm(&users.join("Public")) {
            g.exact.push((n, "the Public profile folder"));
        }
        for prof in &layout.profiles {
            if let Some(n) = norm(prof) {
                g.exact.push((n.clone(), "a user profile folder"));
                g.profiles.push(n);
            }
        }

        // Junk folders inside protected areas that rules may clean.
        let sr = &layout.system_root;
        let pd = &layout.program_data;
        for dir in [
            sr.join("Temp"),
            sr.join("Logs"),
            sr.join("Minidump"),
            sr.join("LiveKernelReports"),
            sr.join(r"SoftwareDistribution\Download"),
            sr.join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"),
            pd.join(r"Microsoft\Windows\WER\ReportArchive"),
            pd.join(r"Microsoft\Windows\WER\ReportQueue"),
            pd.join(r"Microsoft\Windows\WER\Temp"),
        ] {
            if let Some(n) = norm(&dir) {
                g.carve_outs.push(n);
            }
        }
        if let Some(n) = norm(&sr.join("MEMORY.DMP")) {
            g.carve_out_files.push(n);
        }
        g
    }

    /// A guard for Unix development and test machines: system folders are
    /// protected; temporary folders and home subfolders are not.
    pub fn unix(app_dirs: &[PathBuf]) -> Guard {
        let mut g = Guard {
            protected: Vec::new(),
            exact: Vec::new(),
            carve_outs: Vec::new(),
            carve_out_files: Vec::new(),
            profiles: Vec::new(),
            unix: true,
        };
        for dir in [
            "/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/libx32", "/opt",
            "/proc", "/run", "/sbin", "/snap", "/srv", "/sys", "/usr", "/var",
        ] {
            if let Some(n) = norm(Path::new(dir)) {
                g.protected.push((n, "system folder"));
            }
        }
        for d in app_dirs {
            if let Some(n) = norm(d) {
                g.protected.push((n, "WinFastCleaner's own files"));
            }
        }
        if let Some(home) = std::env::var_os("HOME")
            .map(PathBuf::from)
            .and_then(|h| norm(&h))
        {
            g.exact.push((home, "the home folder"));
        }
        if let Some(n) = norm(Path::new("/home")) {
            g.exact.push((n, "the home folders"));
        }
        g
    }

    /// Checks a file or folder that is about to be deleted, moved, or
    /// restored. `path` must be the final resolved path.
    pub fn check(&self, path: &Path) -> Result<(), Blocked> {
        let p = normalize(path)?;
        self.check_norm(&p)
    }

    fn check_norm(&self, p: &NormPath) -> Result<(), Blocked> {
        if p.comps.is_empty() {
            return Err(Blocked("the root of a drive"));
        }
        if !self.unix {
            for c in &p.comps {
                if let Some((_, why)) = PROTECTED_NAMES
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(c))
                {
                    return Err(Blocked(why));
                }
            }
            let top = p.comps[0].as_str();
            if PROTECTED_ROOT_DIRS.contains(&top) {
                return Err(Blocked("Windows boot or system folder"));
            }
            if p.comps.len() == 1 && PROTECTED_ROOT_FILES.contains(&top) {
                return Err(Blocked("Windows system file"));
            }
        }
        if let Some((_, why)) = self.exact.iter().find(|(e, _)| e == p) {
            return Err(Blocked(why));
        }
        let carved_out =
            self.carve_outs.iter().any(|c| p.starts_with(c)) || self.carve_out_files.contains(p);
        if !carved_out
            && let Some((_, why)) = self.protected.iter().find(|(prot, _)| p.starts_with(prot))
        {
            return Err(Blocked(why));
        }
        for prof in &self.profiles {
            if p.starts_with(prof) {
                check_profile_rel(p.rel(prof))?;
            }
        }
        Ok(())
    }

    /// Checks a folder that is about to be removed (because it is empty).
    /// Besides [`Guard::check`], a folder that contains a protected location
    /// (such as `AppData`) or that is itself a cleanable junk folder (such as
    /// `C:\Windows\Temp`) is never removed.
    pub fn check_dir_removal(&self, path: &Path) -> Result<(), Blocked> {
        let p = normalize(path)?;
        self.check_norm(&p)?;
        let contains_protected = self
            .protected
            .iter()
            .map(|(n, _)| n)
            .chain(self.exact.iter().map(|(n, _)| n))
            .chain(self.carve_outs.iter())
            .any(|n| n.starts_with(&p));
        if contains_protected {
            return Err(Blocked("contains or is a Windows folder"));
        }
        for prof in &self.profiles {
            if p.starts_with(prof) {
                let rel = p.rel(prof);
                let joined = rel.join("\\");
                let ancestor_of_protected = PROFILE_PROTECTED_DIRS
                    .iter()
                    .map(|(d, _)| *d)
                    .chain([USRCLASS_DIR])
                    .any(|d| is_same_or_under(d, &joined));
                if ancestor_of_protected || rel.is_empty() {
                    return Err(Blocked("contains protected profile data"));
                }
            }
        }
        Ok(())
    }

    /// Whether `a` and `b` name the same location (case-insensitively).
    pub fn same_path(a: &Path, b: &Path) -> bool {
        match (normalize(a), normalize(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }

    /// Whether `inner` is `outer` or inside it (case-insensitively).
    pub fn is_within(inner: &Path, outer: &Path) -> bool {
        match (normalize(inner), normalize(outer)) {
            (Ok(i), Ok(o)) => i.starts_with(&o),
            _ => false,
        }
    }
}

/// Whether backslash-joined relative path `path` equals `prefix` or lies
/// inside it.
fn is_same_or_under(path: &str, prefix: &str) -> bool {
    path == prefix || (path.starts_with(prefix) && path[prefix.len()..].starts_with('\\'))
}

fn check_profile_rel(rel: &[String]) -> Result<(), Blocked> {
    if rel.len() == 1
        && PROFILE_ROOT_FILE_PREFIXES
            .iter()
            .any(|p| rel[0].starts_with(p))
    {
        return Err(Blocked("your Windows registry settings"));
    }
    let joined = rel.join("\\");
    for (dir, why) in PROFILE_PROTECTED_DIRS {
        if is_same_or_under(&joined, dir) {
            return Err(Blocked(why));
        }
    }
    if let Some(name) = joined
        .strip_prefix(USRCLASS_DIR)
        .and_then(|r| r.strip_prefix('\\'))
        && name.starts_with(USRCLASS_PREFIX)
        && !name.contains('\\')
    {
        return Err(Blocked("your Windows registry settings"));
    }
    Ok(())
}

#[cfg(windows)]
impl WindowsLayout {
    /// Reads the layout of the running system.
    pub fn detect() -> WindowsLayout {
        use crate::platform::windows_dir;
        let env = |k: &str| {
            std::env::var_os(k)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let system_root = windows_dir().unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let drive = system_root
            .to_string_lossy()
            .get(..3)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\"));
        let mut program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .iter()
            .filter_map(|k| env(k))
            .collect();
        program_files.push(drive.join("Program Files"));
        program_files.push(drive.join("Program Files (x86)"));
        let program_data = env("ProgramData").unwrap_or_else(|| drive.join("ProgramData"));
        let users_dir = env("PUBLIC")
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| drive.join("Users"));
        let mut profiles: Vec<PathBuf> = std::fs::read_dir(&users_dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        profiles.extend(env("USERPROFILE"));
        // The data folder, the running program, and its folder when it is an
        // install folder (a portable copy in Downloads must not lock
        // Downloads).
        let mut app_dirs: Vec<PathBuf> = crate::paths::default_data_dir().into_iter().collect();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent()
                && dir
                    .file_name()
                    .is_some_and(|n| n.eq_ignore_ascii_case(crate::APP_DIR_NAME))
            {
                app_dirs.push(dir.to_path_buf());
            }
            app_dirs.push(exe);
        }
        // Environment values may use 8.3 short names, which `normalize`
        // refuses; expand them so every location is really protected.
        let long = |p: PathBuf| crate::platform::long_path(&p);
        WindowsLayout {
            system_root: long(system_root),
            program_files: program_files.into_iter().map(long).collect(),
            program_data: long(program_data),
            users_dir: long(users_dir),
            profiles: profiles.into_iter().map(long).collect(),
            app_dirs: app_dirs.into_iter().map(long).collect(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn test_layout() -> WindowsLayout {
        WindowsLayout {
            system_root: PathBuf::from(r"C:\Windows"),
            program_files: vec![
                PathBuf::from(r"C:\Program Files"),
                PathBuf::from(r"C:\Program Files (x86)"),
            ],
            program_data: PathBuf::from(r"C:\ProgramData"),
            users_dir: PathBuf::from(r"C:\Users"),
            profiles: vec![
                PathBuf::from(r"C:\Users\test-user"),
                PathBuf::from(r"C:\Users\other"),
            ],
            app_dirs: vec![
                PathBuf::from(r"C:\Program Files\WinFastCleaner"),
                PathBuf::from(r"C:\Users\test-user\AppData\Local\WinFastCleaner"),
            ],
        }
    }

    fn guard() -> Guard {
        Guard::windows(&test_layout())
    }

    fn blocked(p: &str) -> bool {
        guard().check(Path::new(p)).is_err()
    }

    #[test]
    fn crucial_windows_locations_are_blocked() {
        for p in [
            r"C:\",
            r"D:\",
            r"C:\Windows",
            r"C:\Windows\explorer.exe",
            r"C:\Windows\System32\drivers\etc\hosts",
            r"C:\WINDOWS\SYSTEM32\ntoskrnl.exe",
            r"C:\Windows\WinSxS\x.dll",
            r"C:\Windows\System32\config\SYSTEM",
            r"C:\Windows\Prefetch\X.pf",
            r"C:\Program Files\App\app.exe",
            r"C:\Program Files (x86)\App\app.exe",
            r"C:\ProgramData\Microsoft\Windows\Start Menu\x.lnk",
            r"C:\pagefile.sys",
            r"C:\hiberfil.sys",
            r"D:\swapfile.sys",
            r"C:\bootmgr",
            r"C:\Boot\BCD",
            r"C:\EFI\Microsoft\Boot\bootmgfw.efi",
            r"C:\Recovery\WindowsRE\winre.wim",
            r"C:\System Volume Information\x",
            r"C:\$Recycle.Bin\S-1-5-21\$R123.txt",
            r"E:\$RECYCLE.BIN\x",
            r"C:\Windows.old\Users\x\file.txt",
            r"D:\Backup\Windows\System32\kernel32.dll",
            r"D:\WindowsApps\app\x.exe",
            r"C:\Users",
            r"C:\Users\test-user",
            r"C:\Users\Public",
            r"C:\Users\Default\NTUSER.DAT",
            r"C:\Users\test-user\NTUSER.DAT",
            r"C:\Users\test-user\ntuser.dat.LOG1",
            r"C:\Users\other\NTUSER.DAT{guid}.TM.blf",
            r"C:\Users\test-user\AppData\Local\Microsoft\Windows\UsrClass.dat",
            r"C:\Users\test-user\AppData\Roaming\Microsoft\Protect\S-1-5\key",
            r"C:\Users\test-user\AppData\Roaming\Microsoft\Credentials\x",
            r"C:\Users\test-user\AppData\Local\Microsoft\Vault\x",
            r"C:\Users\test-user\AppData\Local\Packages\App_1\LocalState\x",
            r"C:\Users\test-user\AppData\Local\WinFastCleaner\recovery\index.json",
            r"C:\Program Files\WinFastCleaner\winfastcleaner.exe",
            r"C:\.winfastcleaner-recovery\x\1",
        ] {
            assert!(blocked(p), "{p} must be blocked");
        }
    }

    #[test]
    fn junk_and_user_files_are_allowed() {
        for p in [
            r"C:\Windows\Temp\setup.log",
            r"C:\Windows\Temp\sub\x.tmp",
            r"C:\Windows\SoftwareDistribution\Download\abc\x.cab",
            r"C:\Windows\Logs\CBS\CbsPersist.log",
            r"C:\Windows\Minidump\101626-1.dmp",
            r"C:\Windows\MEMORY.DMP",
            r"C:\ProgramData\Microsoft\Windows\WER\ReportArchive\x\Report.wer",
            r"C:\Users\test-user\AppData\Local\Temp\x.tmp",
            r"C:\Users\test-user\AppData\Local\Google\Chrome\User Data\Default\Cache\f_000001",
            r"C:\Users\test-user\Downloads\setup.exe",
            r"C:\Users\test-user\.gradle\caches\x.jar",
            r"C:\Users\test-user\AppData\Local\Microsoft\Windows\Explorer\thumbcache_256.db",
            r"C:\Users\test-user\AppData\Local\Microsoft\Windows\INetCache\x",
            r"D:\Projects\app\node_modules\x\index.js",
            r"D:\big.iso",
            r"\\?\C:\Users\test-user\Downloads\a.zip",
        ] {
            assert!(
                !blocked(p),
                "{p} must be allowed: {:?}",
                guard().check(Path::new(p))
            );
        }
    }

    #[test]
    fn carve_outs_do_not_leak_to_siblings_or_protected_names() {
        assert!(blocked(r"C:\Windows\TempEvil\x"));
        assert!(blocked(r"C:\Windows\Temp\System32\x.dll"));
        assert!(blocked(r"C:\Windows\MEMORY.DMP.bak"));
        assert!(blocked(
            r"C:\Windows\SoftwareDistribution\DataStore\DataStore.edb"
        ));
    }

    #[test]
    fn aliases_and_unusual_paths_are_rejected() {
        for p in [
            r"C:\Users\test-user\Downloads\..\NTUSER.DAT",
            r"C:\Users\test-user\Downloads\.\x",
            r"C:\PROGRA~1\App\x.exe",
            r"C:\Users\TEST-U~1\Downloads\x",
            r"C:\Users\test-user\Downloads\file.txt:hidden",
            r"C:\Users\test-user\Downloads\trailing. ",
            r"C:\Users\test-user\Downloads\trailingdot.",
            r"\\server\share\x",
            r"\\?\UNC\server\share\x",
            r"\\.\C:\x",
            r"\\?\Volume{1234}\x",
            r"\??\C:\Windows",
            "relative\\path",
        ] {
            assert!(blocked(p), "{p} must be blocked");
        }
    }

    #[test]
    fn case_and_separators_do_not_matter() {
        assert!(blocked("c:/windows/system32/x"));
        assert!(blocked(r"C:\pRoGrAm FiLeS\x"));
        assert!(!blocked("c:/users/test-user/downloads/x"));
    }

    #[test]
    fn folders_that_contain_protected_data_are_never_removed() {
        let g = guard();
        for p in [
            r"C:\Users\test-user\AppData",
            r"C:\Users\test-user\AppData\Roaming\Microsoft",
            r"C:\Users\test-user\AppData\Local\Microsoft\Windows",
            r"C:\Windows\Temp",
            r"C:\Windows\SoftwareDistribution",
            r"C:\Users\test-user",
            r"D:\WindowsApps",
        ] {
            assert!(g.check_dir_removal(Path::new(p)).is_err(), "{p}");
        }
        for p in [
            r"C:\Windows\Temp\empty-sub",
            r"C:\Users\test-user\AppData\Local\Temp\x",
            r"C:\Users\test-user\Downloads\old",
        ] {
            assert!(g.check_dir_removal(Path::new(p)).is_ok(), "{p}");
        }
    }

    #[test]
    fn short_name_detection() {
        assert!(looks_like_short_name("PROGRA~1"));
        assert!(looks_like_short_name("ABCDE~12.TXT"));
        assert!(looks_like_short_name("AB12F3~1"));
        assert!(!looks_like_short_name("ABCDEF~12.TXT"));
        assert!(!looks_like_short_name("node~modules-cache"));
        assert!(!looks_like_short_name("~$doc.docx"));
        assert!(!looks_like_short_name("a~b"));
    }

    #[test]
    fn path_helpers() {
        assert!(Guard::same_path(Path::new(r"C:\A\b"), Path::new(r"c:/a/B")));
        assert!(Guard::is_within(Path::new(r"C:\A\b\c"), Path::new(r"c:\a")));
        assert!(!Guard::is_within(Path::new(r"C:\Ab"), Path::new(r"c:\a")));
    }
}
