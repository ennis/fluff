//! Windows compositor implementation details
use crate::app_globals::app_backend;
use crate::compositor::{ColorType, LayerID};
use crate::platform::ApplicationBackend;
use crate::Size;
use kurbo::Affine;
use raw_window_handle::RawWindowHandle;
use skia_safe as sk;
use skia_safe::gpu::FlushInfo;
use skia_safe::surface::BackendSurfaceAccess;
use skia_safe::ColorSpace;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use tracing::trace;
use tracy_client::span;
use windows::core::{Interface, Owned};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{D2D_MATRIX_4X4_F, D2D_MATRIX_4X4_F_0, D2D_MATRIX_4X4_F_0_0};
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::Win32::Graphics::DirectComposition::{IDCompositionTarget, IDCompositionVisual3};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGISwapChain3, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::Threading::WaitForSingleObject;

////////////////////////////////////////////////////////////////////////////////////////////////////

/*
/// Compositor layer implementation details.
pub struct LayerInner {
    pub(crate) visual: IDCompositionVisual3,
    size: Cell<Size>,
    swap_chain: IDXGISwapChain3,
    frame_latency_waitable: Owned<HANDLE>,
    window_target: RefCell<Option<IDCompositionTarget>>,
    #[cfg(feature = "vulkan-interop")]
    vulkan_interop: Option<vulkan_interop::VulkanInterop>,
}

/// Compositor layer.
pub type Layer = Rc<LayerInner>;

impl Drop for LayerInner {
    fn drop(&mut self) {
        // Wait for the GPU to finish using the swap chain buffers.
        // This can have a non-negligible performance impact, so users shouldn't create and
        // destroy layers on every frame. The documentation of the public compositor
        // API should mention this.
        app_backend().wait_for_gpu();
    }
}

impl LayerInner {
    pub(crate) fn new(size: Size, format: ColorType) -> Rc<LayerInner> {
        app_backend().create_layer(size, format)
    }

    /// Returns the size of the surface layer.
    pub fn size(&self) -> Size {
        self.size.get()
    }

    /// Resizes a surface layer.
    pub fn resize(&self, size: Size) {
        let app = app_backend();
        // skip if same size
        if self.size.get() == size {
            return;
        }

        let width = size.width as u32;
        let height = size.height as u32;
        // avoid resizing to zero width
        if width == 0 || height == 0 {
            return;
        }

        self.size.set(size);

        // Wait for the GPU to finish using the previous swap chain buffers.
        app.wait_for_gpu();
        // Skia may still hold references to swap chain buffers which would prevent
        // ResizeBuffers from succeeding. This cleans them up.
        app.direct_context.borrow_mut().flush_submit_and_sync_cpu();

        trace!("resizing swap chain buffers to {}x{}", width, height);

        unsafe {
            // SAFETY: basic FFI call
            match self.swap_chain.ResizeBuffers(
                SWAP_CHAIN_BUFFER_COUNT,
                width,
                height,
                SWAP_CHAIN_FORMAT,
                DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
            ) {
                Ok(_) => {}
                Err(hr) => {
                    //let removed_reason = self.device.GetDeviceRemovedReason().unwrap_err();
                    panic!("IDXGISwapChain::ResizeBuffers failed: {}", hr);
                }
            }
        }
    }

    /// Waits for the specified surface to be ready for presentation.
    ///
    /// TODO explain
    pub(crate) fn wait_for_presentation(&self) {
        let _span = span!("wait_for_surface");

        // if the swapchain has a mechanism for syncing with presentation, use it,
        // otherwise do nothing.

        //let t = std::time::Instant::now();
        if !self.frame_latency_waitable.is_invalid() {
            unsafe {
                WaitForSingleObject(*self.frame_latency_waitable, 1000);
            }
        }
        //trace!("wait_for_presentation took {:?}", t.elapsed());
    }

    /// Creates a skia drawing context for the specified surface layer.
    pub(crate) fn acquire_drawing_surface(&self) -> DrawableSurface {
        unsafe {
            // acquire next image from swap chain
            let index = self.swap_chain.GetCurrentBackBufferIndex();
            let swap_chain_buffer = self
                .swap_chain
                .GetBuffer::<ID3D12Resource>(index)
                .expect("failed to retrieve swap chain buffer");

            let surface = create_surface_for_texture(
                swap_chain_buffer,
                SWAP_CHAIN_FORMAT,
                self.size.get(),
                sk::gpu::SurfaceOrigin::TopLeft,
                SKIA_COLOR_TYPE,
                ColorSpace::new_srgb(),
                Some(sk::SurfaceProps::new(
                    sk::SurfacePropsFlags::default(),
                    sk::PixelGeometry::RGBH,
                )),
            );
            DrawableSurface {
                surface,
                swap_chain: self.swap_chain.clone(),
            }
        }
    }

    pub fn add_child(&self, child: &Layer) {
        unsafe {
            self.visual.AddVisual(&child.visual, true, None).unwrap();
        }
    }

    pub fn remove_child(&self, child: &Layer) {
        unsafe {
            self.visual.RemoveVisual(&child.visual).unwrap();
        }
    }

    /// Returns the underlying D3D swap chain associated with the layer.
    pub fn swap_chain(&self) -> &IDXGISwapChain3 {
        &self.swap_chain
    }

    /// Binds a composition layer to a window.
    ///
    /// # Safety
    ///
    /// The window handle is valid.
    ///
    /// TODO: return result
    pub(crate) unsafe fn bind_to_window(&self, window: RawWindowHandle) {
        trace!("binding layer to window");
        let win32_handle = match window {
            RawWindowHandle::Win32(w) => w,
            _ => panic!("expected a Win32 window handle"),
        };
        let window_target = app_backend()
            .composition_device
            .CreateTargetForHwnd(HWND(win32_handle.hwnd.get() as *mut c_void), false)
            .expect("CreateTargetForHwnd failed");
        window_target.SetRoot(&self.visual).expect("SetRoot failed");
        self.window_target.replace(Some(window_target));
    }

    /// Acquires the next image in the swap chain, and returns a vulkan image handle to it.
    ///
    /// When finished you should drop the image and call `present_vulkan` to present the image.
    ///
    /// The layer must have been created with `create_vulkan_interop_layer`.
    #[cfg(feature = "vulkan-interop")]
    pub fn acquire_vulkan_image(&self, cmd: &mut graal::CommandStream) -> &graal::Image {
        self.vulkan_interop
            .as_ref()
            .unwrap()
            .acquire_image(cmd, &self.swap_chain)
    }

    #[cfg(feature = "vulkan-interop")]
    pub fn present_vulkan(&self, cmd: &mut graal::CommandStream) {
        self.vulkan_interop.as_ref().unwrap().present(cmd, &self.swap_chain);
    }
}*/

////////////////////////////////////////////////////////////////////////////////////////////////////
// Compositor impl
////////////////////////////////////////////////////////////////////////////////////////////////////

impl ApplicationBackend {
    /*fn create_layer_inner(&self, size: Size, format: ColorType) -> LayerInner {
        let width = size.width as u32;
        let height = size.height as u32;

        assert!(width != 0 && height != 0, "surface layer cannot be zero-sized");

        // SAFETY: FFI calls
        unsafe {
            let swap_chain = self
                .dxgi_factory
                .CreateSwapChainForComposition(
                    &*self.command_queue,
                    &DXGI_SWAP_CHAIN_DESC1 {
                        Width: width,
                        Height: height,
                        Format: SWAP_CHAIN_FORMAT,
                        Stereo: false.into(),
                        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                        BufferCount: SWAP_CHAIN_BUFFER_COUNT,
                        Scaling: DXGI_SCALING_STRETCH,
                        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                        Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
                    },
                    None,
                )
                .expect("CreateSwapChainForComposition failed")
                .cast::<IDXGISwapChain3>()
                .unwrap();

            swap_chain.SetMaximumFrameLatency(1).unwrap();

            let frame_latency_waitable = swap_chain.GetFrameLatencyWaitableObject();
            assert!(
                !frame_latency_waitable.is_invalid(),
                "GetFrameLatencyWaitableObject returned an invalid handle"
            );
            // SAFETY: handle is valid
            let frame_latency_waitable = Owned::new(frame_latency_waitable);

            let visual = self.composition_device.CreateVisual().unwrap();
            visual.SetContent(&swap_chain).unwrap();

            LayerInner {
                visual: visual.cast().unwrap(),
                size: Cell::new(size),
                swap_chain,
                // SAFETY: handle is valid
                frame_latency_waitable,
                window_target: RefCell::new(None),
                #[cfg(feature = "vulkan-interop")]
                vulkan_interop: None,
            }
        }
    }

    /// Creates a surface layer with Vulkan interop.
    #[cfg(feature = "vulkan-interop")]
    pub fn create_vulkan_interop_layer(&self, device: graal::RcDevice, size: Size, format: ColorType) -> Layer {
        let mut layer = self.create_layer_inner(size, format);
        layer.vulkan_interop = Some(vulkan_interop::VulkanInterop::new(
            device,
            layer.swap_chain(),
            size,
            format,
        ));
        Rc::new(layer)
    }*/
}
