use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Arc;
use byteorder::{ReadBytesExt, LE};
use memmap2::Mmap;
use crate::{read_u32le};
use crate::error::invalid_data;
use crate::group::Group;
use crate::metadata::{read_indexed_metadata, Metadata};
use crate::object::{ObjectHeader, ObjectReader};
use crate::Result;

pub(crate) type ArchiveData = [u8];

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Debug)]
pub struct TimeSamples {
    pub max_sample: u32,
    pub time_per_sample: f64,
    pub samples: Vec<f64>,
}

fn read_time_samples(data: &[u8]) -> crate::Result<TimeSamples> {
    // read time samples
    let mut time_samples_data = io::Cursor::new(data);
    let max_sample = time_samples_data.read_u32::<LE>()?;
    let time_per_sample = time_samples_data.read_f64::<LE>()?;
    let sample_count = time_samples_data.read_u32::<LE>()?;
    let mut samples = Vec::with_capacity(sample_count as usize);
    time_samples_data.read_f64_into::<LE>(&mut samples)?;
    Ok(TimeSamples {
        max_sample,
        time_per_sample,
        samples,
    })
}


////////////////////////////////////////////////////////////////////////////////////////////////////

pub(crate) struct ArchiveInner {
    pub(crate) data: Mmap,
    archive_version: u32,
    file_version: u32,
    time_samples: TimeSamples,
    file_metadata: Metadata,
    pub(crate) indexed_metadata: Vec<Metadata>,
    object_root_offset: usize,
    root_object_header: Arc<ObjectHeader>,

}

pub struct Archive(pub(crate) Arc<ArchiveInner>);

impl Archive {
    pub fn open<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        Self::open_inner(path.as_ref())
    }

    fn open_inner(path: &Path) -> crate::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        // 8-byte header
        if mmap.len() < 8 {
            return Err(invalid_data("file too short"));
        }

        let header = unsafe { &*(mmap.as_ptr() as *const Header) };

        // check signature
        if &header.signature[..] != b"Ogawa" {
            return Err(invalid_data("invalid file signature"));
        }

        // check frozen bytes
        if header.frozen != 0xFF {
            return Err(invalid_data("file was not closed properly"));
        }

        // load root group
        let root = Group::read(&mmap, u64::from_le_bytes(header.root_group_offset).try_into().unwrap())?;
        let archive_version = read_u32le(root.read_data(&mmap, 0)?)?;
        let file_version = read_u32le(root.read_data(&mmap, 1)?)?;
        let object_root_offset = root.stream_offset(2);
        let time_samples = read_time_samples(root.read_data(&mmap, 4)?)?;
        let indexed_metadata = read_indexed_metadata(root.read_data(&mmap, 5)?)?;
        let file_metadata = std::str::from_utf8(root.read_data(&mmap, 3)?)
            .map_err(|_| invalid_data("invalid UTF-8"))?
            .parse()?;
        let root_object_header = Arc::new(ObjectHeader::root(object_root_offset));

        Ok(Archive(Arc::new(ArchiveInner {
            data: mmap,
            archive_version,
            file_version,
            time_samples,
            indexed_metadata,
            object_root_offset,
            file_metadata,
            root_object_header
        })))
    }

    pub fn time_samples(&self) -> &TimeSamples {
        &self.0.time_samples
    }

    pub fn root(&self) -> Result<ObjectReader> {
        ObjectReader::new(self.0.clone(), self.0.root_object_header.clone())
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Header {
    /// File signature (== "Ogawa")
    signature: [u8; 5],
    frozen: u8,
    version: [u8; 2],
    root_group_offset: [u8; 8],
}