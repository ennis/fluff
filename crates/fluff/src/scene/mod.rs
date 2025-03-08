//! 3D scenes.
mod alembic_io;
mod debug_render;
mod deprecated;
mod polymesh;
mod render;

use graal::Buffer;

pub use render::{SceneRenderItem, SceneRenderVisitor};
pub use debug_render::DebugRenderVisitor;

/// Vertex attribute of a 3D mesh.
///
/// Attributes can be animated over time.
struct Attribute<T: ?Sized> {
    /// Time position of each animation frame of the attributes.
    ///
    /// The length of this vector corresponds to the number of animation frames of the mesh.
    /// If the mesh is not animated, this vector will contain a single element.
    time_samples: Vec<f64>,
    /// Attribute data on the GPU.
    buffer: Buffer<T>,
}

/// An animated 3D mesh.
pub struct Mesh3D {
    /// Number of indices (i.e. number of "face vertices").
    index_count: u32,
    /// Index buffer (triplets of indices into attribute arrays).
    indices: Buffer<[u32]>,
    // Attributes, all of them indexed
    positions: Attribute<[[f32; 3]]>,
    normals: Option<Attribute<[[f32; 3]]>>,
    uvs: Option<Attribute<[[f32; 2]]>>,
}

pub struct TimeSample<T> {
    pub time: f64,
    pub value: T,
}

/// 3D scene object
struct Object {
    name: String,
    /// Transform relative to parent.
    transform: Vec<TimeSample<glam::DMat4>>,
    /// Geometry data.
    geometry: Option<Mesh3D>,
    /// Children objects.
    children: Vec<Object>,
}

/// 3D scene.
pub struct Scene3D {
    root: Object,
}

impl Scene3D {
    pub fn new() -> Scene3D {
        Scene3D {
            root: Object {
                name: "root".to_string(),
                transform: vec![],
                geometry: None,
                children: vec![],
            },
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use crate::gpu;

    #[test]
    fn load_alembic() {
        gpu::init();
        let path = Path::new("../alembic-ogawa/tests/data/ellie_animation.abc");
        let scene = Scene3D::load_from_alembic(path).unwrap();
    }
}
