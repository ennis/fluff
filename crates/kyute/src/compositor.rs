//! System compositor interface
use crate::drawing::{FromSkia, ToSkia};
use crate::element::ElementCtx;
use crate::platform::DrawSurface;
use crate::{app_backend, platform};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use skia_safe as sk;
use slotmap::{new_key_type, Key};
use std::ops::Range;
use tracing::trace;
////////////////////////////////////////////////////////////////////////////////////////////////////

new_key_type! {
    /// Uniquely identifies a compositor layer in the application.
    ///
    /// The ID is unique across all layers in the application.
    pub struct LayerID;
}

/// Pixel format of a drawable surface.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Hash)]
pub enum ColorType {
    Alpha8,
    RGBA8888,
    BGRA8888,
    RGBA1010102,
    BGRA1010102,
    RGB101010x,
    BGR101010x,
    BGR101010xXR,
    Gray8,
    RGBAF16,
    RGBAF32,
    A16Float,
    A16UNorm,
    R16G16B16A16UNorm,
    SRGBA8888,
    R8UNorm,
}

impl ColorType {
    pub fn to_skia_color_type(&self) -> sk::ColorType {
        match *self {
            //ColorType::Unknown => sk::ColorType::Unknown,
            ColorType::Alpha8 => sk::ColorType::Alpha8,
            //ColorType::RGB565 => sk::ColorType::RGB565,
            //ColorType::ARGB4444 => sk::ColorType::ARGB4444,
            ColorType::RGBA8888 => sk::ColorType::RGBA8888,
            //ColorType::RGB888x => sk::ColorType::RGB888x,
            ColorType::BGRA8888 => sk::ColorType::BGRA8888,
            ColorType::RGBA1010102 => sk::ColorType::RGBA1010102,
            ColorType::BGRA1010102 => sk::ColorType::BGRA1010102,
            ColorType::RGB101010x => sk::ColorType::RGB101010x,
            ColorType::BGR101010x => sk::ColorType::BGR101010x,
            ColorType::BGR101010xXR => sk::ColorType::BGR101010xXR,
            ColorType::Gray8 => sk::ColorType::Gray8,
            //ColorType::RGBAF16Norm => sk::ColorType::RGBAF16Norm,
            ColorType::RGBAF16 => sk::ColorType::RGBAF16,
            ColorType::RGBAF32 => sk::ColorType::RGBAF32,
            //ColorType::R8G8UNorm => sk::ColorType::R8G8UNorm,
            ColorType::A16Float => sk::ColorType::A16Float,
            //ColorType::R16G16Float => sk::ColorType::R16G16Float,
            ColorType::A16UNorm => sk::ColorType::A16UNorm,
            //ColorType::R16G16UNorm => sk::ColorType::R16G16UNorm,
            ColorType::R16G16B16A16UNorm => sk::ColorType::R16G16B16A16UNorm,
            ColorType::SRGBA8888 => sk::ColorType::SRGBA8888,
            ColorType::R8UNorm => sk::ColorType::R8UNorm,
        }
    }
}

struct PictureLayer {
    picture: sk::Drawable,
    /// Bounds of the picture, in window coordinates.
    bounds: Rect,
    /// Physical bounds of the layer, in physical window coordinates (actual pixels).
    // FIXME: there should be a SizeI for this stuff
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    /// Draw surface that holds the rasterized picture.
    surface: Option<platform::DrawSurface>,
}

impl PictureLayer {
    fn width(&self) -> u32 {
        self.x1 - self.x0
    }

    fn height(&self) -> u32 {
        self.y1 - self.y0
    }
}

/*
struct PlatformSwapChainLayer {
    swap_chain: SwapChain,
    /// Position of the layer in window coordinates.
    pos: Point,
    layer: Option<platform::SwapChainLayer>,
}*/

enum Layer {
    Picture(PictureLayer),
    //SwapChain(PlatformSwapChainLayer),
    Group,
}

struct LayerInfo {
    /// TODO: unused, remove
    transform: Affine,
    parent: Option<LayerID>,
    rev: usize,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum StackOp {
    /// Enter layer group
    Enter(LayerID),
    /// Exit layer group
    Exit,
    /// Layer reference
    Layer(LayerID),
}

pub struct Composition {
    scale_factor: f64,
    layers: slotmap::SlotMap<LayerID, Layer>,
    infos: slotmap::SecondaryMap<LayerID, LayerInfo>,
    stack: Vec<StackOp>,
    rev: usize,
}

impl Default for Composition {
    fn default() -> Self {
        Composition {
            scale_factor: 0.0,
            layers: slotmap::SlotMap::with_key(),
            infos: slotmap::SecondaryMap::new(),
            stack: vec![],
            rev: 0,
        }
    }
}

impl Composition {
    pub fn render_to_window(&self, window: &platform::Window) {
        // attach surfaces to composition layers
        for (id, layer) in self.layers.iter() {
            match layer {
                Layer::Picture(pic) => {
                    assert!(pic.surface.is_some());
                    window.attach_draw_surface(id, &pic.surface.as_ref().unwrap());
                }
                // TODO: external swap chains
                _ => {}
            }
        }

        let mut cc = window.begin_composition();
        for op in self.stack.iter() {
            match *op {
                StackOp::Enter(_) => {}
                StackOp::Exit => {}
                StackOp::Layer(layer) => match &self.layers[layer] {
                    Layer::Picture(pic) => {
                        cc.add_layer(
                            layer,
                            Affine::translate(Vec2 {
                                x: pic.x0 as f64,
                                y: pic.y0 as f64,
                            }),
                        );
                    }
                    Layer::Group => {
                        unreachable!()
                    }
                },
            }
        }
        // end composition and commit
        drop(cc);
    }
}

/// Records compositor commands.
///
/// # Notes
///
/// This doesn't keep a transform stack, and doesn't care about the scale factor.
/// This should be handled by the caller.
///
/// All drawing commands should be in window coordinates.
/// The resulting layers' size and position is determined from the bounds of the picture they contain,
/// except for swap chain layers which are positioned explicitly by the caller.
pub struct CompositionBuilder {
    comp: Composition,
    picture_recorder: Option<sk::PictureRecorder>,
    bounds: Rect,
    /// Current position in the stack.
    sp: usize,
}

impl CompositionBuilder {

    /// Creates a new CompositionBuilder, optionally reusing parts of a previous composition.
    ///
    /// TODO: add a way to only recompose a subtree of the previous composition
    pub fn new(scale_factor: f64, init_bounds: Rect, previous: Option<Composition>) -> CompositionBuilder {
        let mut ctx = CompositionBuilder {
            comp: previous.unwrap_or_default(),
            picture_recorder: None,
            bounds: init_bounds,
            sp: 0,
        };
        ctx.comp.scale_factor = scale_factor;
        ctx.comp.rev += 1;
        ctx
    }

    /// Returns the scale factor of the composition.
    pub fn scale_factor(&self) -> f64 {
        self.comp.scale_factor
    }

    /// Sets the current drawing bounds in window coordinates.
    pub fn set_bounds(&mut self, bounds: Rect) {
        // This will affect the bounds of the next created picture recorder.
        self.bounds = bounds;
        // TODO: If the bounds are larger than the current picture recorder bounds,
        //       finish the current picture recorder and start a new one with the larger bounds.
        //       It's up to the caller to avoid calling set_bounds in a way that would cause
        //       the picture recorder to be recreated too often.
    }


    fn insert_layer(&mut self, layer: Layer) -> LayerID {
        let id = self.comp.layers.insert(layer);
        self.comp.infos.insert(
            id,
            LayerInfo {
                transform: Default::default(),
                parent: None,
                rev: self.comp.rev,
            },
        );
        id
    }

    fn group_range(&self, group: LayerID) -> Option<Range<usize>> {
        let pos = self.sp
            + self.comp.stack[self.sp..]
                .iter()
                .position(|op| op == &StackOp::Enter(group))?;
        let len = self.group_len(pos);
        Some(pos..(pos + len))
    }

    fn group_len(&self, sp: usize) -> usize {
        let mut depth = 0;
        let mut cur = sp + 1; // skip first op which should be StackOp::Enter

        loop {
            if cur >= self.comp.stack.len() {
                break;
            }
            match self.comp.stack[cur] {
                StackOp::Enter(_) => depth += 1,
                StackOp::Exit => {
                    if depth == 0 {
                        break;
                    } else {
                        depth -= 1;
                    }
                }
                _ => {}
            }
            cur += 1;
        }

        cur - sp
    }

    fn find_next_at_matching_depth(&self, op: StackOp) -> Option<usize> {
        let mut depth = 0;
        let mut cur = self.sp;
        loop {
            if cur >= self.comp.stack.len() {
                break;
            }
            match self.comp.stack[cur] {
                StackOp::Enter(_) => depth += 1,
                ref x if x == &op && depth == 0 => return Some(cur),
                StackOp::Exit => {
                    if depth == 0 {
                        break;
                    } else {
                        depth -= 1;
                    }
                }
                _ => {}
            }
            cur += 1;
        }
        None
    }

    /// Returns the current picture recorder.
    pub fn picture_recorder(&mut self) -> &mut sk::PictureRecorder {
        self.picture_recorder.get_or_insert_with(|| {
            eprintln!("new picture recorder: {:?}", self.bounds);
            let mut rec = sk::PictureRecorder::new();
            rec.begin_recording(self.bounds.to_skia(), None);
            rec
        })
    }

    /// Returns the current canvas.
    pub fn canvas(&mut self) -> &sk::Canvas {
        self.picture_recorder().recording_canvas().unwrap()
    }

    /// Enters a layer group.
    pub fn enter_group(&mut self, prev: Option<LayerID>) -> LayerID {
        self.finish_record_and_push_picture_layer();
        if let Some(prev) = prev {
            let g = self.group_range(prev).unwrap();
            let sp = self.sp;
            self.comp.stack[sp..g.end].rotate_left(g.start - sp);
            debug_assert_eq!(self.comp.stack[sp], StackOp::Enter(prev));
            self.sp += 1;
            prev
        } else {
            let group = self.insert_layer(Layer::Group);
            self.comp
                .stack
                .splice(self.sp..self.sp, [StackOp::Enter(group), StackOp::Exit]);
            self.sp += 1;
            group
        }
    }

    /// Exits the last entered layer group.
    pub fn exit_group(&mut self) {
        // All layers from the current stack position to the next matching `Exit` op
        // are stale layers from the previous frame not reused in this frame.
        // Remove them.
        let next_exit = self
            .find_next_at_matching_depth(StackOp::Exit)
            .expect("unbalanced group");
        self.comp.stack.splice(self.sp..next_exit - 1, []);
        debug_assert_eq!(self.comp.stack[self.sp], StackOp::Exit);
        self.sp += 1;
    }

    /// Reuses a layer group.
    pub fn reuse_group(&mut self, group: LayerID) {
        // find the group in the layer stack
        let g = self.group_range(group).unwrap();
        let sp = self.sp;
        // move it at the current position in the stack
        self.comp.stack[sp..g.end].rotate_left(g.start - sp);
        debug_assert_eq!(self.comp.stack[sp], StackOp::Enter(group));
        // mark layers inside the group as used for this revision
        for op in &self.comp.stack[sp..(sp + g.len())] {
            let layer = match op {
                StackOp::Enter(layer) => layer,
                StackOp::Layer(layer) => layer,
                _ => continue,
            };
            self.comp.infos[*layer].rev = self.comp.rev;
        }
        self.sp += g.len();
    }

    /*pub fn add_swap_chain_layer(&mut self, window_pos: Point, swap_chain: &platform::SwapChain) {
        self.finish_record_and_push_picture_layer();

        // cannibalize the next layer if it's a swap chain layer
        if let StackOp::Layer(layer) = self.comp.stack[self.sp] {
            if let Layer::SwapChain(ref mut scl) = self.comp.layers[layer] {
                if scl.swap_chain != *swap_chain {
                    scl.swap_chain = swap_chain.clone();
                    scl.layer = None;
                }
                scl.pos = window_pos;
                self.sp += 1;
                return;
            }
        }

        // otherwise insert a new layer
        let layer = self.insert_layer(Layer::SwapChain(PlatformSwapChainLayer {
            swap_chain: swap_chain.clone(),
            pos: window_pos,
            layer: None,
        }));
        self.comp.stack.insert(self.sp, StackOp::Layer(layer));
        self.sp += 1;
    }*/

    /// Finishes recording draw commands and pushes a picture layer on the stack.
    fn finish_record_and_push_picture_layer(&mut self) {
        if let Some(mut rec) = self.picture_recorder.take() {
            let mut drawable = rec.finish_recording_as_drawable().unwrap();
            let bounds = Rect::from_skia(drawable.bounds());
            let rounded_bounds = enclosing_integer_rect(bounds);

            // Cannibalize the picture layer from the last frame if there's one at the current stack position.
            if let Some(StackOp::Layer(layer)) = self.comp.stack.get(self.sp) {
                if let Layer::Picture(ref mut pic) = self.comp.layers[*layer] {
                    self.comp.infos[*layer].rev = self.comp.rev;
                    pic.picture = drawable;
                    pic.bounds = bounds;
                    pic.x0 = rounded_bounds.x0 as u32;
                    pic.y0 = rounded_bounds.y0 as u32;
                    pic.x1 = rounded_bounds.x1 as u32;
                    pic.y1 = rounded_bounds.y1 as u32;
                    self.sp += 1;
                    return;
                }
            }


            // Otherwise, insert a new picture layer.
            let layer = PictureLayer {
                picture: drawable,
                bounds,
                x0: rounded_bounds.x0 as u32,
                y0: rounded_bounds.y0 as u32,
                x1: rounded_bounds.x1 as u32,
                y1: rounded_bounds.y1 as u32,
                surface: None,
            };
            let layer = self.insert_layer(Layer::Picture(layer));
            self.comp.stack.insert(self.sp, StackOp::Layer(layer));
            self.sp += 1;
        }
    }

    /// Finishes the composition.
    pub fn finish(mut self) -> Composition {
        self.finish_record_and_push_picture_layer();

        // First, remove stale layers that were not reused from the previous frame, by comparing
        // their revision number.
        self.comp
            .layers
            .retain(|layer, _| self.comp.infos[layer].rev == self.comp.rev);

        // Rasterize all picture layers to platform compositor layers.
        for op in &self.comp.stack {
            match op {
                StackOp::Layer(layer) => {
                    match self.comp.layers[*layer] {
                        Layer::Picture(ref mut pic) => {
                            let mut realloc = pic.surface.is_none();
                            if let Some(ref surface) = pic.surface {
                                // Check if the size is still the same, otherwise reallocate the surface.
                                if surface.width() != pic.width() || surface.height() != pic.height() {
                                    realloc = true;
                                }
                            }

                            // allocate a new draw surface
                            if realloc {
                                trace!("create new platform layer: {:?}", pic.bounds);
                                let layer = DrawSurface::new(pic.width(), pic.height(), ColorType::RGBA8888);
                                pic.surface = Some(layer);
                            }

                            // rasterize the picture
                            let mut draw_context = pic.surface.as_mut().unwrap().begin_draw();
                            draw_context.canvas().draw_drawable(&mut pic.picture, None);
                        }
                        Layer::Group => {
                            todo!()
                        }
                    }
                }
                _ => continue,
            };
        }

        self.comp
    }
}

/// Returns the nearest integer rectangle enclosing the given rectangle.
fn enclosing_integer_rect(rect: Rect) -> Rect {
    Rect {
        x0: rect.x0.floor(),
        y0: rect.y0.floor(),
        x1: rect.x1.ceil(),
        y1: rect.y1.ceil(),
    }
}

// Q: is it OK to have layers owned by windows?
// A: the question to ask is whether layers can be shared between windows. The answer is no: layers (Visuals in DirectComposition)
//    are associated with a single layer tree, and in turn with a single window. The **content** of layers can be shared
//    between layers though.
//    We still want to associate layers with LayerIDs so that they can be reused across frames.
//
// If layers can't be shared (but should be retained), but surfaces can, then we need two concepts in the backend:
// - LayerID for layers (per window)
// - SurfaceID for surfaces (global)

trait SystemCompositor {
    // create a drawing surface -> LayerID
    // create a surface backed by an external swap chain (IDXGISwapChain, etc.) -> LayerID

    // begin compositing a target window `begin_composition(target: CompositionTargetID)`
    // end compositing

    // OR:
    // PlatformWindow::begin_composition() -> CompositionBuilder

    // OR:
    // One layer tree per window

    // OR: every layer associated to a window
    // * PlatformWindow::attach_drawing_surface(layerID, surface, size): creates a drawing surface and associates it with the layer ID
    // * PlatformWindow::create_swap_chain_layer(layerID, swap_chain): creates a swap chain surface and associates it with the layer ID
    // * PlatformWindow::destroy_layer(layerID): destroys the layer and releases resources
}

// Final design:
// - two base concepts: Layers & Surfaces
// - the platform provides `DrawingSurface`s on which you can draw with skia
// - there's an API on platform windows to attach a DrawingSurface to a LayerID (this creates a visual if it doesn't exist)
// - also on platform windows, there's a way to specify an ordered list of LayerIDs to be composited on the window (begin_composition, push_layer(layer_id, transform), end_composition)
// - as a platform-specific extension, it's possible to attach a native swap chain to a LayerID (attach_swap_chain)
//      - this takes a IDXGISwapChain directly
// - for convenience, kyute will provide a `SwapChainVkInterop` type on windows which can be used with vulkan, and as a content source for a LayerID
