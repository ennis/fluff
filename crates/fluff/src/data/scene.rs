//! Scene data model.
use crate::scene::Scene3D;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SceneEvent {
    /// The scene changed somehow.
    Changed,
}
