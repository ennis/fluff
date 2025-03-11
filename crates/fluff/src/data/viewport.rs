use crate::camera_control::Camera;
use crate::data::scene::SceneModel;
use kyute::model::{EventEmitter, Model};

/// Event emitted by the viewport model.
pub enum ViewportEvent {
    /// The camera has been changed (either programmatically or by the user).
    CameraChanged,
    /// The camera has been changed programmatically (not by the user).
    CameraChangedInternal,
}

/// Viewport model data.
pub struct ViewportData {
    /// Current camera.
    pub camera: Camera,
    /// The scene to render.
    pub scene: SceneModel,
}

/// Viewport models.
///
/// It emits events of type ViewportEvent.
pub type ViewportModel = Model<ViewportData>;

impl EventEmitter<ViewportEvent> for ViewportModel {}