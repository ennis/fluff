mod xform;
mod polymesh;

use crate::CompoundPropertyReader;
use crate::property::TypedScalarPropertyReader;

pub use xform::{XForm};
pub use polymesh::{PolyMesh};

pub struct GeomBase {
    pub self_bounds: TypedScalarPropertyReader<[f64; 6]>,
    pub child_bounds: Option<TypedScalarPropertyReader<[f64; 6]>>,
    pub geom_params: Option<CompoundPropertyReader>,
    pub user_properties: Option<CompoundPropertyReader>,
}

impl GeomBase {
    pub fn new(properties: &CompoundPropertyReader) -> crate::Result<Self> {
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
