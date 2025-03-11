//! Scene data model.
use kyute::model::Model;
use crate::scene::Scene3D;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SceneEvent {
    /// The scene changed somehow.
    Changed,
}

pub type SceneModel = Model<Scene3D>;