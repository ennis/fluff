use crate::app_backend;
use crate::compositor::ColorType;
use crate::platform::windows::format_to_dxgi_format;
use crate::platform::windows::swap_chain::create_composition_swap_chain;
use skia_safe::gpu::d3d::TextureResourceInfo;
use skia_safe::gpu::{FlushInfo, Protected};
use skia_safe::surface::BackendSurfaceAccess;
use skia_safe::{ColorSpace, SurfaceProps};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Resource, D3D12_RESOURCE_STATE_COMMON,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM,
};
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain3, DXGI_PRESENT};

/// Represents a surface that can be drawn on with a skia canvas.
///
/// # Performance notes
///
/// Creating and destroying `DrawSurface` objects is relatively expensive, as it involves creating
/// swap chains and waiting for the GPU to finish before destroying them.
/// You should avoid creating and destroying `DrawSurface` objects on every frame: try to
/// reuse them as much as possible.
///
pub struct DrawSurface {
    pub(crate) swap_chain: IDXGISwapChain3,
    //frame_latency_waitable: Owned<HANDLE>,
    dxgi_format: DXGI_FORMAT,
    width: u32,
    height: u32,
}

impl Drop for DrawSurface {
    fn drop(&mut self) {
        eprintln!("DrawSurface::drop (width: {}, height: {})", self.width, self.height);
        // Wait for the GPU to finish using the swap chain buffers.
        // This can have a non-negligible performance impact, so users shouldn't create and
        // destroy layers on every frame. The documentation of the public compositor
        // API should mention this.
        app_backend().wait_for_gpu();
    }
}

impl DrawSurface {
    /// Creates a new drawing surface with the specified size and color format.
    ///
    /// # Panics
    ///
    /// Panics if width or height are zero.
    pub fn new(width: u32, height: u32, format: ColorType) -> Self {
        eprintln!(
            "DrawSurface::new (width: {}, height: {}, format: {:?})",
            width, height, format
        );
        let dxgi_format = format_to_dxgi_format(format);
        let swap_chain = create_composition_swap_chain(dxgi_format, width, height);
        /*let frame_latency_waitable = unsafe {
            let handle = swap_chain.GetFrameLatencyWaitableObject();
            //assert!(!handle.is_invalid());
            Owned::new(handle)
        };*/
        let surface = DrawSurface {
            swap_chain,
            dxgi_format,
            //frame_latency_waitable,
            width,
            height,
        };
        // Initial wait for presentation as suggested by the docs.
        //surface.wait_for_presentation();
        surface
    }

    /*fn wait_for_presentation(&self) {
        //let t = std::time::Instant::now();
        if !self.frame_latency_waitable.is_invalid() {
            unsafe {
                WaitForSingleObject(*self.frame_latency_waitable, 1000);
            }
        }
        //trace!("wait_for_presentation took {:?}", t.elapsed());
    }*/

    /// Returns the width of the surface, in physical pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height of the surface, in physical pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Begin drawing on the surface.
    ///
    /// # Return value
    ///
    /// A `DrawCtx` object that can be used to draw on the surface. When it is dropped, the surface
    /// is presented to the screen.
    pub fn begin_draw(&mut self) -> DrawSurfaceContext {
        // Wrap the current back buffer for use by Skia.
        let skia_surface = unsafe {
            let index = self.swap_chain.GetCurrentBackBufferIndex();
            let buffer = self
                .swap_chain
                .GetBuffer::<ID3D12Resource>(index)
                .expect("failed to retrieve swap chain buffer");
            create_skia_surface_for_d3d12_texture(
                buffer,
                self.dxgi_format,
                self.width,
                self.height,
                // Assume sRGB for now
                // TODO don't assume sRGB
                ColorSpace::new_srgb(),
                Some(skia_safe::SurfaceProps::new(
                    skia_safe::SurfacePropsFlags::default(),
                    // TODO query the display's pixel geometry somehow
                    skia_safe::PixelGeometry::RGBH,
                )),
            )
        };

        DrawSurfaceContext {
            draw_surface: self,
            skia_surface,
        }
    }

    /// Finishes drawing on the surface.
    ///
    /// This is called by `DrawSurfaceContext` when it is dropped, and should not be called
    /// by the user.
    fn end_draw(&mut self, skia_surface: &mut skia_safe::Surface) {
        let app = app_backend();
        let mut direct_context = app.direct_context.borrow_mut();

        {
            // "Flush" the Skia surface, whatever that means.
            direct_context.flush_surface_with_access(
                skia_surface,
                BackendSurfaceAccess::Present,
                &FlushInfo::default(),
            );
            // I don't know exactly what this does, but I assume that it submits command lists
            // for the draw to the GPU command queue.
            direct_context.submit(None);
        }

        unsafe {
            // Present the back buffer to the screen.
            // `SyncInterval = 1` for flip model swap chains means wait for the next vertical blanking interval
            // before presenting the back buffer.
            //
            // Note that since there's no back buffer available until the next blanking interval,
            // (DrawSurface swap chains have only two buffers), this call is likely to block until
            // the next blanking interval.

            //let t = std::time::Instant::now();
            
            // SyncInterval = 0 
            self.swap_chain.Present(0, DXGI_PRESENT::default()).unwrap();
            //if r == DXGI_ERROR_WAS_STILL_DRAWING {
            //    eprintln!("DXGI_ERROR_WAS_STILL_DRAWING");
            //}
            //eprintln!("Present took {:?}", t.elapsed());
        }

        // Mark the end of a frame for Tracy.
        // I'm not sure that this works well if there are multiple surfaces that present at different
        // times and/or rates.
        if let Some(client) = tracy_client::Client::running() {
            client.frame_mark();
        }


        // this is not the right way to do frame pacing when there are multiple swap chains,
        // because each swap chain in the compositor tree will wait for the compositor
        // (thus, if there are N layers, the window will take N compositor frames to update!).
        // The correct way to do this is to use the compositor's frame pacing mechanism.
        // (https://learn.microsoft.com/en-us/windows/win32/directcomp/compositor-clock/compositor-clock)
        //self.wait_for_presentation();
    }
}

/// Context for drawing on a surface.
pub struct DrawSurfaceContext<'a> {
    draw_surface: &'a mut DrawSurface,
    skia_surface: skia_safe::Surface,
}

impl<'a> DrawSurfaceContext<'a> {
    /// Returns a skia canvas that can be used to draw on the surface.
    pub fn canvas(&mut self) -> &skia_safe::Canvas {
        self.skia_surface.canvas()
    }
}

impl<'a> Drop for DrawSurfaceContext<'a> {
    fn drop(&mut self) {
        self.draw_surface.end_draw(&mut self.skia_surface);
    }
}

/// Creates a skia surface object backed by the specified D3D12 texture resource.
///
/// # Safety
///
/// The following preconditions must be met:
///
/// - `format`, `width`, and `height` should have the values specified when the texture was created
/// - the texture should not be multi-sampled
/// - the texture should have a single mip level
/// - the texture should be in the `D3D12_RESOURCE_STATE_COMMON` state
///
unsafe fn create_skia_surface_for_d3d12_texture(
    texture_resource: ID3D12Resource,
    format: DXGI_FORMAT,
    width: u32,
    height: u32,
    color_space: ColorSpace,
    surface_props: Option<SurfaceProps>,
) -> skia_safe::Surface {
    let mut direct_context = app_backend().direct_context.borrow_mut();

    let texture_resource_info = TextureResourceInfo {
        resource: texture_resource,
        alloc: None,
        resource_state: D3D12_RESOURCE_STATE_COMMON,
        format,
        sample_count: 1,
        level_count: 1,
        sample_quality_pattern: 0,
        protected: Protected::No,
    };

    let color_type = match format {
        DXGI_FORMAT_R8G8B8A8_UNORM => skia_safe::ColorType::RGBA8888,
        DXGI_FORMAT_B8G8R8A8_UNORM => skia_safe::ColorType::BGRA8888,
        DXGI_FORMAT_R16G16B16A16_FLOAT => skia_safe::ColorType::RGBAF16,
        _ => panic!("unsupported DXGI format"),
    };

    let backend_render_target =
        skia_safe::gpu::BackendRenderTarget::new_d3d((width as i32, height as i32), &texture_resource_info);
    let skia_surface = skia_safe::gpu::surfaces::wrap_backend_render_target(
        &mut *direct_context,
        &backend_render_target,
        skia_safe::gpu::SurfaceOrigin::TopLeft,
        color_type,
        color_space,
        surface_props.as_ref(),
    )
    .expect("skia surface creation failed");
    skia_surface
}
