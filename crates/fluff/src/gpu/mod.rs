//! GPU stuff.
mod append_buffer;
mod pipelines;

pub use append_buffer::AppendBuffer;
pub use pipelines::{
    create_compute_pipeline, create_primitive_pipeline, set_global_pipeline_macro_definitions, invalidate_pipelines, MeshRenderPipelineDesc2,
    PrimitiveRenderPipelineDesc2, Error
};

use std::cell::{OnceCell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::warn;

static INITIALIZED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static DEVICE: OnceCell<&'static graal::Device> = OnceCell::new();
    static PIPELINE_MANAGER: OnceCell<RefCell<pipelines::PipelineManager>> = OnceCell::new();
}

/// Initializes the GPU device & pipeline manager on this thread.
pub fn init() {
    if INITIALIZED
        .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        warn!("GPU device already initialized");
        return;
    }

    // TODO: in theory `Device` could be thread safe, and we could have a global device
    //       usable across threads. But for now, we'll keep it simple and only allow
    //       access on the main thread.
    DEVICE.with(|device| {
        device.get_or_init(|| Box::leak(Box::new(graal::Device::new().expect("failed to create GPU device"))));
    });
    PIPELINE_MANAGER.with(|manager| {
        manager.get_or_init(|| RefCell::new(pipelines::PipelineManager::new()));
    });
}

/// Returns the global GPU device.
///
/// The GPU device is only accessible on the main thread, and after `init` has been called.
///
/// # Panics
///
/// Panics if [`init`] hasn't been called.
pub fn device() -> &'static graal::Device {
    DEVICE.with(|device| *device.get().expect("GPU device not initialized"))
}

/// Runs a closure with the global pipeline manager.
fn with_pipeline_manager<F, R>(f: F) -> R
where
    F: FnOnce(&mut pipelines::PipelineManager) -> R,
{
    PIPELINE_MANAGER.with(|manager| f(&mut *manager.get().expect("pipeline manager not initialized").borrow_mut()))
}
