//! Platform-specific implementations of certain types and functions.

#[cfg(windows)]
pub mod windows;

use kurbo::{Insets, Point, Size};
use kyute_common::Color;
use crate::layout::Alignment;
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


/// Window kind.
#[derive(Clone)]
pub enum WindowKind {
    /// A normal window, with decorations (title bar, close button, etc.).
    Application,
    /// Menu window (context menus, drop-downs, etc.).
    Menu(Option<PlatformWindowHandle>),
    /// Modal dialog window
    Modal(Option<PlatformWindowHandle>),
    /// Tooltip, no decorations.
    Tooltip,
}

impl Default for WindowKind {
    fn default() -> Self {
        WindowKind::Application
    }
}

/// Describes the options for creating a new window.
#[derive(Clone)]
pub struct WindowOptions<'a> {
    /// Initial title of the window.
    pub title: &'a str,
    /// Initial client area size of the window, in device-independent pixels (logical size).
    pub size: Option<Size>,
    /// Whether the window should be created initially visible.
    ///
    /// If false, the window will be created hidden and will need to be shown explicitly by calling
    /// `show`.
    pub visible: bool,
    /// Background color of the window.
    pub background: Color,
    /// Initial position of the window, in device-independent pixels.
    pub position: Option<Point>,
    /// Centers the window relative to the parent window or the screen.
    pub center: bool,
    /// The kind of window to create.
    pub kind: WindowKind,
    /// Whether the window should have decorations (title bar, close button, etc.).
    ///
    /// This is ignored for `WindowKind::Menu` and `WindowKind::Tooltip`.
    pub decorations: bool,
    /// Whether the window should be resizable.
    ///
    /// This is ignored for `WindowKind::Menu` and `WindowKind::Tooltip`.
    pub resizable: bool,
    /// Which monitor the window should be created on.
    pub monitor: Option<Monitor>,
}

impl<'a> Default for WindowOptions<'a> {
    fn default() -> Self {
        Self {
            title: "",
            size: None,
            visible: true,
            background: Color::from_hex("#151515"),
            position: None,
            center: true,
            kind: WindowKind::default(),
            decorations: true,
            resizable: true,
            monitor: None,
        }
    }
}