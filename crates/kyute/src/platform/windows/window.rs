use std::cell::RefCell;
use crate::application::with_event_loop_window_target;
use crate::compositor::LayerID;
use crate::platform::windows::draw_surface::DrawSurface;
use crate::{app_backend, WindowOptions};
use kurbo::Affine;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slotmap::SecondaryMap;
use std::ffi::c_void;
use std::ops::Deref;
use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{D2D_MATRIX_4X4_F, D2D_MATRIX_4X4_F_0, D2D_MATRIX_4X4_F_0_0};
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionTarget, IDCompositionVisual, IDCompositionVisual2, IDCompositionVisual3,
};
use winit::platform::windows::WindowBuilderExtWindows;

/// Win32 window.
///
/// This is a thin wrapper around a winit window that also holds state necessary for
/// DirectComposition.
///
pub struct Window {
    pub(crate) inner: winit::window::Window,
    /// DComp window target bound to this window.
    pub(crate) composition_target: IDCompositionTarget,
    /// Root visual.
    root_visual: IDCompositionVisual2,
    /// Visuals for each layer.
    layer_map: RefCell<SecondaryMap<LayerID, IDCompositionVisual3>>,
}

impl Window {
    /// Creates a new window with the specified options.
    pub fn new(options: &WindowOptions) -> Window {
        let inner = with_event_loop_window_target(|event_loop| {
            let mut builder = winit::window::WindowBuilder::new()
                .with_title(options.title)
                // no_redirection_bitmap is OK since we're using DirectComposition for all rendering
                .with_no_redirection_bitmap(true)
                .with_decorations(options.decorations)
                .with_visible(options.visible)
                .with_inner_size(winit::dpi::LogicalSize::new(options.size.width, options.size.height));

            if options.no_focus {
                builder = builder.with_no_focus();
                builder = builder.with_active(false);
            }
            if let Some(p) = options.position {
                builder = builder.with_position(winit::dpi::LogicalPosition::new(p.x, p.y));
            }

            if let Some(parent) = options.parent {
                match parent {
                    RawWindowHandle::Win32(w) => {
                        builder = builder.with_owner_window(w.hwnd.get());
                    }
                    _ => unreachable!(),
                }
            }

            builder.build(&event_loop).unwrap()
        });

        let hwnd = match inner.window_handle().unwrap().as_raw() {
            RawWindowHandle::Win32(handle) => handle.hwnd.get() as *mut c_void,
            _ => unreachable!(),
        };

        unsafe {
            // Create a DirectComposition target for the window.
            // SAFETY: the HWND handle is valid
            let composition_target = app_backend()
                .composition_device
                .CreateTargetForHwnd(HWND(hwnd), false)
                .unwrap();

            // Create the root visual and attach it to the composition target.
            // SAFETY: FFI call
            let root_visual = app_backend().composition_device.CreateVisual().unwrap();

            // SAFETY: FFI call
            composition_target.SetRoot(&root_visual).unwrap();

            Window {
                inner,
                composition_target,
                root_visual,
                layer_map: Default::default(),
            }
        }
    }
}

impl Deref for Window {
    type Target = winit::window::Window;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Window {
    /// Creates a layer with the specified ID and attaches a `DrawSurface` to it.
    ///
    /// The layer displays the contents of the `DrawSurface`.
    pub fn attach_draw_surface(&self, layer_id: LayerID, surface: &DrawSurface) {
        let mut layer_map = self.layer_map.borrow_mut();
        if let Some(visual) = layer_map.get(layer_id) {
            eprintln!("Window::attach_draw_surface (layer_id: {:?}) -- update", layer_id);
            unsafe {           
                // update existing visual with new content
                visual.SetContent(&surface.swap_chain).unwrap();
            }
        } else {
            unsafe {
                eprintln!("Window::attach_draw_surface (layer_id: {:?}) -- new visual", layer_id);
                // create a new visual
                let visual = app_backend().composition_device.CreateVisual().unwrap();
                // IDCompositionVisual3 is available from Windows 8.1 onwards
                let visual = visual.cast::<IDCompositionVisual3>().unwrap();
                visual.SetContent(&surface.swap_chain).unwrap();
                layer_map.insert(layer_id, visual);
            }
        }
    }

    /// Deletes resources associated with the specified layer.
    pub fn release_layer(&self, layer_id: LayerID) {
        // this should release the associated visual once it's not used anymore
        // by the composition tree
        self.layer_map.borrow_mut().remove(layer_id);
    }

    pub fn begin_composition(&self) -> CompositionContext {
        unsafe {
            // Remove all previous layers from the root visual.
            // The stack is rebuilt from scratch on every call to begin_composition.
            self.root_visual.RemoveAllVisuals().unwrap();
        }
        CompositionContext::new(self)
    }

    fn end_composition(&self) {
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
pub struct CompositionContext<'a> {
    window: &'a Window,
    last: Option<IDCompositionVisual>,
}

impl<'a> CompositionContext<'a> {
    fn new(window: &'a Window) -> Self {
        CompositionContext { window, last: None }
    }

    /// Adds a layer to the composition tree.
    ///
    /// # Panics
    ///
    /// If no layer with the specified ID exists for the window.
    pub fn add_layer(&mut self, layer_id: LayerID, transform: Affine) {
        eprintln!("CompositionContext::add_layer (layer_id: {:?})", layer_id);
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

impl<'a> Drop for CompositionContext<'a> {
    fn drop(&mut self) {
        self.window.end_composition();
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
