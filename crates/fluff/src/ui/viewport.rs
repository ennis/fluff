//! 3D viewport widget

use crate::data::viewport::ViewportModel;
use crate::gpu;
use kyute::compositor::ColorType;
use kyute::element::{HitTestCtx, TreeCtx};
use kyute::layout::{LayoutInput, LayoutOutput};
use kyute::{app_backend, Element, PaintCtx, Point, Size};

const DEFAULT_SIZE: Size = Size::new(500., 500.);

/// The 3D viewport.
///
/// It listens to changes in a ViewportModel and renders the scene.
/// It updates the camera in the ViewportModel when the user interacts with the viewport.
///
/// # Lifecycle
///
/// - The Viewport is created with a ViewportModel.
/// - During painting, create a new layer with the correct size, or resize the existing layer if necessary
///    - Paint the scene into the layer
///    - Add the layer to the compositing tree in `PaintCtx`
pub struct Viewport {
    // Swap chain for the 3D rendering
    //swap_chain: kyute::SwapChain,
    data: ViewportModel,
}

// Layer::set_parent()
//   - if self.parent == parent { return }
//   - parent.backend.add_child(

impl Viewport {
    pub fn new(data: ViewportModel) -> Self {
        // dummy size until we get the actual size from the layout
        /*let swap_chain = kyute::SwapChain::new(DEFAULT_SIZE, ColorType::SRGBA8888);
        Self {
            swap_chain,
            data,
        }*/
        todo!()
    }
}

impl Element for Viewport {
    fn measure(&mut self, _tree: &TreeCtx, layout_input: &LayoutInput) -> Size {
        // take all the available space
        Size::new(
            layout_input.width.available().unwrap_or(DEFAULT_SIZE.width),
            layout_input.height.available().unwrap_or(DEFAULT_SIZE.height),
        )
    }

    fn layout(&mut self, tree: &TreeCtx, size: Size) -> LayoutOutput {
        //self.layer.resize(size);
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
