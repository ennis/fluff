//! An element that manages a compositor layer.

use crate::element::{ElementCtx, HitTestCtx, TreeCtx, WeakElement};
use crate::layout::{LayoutInput, LayoutOutput};
use crate::{compositor, AppGlobals, Element, PaintCtx};
use kurbo::{Point, Size};
use crate::compositor::ColorType;

/// An element that draws itself into a separate compositor layer, independent of the parent window.
pub struct Layer {
    /// Allocated on first layout.
    layer: Option<compositor::Layer>,
    /// Whether we added the layer to the parent compositor layer
    attached: bool,
}

impl Layer {

    pub fn new() -> Self {

        Self {
        }
    }

    /// Returns a reference to the compositor layer.
    pub fn layer(&self) -> &compositor::Layer {
        &self.layer
    }
}

impl Element for Layer {
    fn added(&mut self, _tree: &mut TreeCtx, _ectx: &ElementCtx) {
        // nothing, the layer is created and attached on first layout
    }

    fn removed(&mut self, tree: &mut TreeCtx, ectx: &ElementCtx) {
        if let Some(layer) = &self.layer {
            layer.detach();
        }
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        // we always take as much space as we can get
        Size::new(
            layout_input.width.available().unwrap_or(500.0),
            layout_input.height.available().unwrap_or(500.0),
        )
    }



    fn layout(&mut self, ctx: &mut TreeCtx, size: Size) -> LayoutOutput {
        if size.width != 0.0 && size.height != 0.0 {
            if let Some(layer) = &mut self.layer {
                layer.resize(size);
            } else {
                // TODO ColorType
                self.layer = Some(compositor::Layer::new_surface(size, ColorType::BGRA8888));
            }
        }
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        // TODO
        false
    }

    fn paint(&mut self, ectx: &ElementCtx, ctx: &mut PaintCtx) {
        // attach or re-attach the layer to the parent compositor layer
        if !self.attached {
            ectx.compositor.add_layer(&self.layer);
            self.attached = true;
        }
    }
}