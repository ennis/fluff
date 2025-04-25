pub mod scene;
pub mod viewport;
mod timeline;

use std::rc::Rc;
use kyute::event::EmitterHandle;

pub use timeline::Timeline;
pub use viewport::Viewport;

/// Root application model.
pub struct AppModel {
    emitter: EmitterHandle,
    /// The 3D viewport and its associated camera
    pub viewport: Rc<Viewport>,
    /// Timeline
    pub timeline: Rc<Timeline>,
}

impl AppModel {
    pub fn new() -> Rc<Self> {
        let timeline = Timeline::new();
        let viewport = Viewport::new(timeline.clone());
        Rc::new(Self { emitter: EmitterHandle::new(), viewport, timeline })
    }
}