//! DXGI swap chain wrapper
use crate::app_backend;
use kurbo::Size;
use std::rc::Rc;
use windows::core::{Interface, Owned};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISwapChain3, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

const SWAP_CHAIN_BUFFER_COUNT: u32 = 2;
const SWAP_CHAIN_FORMAT: DXGI_FORMAT = DXGI_FORMAT_R8G8B8A8_UNORM;
const SKIA_COLOR_TYPE: skia_safe::ColorType = skia_safe::ColorType::RGBA8888;

/// Represents a DXGI swap chain.
#[derive(Clone)]
pub struct SwapChain {
    pub(crate) inner: Rc<SwapChainInner>,
}

impl PartialEq for SwapChain {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SwapChain {}

struct SwapChainInner {
    swap_chain: IDXGISwapChain3,
    frame_latency_waitable: Owned<HANDLE>,
    #[cfg(feature = "vulkan-interop")]
    vulkan_interop: Option<vulkan_interop::VulkanInterop>,
}

impl SwapChainInner {
    fn new(size: Size) -> SwapChainInner {
        let app = app_backend();
        let width = size.width as u32;
        let height = size.height as u32;

        assert!(width != 0 && height != 0, "surface layer cannot be zero-sized");

        // SAFETY: FFI calls
        unsafe {
            let swap_chain = app
                .dxgi_factory
                .CreateSwapChainForComposition(
                    &*app.command_queue,
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

            SwapChainInner {
                swap_chain,
                frame_latency_waitable,
                #[cfg(feature = "vulkan-interop")]
                vulkan_interop: None,
            }
        }
    }
}

/*
impl SwapChain {
    pub fn new(size: Size, format: ColorType) -> SwapChain {
        let app = app_backend();
        let width = size.width as u32;
        let height = size.height as u32;

        assert!(width != 0 && height != 0, "surface layer cannot be zero-sized");

        // SAFETY: FFI calls
        unsafe {
            let swap_chain = app
                .dxgi_factory
                .CreateSwapChainForComposition(
                    &*app.command_queue,
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

            SwapChainInner {
                swap_chain,
                frame_latency_waitable,
                #[cfg(feature = "vulkan-interop")]
                vulkan_interop: None,
            }
        }
    }

    #[cfg(feature = "vulkan-interop")]
    pub fn with_vulkan_interop(size: Size, format: ColorType, device: graal::RcDevice) -> SwapChain {
        let mut inner = SwapChainInner::new(size);
        inner.vulkan_interop = Some(vulkan_interop::VulkanInterop::new(
            device,
            &inner.swap_chain,
            size,
            format,
        ));
        SwapChain { inner: Rc::new(inner) }
    }
}*/

/// Creates a DXGI swap chain for use with DirectComposition (`CreateSwapChainForComposition`).
///
/// The swap chain is created with the following parameters:
/// * `AlphaMode = DXGI_ALPHA_MODE_IGNORE`
/// * `BufferCount = 2`
/// * `Scaling = DXGI_SCALING_STRETCH`
/// * `SwapEffect = DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL`
/// * No multisampling (`SampleDesc.Count = 1`)
///
/// In addition, the swap chain is created with the `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` flag.
///
///
/// # Arguments
///
/// * `width` - width in physical pixels of the swap chain buffers.
/// * `height` - height in physical pixels of the swap chain buffers.
///
/// # Panics
///
/// Panics if `width` or `height` are zero (zero-sized swap chains are not supported).
pub(crate) fn create_composition_swap_chain(dxgi_format: DXGI_FORMAT, width: u32, height: u32) -> IDXGISwapChain3 {
    // CreateSwapChainForComposition fails if width or height are zero.
    // Catch this early to avoid a cryptic error message from the system.
    assert!(
        width != 0 && height != 0,
        "swap chain width and height must be non-zero"
    );

    // SAFETY: FFI calls
    unsafe {
        let app = app_backend();

        // Create the swap chain.
        let swap_chain = app
            .dxgi_factory
            .CreateSwapChainForComposition(
                &*app.command_queue,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: width,
                    Height: height,
                    Format: dxgi_format,
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
            .expect("CreateSwapChainForComposition failed");

        // This shouldn't fail (IDXGISwapChain3 is DXGI 1.4 / Windows 10)
        let swap_chain = swap_chain.cast::<IDXGISwapChain3>().unwrap();

        // Only one back buffer can be queued for presentation.
        // I.e. at any time there's one front buffer current being scanned out,
        // and a back buffer being rendered to. This means that once we've rendered a frame,
        // we can't render another until the next vflip.
        swap_chain.SetMaximumFrameLatency(1).unwrap();
        
        swap_chain
    }
}

#[cfg(feature = "vulkan-interop")]
mod vulkan_interop {
    //! Utilities to share compositor swap chain buffers with a Vulkan device
    use super::SWAP_CHAIN_BUFFER_COUNT;
    use crate::app_backend;
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
