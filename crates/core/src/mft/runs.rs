//! NTFS data runs ("mapping pairs"): where an attribute's clusters live.

use crate::{Error, Result};

/// A contiguous run of clusters. `lcn` is `None` for sparse runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub vcn: u64,
    pub lcn: Option<u64>,
    pub len: u64,
}

/// Decodes a mapping-pairs array starting at virtual cluster `start_vcn`.
pub fn decode(data: &[u8], start_vcn: u64) -> Result<Vec<Run>> {
    let mut runs = Vec::new();
    let mut pos = 0usize;
    let mut vcn = start_vcn;
    let mut lcn: i64 = 0;
    while pos < data.len() {
        let header = data[pos];
        if header == 0 {
            return Ok(runs);
        }
        let len_size = (header & 0x0F) as usize;
        let off_size = (header >> 4) as usize;
        pos += 1;
        if len_size == 0 || len_size > 8 || off_size > 8 || pos + len_size + off_size > data.len() {
            return Err(Error::CorruptNtfs("bad data run header".into()));
        }
        let len = read_le_unsigned(&data[pos..pos + len_size]);
        pos += len_size;
        if len == 0 {
            return Err(Error::CorruptNtfs("zero-length data run".into()));
        }
        let run_lcn = if off_size == 0 {
            None
        } else {
            let delta = read_le_signed(&data[pos..pos + off_size]);
            pos += off_size;
            lcn = lcn
                .checked_add(delta)
                .filter(|&l| l >= 0)
                .ok_or_else(|| Error::CorruptNtfs("data run points before the volume".into()))?;
            Some(lcn as u64)
        };
        runs.push(Run {
            vcn,
            lcn: run_lcn,
            len,
        });
        vcn = vcn
            .checked_add(len)
            .ok_or_else(|| Error::CorruptNtfs("data run overflow".into()))?;
    }
    Err(Error::CorruptNtfs("unterminated data runs".into()))
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
    fn rejects_garbage() {
        assert!(decode(&[0x21, 0x18], 0).is_err());
        assert!(decode(&[0x11, 0x05, 0x80, 0x00], 0).is_err()); // negative LCN
        assert!(decode(&[0x11, 0x05, 0x10], 0).is_err()); // no terminator
        assert!(decode(&[0x10, 0x00], 0).is_err()); // zero-size length
    }
}
