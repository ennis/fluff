//! Platform-specific implementations of certain types and functions.

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use self::windows::{Window, CompositionContext, ApplicationBackend, DrawSurface, DrawSurfaceContext};
