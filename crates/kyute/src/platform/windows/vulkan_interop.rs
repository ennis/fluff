//! Composition swap chains with vulkan interop.
use crate::app_backend;
use crate::compositor::ColorType;
use crate::platform::windows::format_to_dxgi_format;
use crate::platform::windows::swap_chain::{create_composition_swap_chain, SWAP_CHAIN_BUFFER_COUNT};
use graal::platform::windows::DeviceExtWindows;
use graal::{vk, SemaphoreWaitKind};
use kurbo::Size;
use std::cell::Cell;
use windows::core::{Interface, Owned};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Fence, ID3D12GraphicsCommandList, ID3D12Resource, D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_FENCE_FLAG_SHARED,
};
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain3, DXGI_ERROR_WAS_STILL_DRAWING, DXGI_PRESENT, DXGI_PRESENT_DO_NOT_WAIT};
use windows::Win32::System::Threading::WaitForSingleObject;

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

/// Image returned by `acquire` that can be rendered to.
pub struct DxgiVulkanInteropImage {
    pub image: graal::Image,
    /// Semaphore that should be waited on before rendering to the image.
    pub ready: graal::SemaphoreWait,
    /// Should be signaled after rendering to the image.
    pub rendering_finished: graal::SemaphoreSignal
}


/// DXGI swap chain that provides facilities for interoperation with Vulkan.
///
/// This holds a composition DXGI swap chain, whose images are imported as Vulkan images.
/// This uses VK_EXT_external_memory_win32 to import the swap chain images.
pub struct DxgiVulkanInteropSwapChain {
    pub dxgi_swap_chain: IDXGISwapChain3,
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

    // frame latency waitable
    frame_latency_waitable: Owned<HANDLE>,
}

impl DxgiVulkanInteropSwapChain {
    /// Creates a new swap chain.
    ///
    /// # Arguments
    ///
    /// * `device` - Vulkan device. The swap chain images will be shared with this device.
    /// * `format` - color format of the swap chain buffers.
    /// * `width` - width in pixels of the swap chain buffers.
    /// * `height` - height in pixels of the swap chain buffers.
    /// * `usage` - Vulkan usage flags for shared swap chain images.
    ///
    /// # Example
    ///
    ///```no_run
    ///# fn test() {
    ///    DxgiVulkanInteropSwapChain::new(
    ///        device,
    ///        ColorType::RGBA8888,
    ///        width,
    ///        height,
    ///        graal::ImageUsage::COLOR_ATTACHMENT | graal::ImageUsage::TRANSFER_DST | graal::ImageUsage::TRANSFER_SRC,
    ///    );
    ///# }
    ///```
    ///
    pub fn new(
        device: graal::RcDevice,
        format: ColorType,
        width: u32,
        height: u32,
        usage: graal::ImageUsage,
    ) -> DxgiVulkanInteropSwapChain {
        let dxgi_format = format_to_dxgi_format(format);
        let vk_format = format_to_vk_format(format);

        // create the DXGI swap chain
        let swap_chain = create_composition_swap_chain(dxgi_format, width, height);

        let app = app_backend();
        let d3d = &app.d3d12_device.0;
        let mut images = vec![];

        unsafe {

            let frame_latency_waitable = Owned::new(swap_chain.GetFrameLatencyWaitableObject());
            //assert!(!frame_latency_waitable.is_invalid());

            app.command_allocator.get_ref().unwrap().Reset().unwrap();

            // wrap swap chain buffers as vulkan images
            for i in 0..SWAP_CHAIN_BUFFER_COUNT {
                // obtain the ID3D12Resource of each swap chain buffer and create a shared handle for them
                let swap_chain_buffer = swap_chain.GetBuffer::<ID3D12Resource>(i).unwrap();
                // NOTE: I'm not sure if CreateSharedHandle is supposed to work on swap chain
                //       buffers. It didn't work with D3D11 if I remember correctly, but
                //       D3D12 doesn't seem to mind. If this breaks at some point, we may work
                //       around that by using a staging texture and copying it to the swap chain
                //       on the D3D12 side.
                //       Also, I can't find the code on github that I used as a reference for this.
                let shared_handle = d3d
                    .CreateSharedHandle(&swap_chain_buffer, None, GENERIC_ALL.0, None)
                    .unwrap();

                // import the buffer to a vulkan image with memory imported from the shared handle
                let imported_image = device.create_imported_image_win32(
                    &graal::ImageCreateInfo {
                        memory_location: graal::MemoryLocation::GpuOnly,
                        type_: graal::ImageType::Image2D,
                        usage,
                        format: vk_format,
                        width,
                        height,
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

                // Create the dummy command list that is executed just before signalling the fence
                // for synchronization from D3D12 to Vulkan. Doing "something" on the D3D side
                // before signalling the fence is necessary to properly synchronize with
                // the presentation engine.
                // In our case we just call DiscardResource on the swap chain buffer.
                // A barrier would also work if contents need to be preserved.
                let discard_cmd_list: ID3D12GraphicsCommandList = d3d
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
            let fence = d3d.CreateFence(0, D3D12_FENCE_FLAG_SHARED).unwrap();
            let fence_shared_handle = d3d.CreateSharedHandle(&fence, None, GENERIC_ALL.0, None).unwrap();
            let fence_semaphore = device.create_imported_semaphore_win32(
                vk::SemaphoreImportFlags::empty(),
                vk::ExternalSemaphoreHandleTypeFlags::D3D12_FENCE,
                fence_shared_handle.0,
                None,
            );
            device.set_object_name(fence_semaphore, "DxgiVulkanSharedFence");

            let swap_chain = DxgiVulkanInteropSwapChain {
                dxgi_swap_chain: swap_chain,
                images,
                fence_value: Cell::new(1),
                fence_semaphore,
                fence,
                fence_shared_handle,
                surface_acquired: Cell::new(false),
                device,
                frame_latency_waitable
            };
            //swap_chain.wait_for_presentation();
            swap_chain
        }
    }

    fn wait_for_presentation(&self) {
        //let t = std::time::Instant::now();
        if !self.frame_latency_waitable.is_invalid() {
            unsafe {
                WaitForSingleObject(*self.frame_latency_waitable, 1000);
            }
        }
        //trace!("wait_for_presentation took {:?}", t.elapsed());
    }

    /// Acquires the next image in the swap chain, and returns a vulkan image handle to it.
    ///
    /// This will queue a wait on the specified command stream that waits for the image
    /// to be ready for rendering.
    ///
    /// You should call `present` to release the returned image, before calling this again.
    ///
    /// # Return value
    /// Returns a tuple containing the vulkan image handle and a semaphore wait object that
    /// should be waited on before rendering to the image.
    pub fn acquire(&self) -> DxgiVulkanInteropImage {
        assert!(!self.surface_acquired.get(), "surface already acquired");

        let app = app_backend();

        let index = unsafe { self.dxgi_swap_chain.GetCurrentBackBufferIndex() };
        let image = &self.images[index as usize];

        let fence_value = self.fence_value.get();
        self.fence_value.set(fence_value + 1);

        // Synchronization: D3D12 -> Vulkan
        unsafe {
            // dummy rendering to synchronize with the presentation engine before signalling the fence
            // needed! there's some implicit synchronization being done here
            app.command_queue
                .ExecuteCommandLists(&[Some(image.discard_cmd_list.cast().unwrap())]);
            app.command_queue.Signal(&self.fence, fence_value).unwrap();
        }

        self.surface_acquired.set(true);

        // FIXME: SemaphoreWait is not the correct type because the caller should choose the dst_stage
        DxgiVulkanInteropImage {
            image: image.image.clone(),
            ready: graal::SemaphoreWait {
                kind: SemaphoreWaitKind::D3D12Fence {
                    semaphore: self.fence_semaphore,
                    fence: Default::default(),
                    value: fence_value,
                },
                dst_stage: vk::PipelineStageFlags::ALL_COMMANDS,
            },
            rendering_finished: graal::SemaphoreSignal::D3D12Fence {
                semaphore: self.fence_semaphore,
                fence: Default::default(),
                value: fence_value + 1,
            }
        }
    }

    /// Submits the last acquired swap chain image for presentation.
    ///
    /// TODO: incremental present
    pub fn present(&self) {
        let fence_value = self.fence_value.get();
        self.fence_value.set(fence_value + 1);

        // Synchronization: Vulkan -> D3D12
        unsafe {
            //eprintln!("DxgiVulkanInteropSwapChain::present: fence_value = {}", fence_value);
            // synchronize with vulkan rendering before presenting
            app_backend().command_queue.Wait(&self.fence, fence_value).unwrap();
            // present the image
            //self.dxgi_swap_chain.Present(0, DXGI_PRESENT::default()).unwrap();
            let r = self.dxgi_swap_chain.Present(0, DXGI_PRESENT_DO_NOT_WAIT);
            if r == DXGI_ERROR_WAS_STILL_DRAWING {
                eprintln!("DXGI_ERROR_WAS_STILL_DRAWING");
            }
        }

        self.surface_acquired.set(false);
        //self.wait_for_presentation();
    }
}

impl Drop for DxgiVulkanInteropSwapChain {
    fn drop(&mut self) {
        // Before releasing the buffers, we must make sure that the swap chain is not in use
        // We don't bother with setting up fences around the swap chain, we just wait for all commands to complete.
        // We could use fences to avoid unnecessary waiting, but not sure that it's worth the complication.
        app_backend().wait_for_gpu();

        unsafe {
            // FIXME: there should be a RAII wrapper for semaphores probably
            self.device.raw().destroy_semaphore(self.fence_semaphore, None);
            CloseHandle(self.fence_shared_handle).unwrap();
            for img in self.images.iter() {
                CloseHandle(img.shared_handle).unwrap();
            }
        }
    }
}

fn format_to_vk_format(format: ColorType) -> vk::Format {
    match format {
        ColorType::RGBA8888 => vk::Format::R8G8B8A8_UNORM,
        ColorType::BGRA8888 => vk::Format::B8G8R8A8_UNORM,
        ColorType::SRGBA8888 => vk::Format::R8G8B8A8_SRGB,
        _ => unimplemented!(),
    }
}
