//! Windows compositor implementation details
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use crate::app_globals::app_backend;
use crate::backend::ApplicationBackend;
use crate::compositor::ColorType;
use crate::Size;
use graal::CommandStream;
use raw_window_handle::RawWindowHandle;
use skia_safe as sk;
use skia_safe::gpu::d3d::TextureResourceInfo;
use skia_safe::gpu::{DirectContext, FlushInfo, Protected};
use skia_safe::surface::BackendSurfaceAccess;
use skia_safe::{ColorSpace, SurfaceProps};
use tracing::trace;
use tracy_client::span;
use windows::core::{Interface, Owned};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Direct3D12::{ID3D12Resource, D3D12_RESOURCE_STATE_RENDER_TARGET};
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionDesktopDevice, IDCompositionTarget, IDCompositionVisual3,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISwapChain3, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::Threading::WaitForSingleObject;

////////////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(feature = "vulkan-interop")]
mod vulkan_interop {
    //! Utilities to share compositor swap chain buffers with a Vulkan device
    //! FIXME: it should probably be in kyute behind a feature flag

    use crate::app_backend;
    use crate::backend::windows::compositor::SWAP_CHAIN_BUFFER_COUNT;
    use crate::compositor::ColorType;
    use graal::platform::windows::DeviceExtWindows;
    use graal::{vk, SemaphoreWaitKind};
    use kurbo::Size;
    use std::cell::Cell;
    use windows::core::Interface;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
    use windows::Win32::Graphics::Direct3D12::{
        ID3D12Fence, ID3D12GraphicsCommandList, ID3D12Resource, D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_FENCE_FLAG_SHARED,
    };
    use windows::Win32::Graphics::Dxgi::{IDXGISwapChain3, DXGI_PRESENT};

    struct VulkanInteropImage {
        /// Shared handle to DXGI swap chain buffer.
        shared_handle: HANDLE,
        /// Imported DXGI swap chain buffer.
        image: graal::Image,
        /// Dummy command list for synchronization with vulkan.
        ///
        /// We need to push some commands to the D3D12 queue after acquiring a buffer from the swap chain and before signalling the DXGI/VK fence,
        /// to force some implicit synchronization with the presentation engine.
        ///
        /// Suggested by a user on the DirectX discord.
        ///
        /// Don't remove it, we get artifacts otherwise.
        discard_cmd_list: ID3D12GraphicsCommandList,
    }

    /// Vulkan interop objects for composition swap chains.
    pub struct VulkanInterop {
        device: graal::RcDevice,
        /// Imported vulkan images for the swap chain buffers.
        images: Vec<VulkanInteropImage>,
        /// Whether a swap chain surface has been acquired and not released yet.
        surface_acquired: Cell<bool>,

        // Fence state for synchronizing between D3D12 presentation and vulkan
        /// Vulkan side of the presentation fence
        fence_semaphore: vk::Semaphore,
        /// D3D12 side of the presentation fence
        fence: ID3D12Fence,
        /// Fence shared handle (imported to vulkan)
        fence_shared_handle: HANDLE,
        /// Presentation fence value
        fence_value: Cell<u64>,
    }

    impl VulkanInterop {
        pub(crate) fn new(
            device: graal::RcDevice,
            swap_chain: &IDXGISwapChain3,
            size: Size,
            _format: ColorType,
        ) -> VulkanInterop {
            let app = app_backend();
            let d3d12_device = &app.d3d12_device.0;
            let mut images = vec![];

            unsafe {
                app.command_allocator.get_ref().unwrap().Reset().unwrap();

                // --- wrap swap chain buffers as vulkan images ---
                for i in 0..SWAP_CHAIN_BUFFER_COUNT {
                    // obtain the ID3D12Resource of each swap chain buffer and create a shared handle for them
                    let swap_chain_buffer: ID3D12Resource =
                        swap_chain.GetBuffer::<ID3D12Resource>(i).expect("GetBuffer failed");
                    let shared_handle = d3d12_device
                        .CreateSharedHandle(&swap_chain_buffer, None, GENERIC_ALL.0, None)
                        .expect("CreateSharedHandle failed");

                    // create a vulkan image with memory imported from the shared handle
                    let imported_image = device.create_imported_image_win32(
                        &graal::ImageCreateInfo {
                            memory_location: graal::MemoryLocation::GpuOnly,
                            type_: graal::ImageType::Image2D,
                            usage: graal::ImageUsage::COLOR_ATTACHMENT
                                | graal::ImageUsage::TRANSFER_DST
                                | graal::ImageUsage::TRANSFER_SRC,
                            format: vk::Format::R8G8B8A8_SRGB, // FIXME use parameter
                            width: size.width as u32,
                            height: size.height as u32,
                            depth: 1,
                            mip_levels: 1,
                            array_layers: 1,
                            samples: 1,
                        },
                        Default::default(),
                        Default::default(),
                        vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE_KHR,
                        shared_handle.0 as vk::HANDLE,
                        None,
                    );

                    let discard_cmd_list: ID3D12GraphicsCommandList = d3d12_device
                        .CreateCommandList(
                            0,
                            D3D12_COMMAND_LIST_TYPE_DIRECT,
                            app.command_allocator.get_ref().unwrap(),
                            None,
                        )
                        .unwrap();
                    discard_cmd_list.DiscardResource(&swap_chain_buffer, None);
                    discard_cmd_list.Close().unwrap();

                    images.push(VulkanInteropImage {
                        shared_handle,
                        image: imported_image,
                        discard_cmd_list,
                    });
                }

                // Create & share a D3D12 fence for VK/DXGI sync
                let fence = d3d12_device.CreateFence(0, D3D12_FENCE_FLAG_SHARED).unwrap();
                let fence_shared_handle = d3d12_device
                    .CreateSharedHandle(&fence, None, GENERIC_ALL.0, None)
                    .unwrap();
                let fence_semaphore = device.create_imported_semaphore_win32(
                    vk::SemaphoreImportFlags::empty(),
                    vk::ExternalSemaphoreHandleTypeFlags::D3D12_FENCE,
                    fence_shared_handle.0,
                    None,
                );

                VulkanInterop {
                    images,
                    fence_value: Cell::new(0),
                    fence_semaphore,
                    fence,
                    fence_shared_handle,
                    surface_acquired: Cell::new(false),
                    device,
                }
            }
        }

        /// Acquires the next image in the swap chain, and returns a vulkan image handle to it.
        ///
        /// You must release the previous acquired image before calling this
        /// function. See `present_image_vk`.
        pub(crate) fn acquire_image(
            &self,
            cmd: &mut graal::CommandStream,
            swap_chain: &IDXGISwapChain3,
        ) -> &graal::Image {
            assert!(!self.surface_acquired.get(), "surface already acquired");

            let app = app_backend();

            let index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
            let image = &self.images[index as usize];

            let fence_value = self.fence_value.get();
            self.fence_value.set(fence_value + 1);

            // initial sync - D3D12 signal
            unsafe {
                // dummy rendering to synchronize with the presentation engine before signalling the fence
                // needed! there's some implicit synchronization being done here
                app.command_queue
                    .ExecuteCommandLists(&[Some(image.discard_cmd_list.cast().unwrap())]);
                app.command_queue.Signal(&self.fence, fence_value).unwrap();

                // DXGI-to-vulkan sync
                // TODO improve API legibility in graal: the flush does nothing but waiting for the fence
                //      maybe add a shorthand for this?
                cmd.flush(
                    &[graal::SemaphoreWait {
                        kind: SemaphoreWaitKind::D3D12Fence {
                            semaphore: self.fence_semaphore,
                            fence: Default::default(),
                            value: fence_value,
                        },
                        dst_stage: vk::PipelineStageFlags::ALL_COMMANDS,
                    }],
                    &[],
                )
                .expect("flush failed");
            }

            self.surface_acquired.set(true);
            &image.image
        }

        /// Submits the last acquired swap chain image for presentation.
        ///
        /// TODO: incremental present
        pub(crate) fn present(&self, cmd: &mut graal::CommandStream, swap_chain: &IDXGISwapChain3) {
            let app = app_backend();
            let fence_value = self.fence_value.get();
            self.fence_value.set(fence_value + 1);

            unsafe {
                // Vulkan-to-DXGI sync
                // TODO we signal the fence on the graphics queue, but work affecting the image might have been scheduled on another in case of async compute.
                cmd.flush(
                    &[],
                    &[graal::SemaphoreSignal::D3D12Fence {
                        semaphore: self.fence_semaphore,
                        fence: Default::default(),
                        value: fence_value,
                    }],
                )
                .expect("signal failed");

                app.command_queue.Wait(&self.fence, fence_value).unwrap();
                swap_chain
                    .Present(1, DXGI_PRESENT::default())
                    .ok()
                    .expect("Present failed");
            }

            self.surface_acquired.set(false);
        }
    }

    impl Drop for VulkanInterop {
        /// Waits for the 3D device to be idle and destroys the vulkan images previously created with `create_interop()`.
        fn drop(&mut self) {
            let app = app_backend();

            // Before releasing the buffers, we must make sure that the swap chain is not in use
            // We don't bother with setting up fences around the swap chain, we just wait for all commands to complete.
            // We could use fences to avoid unnecessary waiting, but not sure that it's worth the complication.
            app.wait_for_gpu();

            unsafe {
                self.device.raw().destroy_semaphore(self.fence_semaphore, None);
                CloseHandle(self.fence_shared_handle).unwrap();
                for img in self.images.iter() {
                    unsafe {
                        CloseHandle(img.shared_handle).unwrap();
                    }
                }
            }

            //let progress = app
            //    .drawing()
            //    .context
            //    .submit_frame(&mut (), frame, &graal::SubmitInfo::default());
            //app.gpu_device()
            //    .wait(&progress.progress, Duration::from_secs(1))
            //    .unwrap();
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

const SWAP_CHAIN_BUFFER_COUNT: u32 = 2;
const SWAP_CHAIN_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
const SKIA_COLOR_TYPE: skia_safe::ColorType = sk::ColorType::RGBA8888;

/// Windows drawable surface backend.
pub struct DrawableSurface {
    composition_device: IDCompositionDesktopDevice,
    context: DirectContext,
    swap_chain: IDXGISwapChain3,
    surface: sk::Surface,
}

thread_local! {
    static LAST_COMMIT_TIME: Cell<std::time::Instant> = Cell::new(std::time::Instant::now());
}

impl DrawableSurface {
    pub fn skia(&self) -> sk::Surface {
        self.surface.clone()
    }

    /// Returns the size of the surface in physical pixels.
    pub fn physical_size(&self) -> Size {
        let surface = self.skia();
        Size::new(surface.width() as f64, surface.height() as f64)
    }

    fn present(&mut self) {
        {
            let _span = span!("skia: flush_and_submit");
            self.context.flush_surface_with_access(
                &mut self.surface,
                BackendSurfaceAccess::Present,
                &FlushInfo::default(),
            );
            self.context.submit(None);
        }

        unsafe {
            let _span = span!("D3D12: present");
            //let t = std::time::Instant::now();
            //let dur = t.duration_since(LAST_COMMIT_TIME.get()).as_millis();
            //trace!("present + commit, {dur}ms");
            //LAST_COMMIT_TIME.set(t);
            self.swap_chain.Present(1, DXGI_PRESENT::default()).unwrap();
            self.composition_device.Commit().unwrap();
        }

        if let Some(client) = tracy_client::Client::running() {
            client.frame_mark();
        }
    }
}

impl Drop for DrawableSurface {
    fn drop(&mut self) {
        self.present();
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Compositor layer implementation details.
pub struct LayerInner {
    visual: IDCompositionVisual3,
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
        let app = app_backend();
        let swap_chain = &self.swap_chain;

        unsafe {
            // acquire next image from swap chain
            let index = swap_chain.GetCurrentBackBufferIndex();
            let swap_chain_buffer = swap_chain
                .GetBuffer::<ID3D12Resource>(index)
                .expect("failed to retrieve swap chain buffer");

            let surface = app.create_surface_for_texture(
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
                composition_device: app.composition_device.clone(),
                context: app.direct_context.borrow().clone(),
                surface,
                swap_chain: swap_chain.clone(),
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
    pub fn acquire_vulkan_image(&self, cmd: &mut CommandStream) -> &graal::Image {
        self.vulkan_interop
            .as_ref()
            .unwrap()
            .acquire_image(cmd, &self.swap_chain)
    }

    #[cfg(feature = "vulkan-interop")]
    pub fn present_vulkan(&self, cmd: &mut CommandStream) {
        self.vulkan_interop.as_ref().unwrap().present(cmd, &self.swap_chain);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Compositor impl
////////////////////////////////////////////////////////////////////////////////////////////////////

impl ApplicationBackend {
    /// Creates a surface backed by the specified D3D texture resource.
    ///
    /// # Safety
    ///
    /// The parameters must match the properties of the vulkan image:
    ///
    /// * `format`, `size` must be the same as specified during creation of the image
    /// * `color_type` must be compatible with `format`
    ///
    /// TODO: other preconditions
    unsafe fn create_surface_for_texture(
        &self,
        image: ID3D12Resource,
        format: DXGI_FORMAT,
        size: Size,
        surface_origin: sk::gpu::SurfaceOrigin,
        color_type: skia_safe::ColorType,
        color_space: ColorSpace,
        surface_props: Option<SurfaceProps>,
    ) -> sk::Surface {
        let texture_resource_info = TextureResourceInfo {
            resource: image,
            alloc: None,
            resource_state: D3D12_RESOURCE_STATE_RENDER_TARGET, // FIXME: either pass in parameters or document assumption
            format,
            sample_count: 1, // FIXME pass in parameters
            level_count: 1,  // FIXME pass in parameters
            sample_quality_pattern: 0,
            protected: Protected::No,
        };

        let backend_render_target =
            sk::gpu::BackendRenderTarget::new_d3d((size.width as i32, size.height as i32), &texture_resource_info);
        let direct_context = &mut *self.direct_context.borrow_mut();
        let sk_surface = skia_safe::gpu::surfaces::wrap_backend_render_target(
            direct_context,
            &backend_render_target,
            surface_origin,
            color_type,
            color_space,
            surface_props.as_ref(),
        )
        .expect("skia surface creation failed");
        sk_surface
    }

    fn create_layer_inner(&self, size: Size, format: ColorType) -> LayerInner {
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

    /// Creates a surface layer.
    ///
    /// FIXME: don't ignore format
    pub fn create_layer(&self, size: Size, _format: ColorType) -> Layer {
        Rc::new(self.create_layer_inner(size, _format))
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
    }
}
