//! NTFS data runs ("mapping pairs"): where an attribute's clusters live.

use crate::{Error, Result};

/// A contiguous run of clusters. `lcn` is `None` for sparse runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub vcn: u64,
    pub lcn: Option<u64>,
    pub len: u64,
}

/// Iterates a mapping-pairs array. Stops after the first error.
struct RunIter<'a> {
    data: &'a [u8],
    pos: usize,
    vcn: u64,
    lcn: i64,
    done: bool,
}

impl Iterator for RunIter<'_> {
    type Item = Result<Run>;

    fn next(&mut self) -> Option<Result<Run>> {
        if self.done {
            return None;
        }
        let item = self.step();
        if !matches!(item, Some(Ok(_))) {
            self.done = true;
        }
        item
    }
}

impl RunIter<'_> {
    fn step(&mut self) -> Option<Result<Run>> {
        let corrupt = |what: &str| Some(Err(Error::CorruptNtfs(what.into())));
        let Some(&header) = self.data.get(self.pos) else {
            return corrupt("unterminated data runs");
        };
        if header == 0 {
            return None;
        }
        let len_size = (header & 0x0F) as usize;
        let off_size = (header >> 4) as usize;
        let pos = self.pos + 1;
        if len_size == 0
            || len_size > 8
            || off_size > 8
            || pos + len_size + off_size > self.data.len()
        {
            return corrupt("bad data run header");
        }
        let len = read_le_unsigned(&self.data[pos..pos + len_size]);
        if len == 0 {
            return corrupt("zero-length data run");
        }
        let lcn = if off_size == 0 {
            None
        } else {
            let delta = read_le_signed(&self.data[pos + len_size..pos + len_size + off_size]);
            match self.lcn.checked_add(delta).filter(|&l| l >= 0) {
                Some(l) => self.lcn = l,
                None => return corrupt("data run points before the volume"),
            }
            Some(self.lcn as u64)
        };
        let run = Run {
            vcn: self.vcn,
            lcn,
            len,
        };
        let Some(next_vcn) = self.vcn.checked_add(len) else {
            return corrupt("data run overflow");
        };
        self.vcn = next_vcn;
        self.pos = pos + len_size + off_size;
        Some(Ok(run))
    }
}

fn iter(data: &[u8], start_vcn: u64) -> RunIter<'_> {
    RunIter {
        data,
        pos: 0,
        vcn: start_vcn,
        lcn: 0,
        done: false,
    }
}

/// Decodes a mapping-pairs array starting at virtual cluster `start_vcn`.
pub fn decode(data: &[u8], start_vcn: u64) -> Result<Vec<Run>> {
    iter(data, start_vcn).collect()
}

/// Number of clusters actually allocated by a mapping-pairs array: sparse
/// holes (and the space saved by compression) are not counted. This is
/// exact even for attributes whose header sizes are misleading, such as
/// `$BadClus:$Bad`, which spans the whole volume without allocating it.
pub fn allocated_clusters(data: &[u8]) -> Result<u64> {
    let mut total = 0u64;
    for run in iter(data, 0) {
        let run = run?;
        if run.lcn.is_some() {
            total = total.saturating_add(run.len);
        }
    }
    Ok(total)
}

fn read_le_unsigned(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
}

fn read_le_signed(bytes: &[u8]) -> i64 {
    let unsigned = read_le_unsigned(bytes);
    let bits = bytes.len() * 8;
    if bits == 64 {
        unsigned as i64
    } else if unsigned & (1 << (bits - 1)) != 0 {
        (unsigned | (!0u64 << bits)) as i64
    } else {
        unsigned as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_simple_and_relative_runs() {
        // 0x21: 1-byte length, 2-byte offset. Len 0x18 at LCN 0x5634.
        // 0x11: len 0x10, offset -0x10 relative => 0x5624.
        // 0x01: sparse run of 4 clusters.
        let data = [0x21, 0x18, 0x34, 0x56, 0x11, 0x10, 0xF0, 0x01, 0x04, 0x00];
        let runs = decode(&data, 0).unwrap();
        assert_eq!(
            runs,
            vec![
                Run {
                    vcn: 0,
                    lcn: Some(0x5634),
                    len: 0x18
                },
                Run {
                    vcn: 0x18,
                    lcn: Some(0x5624),
                    len: 0x10
                },
                Run {
                    vcn: 0x28,
                    lcn: None,
                    len: 4
                },
            ]
        );
    }

    #[test]
    fn counts_only_allocated_clusters() {
        let data = [0x21, 0x18, 0x34, 0x56, 0x11, 0x10, 0xF0, 0x01, 0x04, 0x00];
        assert_eq!(allocated_clusters(&data).unwrap(), 0x18 + 0x10);
        // Entirely sparse, like $BadClus:$Bad on a healthy disk.
        assert_eq!(
            allocated_clusters(&[0x03, 0x00, 0x00, 0x10, 0x00]).unwrap(),
            0
        );
        assert!(allocated_clusters(&[0x21, 0x18]).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(&[0x21, 0x18], 0).is_err());
        assert!(decode(&[0x11, 0x05, 0x80, 0x00], 0).is_err()); // negative LCN
        assert!(decode(&[0x11, 0x05, 0x10], 0).is_err()); // no terminator
        assert!(decode(&[0x10, 0x00], 0).is_err()); // zero-size length
    }
}
