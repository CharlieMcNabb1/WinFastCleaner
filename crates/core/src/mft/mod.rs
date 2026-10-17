//! Fast scanning by reading the NTFS Master File Table (MFT) directly.
//!
//! Instead of listing folders one by one, this reads every file record of
//! the volume in large sequential chunks and rebuilds the folder tree from
//! each record's parent reference. That gives the whole tree, with exact
//! sizes on disk and hard-link information, in one pass.
//!
//! The reader works on anything that implements `Read + Seek`: a raw volume
//! handle on Windows (which needs admin rights), or an image file in tests.
//! It only reads; nothing is ever written to the volume.

pub mod record;
pub mod runs;

use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use rayon::prelude::*;

use crate::format::filetime_to_unix;
use crate::progress::{CancelToken, Counters, ScanPhase};
use crate::tree::{NO_NODE, NodeData, NodeId, Tree, TreeBuilder, flags};
use crate::{Error, Result};
use record::{
    ATTR_ATTRIBUTE_LIST, ATTR_DATA, ATTR_FILE_NAME, ATTR_REPARSE_POINT, ATTR_STANDARD_INFORMATION,
    Boot, NAMESPACE_DOS, RECORD_IN_USE, RECORD_IS_DIR,
};
use runs::Run;

/// Record number of the root directory.
const ROOT_RECORD: u64 = 5;
/// Record number of `$Extend`, which holds more NTFS metadata files.
const EXTEND_RECORD: u64 = 11;
/// Records below this number are reserved for NTFS metadata.
const FIRST_USER_RECORD: u64 = 16;
/// Bytes read per chunk. A multiple of every valid cluster and record size.
const CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// Refuse absurd MFT sizes (corrupt boot sector or hostile image).
const MAX_RECORDS: u64 = u32::MAX as u64 - 1;

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
const REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;

/// Statistics about an MFT read.
#[derive(Clone, Copy, Debug, Default)]
pub struct MftStats {
    /// Total file records in the MFT.
    pub records: u64,
    /// Records that were in use.
    pub in_use: u64,
    /// Records that failed validation (torn or corrupt) and were skipped.
    pub corrupt: u64,
    /// Names whose parent folder no longer exists (changed during the scan).
    pub orphans: u64,
    pub cluster_size: u64,
}

/// Reads the MFT of the NTFS volume behind `volume` and returns the tree for
/// `components` below the volume root (all of it when empty).
///
/// Returns `Ok(None)` when that folder is not in the MFT, for example
/// because the path goes through a junction.
pub fn read_tree<R: Read + Seek + Send>(
    volume: R,
    root_path: PathBuf,
    components: &[&OsStr],
    cancel: &CancelToken,
) -> Result<Option<(Tree, MftStats)>> {
    read_tree_with(volume, root_path, components, &Counters::new(), cancel)
}

pub(crate) fn read_tree_with<R: Read + Seek + Send>(
    mut volume: R,
    root_path: PathBuf,
    components: &[&OsStr],
    counters: &Counters,
    cancel: &CancelToken,
) -> Result<Option<(Tree, MftStats)>> {
    counters.set_phase(ScanPhase::ReadingMft);
    let mut boot_buf = vec![0u8; 4096];
    read_at(&mut volume, 0, &mut boot_buf)?;
    let boot = record::parse_boot(&boot_buf)?;
    let map = locate_mft(&mut volume, &boot)?;
    let total_records = map.data_size / boot.record_size as u64;
    if total_records <= ROOT_RECORD || total_records > MAX_RECORDS {
        return Err(Error::CorruptNtfs("implausible MFT size".into()));
    }
    counters.total.store(total_records, Ordering::Relaxed);

    let mut state = MergeState::new(total_records as usize);
    let rec_size = boot.record_size;
    let chunk_records = (CHUNK_BYTES / rec_size).max(1);
    let cluster = boot.cluster_size as usize;

    std::thread::scope(|scope| -> Result<()> {
        let (tx, rx) = mpsc::sync_channel::<Result<(u64, Vec<u8>)>>(3);
        let map = &map;
        let volume = &mut volume;
        let reader = scope.spawn(move || {
            let mut first = 0u64;
            while first < total_records {
                if cancel.is_cancelled() {
                    break;
                }
                let count = (total_records - first).min(chunk_records as u64) as usize;
                let len = (count * rec_size).next_multiple_of(cluster);
                let mut buf = vec![0u8; len];
                let res = map.read(volume, first * rec_size as u64, &mut buf);
                let failed = res.is_err();
                if tx.send(res.map(|()| (first, buf))).is_err() || failed {
                    break;
                }
                first += count as u64;
            }
        });

        let mut result = Ok(());
        for msg in rx {
            if cancel.is_cancelled() {
                break;
            }
            let (first, mut buf) = match msg {
                Ok(m) => m,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };
            let count = ((total_records - first) as usize).min(buf.len() / rec_size);
            let parsed: Vec<Option<Parsed>> = buf[..count * rec_size]
                .par_chunks_mut(rec_size)
                .enumerate()
                .map(|(i, rec)| parse_record(rec, first + i as u64, cluster as u64))
                .collect();
            for p in parsed {
                state.merge(p, counters);
            }
            counters.done.fetch_add(count as u64, Ordering::Relaxed);
        }
        let _ = reader.join();
        result
    })?;
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    counters.set_phase(ScanPhase::Building);
    let mut stats = MftStats {
        records: total_records,
        cluster_size: boot.cluster_size,
        ..MftStats::default()
    };
    let (builder, root) = state.link(&mut stats)?;
    Ok(builder
        .finish_at(root, components, root_path)
        .map(|tree| (tree, stats)))
}

fn read_at<R: Read + Seek>(r: &mut R, offset: u64, buf: &mut [u8]) -> Result<()> {
    r.seek(SeekFrom::Start(offset))
        .and_then(|_| r.read_exact(buf))
        .map_err(|e| Error::io("volume", e))
}

/// Where the `$MFT` data lives on the volume.
#[derive(Debug)]
struct MftMap {
    runs: Vec<Run>,
    cluster: u64,
    data_size: u64,
}

impl MftMap {
    /// Reads `buf.len()` bytes of the MFT stream starting at `offset`,
    /// following the data runs. Offsets and lengths are cluster-aligned.
    fn read<R: Read + Seek>(&self, vol: &mut R, offset: u64, buf: &mut [u8]) -> Result<()> {
        let mut done = 0usize;
        while done < buf.len() {
            let pos = offset + done as u64;
            let vcn = pos / self.cluster;
            let idx = self.runs.partition_point(|r| r.vcn + r.len <= vcn);
            let run = self
                .runs
                .get(idx)
                .filter(|r| r.vcn <= vcn)
                .ok_or_else(|| Error::CorruptNtfs("MFT offset outside its data runs".into()))?;
            let run_end = (run.vcn + run.len) * self.cluster;
            let n = ((run_end - pos) as usize).min(buf.len() - done);
            let out = &mut buf[done..done + n];
            match run.lcn {
                Some(lcn) => {
                    let disk = (lcn + (vcn - run.vcn)) * self.cluster + pos % self.cluster;
                    read_at(vol, disk, out)?;
                }
                None => out.fill(0),
            }
            done += n;
        }
        Ok(())
    }

    fn read_record<R: Read + Seek>(
        &self,
        vol: &mut R,
        rec: u64,
        rec_size: usize,
    ) -> Result<Vec<u8>> {
        let start = rec * rec_size as u64;
        let aligned = start - start % self.cluster;
        let len = ((start - aligned) as usize + rec_size).next_multiple_of(self.cluster as usize);
        let mut buf = vec![0u8; len];
        self.read(vol, aligned, &mut buf)?;
        let off = (start - aligned) as usize;
        Ok(buf[off..off + rec_size].to_vec())
    }
}

/// Finds all data runs of `$MFT` itself, including extents that live in
/// extension records listed in its `$ATTRIBUTE_LIST`.
fn locate_mft<R: Read + Seek>(vol: &mut R, boot: &Boot) -> Result<MftMap> {
    let cluster = boot.cluster_size;
    let rec_size = boot.record_size;
    let first_len = (rec_size as u64).next_multiple_of(cluster) as usize;
    let mut buf = vec![0u8; first_len];
    read_at(vol, boot.mft_lcn * cluster, &mut buf)?;
    let mut rec0 = buf[..rec_size].to_vec();
    let bad = |what: &str| Error::CorruptNtfs(format!("$MFT record: {what}"));
    if !record::apply_fixups(&mut rec0) {
        return Err(bad("failed fixup check"));
    }
    let h = record::header(&rec0).ok_or_else(|| bad("bad header"))?;

    let mut runs: Vec<Run> = Vec::new();
    let mut data_size = None;
    let mut extension_records: Vec<u64> = Vec::new();
    collect_mft_extents(&rec0, &h, &mut runs, &mut data_size)?;

    for attr in record::attributes(&rec0, &h) {
        if attr.ty != ATTR_ATTRIBUTE_LIST {
            continue;
        }
        let list = match (attr.resident_value(), attr.non_resident()) {
            (Some(v), _) => v.to_vec(),
            (None, Some(nr)) => {
                let mut out = Vec::new();
                for run in runs::decode(nr.runs, 0)? {
                    let mut chunk = vec![0u8; (run.len * cluster) as usize];
                    if let Some(lcn) = run.lcn {
                        read_at(vol, lcn * cluster, &mut chunk)?;
                    }
                    out.extend_from_slice(&chunk);
                }
                out.truncate(nr.data_size as usize);
                out
            }
            _ => continue,
        };
        let mut off = 0usize;
        while off + 0x1A <= list.len() {
            let len = record::u16_at(&list, off + 4).unwrap_or(0) as usize;
            if len < 0x1A || off + len > list.len() {
                break;
            }
            let ty = record::u32_at(&list, off).unwrap_or(0);
            let name_len = list[off + 6];
            let rec = record::u64_at(&list, off + 0x10).unwrap_or(0) & 0x0000_FFFF_FFFF_FFFF;
            if ty == ATTR_DATA && name_len == 0 && rec != 0 && !extension_records.contains(&rec) {
                extension_records.push(rec);
            }
            off += len;
        }
    }

    // Extension records are read through the runs known so far; repeat
    // while that uncovers more runs.
    let mut pending = extension_records;
    while !pending.is_empty() {
        runs.sort_by_key(|r| r.vcn);
        let map = MftMap {
            runs: runs.clone(),
            cluster,
            data_size: 0,
        };
        let mut progress = false;
        let mut still_pending = Vec::new();
        for rec in pending {
            match map.read_record(vol, rec, rec_size) {
                Ok(mut r) => {
                    if record::apply_fixups(&mut r)
                        && let Some(eh) = record::header(&r)
                    {
                        collect_mft_extents(&r, &eh, &mut runs, &mut data_size)?;
                        progress = true;
                    }
                }
                Err(_) => still_pending.push(rec),
            }
        }
        if !progress && !still_pending.is_empty() {
            return Err(bad("extension records not reachable"));
        }
        pending = still_pending;
    }

    runs.sort_by_key(|r| r.vcn);
    runs.dedup_by_key(|r| r.vcn);
    let data_size = data_size.ok_or_else(|| bad("no data size"))?;
    let needed = data_size.div_ceil(cluster);
    let mut expect = 0u64;
    for r in &runs {
        if r.vcn != expect {
            return Err(bad("gap in data runs"));
        }
        expect = r.vcn + r.len;
    }
    if expect < needed {
        return Err(bad("data runs shorter than the MFT"));
    }
    Ok(MftMap {
        runs,
        cluster,
        data_size,
    })
}

fn collect_mft_extents(
    rec: &[u8],
    h: &record::Header,
    runs: &mut Vec<Run>,
    data_size: &mut Option<u64>,
) -> Result<()> {
    for attr in record::attributes(rec, h) {
        if attr.ty != ATTR_DATA || attr.name_len != 0 {
            continue;
        }
        let Some(nr) = attr.non_resident() else {
            continue;
        };
        if nr.lowest_vcn == 0 {
            *data_size = Some(nr.data_size);
        }
        runs.extend(runs::decode(nr.runs, nr.lowest_vcn)?);
    }
    Ok(())
}

/// What one file record contributes to its base file.
#[derive(Debug)]
struct Parsed {
    record: u64,
    base: u64,
    seq: u16,
    flags: u16,
    modified: Option<i64>,
    attributes: Option<u32>,
    size: Option<u64>,
    allocated: u64,
    reparse_tag: Option<u32>,
    names: Vec<ParsedName>,
}

#[derive(Debug)]
struct ParsedName {
    parent: u64,
    parent_seq: u16,
    name: Vec<u16>,
}

fn parse_record(rec: &mut [u8], number: u64, cluster: u64) -> Option<Parsed> {
    if rec.get(0..4)? != b"FILE" {
        return None;
    }
    if !record::apply_fixups(rec) {
        return Some(Parsed::corrupt(number));
    }
    let h = record::header(rec)?;
    if h.flags & RECORD_IN_USE == 0 {
        return None;
    }
    let mut p = Parsed {
        record: number,
        base: if h.base == 0 { number } else { h.base },
        seq: h.seq,
        flags: h.flags,
        modified: None,
        attributes: None,
        size: None,
        allocated: 0,
        reparse_tag: None,
        names: Vec::new(),
    };
    for attr in record::attributes(rec, &h) {
        match attr.ty {
            ATTR_STANDARD_INFORMATION => {
                if let Some((ft, attrs)) =
                    attr.resident_value().and_then(record::standard_information)
                {
                    p.modified = Some(if ft == 0 { 0 } else { filetime_to_unix(ft) });
                    p.attributes = Some(attrs);
                }
            }
            ATTR_FILE_NAME => {
                let Some(fname) = attr.resident_value().and_then(record::file_name) else {
                    continue;
                };
                // DOS 8.3 names always duplicate a long name.
                if fname.namespace == NAMESPACE_DOS {
                    continue;
                }
                p.names.push(ParsedName {
                    parent: fname.parent,
                    parent_seq: fname.parent_seq,
                    name: fname
                        .name
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect(),
                });
            }
            ATTR_DATA => {
                if let Some(v) = attr.resident_value() {
                    if attr.name_len == 0 {
                        p.size = Some(v.len() as u64);
                    }
                } else if let Some(nr) = attr.non_resident() {
                    // The logical size is only valid in the first extent.
                    if nr.lowest_vcn == 0 && attr.name_len == 0 {
                        p.size = Some(nr.data_size);
                    }
                    // Size on disk comes from the clusters each extent really
                    // uses, not from header fields: some attributes (such as
                    // $BadClus:$Bad) claim far more than they allocate.
                    let clusters = runs::allocated_clusters(nr.runs).unwrap_or(0);
                    p.allocated = p.allocated.saturating_add(clusters.saturating_mul(cluster));
                }
            }
            ATTR_REPARSE_POINT => {
                p.reparse_tag = Some(
                    attr.resident_value()
                        .and_then(|v| record::u32_at(v, 0))
                        // Unreadable tag: assume it is a link (the safe choice).
                        .unwrap_or(REPARSE_TAG_NAME_SURROGATE),
                );
            }
            _ => {}
        }
    }
    Some(p)
}

impl Parsed {
    fn corrupt(number: u64) -> Self {
        Self {
            record: number,
            base: u64::MAX,
            seq: 0,
            flags: 0,
            modified: None,
            attributes: None,
            size: None,
            allocated: 0,
            reparse_tag: None,
            names: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    in_use: bool,
    is_dir: bool,
    seq: u16,
    names: u16,
    attributes: u32,
    reparse_tag: Option<u32>,
    modified: i64,
    size: u64,
    allocated: u64,
}

#[derive(Clone, Copy, Debug)]
struct Link {
    node: NodeId,
    record: u32,
    parent: u64,
    parent_seq: u16,
}

struct MergeState {
    table: Vec<Entry>,
    links: Vec<Link>,
    builder: TreeBuilder,
    corrupt: u64,
}

impl MergeState {
    fn new(records: usize) -> Self {
        Self {
            table: vec![Entry::default(); records],
            links: Vec::with_capacity(records),
            builder: TreeBuilder::with_capacity(records, records * 16),
            corrupt: 0,
        }
    }

    fn merge(&mut self, parsed: Option<Parsed>, counters: &Counters) {
        let Some(p) = parsed else { return };
        if p.base == u64::MAX {
            self.corrupt += 1;
            return;
        }
        let Some(e) = self.table.get_mut(p.base as usize) else {
            return;
        };
        let is_base = p.base == p.record;
        if is_base {
            e.in_use = true;
            e.is_dir = p.flags & RECORD_IS_DIR != 0;
            e.seq = p.seq;
            if e.is_dir {
                counters.dirs.fetch_add(1, Ordering::Relaxed);
            } else {
                counters
                    .files
                    .fetch_add(p.names.len() as u64, Ordering::Relaxed);
            }
        }
        if let Some(m) = p.modified {
            e.modified = m;
        }
        if let Some(a) = p.attributes {
            e.attributes = a;
        }
        if let Some(s) = p.size {
            e.size = s;
            counters.bytes.fetch_add(s, Ordering::Relaxed);
        }
        if p.reparse_tag.is_some() {
            e.reparse_tag = p.reparse_tag;
        }
        e.allocated += p.allocated;
        e.names = e.names.saturating_add(p.names.len() as u16);
        for name in p.names {
            let node = self.builder.push_utf16(
                &name.name,
                NodeData {
                    parent: NO_NODE,
                    flags: 0,
                    size: 0,
                    allocated: 0,
                    modified: 0,
                    link_group: None,
                },
            );
            self.links.push(Link {
                node,
                record: p.base as u32,
                parent: name.parent,
                parent_seq: name.parent_seq,
            });
        }
    }

    /// Connects every name to its parent folder and fills in its data.
    fn link(mut self, stats: &mut MftStats) -> Result<(TreeBuilder, NodeId)> {
        stats.corrupt = self.corrupt;
        stats.in_use = self.table.iter().filter(|e| e.in_use).count() as u64;
        let mut first_node = vec![NO_NODE; self.table.len()];
        for l in &self.links {
            let slot = &mut first_node[l.record as usize];
            if *slot == NO_NODE {
                *slot = l.node;
            }
        }
        for l in &self.links {
            let rec = u64::from(l.record);
            let e = self.table[l.record as usize];
            if !e.in_use {
                continue;
            }
            // Folders cannot have hard links; ignore any extra names.
            if e.is_dir && first_node[l.record as usize] != l.node {
                continue;
            }
            let parent = if rec == ROOT_RECORD {
                NO_NODE
            } else {
                match self.table.get(l.parent as usize) {
                    Some(pe)
                        if pe.in_use
                            && pe.is_dir
                            && (l.parent_seq == 0 || l.parent_seq == pe.seq) =>
                    {
                        first_node[l.parent as usize]
                    }
                    _ => {
                        stats.orphans += 1;
                        NO_NODE
                    }
                }
            };
            let mut fl = 0u16;
            if e.is_dir {
                fl |= flags::DIR;
            }
            if e.attributes & FILE_ATTRIBUTE_HIDDEN != 0 {
                fl |= flags::HIDDEN;
            }
            if e.attributes & FILE_ATTRIBUTE_SYSTEM != 0 {
                fl |= flags::SYSTEM;
            }
            if e.attributes & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0 {
                fl |= flags::CLOUD;
            }
            let is_link = e
                .reparse_tag
                .is_some_and(|t| t & REPARSE_TAG_NAME_SURROGATE != 0);
            if is_link {
                fl |= flags::LINK;
            }
            if (rec < FIRST_USER_RECORD && rec != ROOT_RECORD) || l.parent == EXTEND_RECORD {
                fl |= flags::METAFILE;
            }
            let (size, allocated) = if e.is_dir || is_link {
                (0, 0)
            } else {
                (e.size, e.allocated)
            };
            self.builder.set_data(
                l.node,
                NodeData {
                    parent,
                    flags: fl,
                    size,
                    allocated,
                    modified: e.modified,
                    link_group: (!e.is_dir && e.names > 1).then_some(l.record),
                },
            );
        }
        let root = first_node
            .get(ROOT_RECORD as usize)
            .copied()
            .filter(|&r| r != NO_NODE)
            .ok_or_else(|| Error::CorruptNtfs("no root directory record".into()))?;
        Ok((self.builder, root))
    }
}

#[cfg(test)]
mod tests {
    use super::record::tests::RecordBuilder;
    use super::*;

    fn si(ft: u64, attrs: u32) -> Vec<u8> {
        let mut v = vec![0u8; 0x48];
        v[0x08..0x10].copy_from_slice(&ft.to_le_bytes());
        v[0x20..0x24].copy_from_slice(&attrs.to_le_bytes());
        v
    }

    #[test]
    fn parse_record_collects_names_sizes_and_skips_dos_names() {
        let mut r = RecordBuilder::new(RECORD_IN_USE)
            .resident(
                ATTR_STANDARD_INFORMATION,
                &[],
                &si(116_444_736_000_000_000 + 10_000_000, 0x20),
            )
            .file_name(40, 2, 1, "Long File Name.txt")
            .file_name(40, 2, NAMESPACE_DOS, "LONGFI~1.TXT")
            .non_resident(
                ATTR_DATA,
                &[],
                0,
                0,
                12288,
                10_000,
                None,
                &[0x11, 0x03, 0x20, 0x00],
            )
            .build();
        let p = parse_record(&mut r, 77, 4096).unwrap();
        assert_eq!(p.base, 77);
        assert_eq!(p.names.len(), 1);
        assert_eq!(
            String::from_utf16_lossy(&p.names[0].name),
            "Long File Name.txt"
        );
        assert_eq!(
            (p.size, p.allocated, p.modified),
            (Some(10_000), 12288, Some(1))
        );
    }

    #[test]
    fn size_on_disk_comes_from_real_clusters_not_headers() {
        // Like $BadClus:$Bad: the header claims the whole volume, but every
        // run is sparse and nothing is allocated.
        let bad: Vec<u16> = "$Bad".encode_utf16().collect();
        let mut r = RecordBuilder::new(RECORD_IN_USE)
            .file_name(5, 5, 3, "$BadClus")
            .non_resident(
                ATTR_DATA,
                &bad,
                0,
                0,
                1 << 40,
                1 << 40,
                None,
                &[0x04, 0x00, 0x00, 0x00, 0x01, 0x00],
            )
            .build();
        let p = parse_record(&mut r, 8, 4096).unwrap();
        assert_eq!(p.allocated, 0);
        assert_eq!(p.size, None);
    }

    #[test]
    fn extension_records_contribute_to_their_base() {
        let mut ext = RecordBuilder::new(RECORD_IN_USE)
            .base(77 | (3 << 48))
            .file_name(41, 1, 0, "second-link")
            .build();
        let p = parse_record(&mut ext, 900, 4096).unwrap();
        assert_eq!((p.record, p.base), (900, 77));
    }

    #[test]
    fn unused_and_torn_records() {
        let mut unused = RecordBuilder::new(0).file_name(5, 5, 1, "gone").build();
        assert!(parse_record(&mut unused, 30, 4096).is_none());
        let mut torn = RecordBuilder::new(RECORD_IN_USE)
            .file_name(5, 5, 1, "x")
            .build();
        torn[510] ^= 0xFF;
        assert_eq!(parse_record(&mut torn, 31, 4096).unwrap().base, u64::MAX);
    }

    #[test]
    fn linking_builds_a_tree_with_hard_links_and_orphans() {
        let counters = Counters::new();
        let mut st = MergeState::new(64);
        let mut push = |rec: u64, r: RecordBuilder| {
            let mut bytes = r.build();
            st.merge(parse_record(&mut bytes, rec, 4096), &counters);
        };
        push(
            5,
            RecordBuilder::new(RECORD_IN_USE | RECORD_IS_DIR)
                .seq(5)
                .file_name(5, 5, 3, "."),
        );
        push(
            30,
            RecordBuilder::new(RECORD_IN_USE | RECORD_IS_DIR).file_name(5, 5, 1, "docs"),
        );
        push(
            31,
            RecordBuilder::new(RECORD_IN_USE | RECORD_IS_DIR).file_name(5, 5, 1, "other"),
        );
        push(
            40,
            RecordBuilder::new(RECORD_IN_USE)
                .file_name(30, 1, 1, "a.bin")
                .file_name(31, 1, 0, "a-link.bin")
                .non_resident(
                    ATTR_DATA,
                    &[],
                    0,
                    0,
                    8192,
                    8000,
                    None,
                    &[0x11, 0x02, 0x20, 0x00],
                ),
        );
        // Parent sequence mismatch: the folder was deleted and its record reused.
        push(
            41,
            RecordBuilder::new(RECORD_IN_USE).file_name(30, 9, 1, "stale.bin"),
        );
        push(
            2,
            RecordBuilder::new(RECORD_IN_USE)
                .file_name(5, 5, 3, "$LogFile")
                .non_resident(
                    ATTR_DATA,
                    &[],
                    0,
                    0,
                    65536,
                    65536,
                    None,
                    &[0x11, 0x10, 0x40, 0x00],
                ),
        );
        let mut stats = MftStats::default();
        let (builder, root) = st.link(&mut stats).unwrap();
        let tree = builder.finish(root, PathBuf::from("/x"));
        assert_eq!(stats.orphans, 1);
        assert_eq!(tree.node(0).size(), 8000 + 65536);
        assert_eq!(tree.node(0).files(), 3);
        let log = tree.find(std::path::Path::new("/x/$LogFile")).unwrap();
        assert!(tree.node(log).has(flags::METAFILE));
        assert!(tree.node(0).has(flags::HAS_HARDLINKS));
    }
}
