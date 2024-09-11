use glam::DVec2;
use crate::scene::Scene;

/// Which mouse button was pressed.
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// A mouse event.
pub struct MouseEvent {
    /// Position in the viewport.
    pub position: DVec2,
    pub button: MouseButton,
    pub pressed: bool,
}

pub struct StylusEvent {
    pub position: DVec2,
    pub delta: DVec2,
    pub pressure: f32,
    pub tilt: DVec2,
}

pub struct ToolCtx<'a> {
    pub scene: &'a mut Scene,
    pub viewport_size: DVec2,
}

impl<'a> ToolCtx<'a> {
    /// Sets the current active tool.
    pub fn set_active_tool<T: Tool + 'static>(&mut self) {
        // TODO
    }

    /// Sets the tool information text in the status bar.
    ///
    /// This stays until the next call to this function or the current tool becomes inactive.
    pub fn set_tool_info_text(&mut self, info: &str) {
        // TODO
    }
}


pub trait Tool {
    /// Called
    fn pick(&mut self, ctx: &mut ToolCtx, position: DVec2);
    fn stylus_pressed(&mut self, ctx: &mut ToolCtx, position: DVec2);
    fn stylus_moved(&mut self, ctx: &mut ToolCtx, position: DVec2);
    fn stylus_released(&mut self, ctx: &mut ToolCtx, position: DVec2);
    fn commit_gesture(&mut self, ctx: &mut ToolCtx);
    fn cancel(&mut self, ctx: &mut ToolCtx);
}

// CameraTool
// StrokeTool
// Selectool