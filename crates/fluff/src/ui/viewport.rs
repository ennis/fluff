//! 3D viewport widget
use crate::data::viewport::ViewportEvent;
use crate::gpu;
use kyute::compositor::ColorType;
use kyute::element::{ElementBuilder, HitTestCtx, Measurement, TreeCtx};
use kyute::layout::{LayoutInput, LayoutOutput};
use kyute::platform::windows::{DxgiVulkanInteropImage, DxgiVulkanInteropSwapChain};
use kyute::{Element, Event, PaintCtx, Point, Size};
use std::rc::Rc;

const DEFAULT_SIZE: Size = Size::new(500., 500.);

type ViewportModel = Rc<crate::data::viewport::Viewport>;

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
    data: ViewportModel,
    swap_chain: Option<DxgiVulkanInteropSwapChain>,
}

impl Viewport {
    pub fn new(data: ViewportModel) -> ElementBuilder<Self> {
        let this = ElementBuilder::new(Viewport { data: data.clone(), swap_chain: None });
        
        this.connect(&*data, |_this, cx, event: &ViewportEvent| {
            match event {
                ViewportEvent::CameraChangedInternal => {
                    // only repaint on external camera changes
                    cx.mark_needs_paint();
                }
                _ => {}
            }
        });
        
        this
    }
}

impl Element for Viewport {
    fn measure(&mut self, _tree: &TreeCtx, layout_input: &LayoutInput) -> Measurement {
        // take all the available space
        Size::new(
            layout_input.width.available().unwrap_or(DEFAULT_SIZE.width),
            layout_input.height.available().unwrap_or(DEFAULT_SIZE.height),
        ).into()
    }

    fn layout(&mut self, tree: &TreeCtx, size: Size)  {
        // Create the swap chain
        // TODO scale factor
        let width = size.width as u32;
        let height = size.height as u32;
        let swap_chain = DxgiVulkanInteropSwapChain::new(
            gpu::device().clone(),
            ColorType::SRGBA8888,
            width,
            height,
            graal::ImageUsage::TRANSFER_DST | graal::ImageUsage::TRANSFER_SRC,
        );
        self.swap_chain = Some(swap_chain);
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        // capture all events
        true
    }

    fn paint(&mut self, tctx: &TreeCtx, ctx: &mut PaintCtx) {

        let device = gpu::device();
        
        let swap_chain = self.swap_chain.as_ref().unwrap();
        let DxgiVulkanInteropImage {
            image,
            ready,
            rendering_finished,
        } = swap_chain.acquire();

        // Render the scene
        let mut cmd = device.create_command_stream();
        //let (r, g, b, a) = Color::from_hex("FFE20E").to_rgba();
        //cmd.clear_image(&image, ClearColorValue::Float([r, g, b, a]));
        
        self.data.render(&mut cmd, &image);
        
        cmd.flush(&[ready], &[rendering_finished]).unwrap();

        swap_chain.present();
        ctx.add_swap_chain(ctx.bounds.origin(), swap_chain.dxgi_swap_chain.clone());
        gpu::device().cleanup();
    }

    fn event(&mut self, ctx: &TreeCtx, event: &mut Event) {
        // handle events
    }
}
