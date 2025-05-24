use crate::app_backend;
use crate::compositor::LayerID;
use crate::platform::windows::draw_surface::DrawSurface;
use crate::platform::windows::event_loop::with_event_loop_window_target;
use crate::platform::{WindowHandler, WindowKind, WindowOptions};
use kurbo::{Affine, Point, Rect, Size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slotmap::SecondaryMap;
use std::cell::{Ref, RefCell};
use std::ffi::c_void;
use std::fmt;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::rc::{Rc, Weak};
use tracing::warn;
use windows::core::Interface;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_MATRIX_4X4_F, D2D_MATRIX_4X4_F_0, D2D_MATRIX_4X4_F_0_0};
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionTarget, IDCompositionVisual, IDCompositionVisual2, IDCompositionVisual3,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain3;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoA, HMONITOR, MONITORINFO};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
use windows::Win32::UI::WindowsAndMessaging::EnumThreadWindows;
use winit::monitor::MonitorHandle;
use winit::platform::windows::{MonitorHandleExtWindows, WindowBuilderExtWindows};
use winit::window::{WindowButtons, WindowId};

// Some bullshit to get the HWND from winit
fn get_hwnd(handle: RawWindowHandle) -> HWND {
    match handle {
        RawWindowHandle::Win32(win32) => HWND(win32.hwnd.get() as *mut c_void),
        _ => unreachable!("only win32 windows are supported"),
    }
}

/// Win32 window.
///
/// This is a thin wrapper around a winit window that also holds state necessary for
/// DirectComposition.
#[derive(Clone)]
pub struct PlatformWindowHandle {
    state: Weak<WindowState>,
}

impl fmt::Debug for PlatformWindowHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(_) = self.state.upgrade() {
            write!(f, "HWND@{:08x}", self.hwnd().0 as isize)
        } else {
            write!(f, "(dropped window)")
        }
    }
}

impl PlatformWindowHandle {
    /// Returns the HWND of the window.
    pub fn hwnd(&self) -> HWND {
        match self.state.upgrade() {
            Some(state) => get_hwnd(state.inner.window_handle().unwrap().as_raw()),
            None => {
                panic!("Window has been dropped");
            }
        }
    }

    /// Enables or disables inputs to this window.
    ///
    /// Internally this calls `EnableWindow` on the HWND of the window.
    pub fn enable_input(&self, enabled: bool) {
        unsafe {
            let _ = EnableWindow(self.hwnd(), enabled);
        }
    }

    /// Closes the window.
    pub fn close(&self) {
        if let Some(state) = self.state.upgrade() {
            // remove the window from the global list
            ALL_WINDOWS.with_borrow_mut(|windows| {
                windows.retain(|w| !Rc::ptr_eq(w, &state));
            });
            // Here the last reference should drop, and the window will be closed.
            //
            // If this is called as a result of a window event,
            // the window will be closed when exiting from `handle_window_event`.
        }
    }
}

#[allow(dead_code)]
pub(super) struct WindowState {
    pub(crate) inner: winit::window::Window,
    /// DComp window target bound to this window.
    pub(crate) composition_target: IDCompositionTarget,
    /// Root visual.
    root_visual: IDCompositionVisual2,
    /// Visuals for each layer.
    layer_map: RefCell<SecondaryMap<LayerID, IDCompositionVisual3>>,

    /// If this is a modal window, the handles of disabled windows that should be re-enabled
    /// when this modal window is closed.
    modal_disabled_windows: Vec<PlatformWindowHandle>,

    /// Window event handler.
    handler: RefCell<Option<Box<dyn WindowHandler>>>,
}

impl Drop for WindowState {
    fn drop(&mut self) {
        // re-enable all disabled windows
        for handle in &self.modal_disabled_windows {
            handle.enable_input(true);
        }
    }
}

thread_local! {
    /// All windows in the main thread.
    ///
    /// When a platform window is created, it is added to this list.
    static ALL_WINDOWS: RefCell<Vec<Rc<WindowState>>> = RefCell::new(Vec::new());
}

pub(super) fn find_window_by_id(id: WindowId) -> Option<Rc<WindowState>> {
    ALL_WINDOWS.with(|windows| windows.borrow().iter().find(|w| w.inner.id() == id).cloned())
}

//--------------------------------------------------------------------------------------------------

/// Represents a monitor that shows a window.
#[derive(Debug, Clone)]
pub struct Monitor(MonitorHandle);

impl Monitor {
    /// Returns the size in device-independent pixels of the monitor.
    pub fn logical_size(&self) -> Size {
        let size = self.0.size().to_logical(self.0.scale_factor());
        Size::new(size.width, size.height)
    }

    /// Returns the work area of the monitor, i.e. the area available for windows, excluding taskbars and other system UI.
    pub fn work_area(&self) -> Rect {
        let hmonitor = self.0.hmonitor();
        let hmonitor = HMONITOR(hmonitor as *mut c_void);
        unsafe {
            let mut monitor_info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                rcMonitor: Default::default(),
                rcWork: Default::default(),
                dwFlags: 0,
            };
            GetMonitorInfoA(hmonitor, &mut monitor_info).unwrap();
            let sf = self.0.scale_factor();
            Rect {
                x0: monitor_info.rcWork.left as f64 / sf,
                y0: monitor_info.rcWork.top as f64 / sf,
                x1: monitor_info.rcWork.right as f64 / sf,
                y1: monitor_info.rcWork.bottom as f64 / sf,
            }
        }
    }
}

//--------------------------------------------------------------------------------------------------

struct CreateWindowResult {
    window: winit::window::Window,
    composition_target: IDCompositionTarget,
    root_visual: IDCompositionVisual2,
    modal_disabled_windows: Vec<PlatformWindowHandle>,
}

fn create_window(options: &WindowOptions) -> CreateWindowResult {

    let size = options.size.unwrap_or(Size::new(800.0, 600.0));

    let modal;
    let owner;
    let no_focus;

    match options.kind {
        WindowKind::Application => {
            modal = false;
            owner = None;
            no_focus = false;
        }
        WindowKind::Menu(ref o) => {
            modal = false;
            owner = o.clone();
            no_focus = true;
        }
        WindowKind::Modal(ref o) => {
            modal = true;
            owner = o.clone();
            no_focus = false;
        }
        WindowKind::Tooltip => {
            modal = false;
            owner = None;
            no_focus = true;
        }
    }

    // position the window
    let mut position = None;
    if let Some(p) = options.position {
        // If a position is explicitly set, use it.
        position = Some(p);
    } else if options.center {
        match owner {
            Some(ref owner) => {
                let ref_rect = owner.bounds();

                let mut pos = Point::new(
                    ref_rect.x0 + (ref_rect.width() - size.width) / 2.0,
                    ref_rect.y0 + (ref_rect.height() - size.height) / 2.0,
                );

                // When centering, ensure the window is within the work area of the monitor.
                // If there's not enough space, the window will be positioned in the top-left corner
                // of the work area but will be clipped.
                let monitor = owner.monitor();
                let work_area = monitor.work_area();
                if pos.x + size.width > work_area.x1 {
                    pos.x = work_area.x1 - size.width;
                }
                if pos.x < work_area.x0 {
                    pos.x = work_area.x0;
                }
                if pos.y + size.height > work_area.y1 {
                    pos.y = work_area.y1 - size.height;
                }
                if pos.y < work_area.y0 {
                    pos.y = work_area.y0;
                }

                position = Some(pos);
            }
            None => {
                // TODO: center relative to the primary monitor
            }
        }
    };

    // if creating a modal window, disable all other windows
    let modal_disabled_windows = if modal {
        // If the owner is set, disable only the owner window.
        // Otherwise, disable all other windows, and re-enable them when exiting the modal.
        if let Some(ref owner) = owner {
            owner.enable_input(false);
            vec![owner.clone()]
        } else {
            todo!("modal windows without an owner are not yet supported");
        }
    } else {
        vec![]
    };
    eprintln!("modal_disabled_windows: {:?}", modal_disabled_windows);

    let mut enabled_buttons = WindowButtons::CLOSE;
    if !modal {
        enabled_buttons |= WindowButtons::MINIMIZE;
        if options.resizable {
            enabled_buttons |= WindowButtons::MAXIMIZE;
        }
    }

    // Create the window.
    let mut builder = winit::window::WindowBuilder::new()
        .with_title(options.title)
        // no_redirection_bitmap is OK since we're using DirectComposition for all rendering
        .with_no_redirection_bitmap(true)
        .with_decorations(options.decorations)
        .with_visible(options.visible)
        .with_enabled_buttons(enabled_buttons)
        .with_resizable(options.resizable)
        .with_inner_size(winit::dpi::LogicalSize::new(size.width, size.height));

    if no_focus {
        builder = builder.with_no_focus();
        builder = builder.with_active(false);
    }
    if let Some(p) = position {
        builder = builder.with_position(winit::dpi::LogicalPosition::new(p.x, p.y));
    }
    if let Some(ref owner) = owner {
        builder = builder.with_owner_window(owner.hwnd().0 as isize);
    }

    // create the winit window
    let window_inner = with_event_loop_window_target(|event_loop| builder.build(&event_loop).unwrap());

    // Create a DirectComposition target for the window.
    // SAFETY: the HWND handle is valid
    let composition_target = unsafe {
        let hwnd = get_hwnd(window_inner.window_handle().unwrap().as_raw());
        app_backend()
            .composition_device
            .CreateTargetForHwnd(hwnd, false)
            .unwrap()
    };

    // Create the root visual and attach it to the composition target.
    // SAFETY: FFI call
    let root_visual = unsafe { app_backend().composition_device.CreateVisual().unwrap() };

    // SAFETY: FFI call
    unsafe { composition_target.SetRoot(&root_visual).unwrap() };

    CreateWindowResult {
        window: window_inner,
        composition_target,
        root_visual,
        modal_disabled_windows,
    }
}

impl PlatformWindowHandle {
    /// Creates a new window.
    pub fn new(options: &WindowOptions) -> PlatformWindowHandle {
        // TODO check that we're not creating a window outside the main thread
        let CreateWindowResult {
            window: window_inner,
            composition_target,
            root_visual,
            modal_disabled_windows,
        } = create_window(options);

        let state = Rc::new(WindowState {
            inner: window_inner,
            composition_target,
            root_visual,
            layer_map: Default::default(),
            modal_disabled_windows,
            handler: RefCell::new(None),
        });

        // add to the global list
        ALL_WINDOWS.with_borrow_mut(|windows| {
            windows.push(state.clone());
        });

        PlatformWindowHandle {
            state: Rc::downgrade(&state),
        }
    }

    /// Sets the handler of the window.
    pub fn set_handler(&self, handler: Box<dyn WindowHandler>) {
        let state = self.state();
        *state.handler.borrow_mut() = Some(handler);
    }

    fn state(&self) -> Rc<WindowState> {
        self.state.upgrade().expect("window has been dropped")
    }

    /// Returns the monitor on which the window is currently displayed.
    pub fn monitor(&self) -> Monitor {
        Monitor(self.state().inner.current_monitor().unwrap())
    }

    /// Returns the unique identifier for this window.
    pub fn id(&self) -> WindowId {
        self.state().inner.id()
    }

    /// Returns the current scale factor of the window, i.e. the ratio of the window's
    /// pixel size to its logical size.
    ///
    /// For example, a scale factor of 2.0 means that 1 logical pixel corresponds to 2 physical pixels.
    pub fn scale_factor(&self) -> f64 {
        self.state().inner.scale_factor()
    }

    /// Requests a redraw of the window.
    pub fn request_redraw(&self) {
        self.state().inner.request_redraw();
    }

    /// Returns the logical size of the client area (the window without its decorations) of this window.
    pub fn client_area_size(&self) -> Size {
        let size = self.state().inner.inner_size().to_logical(self.scale_factor());
        Size::new(size.width, size.height)
    }

    /// Returns the logical coordinates of the window client area on the screen.
    pub fn client_area_position(&self) -> Point {
        let pos = self.state().inner.inner_position().unwrap();
        let pos = pos.to_logical(self.scale_factor());
        Point::new(pos.x, pos.y)
    }

    /// Returns the window bounds (client area and decorations) in logical coordinates.
    pub fn bounds(&self) -> Rect {
        let size = self.state().inner.outer_size().to_logical::<f64>(self.scale_factor());
        let pos = self.state().inner.outer_position().unwrap().to_logical::<f64>(self.scale_factor());
        Rect::new(pos.x, pos.y, pos.x + size.width, pos.y + size.height)
    }

    /// Returns whether the window is still open.
    pub fn is_open(&self) -> bool {
        self.state.upgrade().is_some()
    }

    /// Shows or hides the window.
    pub fn set_visible(&self, visible: bool) {
        let state = self.state();
        state.inner.set_visible(visible);
    }

    /// Returns whether the window is currently visible.
    pub fn is_visible(&self) -> bool {
        self.state().inner.is_visible().unwrap_or(true)
    }

    /// Creates a layer with the specified ID and attaches a `DrawSurface` to it.
    ///
    /// The layer displays the contents of the `DrawSurface`.
    pub fn attach_draw_surface(&self, layer_id: LayerID, surface: &DrawSurface) {
        let visual = self.state().get_or_create_dcomp_visual(layer_id);
        unsafe {
            visual.SetContent(&surface.swap_chain).unwrap();
        }
    }

    /// Attaches a swap chain to the specified layer.
    pub fn attach_swap_chain(&self, layer_id: LayerID, swap_chain: IDXGISwapChain3) {
        let visual = self.state().get_or_create_dcomp_visual(layer_id);
        unsafe {
            visual.SetContent(&swap_chain).unwrap();
        }
    }

    /// Deletes resources associated with the specified layer.
    pub fn release_layer(&self, layer_id: LayerID) {
        // this should release the associated visual once it's not used anymore
        // by the composition tree
        self.state().layer_map.borrow_mut().remove(layer_id);
    }

    /// Starts building a new composition tree for the window.
    ///
    /// It's reasonable to call this function once per frame.
    pub fn begin_composition(&self) -> CompositionContext {
        let state = self.state();
        unsafe {
            // Remove all previous layers from the root visual.
            // The stack is rebuilt from scratch on every call to begin_composition.
            state.root_visual.RemoveAllVisuals().unwrap();
        }
        CompositionContext::new(state)
    }
}

/*
impl Deref for Window {
    type Target = winit::window::Window;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}*/

impl WindowState {
    fn get_or_create_dcomp_visual(&self, layer_id: LayerID) -> IDCompositionVisual3 {
        let mut layer_map = self.layer_map.borrow_mut();
        if let Some(visual) = layer_map.get(layer_id) {
            //eprintln!("get_or_create_dcomp_visual (layer_id: {:?}) -- update", layer_id);
            visual.clone()
        } else {
            unsafe {
                //eprintln!("get_or_create_dcomp_visual (layer_id: {:?}) -- new visual", layer_id);
                let visual = app_backend().composition_device.CreateVisual().unwrap();
                let visual = visual.cast::<IDCompositionVisual3>().unwrap();
                layer_map.insert(layer_id, visual.clone());
                visual
            }
        }
    }

    fn commit_composition(&self) {
        // TODO: all layers not used in the composition should increment an "age" counter
        //       and be deleted if they are not used for a certain number of frames.
        unsafe {
            app_backend().composition_device.Commit().unwrap();
        }
    }
}

/// Context for building the composition tree of a window.
///
/// # Usage
///
/// Call `add_layer` for each layer that should be displayed in the window.
/// The Z-order of layers is derived from the order in which they are added.
///
/// Dropping the `CompositionContext` will commit the composition tree to the system compositor.
pub struct CompositionContext {
    window: Rc<WindowState>,
    last: Option<IDCompositionVisual>,
}

impl CompositionContext {
    fn new(window: Rc<WindowState>) -> Self {
        CompositionContext { window, last: None }
    }

    /// Adds a layer to the composition tree.
    ///
    /// # Panics
    ///
    /// If no layer with the specified ID exists for the window.
    pub fn add_layer(&mut self, layer_id: LayerID, transform: Affine) {
        //eprintln!("CompositionContext::add_layer (layer_id: {:?})", layer_id);
        let layer_map = self.window.layer_map.borrow();
        let transform = affine_to_d2d_matrix_4x4(&transform);
        let visual = layer_map.get(layer_id).unwrap();

        // SAFETY: basic FFI
        unsafe {
            visual.SetTransform2(&transform).unwrap();
            self.window
                .root_visual
                .AddVisual(visual, true, self.last.as_ref().clone())
                .unwrap();
        }
        self.last = Some(visual.cast().unwrap());
    }
}

impl Drop for CompositionContext {
    fn drop(&mut self) {
        self.window.commit_composition();
    }
}

/// Converts an `Affine` transform to a Direct2D matrix.
fn affine_to_d2d_matrix_4x4(affine: &Affine) -> D2D_MATRIX_4X4_F {
    let m = affine.as_coeffs();
    D2D_MATRIX_4X4_F {
        Anonymous: D2D_MATRIX_4X4_F_0 {
            Anonymous: D2D_MATRIX_4X4_F_0_0 {
                _11: m[0] as f32,
                _12: m[1] as f32,
                _13: 0.0,
                _14: 0.0,
                _21: m[2] as f32,
                _22: m[3] as f32,
                _23: 0.0,
                _24: 0.0,
                _31: 0.0,
                _32: 0.0,
                _33: 1.0,
                _34: 0.0,
                _41: m[4] as f32,
                _42: m[5] as f32,
                _43: 0.0,
                _44: 1.0,
            },
        },
    }
}

//--------------------------------------------------------------------------------------------------

/// Handles a window event.
pub(super) fn handle_window_event(id: WindowId, event: winit::event::WindowEvent) {
    let window = find_window_by_id(id);
    if let Some(window) = window {
        if let Some(handler) = window.handler.borrow().as_ref() {
            handler.event(&event);
        }
    } else {
        warn!("Window event for unknown window: {:?}", event);
    }
}

/// Requests a redraw of all windows.
///
/// This is called internally as a result of a composition clock tick.
pub(super) fn redraw_windows() {
    ALL_WINDOWS.with(|windows| {
        for window in windows.borrow().iter() {
            // This will loop back into the event loop, but that's fine.
            window.inner.request_redraw();
        }
    });
}
