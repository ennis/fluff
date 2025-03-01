use crate::{Result};
use byteorder::{LE, ReadBytesExt};
use std::io::Read;
use std::{io, slice};
use crate::archive::ArchiveData;
use crate::error::invalid_data;

const DATA_BIT: u64 = 0x8000_0000_0000_0000;
const OFFSET_MASK: u64 = 0x7FFF_FFFF_FFFF_FFFF;

pub(crate) fn is_group(stream_id: u64) -> bool {
    (stream_id & DATA_BIT) == 0
}

pub(crate) fn is_data(stream_id: u64) -> bool {
    !is_group(stream_id)
}

#[derive(Clone)]
pub(crate) struct Group {
    pub(crate) children: Vec<u64>,
}

impl Group {
    pub(crate) fn read(archive: &ArchiveData, offset: usize) -> Result<Self> {
        if offset == 0 {
            // empty group
            return Ok(Self { children: Vec::new() });
        }

        let mut reader = io::Cursor::new(&archive[offset..]);
        let count = reader.read_u64::<LE>()? as usize;
        let mut children = Vec::with_capacity(count);
        for _ in 0..count {
            let mut child = [0u8; 8];
            reader.read_exact(&mut child)?;
            children.push(u64::from_le_bytes(child));
        }
        Ok(Self { children })
    }

    pub(crate) fn is_group(&self, index: usize) -> bool {
        is_group(self.children[index])
    }

    pub(crate) fn stream_offset(&self, index: usize) -> usize {
        (self.children[index] & OFFSET_MASK).try_into().unwrap()
    }

    pub(crate) fn read_data<'a>(&self, archive: &'a ArchiveData, index: usize) -> Result<&'a [u8]> {
        if !is_data(self.children[index]) {
            return Err(invalid_data("expected data"));
        }
        let offset = (self.children[index] & OFFSET_MASK).try_into().unwrap();
        if offset == 0 {
            // empty data
            return Ok(&[]);
        }
        let data = read_length_prefixed_slice(archive, offset)?;
        Ok(data)
    }
}

fn read_u64le(data: &[u8], offset: usize) -> Result<u64> {
    if data.len() < offset + 8 {
        return Err(invalid_data("data too short"));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[offset..offset + 8]);
    Ok(u64::from_le_bytes(bytes))
}

fn read_length_prefixed_slice<'a, T>(data: &[u8], offset: usize) -> Result<&'a [T]> {
    let count = read_u64le(data, offset)? as usize;
    if data.len() < offset + count * size_of::<T>() {
        return Err(invalid_data("data too short"));
    }
    unsafe {
        let ptr = data.as_ptr().add(offset + 8) as *const T;
        if !ptr.is_aligned() {
            return Err(invalid_data("unaligned data"));
        }
        Ok(slice::from_raw_parts(ptr, count))
    }
}
