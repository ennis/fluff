use crate::CompoundPropertyReader;
use crate::geom::GeomBase;
use crate::property::TypedArrayPropertyReader;

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
    pub fn new(parent_props: &CompoundPropertyReader, prop_name: &str) -> crate::Result<Self> {
        let properties = parent_props.compound_property(prop_name)?;
        let geom_base = GeomBase::new(&properties)?;
        let positions = TypedArrayPropertyReader::new(&properties, "P")?;

        let counts = TypedArrayPropertyReader::new(&properties, ".faceCounts")?;
        let indices = TypedArrayPropertyReader::new(&properties, ".faceIndices")?;
        let uvs = TypedArrayPropertyReader::new(&properties, "uv").ok();
        let normals = TypedArrayPropertyReader::new(&properties, "N").ok();
        let velocities = TypedArrayPropertyReader::new(&properties, ".velocities").ok();

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
    
    pub fn sample_count(&self) -> usize {
        self.positions.sample_count()
    }
}
