mod archive;
mod data_type;
mod error;
mod group;
mod metadata;
mod object;
mod property;

pub mod geom;

use crate::error::{Error, invalid_data};
use crate::group::Group;
use std::io::Read;
use std::{io, ptr};

type Result<T> = std::result::Result<T, Error>;

fn read_string(cursor: &mut io::Cursor<&[u8]>, size: usize) -> Result<String> {
    let mut buffer = vec![0; size];
    cursor.read_exact(&mut buffer)?;
    String::from_utf8(buffer).map_err(|_| invalid_data("invalid UTF-8"))
}

fn read_as<T: Copy>(data: &[u8]) -> Result<T> {
    if data.len() < size_of::<T>() {
        return Err(invalid_data("data too short"));
    }
    let value = unsafe { ptr::read_unaligned(data.as_ptr() as *const T) };
    Ok(value)
}

fn read_u32le(data: &[u8]) -> Result<u32> {
    let bytes = read_as::<[u8; 4]>(data)?;
    Ok(u32::from_le_bytes(bytes))
}

/// Time sample.
pub struct TimeSample<T> {
    /// Time.
    pub time: f64,
    /// Value.
    pub value: T,
}

pub type SampleIndex = usize;

// Reexports
pub use archive::{Archive, TimeSampling};
pub use metadata::Metadata;
pub use object::ObjectReader;
pub use data_type::DataType;
pub use property::{ArrayPropertyReader, CompoundPropertyReader, NDArraySample, PropertyType, ScalarPropertyReader, TypedArrayPropertyReader, TypedScalarPropertyReader};

// Q: should reader objects keep a reference to the archive?
// Options:
// (A) objects/property readers borrow the archive (lifetime)
// (B) objects/property readers share the archive (Rc)
// (C) objects/property readers don't hold a reference to the archive, and need to be passed the archive when needed (e.g. read_*_property)
//
// Option A is a no-go if we need to keep reader objects alive for a long (i.e. unpredictable time).
//        use case: load time samples on demand as the user seeks through the timeline
// Option C is not very ergonomic, and there's still the question of how to handle the lifetime of the archive
//        (i.e. users of the API will have to figure out how to manage the lifetime of the archive, possibly with manual wrapping with Rc).
//        It's also possible to mix-up archives and readers, which could lead to bugs.
// Option B is what the C++ Alembic library does, but for us this means wrapping types in Rc.

////////////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::Archive;
    use crate::geom::{PolyMesh, XForm};
    use crate::object::ObjectReader;
    use crate::property::{CompoundPropertyReader, PropertyType};

    fn dump_property(reader: &CompoundPropertyReader, depth: usize) -> Result<()> {
        for (i, header) in reader.property_headers().enumerate() {
            eprintln!(
                "{}{}, type={:?}, datatype={:?}, sampleCount={:?}, metadata=[{:02x}]{:?}",
                "   ".repeat(depth),
                header.name,
                header.ty,
                header.data_type,
                header.next_sample_index,
                header.metadata_index,
                header.metadata
            );
            if header.ty == PropertyType::Compound {
                let child = reader.compound_property(&header.name)?;
                dump_property(&child, depth + 1)?;
            }
        }
        Ok(())
    }

    fn dump_object(reader: &ObjectReader, depth: usize) -> Result<()> {
        dump_property(&reader.properties(), depth + 1)?;
        for (i, child) in reader.headers().enumerate() {
            eprintln!("{}/{}", "   ".repeat(depth), child.name);
            dump_object(&reader.get(&child.name)?, depth + 1)?;
        }
        Ok(())
    }

    #[test]
    fn load_archive() {
        let archive = Archive::open("tests/data/ellie_animation.abc").unwrap();
        eprintln!("Time samplings:");
        for (i, sampling) in archive.time_samplings().iter().enumerate() {
            eprintln!("  {}: {:?}", i, sampling);
        }

        let root = archive.root().unwrap();
        dump_object(&root, 0).unwrap();
    }

    #[test]
    fn xform_schema() {
        let archive = Archive::open("tests/data/ellie_animation.abc").unwrap();
        let root = archive.root().unwrap();
        let xform = XForm::new(
            root.get("GEO-ellie_fannypack_strap_end_001").unwrap().properties(),
            ".xform",
        )
        .unwrap();

        // print all samples
        for (time, value) in xform.samples() {
            eprintln!("time={}, matrix={:?}", time, value);
        }
    }

    #[test]
    fn polymesh_schema() {
        let archive = Archive::open("tests/data/ellie_animation.abc").unwrap();
        let root = archive.root().unwrap();
        // "/GEO-ellie_fannypack_strap_end_001/Data_GEO-ellie_fannypack_strap_end"
        let mesh = PolyMesh::new(
            root.get("GEO-ellie_fannypack_strap_end_001").unwrap()
                .get("Data_GEO-ellie_fannypack_strap_end").unwrap().properties(),
            ".geom",
        )
        .unwrap();

        // dump all indices
        for s in 0..mesh.face_counts.sample_count() {
            //let time = mesh.positions.time_sampling().get_sample_time(s).unwrap();
            //let positions = mesh.positions.get(s).unwrap();
            //assert_eq!(positions.dimensions.len(), 1);
            //let indices = mesh.face_indices.get(s).unwrap();
            //assert_eq!(indices.dimensions.len(), 1);
            //let counts = mesh.face_counts.get(s).unwrap();
           // assert_eq!(counts.dimensions.len(), 1);
            
            let face_counts = mesh.face_counts.get(0);

            eprintln!("counts={:?}", face_counts);
        }
    }
}
