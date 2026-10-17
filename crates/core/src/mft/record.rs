//! Parsing of the NTFS boot sector and MFT file records.
//!
//! Every read is bounds-checked: a corrupt or hostile record can make us skip
//! that record, never panic or read out of bounds.

use crate::{Error, Result};

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_DATA: u32 = 0x80;
pub const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

pub const RECORD_IN_USE: u16 = 0x0001;
pub const RECORD_IS_DIR: u16 = 0x0002;

const ATTR_FLAG_COMPRESSED: u16 = 0x0001;
const ATTR_FLAG_SPARSE: u16 = 0x8000;

/// Update sequence (fixup) stride. NTFS always uses 512 bytes, whatever the
/// physical sector size.
const FIXUP_STRIDE: usize = 512;

pub fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

pub fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

pub fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}

/// Volume geometry from the NTFS boot sector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boot {
    pub bytes_per_sector: u32,
    pub cluster_size: u64,
    pub total_sectors: u64,
    pub mft_lcn: u64,
    pub record_size: usize,
}

pub fn parse_boot(b: &[u8]) -> Result<Boot> {
    if b.get(3..11) != Some(b"NTFS    ".as_slice()) {
        return Err(Error::NotNtfs);
    }
    let bad = |what: &str| Error::CorruptNtfs(format!("boot sector: bad {what}"));
    let bps = u32::from(u16_at(b, 0x0B).ok_or_else(|| bad("sector size"))?);
    if !(256..=4096).contains(&bps) || !bps.is_power_of_two() {
        return Err(bad("sector size"));
    }
    let spc_raw = *b.get(0x0D).ok_or_else(|| bad("cluster size"))?;
    let spc: u64 = if spc_raw <= 0x80 {
        u64::from(spc_raw)
    } else {
        // Clusters of 128 sectors or more are stored as a negative power of
        // two (Windows 10 1709+ supports up to 2 MB clusters).
        let shift = 256 - u32::from(spc_raw);
        if shift > 31 {
            return Err(bad("cluster size"));
        }
        1u64 << shift
    };
    if spc == 0 || !spc.is_power_of_two() {
        return Err(bad("cluster size"));
    }
    let cluster_size = u64::from(bps) * spc;
    if cluster_size > 2 * 1024 * 1024 {
        return Err(bad("cluster size"));
    }
    let total_sectors = u64_at(b, 0x28).ok_or_else(|| bad("volume size"))?;
    let mft_lcn = u64_at(b, 0x30).ok_or_else(|| bad("MFT location"))?;
    let cpr = *b.get(0x40).ok_or_else(|| bad("record size"))? as i8;
    let record_size: u64 = if cpr > 0 {
        cpr as u64 * cluster_size
    } else {
        let shift = u32::from(cpr.unsigned_abs());
        if shift > 16 {
            return Err(bad("record size"));
        }
        1u64 << shift
    };
    if !(256..=65536).contains(&record_size) || !record_size.is_power_of_two() {
        return Err(bad("record size"));
    }
    if mft_lcn == 0
        || mft_lcn.saturating_mul(cluster_size) >= total_sectors.saturating_mul(u64::from(bps))
    {
        return Err(bad("MFT location"));
    }
    Ok(Boot {
        bytes_per_sector: bps,
        cluster_size,
        total_sectors,
        mft_lcn,
        record_size: record_size as usize,
    })
}

/// Applies the update-sequence fixups in place. Returns `false` if the
/// record is torn or corrupt and must be ignored.
pub fn apply_fixups(rec: &mut [u8]) -> bool {
    let (Some(usa_off), Some(usa_count)) = (u16_at(rec, 4), u16_at(rec, 6)) else {
        return false;
    };
    let (usa_off, usa_count) = (usa_off as usize, usa_count as usize);
    if usa_count < 2 || (usa_count - 1) * FIXUP_STRIDE > rec.len() {
        return false;
    }
    if usa_off + usa_count * 2 > rec.len() {
        return false;
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_count {
        let end = i * FIXUP_STRIDE - 2;
        if rec[end..end + 2] != usn {
            return false;
        }
        let saved = usa_off + i * 2;
        rec[end] = rec[saved];
        rec[end + 1] = rec[saved + 1];
    }
    true
}

/// Header fields of a file record.
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub seq: u16,
    pub flags: u16,
    /// Record number of the base record, or 0 if this is a base record.
    pub base: u64,
    pub attrs_off: usize,
    pub used: usize,
}

pub fn header(rec: &[u8]) -> Option<Header> {
    if rec.get(0..4)? != b"FILE" {
        return None;
    }
    let used = (u32_at(rec, 0x18)? as usize).min(rec.len());
    let attrs_off = u16_at(rec, 0x14)? as usize;
    if attrs_off < 0x18 || attrs_off >= used {
        return None;
    }
    Some(Header {
        seq: u16_at(rec, 0x10)?,
        flags: u16_at(rec, 0x16)?,
        base: u64_at(rec, 0x20)? & 0x0000_FFFF_FFFF_FFFF,
        attrs_off,
        used,
    })
}

/// One attribute inside a record.
#[derive(Clone, Copy, Debug)]
pub struct Attr<'a> {
    pub ty: u32,
    pub flags: u16,
    pub name_len: u8,
    raw: &'a [u8],
}

/// Header of a non-resident attribute.
#[derive(Clone, Copy, Debug)]
pub struct NonResident<'a> {
    pub lowest_vcn: u64,
    pub allocated: u64,
    pub data_size: u64,
    /// Actual clusters in use for compressed or sparse attributes.
    pub compressed_size: Option<u64>,
    pub runs: &'a [u8],
}

impl<'a> Attr<'a> {
    pub fn is_resident(&self) -> bool {
        self.raw[8] == 0
    }

    pub fn name(&self) -> Option<Vec<u16>> {
        let off = u16_at(self.raw, 0x0A)? as usize;
        let bytes = self.raw.get(off..off + self.name_len as usize * 2)?;
        Some(
            bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
        )
    }

    pub fn resident_value(&self) -> Option<&'a [u8]> {
        if !self.is_resident() {
            return None;
        }
        let len = u32_at(self.raw, 0x10)? as usize;
        let off = u16_at(self.raw, 0x14)? as usize;
        self.raw.get(off..off.checked_add(len)?)
    }

    pub fn non_resident(&self) -> Option<NonResident<'a>> {
        if self.is_resident() {
            return None;
        }
        let runs_off = u16_at(self.raw, 0x20)? as usize;
        let compressed_size =
            if self.flags & (ATTR_FLAG_COMPRESSED | ATTR_FLAG_SPARSE) != 0 && runs_off >= 0x48 {
                Some(u64_at(self.raw, 0x40)?)
            } else {
                None
            };
        Some(NonResident {
            lowest_vcn: u64_at(self.raw, 0x10)?,
            allocated: u64_at(self.raw, 0x28)?,
            data_size: u64_at(self.raw, 0x30)?,
            compressed_size,
            runs: self.raw.get(runs_off..)?,
        })
    }
}

/// Iterates the attributes of a (fixed-up) record. Stops at the first
/// malformed attribute.
pub fn attributes<'a>(rec: &'a [u8], h: &Header) -> impl Iterator<Item = Attr<'a>> + use<'a> {
    let used = &rec[..h.used];
    let mut off = h.attrs_off;
    std::iter::from_fn(move || {
        let ty = u32_at(used, off)?;
        if ty == ATTR_END {
            return None;
        }
        let len = u32_at(used, off + 4)? as usize;
        if len < 0x18 || off + len > used.len() {
            return None;
        }
        let raw = &used[off..off + len];
        off += len;
        Some(Attr {
            ty,
            name_len: raw[9],
            flags: u16_at(raw, 0x0C)?,
            raw,
        })
    })
}

/// A parsed `$FILE_NAME` attribute.
#[derive(Clone, Debug)]
pub struct FileNameAttr<'a> {
    pub parent: u64,
    pub parent_seq: u16,
    pub namespace: u8,
    /// UTF-16LE bytes of the name.
    pub name: &'a [u8],
}

pub const NAMESPACE_DOS: u8 = 2;

pub fn file_name(value: &[u8]) -> Option<FileNameAttr<'_>> {
    let parent_ref = u64_at(value, 0)?;
    let len = *value.get(0x40)? as usize;
    let namespace = *value.get(0x41)?;
    let name = value.get(0x42..0x42 + len * 2)?;
    Some(FileNameAttr {
        parent: parent_ref & 0x0000_FFFF_FFFF_FFFF,
        parent_seq: (parent_ref >> 48) as u16,
        namespace,
        name,
    })
}

/// `(modified FILETIME, file attributes)` from `$STANDARD_INFORMATION`.
pub fn standard_information(value: &[u8]) -> Option<(u64, u32)> {
    Some((u64_at(value, 0x08)?, u32_at(value, 0x20)?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a minimal 1 KB file record for tests.
    pub struct RecordBuilder {
        attrs: Vec<u8>,
        flags: u16,
        seq: u16,
        base: u64,
    }

    impl RecordBuilder {
        pub fn new(flags: u16) -> Self {
            Self {
                attrs: Vec::new(),
                flags,
                seq: 1,
                base: 0,
            }
        }

        pub fn seq(mut self, seq: u16) -> Self {
            self.seq = seq;
            self
        }

        pub fn base(mut self, base: u64) -> Self {
            self.base = base;
            self
        }

        fn push_attr(
            &mut self,
            ty: u32,
            non_resident: bool,
            flags: u16,
            name: &[u16],
            body: &[u8],
            header_len: usize,
        ) {
            let name_off = header_len;
            let body_off = (name_off + name.len() * 2).next_multiple_of(8);
            let len = (body_off + body.len()).next_multiple_of(8);
            let mut a = vec![0u8; len];
            a[0..4].copy_from_slice(&ty.to_le_bytes());
            a[4..8].copy_from_slice(&(len as u32).to_le_bytes());
            a[8] = u8::from(non_resident);
            a[9] = name.len() as u8;
            a[0x0A..0x0C].copy_from_slice(&(name_off as u16).to_le_bytes());
            a[0x0C..0x0E].copy_from_slice(&flags.to_le_bytes());
            for (i, c) in name.iter().enumerate() {
                a[name_off + i * 2..name_off + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
            }
            if non_resident {
                a[0x20..0x22].copy_from_slice(&(body_off as u16).to_le_bytes());
            } else {
                a[0x10..0x14].copy_from_slice(&(body.len() as u32).to_le_bytes());
                a[0x14..0x16].copy_from_slice(&(body_off as u16).to_le_bytes());
            }
            a[body_off..body_off + body.len()].copy_from_slice(body);
            self.attrs.extend_from_slice(&a);
        }

        pub fn resident(mut self, ty: u32, name: &[u16], value: &[u8]) -> Self {
            self.push_attr(ty, false, 0, name, value, 0x18);
            self
        }

        pub fn file_name(self, parent: u64, parent_seq: u16, namespace: u8, name: &str) -> Self {
            let units: Vec<u16> = name.encode_utf16().collect();
            let mut v = vec![0u8; 0x42 + units.len() * 2];
            v[0..8].copy_from_slice(&(parent | (u64::from(parent_seq) << 48)).to_le_bytes());
            v[0x40] = units.len() as u8;
            v[0x41] = namespace;
            for (i, c) in units.iter().enumerate() {
                v[0x42 + i * 2..0x44 + i * 2].copy_from_slice(&c.to_le_bytes());
            }
            self.resident(ATTR_FILE_NAME, &[], &v)
        }

        #[allow(clippy::too_many_arguments)]
        pub fn non_resident(
            mut self,
            ty: u32,
            name: &[u16],
            flags: u16,
            lowest_vcn: u64,
            allocated: u64,
            data_size: u64,
            compressed: Option<u64>,
            runs: &[u8],
        ) -> Self {
            let header_len = if compressed.is_some() { 0x48 } else { 0x40 };
            let start = self.attrs.len();
            self.push_attr(ty, true, flags, name, runs, header_len);
            let a = &mut self.attrs[start..];
            a[0x10..0x18].copy_from_slice(&lowest_vcn.to_le_bytes());
            a[0x28..0x30].copy_from_slice(&allocated.to_le_bytes());
            a[0x30..0x38].copy_from_slice(&data_size.to_le_bytes());
            a[0x38..0x40].copy_from_slice(&data_size.to_le_bytes());
            if let Some(c) = compressed {
                a[0x40..0x48].copy_from_slice(&c.to_le_bytes());
            }
            self
        }

        /// Serializes with fixups applied "on disk" (so `apply_fixups` must
        /// undo them).
        pub fn build(self) -> Vec<u8> {
            let mut r = vec![0u8; 1024];
            r[0..4].copy_from_slice(b"FILE");
            r[4..6].copy_from_slice(&0x30u16.to_le_bytes()); // USA offset
            r[6..8].copy_from_slice(&3u16.to_le_bytes()); // USN + 2 sectors
            r[0x10..0x12].copy_from_slice(&self.seq.to_le_bytes());
            r[0x14..0x16].copy_from_slice(&0x38u16.to_le_bytes());
            r[0x16..0x18].copy_from_slice(&self.flags.to_le_bytes());
            r[0x20..0x28].copy_from_slice(&self.base.to_le_bytes());
            let end = 0x38 + self.attrs.len();
            r[0x38..end].copy_from_slice(&self.attrs);
            r[end..end + 4].copy_from_slice(&ATTR_END.to_le_bytes());
            r[0x18..0x1C].copy_from_slice(&((end + 8) as u32).to_le_bytes());
            r[0x1C..0x20].copy_from_slice(&1024u32.to_le_bytes());
            // Fixups: save the last two bytes of each 512-byte stride and
            // replace them with the USN.
            let usn = [0x07, 0x00];
            r[0x30..0x32].copy_from_slice(&usn);
            for i in 1..3 {
                let e = i * 512 - 2;
                let saved = 0x30 + i * 2;
                r[saved] = r[e];
                r[saved + 1] = r[e + 1];
                r[e..e + 2].copy_from_slice(&usn);
            }
            r
        }
    }

    #[test]
    fn fixups_restore_sector_tails_and_detect_tears() {
        let mut r = RecordBuilder::new(RECORD_IN_USE)
            .file_name(5, 5, 1, "x")
            .build();
        let mut torn = r.clone();
        assert!(apply_fixups(&mut r));
        assert!(header(&r).is_some());
        torn[1022] = 0x99; // simulate a torn write
        assert!(!apply_fixups(&mut torn));
    }

    #[test]
    fn parses_attributes() {
        let mut r = RecordBuilder::new(RECORD_IN_USE)
            .file_name(42, 3, 1, "report.pdf")
            .non_resident(
                ATTR_DATA,
                &[],
                0,
                0,
                8192,
                5000,
                None,
                &[0x11, 0x02, 0x10, 0x00],
            )
            .non_resident(
                ATTR_DATA,
                &[0x41],
                ATTR_FLAG_SPARSE,
                0,
                1 << 20,
                1 << 20,
                Some(4096),
                &[0x01, 0x80, 0x00],
            )
            .build();
        assert!(apply_fixups(&mut r));
        let h = header(&r).unwrap();
        let attrs: Vec<_> = attributes(&r, &h).collect();
        assert_eq!(attrs.len(), 3);
        let fname = file_name(attrs[0].resident_value().unwrap()).unwrap();
        assert_eq!(
            (fname.parent, fname.parent_seq, fname.namespace),
            (42, 3, 1)
        );
        assert_eq!(fname.name.len(), 20);
        let data = attrs[1].non_resident().unwrap();
        assert_eq!(
            (data.allocated, data.data_size, data.compressed_size),
            (8192, 5000, None)
        );
        let ads = attrs[2].non_resident().unwrap();
        assert_eq!(ads.compressed_size, Some(4096));
        assert_eq!(attrs[2].name().unwrap(), vec![0x41]);
    }

    #[test]
    fn rejects_malformed_records_without_panicking() {
        let base = RecordBuilder::new(RECORD_IN_USE)
            .file_name(5, 5, 1, "x")
            .build();
        for i in 0..base.len() {
            for v in [0x00, 0xFF, 0x7F] {
                let mut r = base.clone();
                r[i] = v;
                if apply_fixups(&mut r)
                    && let Some(h) = header(&r)
                {
                    for a in attributes(&r, &h) {
                        let _ = a.resident_value().map(file_name);
                        let _ = a.non_resident();
                        let _ = a.name();
                    }
                }
            }
        }
    }

    #[test]
    fn boot_sector_validation() {
        let mut b = vec![0u8; 512];
        b[3..11].copy_from_slice(b"NTFS    ");
        b[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        b[0x0D] = 8;
        b[0x28..0x30].copy_from_slice(&1_000_000u64.to_le_bytes());
        b[0x30..0x38].copy_from_slice(&4u64.to_le_bytes());
        b[0x40] = 0xF6; // -10 => 1024-byte records
        let boot = parse_boot(&b).unwrap();
        assert_eq!(boot.cluster_size, 4096);
        assert_eq!(boot.record_size, 1024);
        assert_eq!(boot.mft_lcn, 4);

        let mut not_ntfs = b.clone();
        not_ntfs[3] = b'F';
        assert!(matches!(parse_boot(&not_ntfs), Err(Error::NotNtfs)));
        let mut bad = b.clone();
        bad[0x0B..0x0D].copy_from_slice(&500u16.to_le_bytes());
        assert!(parse_boot(&bad).is_err());
        let mut bad = b;
        bad[0x30..0x38].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(parse_boot(&bad).is_err());
    }
}
