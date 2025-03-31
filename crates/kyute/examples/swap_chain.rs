//! Illustrates how to embed a custom DXGI swap chain in the element tree.
use kurbo::{Point, Size};
use kyute::elements::Frame;
use kyute::{application, Element, PaintCtx, Window, WindowOptions};
use kyute_common::Color;
use tokio::select;
use kyute::compositor::ColorType;
use kyute::element::{HitTestCtx, TreeCtx};
use kyute::layout::{LayoutInput, LayoutOutput};
use kyute::platform::DxgiVulkanInteropSwapChain;

struct CustomSwapChainElement {
    device: graal::RcDevice,
    /// The swap chain with vulkan interop. This is provided by kyute as a convenience.
    swap_chain: Option<DxgiVulkanInteropSwapChain>,
}

impl CustomSwapChainElement {
    fn new(device: graal::RcDevice) -> Self {
        Self { device, swap_chain: None }
    }
}

impl Element for CustomSwapChainElement {
    fn measure(&mut self, tree: &TreeCtx, layout_input: &LayoutInput) -> Size {
        // use the available space
        Size::new(layout_input.width.available().unwrap_or_default(), layout_input.height.available().unwrap_or_default())
    }

    fn layout(&mut self, tree: &TreeCtx, size: Size) -> LayoutOutput {
        // convert to real pixels
        // FIXME: get the scale factor. It's not available currently in the layout context.
        let width = size.width as u32;
        let height = size.height as u32;

        if width != 0 && height != 0 {
            // create the swap chain
            let swap_chain = DxgiVulkanInteropSwapChain::new(self.device.clone(), ColorType::SRGBA8888, width, height, graal::ImageUsage::TRANSFER_DST | graal::ImageUsage::TRANSFER_SRC);
            self.swap_chain = Some(swap_chain);
        }

        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        todo!()
    }

    fn paint(&mut self, tctx: &TreeCtx, ctx: &mut PaintCtx) {
        // draw something to the swap chain
        // FIXME: we need access to a CommandStream here, the only way to do that for now is for the
        //        element to own it. This makes it difficult to have multiple elements doing vulkan
        //        rendering at the same time (e.g. multiple windows).
        //        There should be a way to retrieve the "current" command stream. Possibly via
        //        scoped TLS or something.

        // TODO: we should be able to build multiple command streams concurrently (not necessarily in parallel though).
        //       CommandStreams as they are now are too error prone because they are expected to
        //       be submitted in the same order as they are created (batches are assigned a sequence number
        //       *on creation*, not after they are submitted).
        // Proposal: allow creating multiple command streams from a device, but enforce the correct
        //           submission order (check that the sequence number is the expected one before submission).
    }
}

fn main() {

    // create the vulkan device and command stream
    let (device, cmd) = graal::create_device_and_command_stream().unwrap();

    application::run(async {
        let root = Frame::new().background_color(Color::from_hex("413e13"));

        let main_window = Window::new(
            &WindowOptions {
                title: "System Compositor Example",
                size: Size::new(800.0, 600.0),
                background: Color::from_hex("413e13"),
                ..Default::default()
            },
            root,
        );

        loop {
            select! {
                _ = main_window.close_requested() => {
                    application::quit();
                    break
                }
            }
        }
    })
    .unwrap()
}
