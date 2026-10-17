//! File types by extension, for filtering the file finders.

use serde::{Deserialize, Serialize};

/// A broad file type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    Video,
    Audio,
    Image,
    Archive,
    Installer,
    DiskImage,
    Document,
    Other,
}

impl FileKind {
    pub const ALL: [FileKind; 8] = [
        FileKind::Video,
        FileKind::Audio,
        FileKind::Image,
        FileKind::Archive,
        FileKind::Installer,
        FileKind::DiskImage,
        FileKind::Document,
        FileKind::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FileKind::Video => "Video",
            FileKind::Audio => "Audio",
            FileKind::Image => "Images",
            FileKind::Archive => "Archives",
            FileKind::Installer => "Installers",
            FileKind::DiskImage => "Disk images",
            FileKind::Document => "Documents",
            FileKind::Other => "Other",
        }
    }

    /// Parses a name such as `video` or `disk-image` (for the CLI).
    pub fn parse(s: &str) -> Option<FileKind> {
        let s = s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "");
        FileKind::ALL.into_iter().find(|k| {
            let name = format!("{k:?}").to_ascii_lowercase();
            s == name || s == format!("{name}s")
        })
    }
}

const TABLE: &[(FileKind, &[&str])] = &[
    (
        FileKind::Video,
        &[
            "mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "flv", "mpg", "mpeg", "m2ts", "ts",
            "vob", "3gp",
        ],
    ),
    (
        FileKind::Audio,
        &[
            "mp3", "flac", "wav", "aac", "ogg", "m4a", "wma", "opus", "aiff",
        ],
    ),
    (
        FileKind::Image,
        &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "tif", "tiff", "raw", "cr2", "cr3",
            "nef", "arw", "dng", "psd",
        ],
    ),
    (
        FileKind::Archive,
        &[
            "zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst", "tgz", "cab",
        ],
    ),
    (
        FileKind::Installer,
        &[
            "exe",
            "msi",
            "msix",
            "msixbundle",
            "appx",
            "appxbundle",
            "msu",
        ],
    ),
    (
        FileKind::DiskImage,
        &[
            "iso", "img", "vhd", "vhdx", "vmdk", "qcow2", "vdi", "wim", "esd", "dmg",
        ],
    ),
    (
        FileKind::Document,
        &[
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "odt", "ods", "odp",
            "csv", "epub", "md",
        ],
    ),
];

/// The type of a file, from its extension.
pub fn kind_of(name: &str) -> FileKind {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return FileKind::Other;
    };
    if stem.is_empty() {
        return FileKind::Other;
    }
    let ext = ext.to_ascii_lowercase();
    TABLE
        .iter()
        .find(|(_, exts)| exts.contains(&ext.as_str()))
        .map_or(FileKind::Other, |(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind_of("Holiday.MKV"), FileKind::Video);
        assert_eq!(kind_of("setup.exe"), FileKind::Installer);
        assert_eq!(kind_of("win11.iso"), FileKind::DiskImage);
        assert_eq!(kind_of("backup.tar.gz"), FileKind::Archive);
        assert_eq!(kind_of(".gitignore"), FileKind::Other);
        assert_eq!(kind_of("README"), FileKind::Other);
        assert_eq!(FileKind::parse("disk-image"), Some(FileKind::DiskImage));
        assert_eq!(FileKind::parse("Videos"), Some(FileKind::Video));
        assert_eq!(FileKind::parse("bogus"), None);
    }
}
