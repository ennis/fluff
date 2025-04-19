use crate::camera_control::Camera;
use crate::data::scene::SceneModel;
use crate::data::timeline::{Timeline, TimelineEvent, TimelineModel};
use crate::gpu;
use graal::CommandStream;
use kyute::model::{EventEmitter, Model};
use crate::scene::Scene3D;

/// Event emitted by the viewport model.
pub enum ViewportEvent {
    /// The camera has been changed (either programmatically or by the user).
    CameraChanged,
    /// The camera has been changed programmatically (not by the user).
    CameraChangedInternal,
}

/// Viewport model data.
pub struct Viewport {
    /// Current camera.
    pub camera: Camera,
    /// The scene to render.
    pub scene: Model<Scene3D>,
    /// Timeline associated to this viewport.
    pub timeline: Model<Timeline>,
}


impl EventEmitter<ViewportEvent> for Viewport {}
impl EventEmitter<TimelineEvent> for Viewport {}

impl Viewport {
    
    pub fn new(camera: Camera, scene: Model<Scene3D>, timeline: Model<Timeline>) -> Model<Self> {
        
        let mut model = Model::new(Self { camera, scene, timeline });
        // reemit timeline events
        //model.connect::<TimelineEvent, _>(&timeline, |model, _ctx, event| {
        //    model.emit(event);
        //});
        
        model
        
    }
    
    /// Renders the viewport to the target image.
    ///
    /// # Arguments
    ///
    /// * `cmd` - The command stream to use for rendering.
    /// * `target` - The target image to render to.
    ///
    pub fn render(&self, cmd: &mut CommandStream, target: &graal::Image) {
        // TODO

        // prepare scene parameters
        let scene_params = crate::shaders::SceneParams {
            view_matrix: self.camera.view.to_cols_array_2d(),
            projection_matrix: self.camera.projection.to_cols_array_2d(),
            view_projection_matrix: (self.camera.projection * self.camera.view).to_cols_array_2d(),
            eye: self.camera.eye().as_vec3(),
            near_clip: self.camera.frustum.near_plane,
            far_clip: self.camera.frustum.far_plane,
            left: self.camera.frustum.left,
            right: self.camera.frustum.right,
            top: self.camera.frustum.top,
            bottom: self.camera.frustum.bottom,
            viewport_size: Default::default(),
            cursor_pos: Default::default(),
            time: 0.0,
        };

        // upload camera data to buffer
        let device = gpu::device();

        //let camera_data = cmd.upload_temporary()
    }
}

// All the issues with models:
//
// - need to declare two types: `*Data` & `*Model` (typedef)
//      -> maybe drop the typedef
//
// The event system is heavily dependent on `Weak` references (specifically, `Weak<dyn Any>`).
// This basically forces every object that wants to participate in the event system
// (even when only as an emitter) to be wrapped in `Rc`.
// This also makes the system incompatible for use in multiple threads at the same time (not great).
//
// Event callbacks are not very ergonomic, due to the need for downgrading to a weak ref, move the
// ref to the callback, then upgrade it again in the callback.
// You can provide shortcuts for this, but it's still not great.
//
// We should be able to forward events of a field to the parent model easily.
// (and by "easy" I mean syntactically easy: short, boilerplate-free)
//
// I don't want to see any refcell at all (hide the interior mutability somewhere).
// Interior mutability should be OK as long as callbacks are not reentrant. 
//
// Q: on what type should `EventEmitter` be implemented? `Model<Data>` or `Data` directly?
//
// Events should be emittable from any thread.



// Idea jam:
//
// Emitters need to be refcounted because we want to clean up submissions when the emitter is dropped.
// Model data could be shared as well, but that's not necessary for the event system.
// -> decouple event system from model sharing
//
// Every object that wants to participate in the event system should hold a handle internally.
// This handle is like a refcounted weak pointer. When the refcount reaches 0, the object is dropped.
// The event subscriber table holds a map from source to target handle.
//
// Idea jam:
// 
// Atomic refcounted pointer for which we can get only the refcount (no need for Weak<dyn Any>).
// On drop, lock the refcount table and decrease the refcount.




// Data sharing:
// We want models to refer to other models. However one piece of data can be referenced by multiple
// other models (e.g. the timeline's current time can be referenced by multiple viewports, effects, etc.).
//
// Basically, there needs to be some sharing involved. There are two paradigms for this:
// - shared refs to interior mutable data (e.g. Rc<RefCell<T>>): the most straightforward approach, 
//   but one that has some syntactic overhead.
// - immutable data: all data is immutable. Updated data is sent to dependent models in messages.
//   This requires immutable data structures with copy-on-write semantics.
//
// Undo/redo:
// - it's easier to implement with immutable data.


// Event emitters
// 
// The event system has a global table of subscriptions. The event system needs to know whether
// an emitter is still alive. Currently, this is done by keeping a `Weak<dyn Any>` in the 
// subscription table.
// One advantage of this is that if the emitter is `Rc<Something>` it doesn't need anything
// a separate handle for the event system.