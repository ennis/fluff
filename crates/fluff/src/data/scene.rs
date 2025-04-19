//! Scene data model.
use crate::scene::Scene3D;
use kyute::model::Model;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SceneEvent {
    /// The scene changed somehow.
    Changed,
}
