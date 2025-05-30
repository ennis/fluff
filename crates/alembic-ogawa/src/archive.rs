use crate::error::{Error, invalid_data};
use crate::group::Group;
use crate::metadata::{Metadata, read_indexed_metadata};
use crate::object::{ObjectHeader, ObjectReader};
use crate::{Result, read_u32le};
use byteorder::{LE, ReadBytesExt};
use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Arc;

pub(crate) type ArchiveData = [u8];

////////////////////////////////////////////////////////////////////////////////////////////////////

//const ACYCLIC_NUM_SAMPLES: u32 = u32::MAX;
const ACYCLIC_TIME_PER_SAMPLE: f64 = f64::MAX / 32.;

#[derive(Clone, Debug)]
pub enum TimeSampling {
    Acyclic {
        sample_times: Vec<f64>,
    },
    Cyclic {
        time_per_cycle: f64,
        sample_times: Vec<f64>,
    },
    Uniform {
        time_per_cycle: f64,
        sample_time: f64,
    },
}

impl TimeSampling {
    pub fn get_sample_time(&self, i: usize) -> Result<f64> {
        let t = match *self {
            TimeSampling::Acyclic { ref sample_times } => {
                if i >= sample_times.len() {
                    return Err(Error::TimeSampleOutOfRange);
                }
                sample_times[i]
            }
            TimeSampling::Cyclic {
                time_per_cycle,
                ref sample_times,
            } => {
                let n = sample_times.len();
                (i / n) as f64 * time_per_cycle + sample_times[i % n]
            }
            TimeSampling::Uniform {
                sample_time,
                time_per_cycle,
            } => sample_time + time_per_cycle * i as f64,
        };
        Ok(t)
    }

    pub fn get_floor_sample(&self, time: f64) -> (f64, usize) {
        match *self {
            TimeSampling::Acyclic { ref sample_times } => {
                match sample_times.binary_search_by(|t| t.partial_cmp(&time).unwrap()) {
                    Ok(i) => (sample_times[i], i),
                    Err(i) => {
                        if i == 0 {
                            (sample_times[0], 0)
                        } else {
                            (sample_times[i - 1], i - 1)
                        }
                    }
                }
            }
            TimeSampling::Cyclic {
                time_per_cycle,
                ref sample_times,
            } => {
                // TODO test that
                let cycle = (time / time_per_cycle).floor();
                let cycle_start = cycle * time_per_cycle;
                let rel_time = time - cycle * time_per_cycle;
                match sample_times.binary_search_by(|t| t.partial_cmp(&rel_time).unwrap()) {
                    Ok(i) => (cycle_start + sample_times[i], i),
                    Err(i) => {
                        if i == 0 {
                            (cycle_start + sample_times.last().unwrap(), sample_times.len() - 1)
                        } else {
                            (cycle_start + sample_times[i - 1], i - 1)
                        }
                    }
                }
            }
            TimeSampling::Uniform {
                sample_time,
                time_per_cycle,
            } => {
                // there's only one sample
                let cycle = (time / time_per_cycle).floor();
                (sample_time + cycle * time_per_cycle, 0)
            }
        }
    }

    /// Returns the number of unique time samples.
    pub fn num_samples(&self) -> usize {
        match *self {
            TimeSampling::Acyclic { ref sample_times } => sample_times.len(),
            TimeSampling::Cyclic { ref sample_times, .. } => sample_times.len(),
            TimeSampling::Uniform { .. } => 1,
        }
    }
}

fn read_time_samples(data: &[u8]) -> Result<Vec<TimeSampling>> {
    // read time samples
    let mut reader = io::Cursor::new(data);
    let mut samplings = Vec::new();
    while reader.position() < data.len() as u64 {
        let _max_sample = reader.read_u32::<LE>()?;
        let time_per_cycle = reader.read_f64::<LE>()?;
        let sample_count = reader.read_u32::<LE>()?;
        let mut sample_times = vec![0.0; sample_count as usize];
        reader.read_f64_into::<LE>(&mut sample_times)?;

        let sampling = if time_per_cycle == ACYCLIC_TIME_PER_SAMPLE {
            TimeSampling::Acyclic { sample_times }
        } else if sample_count == 1 {
            TimeSampling::Uniform {
                time_per_cycle,
                sample_time: sample_times[0],
            }
        } else {
            TimeSampling::Cyclic {
                time_per_cycle,
                sample_times,
            }
        };
        samplings.push(sampling);
    }

    Ok(samplings)
}

////////////////////////////////////////////////////////////////////////////////////////////////////

pub(crate) struct ArchiveInner {
    pub(crate) data: Mmap,
    archive_version: u32,
    file_version: u32,
    pub(crate) time_samplings: Vec<TimeSampling>,
    file_metadata: Metadata,
    pub(crate) indexed_metadata: Vec<Metadata>,
    root_object_header: Arc<ObjectHeader>,
}

pub struct Archive(pub(crate) Arc<ArchiveInner>);

impl Archive {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_inner(path.as_ref())
    }

    pub fn archive_version(&self) -> u32 {
        self.0.archive_version
    }

    pub fn file_version(&self) -> u32 {
        self.0.file_version
    }

    pub fn file_metadata(&self) -> &Metadata {
        &self.0.file_metadata
    }

    fn open_inner(path: &Path) -> Result<Self> {
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
        let time_samplings = read_time_samples(root.read_data(&mmap, 4)?)?;
        let indexed_metadata = read_indexed_metadata(root.read_data(&mmap, 5)?)?;
        let file_metadata = std::str::from_utf8(root.read_data(&mmap, 3)?)
            .map_err(|_| invalid_data("invalid UTF-8"))?
            .parse()?;
        let root_object_header = Arc::new(ObjectHeader::root(object_root_offset));

        Ok(Archive(Arc::new(ArchiveInner {
            data: mmap,
            archive_version,
            file_version,
            time_samplings,
            indexed_metadata,
            file_metadata,
            root_object_header,
        })))
    }

    pub fn time_samplings(&self) -> &[TimeSampling] {
        &self.0.time_samplings
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
