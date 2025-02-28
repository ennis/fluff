use crate::property::{TypedArrayPropertyReader, TypedScalarPropertyReader};
use crate::{CompoundPropertyReader, Result};

pub struct GeomBase {
    pub self_bounds: TypedScalarPropertyReader<[f32; 6]>,
    pub child_bounds: Option<TypedScalarPropertyReader<[f32; 6]>>,
    pub geom_params: Option<CompoundPropertyReader>,
    pub user_properties: Option<CompoundPropertyReader>,
}

impl GeomBase {
    pub fn new(properties: &CompoundPropertyReader) -> Result<Self> {
        let self_bounds = TypedScalarPropertyReader::new(properties, ".selfBnds")?;
        let child_bounds = TypedScalarPropertyReader::new(properties, ".childBnds").ok();
        let geom_params = CompoundPropertyReader::new(properties, ".geomParams").ok();
        let user_properties = CompoundPropertyReader::new(properties, ".userProperties").ok();

        Ok(Self {
            self_bounds,
            child_bounds,
            geom_params,
            user_properties,
        })
    }
}

pub struct PolyMesh {
    pub geom_base: GeomBase,
    pub positions: TypedArrayPropertyReader<[f32; 3]>,
    pub counts: TypedArrayPropertyReader<i32>,
    pub indices: TypedArrayPropertyReader<i32>,
    pub uvs: Option<TypedArrayPropertyReader<[f32; 2]>>,
    pub normals: Option<TypedArrayPropertyReader<[f32; 3]>>,
    pub velocities: Option<TypedArrayPropertyReader<[f32; 3]>>,
}

impl PolyMesh {
    pub fn new(properties: &CompoundPropertyReader) -> Result<Self> {
        let geom_base = GeomBase::new(properties)?;
        let positions = TypedArrayPropertyReader::new(properties, "P")?;

        let counts = TypedArrayPropertyReader::new(properties, ".faceCounts")?;
        let indices = TypedArrayPropertyReader::new(properties, ".faceIndices")?;
        let uvs = TypedArrayPropertyReader::new(properties, "uv").ok();
        let normals = TypedArrayPropertyReader::new(properties, "N").ok();
        let velocities = TypedArrayPropertyReader::new(properties, ".velocities").ok();

        Ok(Self {
            geom_base,
            positions,
            counts,
            indices,
            uvs,
            normals,
            velocities,
        })
    }
}

/*
enum XFormValues {
    Scalar(TypedScalarPropertyReader<f32>),
    Array(TypedArrayPropertyReader<f32>),
}
*/

pub struct XForm {
    child_bounds: Option<TypedScalarPropertyReader<[f32; 6]>>,
    inherits: Option<TypedScalarPropertyReader<bool>>,
}

impl XForm {
    pub fn new(properties: &CompoundPropertyReader) -> Result<Self> {
        let child_bounds = TypedScalarPropertyReader::new(properties, ".childBnds").ok();
        let inherits = TypedScalarPropertyReader::new(properties, ".inherits").ok();

        Ok(Self {
            child_bounds,
            inherits,
        })
    }
}