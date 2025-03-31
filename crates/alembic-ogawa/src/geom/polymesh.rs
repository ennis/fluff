use crate::CompoundPropertyReader;
use crate::geom::{GeomBase, GeomParam};
use crate::property::TypedArrayPropertyReader;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MeshTopologyVariance {
    /// Same topology, constant vertex positions.
    Constant,
    /// Same topology, varying vertex positions.
    Homogeneous,
    /// Varying topology.
    Heterogeneous,
}

pub struct PolyMesh {
    pub geom_base: GeomBase,
    pub positions: TypedArrayPropertyReader<[f32; 3]>,
    /// How many vertices each face has.
    pub face_counts: TypedArrayPropertyReader<i32>,
    /// The vertex indices for each face.
    pub face_indices: TypedArrayPropertyReader<i32>,
    pub uvs: Option<GeomParam<[f32; 2]>>,
    pub normals: Option<GeomParam<[f32; 3]>>,
    pub velocities: Option<TypedArrayPropertyReader<[f32; 3]>>,
}

impl PolyMesh {
    pub fn new(parent_props: &CompoundPropertyReader, prop_name: &str) -> crate::Result<Self> {
        let properties = parent_props.compound_property(prop_name)?;
        let geom_base = GeomBase::new(&properties)?;
        let positions = TypedArrayPropertyReader::new(&properties, "P")?;

        let face_counts = TypedArrayPropertyReader::new(&properties, ".faceCounts")?;
        let face_indices = TypedArrayPropertyReader::new(&properties, ".faceIndices")?;
        let uvs = GeomParam::new(&properties, "uv").ok();
        let normals = GeomParam::new(&properties, "N").ok();
        let velocities = TypedArrayPropertyReader::new(&properties, ".velocities").ok();

        Ok(Self {
            geom_base,
            positions,
            face_counts,
            face_indices,
            uvs,
            normals,
            velocities,
        })
    }

    pub fn sample_count(&self) -> usize {
        self.positions.sample_count()
    }

    pub fn topology_variance(&self) -> MeshTopologyVariance {
        if self.face_indices.is_constant() && self.face_counts.is_constant() {
            if self.positions.is_constant() {
                MeshTopologyVariance::Constant
            } else {
                MeshTopologyVariance::Homogeneous
            }
        } else {
            MeshTopologyVariance::Heterogeneous
        }
    }
}
