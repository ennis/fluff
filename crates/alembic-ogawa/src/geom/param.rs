use crate::error::Error;
use crate::property::PropertyReader;
use crate::{CompoundPropertyReader, DataType, Metadata, Result, TimeSampling, TypedArrayPropertyReader};
use std::mem::MaybeUninit;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GeometryScope {
    // Like USD primvar interpolation
    /// Constant over the entire mesh (one value)
    Constant = 0,
    /// Same as constant for the primitives that we care about
    Uniform = 1,
    /// Varies per face vertex
    Varying = 2,
    /// Same as varying for the primitives that we care about
    Vertex = 3,
    /// Same as varying for the primitives that we care about
    FaceVarying = 4,
    Unknown = 127,
}

fn get_geometry_scope(metadata: &Metadata) -> GeometryScope {
    match metadata.get_str("geoScope") {
        Some(scope) => match scope {
            "con" | "" => GeometryScope::Constant,
            "uni" => GeometryScope::Uniform,
            "var" => GeometryScope::Varying,
            "vtx" => GeometryScope::Vertex,
            "fvr" => GeometryScope::FaceVarying,
            _ => GeometryScope::Unknown,
        },
        None => GeometryScope::Unknown,
    }
}

pub struct GeomParam<T> {
    pub scope: GeometryScope,
    pub indices: Option<TypedArrayPropertyReader<u32>>,
    pub values: TypedArrayPropertyReader<T>,
}

impl<T: DataType> GeomParam<T> {
    pub fn new(parent_prop: &CompoundPropertyReader, name: &str) -> Result<Self> {
        match parent_prop.property(name)? {
            PropertyReader::Compound(c) => {
                let indices = Some(TypedArrayPropertyReader::new(&c, "indices")?);
                let values = TypedArrayPropertyReader::new(&c, "vals")?;
                let scope = get_geometry_scope(values.metadata());
                Ok(Self { scope, indices, values })
            }
            PropertyReader::Scalar(_) => Err(Error::MalformedData),
            PropertyReader::Array(a) => {
                let scope = get_geometry_scope(a.metadata());
                Ok(Self {
                    scope,
                    indices: None,
                    values: TypedArrayPropertyReader::new(parent_prop, name)?,
                })
            }
        }
    }

    pub fn is_indexed(&self) -> bool {
        self.indices.is_some()
    }

    pub fn read_sample_into(&self, sample_index: usize, sample: &mut [MaybeUninit<T>]) -> Result<usize> {
        // TODO indexed read
        self.values.read_sample_into(sample_index, sample)
    }

    pub fn time_sampling(&self) -> &TimeSampling {
        self.values.time_sampling()
    }

    pub fn sample_count(&self) -> usize {
        self.values.sample_count()
    }

    pub fn is_constant(&self) -> bool {
        self.values.is_constant()
    }
}
