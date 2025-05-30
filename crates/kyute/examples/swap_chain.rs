//! Illustrates how to embed a custom DXGI swap chain in the element tree.
use graal::ClearColorValue;
use kurbo::{Line, Point, Size};
use kyute::compositor::ColorType;
use kyute::element::{HitTestCtx, Measurement, TreeCtx};
use kyute::elements::Frame;
use kyute::layout::{LayoutInput};
use kyute::platform::windows::{DxgiVulkanInteropImage, DxgiVulkanInteropSwapChain};
use kyute::{application, Element, PaintCtx, Window};
use kyute_common::Color;
use tokio::select;
use kyute::platform::WindowOptions;

struct CustomSwapChainElement {
    device: graal::RcDevice,
    /// The swap chain with vulkan interop. This is provided by kyute as a convenience.
    swap_chain: Option<DxgiVulkanInteropSwapChain>,
}

impl CustomSwapChainElement {
    fn new(device: graal::RcDevice) -> Self {
        Self {
            device,
            swap_chain: None,
        }
    }
}

impl Element for CustomSwapChainElement {
    fn measure(&mut self, tree: &TreeCtx, layout_input: &LayoutInput) -> Measurement {
        // use the available space
        Size::new(
            layout_input.width.available().unwrap_or_default(),
            layout_input.height.available().unwrap_or_default(),
        ).into()
    }

    fn layout(&mut self, tree: &TreeCtx, size: Size) {
        
        
        // convert to real pixels
        // FIXME: get the scale factor. It's not available currently in the layout context.
        
        // TODO: We can't convert to real pixels during layout since we don't know the scale factor.
        //       At the moment, the scale factor (DPI) is only known during painting. 
        //       Either perform layout while knowing the target DPI, or move the creation of the
        //       swap chain to the paint method.
        let width = size.width as u32;
        let height = size.height as u32;

        if width != 0 && height != 0 {
            // create the swap chain
            let swap_chain = DxgiVulkanInteropSwapChain::new(
                self.device.clone(),
                ColorType::SRGBA8888,
                width,
                height,
                graal::ImageUsage::TRANSFER_DST | graal::ImageUsage::TRANSFER_SRC,
            );
            self.swap_chain = Some(swap_chain);
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        // this is the default implementation
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        // by the time we get here, `layout` has been called and the swap chain has been created
        // so we can unwrap safely
        let swap_chain = self.swap_chain.as_ref().unwrap();

        // acquire an image to draw into
        let DxgiVulkanInteropImage {
            image,
            ready,
            rendering_finished,
        } = swap_chain.acquire();

        // draw something to the swap chain
        // (we just clear the image with a color, but you can do something more interesting)
        let mut cmd = self.device.create_command_stream();
        let (r, g, b, a) = Color::from_hex("FFE20E").to_rgba();
        cmd.clear_image(&image, ClearColorValue::Float([r, g, b, a]));

        // Submit the commands.
        //
        // It is **essential** to synchronize with the presentation engine by using the two semaphores
        // returned by `acquire`:
        // - before executing the commands, synchronize with the presentation by waiting on `ready`
        // - after executing the commands, signal `rendering_finished` to indicate that we are done
        //   rendering to the image.
        //
        // If we don't do this, `swap_chain.present()` will deadlock.
        cmd.flush(&[ready], &[rendering_finished]).unwrap();

        // Present the image. Internally, this is synchronized with our rendering via the
        // `rendering_finished` semaphore.
        swap_chain.present();

        // Add the swap chain to the composition tree.
        // This will display the swap chain on the screen.
        ctx.add_swap_chain(ctx.bounds.origin(), swap_chain.dxgi_swap_chain.clone());
        
        // We can still draw things on top of the swap chain if we want to.
        // This will be put on a separate compositor layer, above the swap chain in Z-order.
        // Draw a white crosshair in the center of the window.
        let center = ctx.bounds.center();
        let size = 100.0;
        ctx.draw_line(
            Line::new((center.x - size, center.y), (center.x + size, center.y)),
            2.0,
            Color::from_rgb_u8(255, 255, 255),
        );
        ctx.draw_line(
            Line::new((center.x, center.y - size), (center.x, center.y + size)),
            2.0,
            Color::from_rgb_u8(255, 255, 255),
        );

        // Request a repaint to redraw the window continuously.
        // In a real application you would only request a repaint when the content changes,
        // but here we do that to check for memory leaks.
        ctx.tree.mark_needs_paint();

        // we should call `device::cleanup` periodically to free resources (on every frame)
        self.device.cleanup();
    }
}

fn main() {
    // create the vulkan device and command stream
    let device = graal::create_device().unwrap();

    application::run(async move {
        // Embed the custom swap chain element in a frame
        let root = Frame::new()
            .background_color(Color::from_hex("413e13"))
            .padding(50.0)
            .content(CustomSwapChainElement::new(device));

        let main_window = Window::new(
            &WindowOptions {
                title: "System Compositor Example",
                size: Some(Size::new(800.0, 600.0)),
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
