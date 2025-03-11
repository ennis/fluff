//! 3D viewport widget

use kyute::{app_backend, Element, PaintCtx, Point, Size};
use kyute::compositor::ColorType;
use kyute::element::{HitTestCtx, TreeCtx};
use kyute::layout::{LayoutInput, LayoutOutput};
use crate::data::viewport::ViewportModel;

const DEFAULT_SIZE: Size = Size::new(500., 500.);

/// The 3D viewport.
///
/// It listens to changes in a ViewportModel and renders the scene.
/// It updates the camera in the ViewportModel when the user interacts with the viewport.
///
/// # Lifecycle
///
/// - The Viewport is created with a ViewportModel.
/// - When the layout of the window changes, the layer is resized
pub struct Viewport {
    layer: kyute::compositor::Layer,
    data: ViewportModel,
}

impl Viewport {
    pub fn new(data: ViewportModel) -> Self {
        Self {
            // dummy size until we get the actual size from the layout
            layer: app_backend().create_surface_layer(DEFAULT_SIZE, ColorType::SRGBA8888),
            data,
        }
    }
}

impl Element for Viewport {
    fn added(&mut self, ctx: &TreeCtx) {
        // insert the layer into the compositor tree
        if let Some(layer) = ctx.get_parent_layer() {
            layer.add_child(&self.layer);
        }
    }

    fn measure(&mut self, _tree: &TreeCtx, layout_input: &LayoutInput) -> Size {
        // take all the available space
        Size::new(layout_input.width.available().unwrap_or(DEFAULT_SIZE.width), layout_input.height.available().unwrap_or(DEFAULT_SIZE.height))
    }
    
    

    fn layout(&mut self, tree: &TreeCtx, size: Size) -> LayoutOutput {
        self.layer.resize(size);
        LayoutOutput::from(size)
    }


    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        true
    }

    fn paint(&mut self, tctx: &TreeCtx, ctx: &mut PaintCtx) {
        // Nothing to paint in the parent layer.
        // All rendering is done in `self.layer`.
    }
}