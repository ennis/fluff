//! Platform-specific implementations of certain types and functions.

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use self::windows::*;


/// Handler for window events.
pub trait WindowHandler {
    /// Called by the event loop when a window event is received that targets this window.
    fn event(&self, event: &winit::event::WindowEvent);

    /// Redraws the window.
    fn redraw(&self);

    fn request_redraw(&self);
}

/// Reason for waking  the event loop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventLoopWakeReason {
    /// Triggers an UI update
    DispatchCallbacks,
    /// Compositor clock tick
    CompositorClockTick,
    Redraw,
}
