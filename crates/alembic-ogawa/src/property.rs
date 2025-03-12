use crate::archive::ArchiveInner;
use crate::data_type::{DataType, PodType};
use crate::error::{Error, invalid_data};
use crate::group::Group;
use crate::metadata::Metadata;
use crate::{Result, SampleIndex, TimeSampling, read_string};
use arrayvec::ArrayVec;
use byteorder::{LE, ReadBytesExt};
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::Range;
use std::sync::Arc;
use std::{io, mem, slice};

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PropertyType {
    Compound = 0,
    Scalar,
    Array,
}

pub enum PropertyReader {
    Compound(CompoundPropertyReader),
    Scalar(ScalarPropertyReader),
    Array(ArrayPropertyReader),
}

pub type Dimensions = ArrayVec<usize, 3>;

impl PropertyReader {
    pub fn header(&self) -> &PropertyHeader {
        match self {
            PropertyReader::Compound(reader) => &reader.header,
            PropertyReader::Scalar(reader) => &reader.header,
            PropertyReader::Array(reader) => &reader.header,
        }
    }

    pub fn time_sampling(&self) -> &TimeSampling {
        match self {
            PropertyReader::Compound(reader) => &reader.archive.time_samplings[reader.header.time_sampling_index],
            PropertyReader::Scalar(reader) => reader.time_sampling(),
            PropertyReader::Array(reader) => reader.time_sampling(),
        }
    }

    pub fn sample_count(&self) -> usize {
        self.header().next_sample_index
    }
}

/// Represents a compound property being read from an archive.
#[derive(Clone)]
pub struct CompoundPropertyReader {
    archive: Arc<ArchiveInner>,
    header: Arc<PropertyHeader>,
    pub(crate) property_headers: Vec<Arc<PropertyHeader>>,
    properties_by_name: BTreeMap<String, usize>,
}

impl CompoundPropertyReader {
    pub(crate) fn new_inner(archive: Arc<ArchiveInner>, header: Arc<PropertyHeader>) -> Result<Self> {
        let property_headers = read_property_headers(&archive, &header)?;
        let properties_by_name = property_headers
            .iter()
            .enumerate()
            .map(|(i, header)| (header.name.clone(), i))
            .collect();
        Ok(Self {
            archive,
            header,
            property_headers,
            properties_by_name,
        })
    }

    pub fn new(parent: &CompoundPropertyReader, name: &str) -> Result<Self> {
        parent.compound_property(name)
    }

    /// Returns whether the specified property exists.
    pub fn has_property(&self, name: &str) -> bool {
        self.properties_by_name.contains_key(name)
    }

    /// Returns the number of sub-properties.
    pub fn property_count(&self) -> usize {
        self.property_headers.len()
    }

    fn find_property(&self, name: &str) -> Result<usize> {
        self.properties_by_name
            .get(name)
            .copied()
            .ok_or(Error::PropertyNotFound)
    }

    /// Returns an iterator over the property headers.
    pub fn property_headers(&self) -> impl Iterator<Item = &PropertyHeader> {
        self.property_headers.iter().map(|x| &**x)
    }

    /// Returns information about the specified sub-property.
    pub fn property_header(&self, name: &str) -> Result<&PropertyHeader> {
        let index = self.find_property(name)?;
        Ok(&self.property_headers[index])
    }

    /// Returns information about the specified sub-property.
    pub fn property_header_by_index(&self, index: usize) -> Result<&PropertyHeader> {
        Ok(&self.property_headers[index])
    }

    /// Reads a child compound property.
    pub fn compound_property(&self, name: &str) -> Result<CompoundPropertyReader> {
        let index = self.find_property(name)?;
        let header = &self.property_headers[index];
        if header.ty != PropertyType::Compound {
            return Err(Error::UnexpectedPropertyType);
        }
        CompoundPropertyReader::new_inner(self.archive.clone(), header.clone())
    }

    /// Reads a scalar property.
    pub fn scalar_property(&self, name: &str) -> Result<ScalarPropertyReader> {
        let index = self.find_property(name)?;
        let header = &self.property_headers[index];
        if header.ty != PropertyType::Scalar {
            return Err(Error::UnexpectedPropertyType);
        }
        ScalarPropertyReader::new_inner(self.archive.clone(), header.clone())
    }

    /// Reads an array property.
    pub fn array_property(&self, name: &str) -> Result<ArrayPropertyReader> {
        let index = self.find_property(name)?;
        let header = &self.property_headers[index];
        if header.ty != PropertyType::Array {
            return Err(Error::UnexpectedPropertyType);
        }
        ArrayPropertyReader::new_inner(self.archive.clone(), header.clone())
    }

    /// Reads a sub-property by name.
    pub fn property(&self, name: &str) -> Result<PropertyReader> {
        let index = self.find_property(name)?;
        let header = &self.property_headers[index];
        match header.ty {
            PropertyType::Compound => Ok(PropertyReader::Compound(CompoundPropertyReader::new_inner(
                self.archive.clone(),
                self.property_headers[index].clone(),
            )?)),
            PropertyType::Scalar => Ok(PropertyReader::Scalar(self.scalar_property(name)?)),
            PropertyType::Array => Ok(PropertyReader::Array(self.array_property(name)?)),
        }
    }
}

fn remap_sample_index(i: usize, changed: Range<usize>) -> usize {
    if changed.is_empty() {
        return 0;
    }
    if changed.contains(&i) {
        i - changed.start + 1
    } else if i < changed.start {
        0
    } else {
        changed.len() + 1
    }
}

pub struct ScalarPropertyReader {
    archive: Arc<ArchiveInner>,
    header: Arc<PropertyHeader>,
    group: Group,
}

impl ScalarPropertyReader {
    pub(crate) fn new_inner(archive: Arc<ArchiveInner>, header: Arc<PropertyHeader>) -> Result<Self> {
        let group = Group::read(&archive.data, header.offset)?;
        Ok(Self { archive, header, group })
    }

    /// Returns the number of samples.
    pub fn sample_count(&self) -> usize {
        self.header.next_sample_index
    }

    /// Returns whether this property is constant.
    pub fn is_constant(&self) -> bool {
        self.header.first_changed_index == 0
    }

    /// Returns the extent (number of scalar elements) in each sample.
    pub fn extent(&self) -> usize {
        self.header.extent
    }

    /// Returns the time sampling of this property.
    pub fn time_sampling(&self) -> &TimeSampling {
        &self.archive.time_samplings[self.header.time_sampling_index]
    }

    /// Reads a sample by index.
    pub fn read_sample_into<'a, T: DataType>(
        &self,
        sample_index: usize,
        sample: &'a mut MaybeUninit<T>,
    ) -> Result<&'a mut T> {
        self.read_array_sample_into(sample_index, slice::from_mut(sample))?;
        // SAFETY: the sample has been initialized by read_array_sample_into, or this function would
        // have already returned an error.
        Ok(unsafe { sample.assume_init_mut() })
    }

    /// Reads a sample by index into an array.
    ///
    /// This can be used when the extent of the scalar property is not known beforehand.
    ///
    /// # Arguments
    /// * `sample_index` - index of the sample to read
    /// * `sample` - slice to read each element of the sample into. Must have the same length as the extent of the property.
    pub fn read_array_sample_into<T: DataType>(
        &self,
        sample_index: usize,
        sample: &mut [MaybeUninit<T>],
    ) -> Result<()> {
        if T::ELEMENT_TYPE != self.header.data_type {
            return Err(Error::UnexpectedDataType);
        }

        let index = remap_sample_index(
            sample_index,
            self.header.first_changed_index..self.header.last_changed_index,
        );
        let data = self.group.read_data(&self.archive.data, index)?;

        for i in 0..self.header.extent {
            // read value (skip first 16 bytes which contain the hash)
            sample[i].write(T::from_bytes(&data[16 + i * size_of::<T>()..])?);
        }

        Ok(())
    }

    /// Reads all samples into the specified slice.
    pub fn read_samples<'a, T: DataType>(&self, samples: &'a mut [MaybeUninit<T>]) -> Result<&'a mut [T]> {
        assert!(samples.len() == self.sample_count());
        for i in 0..self.sample_count() {
            self.read_sample_into(i, &mut samples[i])?;
        }
        unsafe { Ok(mem::transmute::<_, _>(samples)) }
    }
}

/// Typed scalar property.
pub struct TypedScalarPropertyReader<T: ?Sized>(ScalarPropertyReader, PhantomData<T>);

impl<T: DataType> TypedScalarPropertyReader<T> {
    pub fn new(parent: &CompoundPropertyReader, name: &str) -> Result<Self> {
        let reader = parent.scalar_property(name)?;
        if T::ELEMENT_TYPE != reader.header.data_type || T::EXTENT != Some(reader.header.extent) {
            return Err(Error::UnexpectedDataType);
        }
        Ok(Self(reader, PhantomData))
    }

    /// Returns whether this property is constant.
    pub fn is_constant(&self) -> bool {
        self.0.is_constant()
    }

    pub fn read_sample_into<'a>(&self, sample_index: usize, sample: &'a mut MaybeUninit<T>) -> Result<&'a mut T> {
        self.0.read_sample_into(sample_index, sample)
    }

    pub fn read_samples<'a>(&self, samples: &'a mut [MaybeUninit<T>]) -> Result<&'a mut [T]> {
        self.0.read_samples(samples)
    }

    pub fn get(&self, sample_index: usize) -> Result<T> {
        let mut sample = MaybeUninit::uninit();
        self.read_sample_into(sample_index, &mut sample)?;
        Ok(unsafe { sample.assume_init() })
    }

    pub fn time_sampling(&self) -> &TimeSampling {
        self.0.time_sampling()
    }
}

impl<T: DataType + Copy> TypedScalarPropertyReader<[T]> {
    pub fn new_array(parent: &CompoundPropertyReader, name: &str) -> Result<Self> {
        let reader = parent.scalar_property(name)?;
        if T::ELEMENT_TYPE != reader.header.data_type {
            return Err(Error::UnexpectedDataType);
        }
        Ok(Self(reader, PhantomData))
    }

    pub fn get(&self, sample_index: usize) -> Result<Vec<T>> {
        let mut sample = vec![MaybeUninit::<T>::uninit(); self.0.header.extent];
        self.0.read_array_sample_into(sample_index, &mut sample)?;
        Ok(unsafe { mem::transmute(sample) })
    }
}

/// Represents an array property being read from an archive.
pub struct ArrayPropertyReader {
    archive: Arc<ArchiveInner>,
    header: Arc<PropertyHeader>,
    group: Group,
}

impl ArrayPropertyReader {
    pub(crate) fn new_inner(archive: Arc<ArchiveInner>, header: Arc<PropertyHeader>) -> Result<Self> {
        let group = Group::read(&archive.data, header.offset)?;
        Ok(Self { archive, header, group })
    }

    /// Returns the time sampling of this property.
    pub fn time_sampling(&self) -> &TimeSampling {
        &self.archive.time_samplings[self.header.time_sampling_index]
    }

    /// Returns the metadata of this property.
    pub fn metadata(&self) -> &Metadata {
        &self.header.metadata
    }

    /// Returns whether this property is constant.
    pub fn is_constant(&self) -> bool {
        self.header.first_changed_index == 0
    }

    /// Returns the dimensions of the specified sample.
    pub fn dimensions(&self, sample_index: SampleIndex) -> Dimensions {
        let index = self.header.remap_sample_index(sample_index);

        let dim_data = self.group.read_data(&self.archive.data, 2 * index + 1).unwrap();
        let data = self.group.read_data(&self.archive.data, 2 * index).unwrap();

        if data.len() < 16 {
            // no data (empty array)
            return Dimensions::from_iter([0]);
        }

        let dimensions = if dim_data.len() == 0 {
            // no dimensions specified, assume rank 1 array, compute element count from data length
            assert!(
                self.header.data_type != PodType::String && self.header.data_type != PodType::WideString,
                "invalid array element type"
            );
            let elem_size = self.header.extent * self.header.data_type.byte_size();
            let elem_count = (data.len() - 16) / elem_size;
            if (data.len() - 16) % elem_size != 0 {
                eprintln!("warning: array data size is not a multiple of element size");
            }
            Dimensions::from_iter([elem_count])
        } else {
            // read dimensions
            let mut reader = io::Cursor::new(dim_data);
            let mut v = Dimensions::new();
            while reader.position() < dim_data.len() as u64 {
                v.push(reader.read_u32::<LE>().unwrap() as usize);
            }
            v
        };
        dimensions
    }

    pub fn read_sample<T: DataType>(&self, sample_index: SampleIndex) -> Result<NDArraySample<T>> {
        let index = remap_sample_index(
            sample_index,
            self.header.first_changed_index..self.header.last_changed_index,
        );

        if index >= self.header.next_sample_index {
            return Err(Error::TimeSampleOutOfRange);
        }

        let data = self.group.read_data(&self.archive.data, 2 * index)?;

        if data.len() == 0 {
            return Ok(NDArraySample {
                values: vec![],
                dimensions: Dimensions::from_iter([0]),
            });
        }

        if data.len() < 16 {
            return Err(invalid_data("data too short"));
        }

        // skip first 16 bytes which contain the hash
        let values = data[16..]
            .chunks_exact(size_of::<T>())
            .map(|chunk| T::from_bytes(chunk))
            .collect::<Result<Vec<T>>>()?;

        let dimensions = self.dimensions(sample_index);
        Ok(NDArraySample { values, dimensions })
    }

    pub fn read_sample_into<T: DataType>(&self, sample_index: usize, sample: &mut [MaybeUninit<T>]) -> Result<usize> {
        if T::ELEMENT_TYPE != self.header.data_type || T::EXTENT != Some(self.header.extent) {
            return Err(Error::UnexpectedDataType);
        }

        let index = self.header.remap_sample_index(sample_index);
        let data = self.group.read_data(&self.archive.data, 2 * index)?;
        let dimensions = self.dimensions(sample_index);
        let elem_count: usize = dimensions.iter().product();

        if elem_count > sample.len() {
            return Err(Error::NotEnoughSpaceInOutput);
        }

        if data.len() == 0 {
            return Ok(0);
        }

        if data.len() < 16 {
            return Err(invalid_data("data too short"));
        }

        // skip first 16 bytes which contain the hash
        for (i, values) in data[16..].chunks_exact(size_of::<T>()).enumerate() {
            // TODO don't panic
            assert!(i < elem_count, "too many elements in array");
            sample[i].write(T::from_bytes(values)?);
        }
        Ok(elem_count)
    }
}

/// Typed array property.
pub struct TypedArrayPropertyReader<T>(ArrayPropertyReader, PhantomData<T>);

impl<T: DataType> TypedArrayPropertyReader<T> {
    pub fn new(parent: &CompoundPropertyReader, name: &str) -> Result<Self> {
        let reader = parent.array_property(name)?;
        if T::ELEMENT_TYPE != reader.header.data_type {
            return Err(Error::UnexpectedDataType);
        }
        Ok(Self(reader, PhantomData))
    }

    /// Returns the length of the array in the specified dimension.
    pub fn dimensions(&self, sample_index: usize) -> Dimensions {
        self.0.dimensions(sample_index)
    }

    /// Returns the metadata of this property.
    pub fn metadata(&self) -> &Metadata {
        &self.0.header.metadata
    }

    pub fn get(&self, sample_index: usize) -> Result<NDArraySample<T>> {
        self.0.read_sample(sample_index)
    }

    pub fn read_sample_into(&self, sample_index: usize, sample: &mut [MaybeUninit<T>]) -> Result<usize> {
        self.0.read_sample_into(sample_index, sample)
    }

    pub fn time_sampling(&self) -> &TimeSampling {
        self.0.time_sampling()
    }

    pub fn sample_count(&self) -> usize {
        self.0.header.next_sample_index
    }

    pub fn is_constant(&self) -> bool {
        self.0.is_constant()
    }
}

/// Represents a sample of an array property.
#[derive(Clone, Debug)]
pub struct NDArraySample<T> {
    pub values: Vec<T>,
    pub dimensions: Dimensions,
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Information about a property.
#[derive(Clone, Debug)]
pub struct PropertyHeader {
    pub ty: PropertyType,
    pub metadata: Metadata,
    pub metadata_index: u32,
    pub data_type: PodType,
    pub name: String,
    path: String,
    /// Offset to property data (either a group or a data block) depending on the property type.
    offset: usize,
    first_changed_index: usize,
    last_changed_index: usize,
    pub(crate) next_sample_index: usize,
    time_sampling_index: usize,
    pub extent: usize,
    pub is_homogeneous: bool,
}

impl PropertyHeader {
    pub(crate) fn root_compound_property(offset: usize) -> Self {
        Self {
            ty: PropertyType::Compound,
            metadata: Metadata::default(),
            metadata_index: 0xFF,
            data_type: PodType::Bool,
            name: "".to_string(),
            path: "".to_string(),
            offset,
            first_changed_index: 0,
            last_changed_index: 0,
            next_sample_index: 0,
            time_sampling_index: 0,
            extent: 0,
            is_homogeneous: false,
        }
    }

    pub(crate) fn remap_sample_index(&self, i: SampleIndex) -> usize {
        remap_sample_index(i, self.first_changed_index..self.last_changed_index)
    }
}

/// Reads child property headers from a compound property.
///
/// # Arguments
/// * `archive` - archive to read from
/// * `header` - property header of the parent compound property
/// * `path` - path of the parent compound property
fn read_property_headers(archive: &ArchiveInner, header: &PropertyHeader) -> Result<Vec<Arc<PropertyHeader>>> {
    let path = &header.path;
    let prop_data = Group::read(&archive.data, header.offset)?;
    if prop_data.children.is_empty() {
        return Ok(Vec::new());
    }
    // child headers are stored in the last child of the group
    let headers_data = prop_data.read_data(&*archive.data, prop_data.children.len() - 1)?;
    let mut reader = io::Cursor::new(headers_data);
    let mut headers = Vec::new();

    let mut index = 0;
    while reader.position() < headers_data.len() as u64 {
        let mut first_changed_index = 0;
        let mut last_changed_index = 0;

        fn bit_range(value: u32, range: Range<u32>) -> u32 {
            (value >> range.start) & ((1 << (range.end - range.start)) - 1)
        }

        let info = reader.read_u32::<LE>()?;
        let property_type = bit_range(info, 0..2);
        let property_type = match property_type {
            0 => PropertyType::Compound,
            1 => PropertyType::Scalar,
            _ => PropertyType::Array,
        };
        let size_hint = bit_range(info, 2..4);
        let data_type: PodType = bit_range(info, 4..8).try_into()?;
        let is_homogeneous = bit_range(info, 10..11) == 1;
        let extent = bit_range(info, 12..20) as usize;
        let metadata_index = bit_range(info, 20..28);

        fn read_size(reader: &mut io::Cursor<&[u8]>, size_hint: u32) -> crate::Result<usize> {
            Ok(match size_hint {
                0 => reader.read_u8()? as usize,
                1 => reader.read_u16::<LE>()? as usize,
                2 => reader.read_u32::<LE>()? as usize,
                _ => return Err(invalid_data("invalid size hint")),
            })
        }

        let mut next_sample_index = 0;
        let mut time_sampling_index = 0;
        if property_type != PropertyType::Compound {
            next_sample_index = read_size(&mut reader, size_hint)?;
            if bit_range(info, 9..10) == 1 {
                first_changed_index = read_size(&mut reader, size_hint)?;
                last_changed_index = read_size(&mut reader, size_hint)?;
            } else if bit_range(info, 11..12) == 1 {
                first_changed_index = 0;
                last_changed_index = 0;
            } else {
                first_changed_index = 1;
                last_changed_index = next_sample_index - 1;
            }

            if bit_range(info, 8..9) == 1 {
                time_sampling_index = read_size(&mut reader, size_hint)?;
            }
        }

        let name_size = read_size(&mut reader, size_hint)?;
        let name = read_string(&mut reader, name_size)?;

        let metadata = if metadata_index == 0xFF {
            let metadata_size = read_size(&mut reader, size_hint)?;
            let metadata: Metadata = read_string(&mut reader, metadata_size)?.parse()?;
            metadata
        } else {
            archive.indexed_metadata[metadata_index as usize].clone()
        };

        let offset = prop_data.stream_offset(index);
        let prop_path = format!("{}{}", path, name);

        headers.push(Arc::new(PropertyHeader {
            ty: property_type,
            metadata,
            data_type,
            name,
            path: prop_path,
            offset,
            metadata_index,
            first_changed_index,
            last_changed_index,
            next_sample_index,
            time_sampling_index,
            extent,
            is_homogeneous,
        }));

        index += 1;
    }
    Ok(headers)
}
