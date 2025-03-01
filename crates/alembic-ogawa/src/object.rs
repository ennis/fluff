use crate::archive::ArchiveInner;
use crate::error::invalid_data;
use crate::metadata::Metadata;
use crate::property::{CompoundPropertyReader, PropertyHeader};
use crate::{Group, Result, read_string};
use byteorder::{LE, ReadBytesExt};
use std::collections::BTreeMap;
use std::io;
use std::io::Seek;
use std::sync::Arc;
use crate::group::is_group;

/// Information about an object.
#[derive(Clone)]
pub struct ObjectHeader {
    pub name: String,
    pub metadata: Metadata,
    /// Offset to the group containing the object's data.
    offset: usize,
    /// The full path of the object in the archive.
    path: String,
}

impl ObjectHeader {
    pub(crate) fn root(data_offset: usize) -> Self {
        Self {
            name: String::new(),
            metadata: Metadata::default(),
            offset: data_offset,
            path: String::new(),
        }
    }
}

/// Represents an object being read from an archive.
pub struct ObjectReader {
    archive: Arc<ArchiveInner>,
    header: Arc<ObjectHeader>,
    children: Vec<Arc<ObjectHeader>>,
    children_by_name: BTreeMap<String, usize>,
    properties: CompoundPropertyReader,
}

impl ObjectReader {
    pub(crate) fn new(archive: Arc<ArchiveInner>, header: Arc<ObjectHeader>) -> Result<Self> {
        let group = Group::read(&archive.data, header.offset)?;
        let headers = read_object_headers(&archive, &header, &group)?;
        let root_compound_header = if is_group(group.children[0]) {
            Arc::new(PropertyHeader::root_compound_property(group.stream_offset(0)))
        } else {
            Arc::new(PropertyHeader::root_compound_property(0))
        };
        let properties = CompoundPropertyReader::new_inner(archive.clone(), root_compound_header)?;
        let children_by_name = headers
            .iter()
            .enumerate()
            .map(|(i, header)| (header.name.clone(), i))
            .collect();
        Ok(Self {
            archive: archive.clone(),
            header,
            children: headers,
            children_by_name,
            properties,
        })
    }

    /// Returns the number of child objects.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Loads a child object by index.
    pub fn get(&self, index: usize) -> Result<ObjectReader> {
        ObjectReader::new(self.archive.clone(), self.children[index].clone())
    }

    /// Returns information about a child object by index.
    pub fn header(&self, index: usize) -> &ObjectHeader {
        &self.children[index]
    }

    /// Returns an iterator over child object headers.
    pub fn headers(&self) -> impl Iterator<Item = &ObjectHeader> {
        self.children.iter().map(|header| header.as_ref())
    }

    /// Finds a child object by name.
    ///
    /// # Return value
    /// The index of the object if found, or `None` if not found.
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        self.children_by_name.get(name).copied()
    }

    /// Returns the `CompoundPropertyReader` for this object's own properties.
    pub fn properties(&self) -> &CompoundPropertyReader {
        &self.properties
    }
}

fn read_object_headers(archive: &ArchiveInner, header: &ObjectHeader, group: &Group) -> Result<Vec<Arc<ObjectHeader>>> {
    let path = &header.path;
    let headers = group.read_data(&archive.data, group.children.len() - 1)?;
    // last 32 bytes contain the hashes?
    // (https://github.com/Traverse-Research/ogawa-rs/blob/c56cc6f98b194582b5ced6a12624b72e8da7fabc/src/object_reader.rs#L122)
    let headers = &headers[..headers.len() - 32];
    let mut reader = io::Cursor::new(headers);
    let mut object_headers = Vec::new();

    let mut child_index = 0;
    while reader.position() < headers.len() as u64 {
        let name_size = reader.read_u32::<LE>()? as usize;
        let name = read_string(&mut reader, name_size)?;

        let metadata_index = reader.read_u8()?;
        let metadata = if metadata_index == 0xFF {
            // inline metadata
            let metadata_size = reader.read_u32::<LE>()? as usize;
            let metadata_offset = reader.position() as usize;

            let metadata = std::str::from_utf8(&headers[metadata_offset..metadata_offset + metadata_size])
                .map_err(|_| invalid_data("invalid UTF-8"))?;
            let metadata = metadata.parse()?;
            reader.seek_relative(metadata_size as i64)?;
            metadata
        } else {
            // indexed metadata
            assert!(
                metadata_index < archive.indexed_metadata.len() as u8,
                "invalid metadata index {}, number of entries is {}",
                metadata_index,
                archive.indexed_metadata.len()
            );
            archive.indexed_metadata[metadata_index as usize].clone()
        };

        let offset = group.stream_offset(child_index + 1);
        let child_path = format!("{}/{}", path, name);
        object_headers.push(Arc::new(ObjectHeader {
            name,
            metadata,
            offset,
            path: child_path,
        }));
        child_index += 1;
    }

    Ok(object_headers)
}
