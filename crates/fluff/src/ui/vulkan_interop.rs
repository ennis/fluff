//! Utilities to share compositor swap chain buffers with a Vulkan device

use graal::{platform::windows::DeviceExtWindows, vk};
use kyute::Size;
use std::{cell::Cell, ffi::c_void, time::Duration};
use windows::{
    core::ComInterface,
    Win32::{
        Foundation::{CloseHandle, GENERIC_ALL, HANDLE},
        Graphics::{
            Direct3D12::{
                ID3D12Fence, ID3D12GraphicsCommandList, ID3D12Resource, D3D12_COMMAND_LIST_TYPE_DIRECT,
                D3D12_FENCE_FLAG_SHARED,
            },
            Dxgi::IDXGISwapChain3,
        },
    },
};

struct VulkanInteropImage {
    /// Shared handle to DXGI swap chain buffer.
    shared_handle: HANDLE,
    /// Imported DXGI swap chain buffer.
    image: graal::ImageInfo,
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
pub(crate) struct VulkanInterop {
    /// Imported vulkan images for the swap chain buffers.
    images: Vec<VulkanInteropImage>,

    // --- Fence for synchronizing between D3D12 presentation and vulkan ---
    /// Vulkan side of the presentation fence
    presentation_fence_semaphore: vk::Semaphore,
    /// D3D12 side of the presentation fence
    presentation_fence: ID3D12Fence,
    /// Fence shared handle (imported to vulkan)
    presentation_fence_shared_handle: HANDLE,

    /// Presentation fence value
    presentation_fence_value: Cell<u64>,

    /// Whether a swap chain surface has been acquired and not released yet.
    surface_acquired: Cell<bool>,
}

impl VulkanInterop {
    pub(super) fn new(swap_chain: &IDXGISwapChain3, size: Size) -> VulkanInterop {
        let app = Application::global();
        let d3d12_device = &app.backend().d3d12_device.0;
        let gr_device = app.gpu_device();
        let mut images = vec![];

        unsafe {
            app.backend()
                .d3d12_command_allocator
                .get_ref()
                .unwrap()
                .Reset()
                .unwrap();

            // --- wrap swap chain buffers as vulkan images ---
            for i in 0..2 {
                // obtain the ID3D12Resource of each swap chain buffer and create a shared handle for them
                let swap_chain_buffer: ID3D12Resource =
                    swap_chain.GetBuffer::<ID3D12Resource>(i).expect("GetBuffer failed");
                let shared_handle = d3d12_device
                    .CreateSharedHandle(&swap_chain_buffer, None, GENERIC_ALL.0, None)
                    .expect("CreateSharedHandle failed");

                // create a vulkan image with memory imported from the shared handle
                let imported_image = gr_device.create_imported_image_win32(
                    "composition surface",
                    &graal::ImageResourceCreateInfo {
                        image_type: vk::ImageType::TYPE_2D,
                        usage: vk::ImageUsageFlags::COLOR_ATTACHMENT
                            | vk::ImageUsageFlags::TRANSFER_DST
                            | vk::ImageUsageFlags::TRANSFER_SRC,
                        format: vk::Format::R16G16B16A16_SFLOAT,
                        extent: vk::Extent3D {
                            width: size.width as u32,
                            height: size.height as u32,
                            depth: 1,
                        },
                        mip_levels: 1,
                        array_layers: 1,
                        samples: 1,
                        tiling: Default::default(),
                    },
                    vk::MemoryPropertyFlags::default(),
                    vk::MemoryPropertyFlags::default(),
                    vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE_KHR,
                    shared_handle.0 as vk::HANDLE,
                    None,
                );

                let discard_command_list: ID3D12GraphicsCommandList = d3d12_device
                    .CreateCommandList(
                        0,
                        D3D12_COMMAND_LIST_TYPE_DIRECT,
                        app.backend().d3d12_command_allocator.get_ref().unwrap(),
                        None,
                    )
                    .unwrap();
                discard_command_list.DiscardResource(&swap_chain_buffer, None);
                discard_command_list.Close().unwrap();

                images.push(VulkanInteropImage {
                    shared_handle,
                    image: imported_image,
                    discard_cmd_list: discard_command_list,
                });
            }

            // Create & share a D3D12 fence for VK/DXGI sync
            let presentation_fence = d3d12_device.CreateFence(0, D3D12_FENCE_FLAG_SHARED).unwrap();
            let presentation_fence_shared_handle = d3d12_device
                .CreateSharedHandle(&presentation_fence, None, GENERIC_ALL.0 as u32, None)
                .unwrap();
            let presentation_fence_semaphore = gr_device.create_imported_semaphore_win32(
                vk::SemaphoreImportFlags::empty(),
                vk::ExternalSemaphoreHandleTypeFlags::D3D12_FENCE,
                presentation_fence_shared_handle.0 as *mut c_void,
                None,
            );

            VulkanInterop {
                images,
                presentation_fence_value: Cell::new(0),
                presentation_fence_semaphore,
                presentation_fence,
                presentation_fence_shared_handle,
                surface_acquired: Cell::new(false),
            }
        }
    }

    /// Acquires the next image in the swap chain, and returns a vulkan image handle to it.
    ///
    /// You must release the previous acquired image before calling this
    /// function. See `present_image_vk`.
    pub(super) fn acquire_image_vk(&mut self, swap_chain: &IDXGISwapChain3) -> graal::ImageInfo {
        assert!(!self.surface_acquired.get());

        let app = Application::global();
        let buf_index = unsafe { swap_chain.GetCurrentBackBufferIndex() };
        let vk_img = &self.images[buf_index as usize];

        let fence_value = self.presentation_fence_value.get();
        self.presentation_fence_value.set(fence_value + 1);

        // initial sync - D3D12 signal
        let command_queue = &app.backend().d3d12_command_queue;
        unsafe {
            // dummy rendering to synchronize with the presentation engine before signalling the fence
            // needed! there's some implicit synchronization being done here
            command_queue.ExecuteCommandLists(&[Some(vk_img.discard_cmd_list.cast().unwrap())]);
            command_queue.Signal(&self.presentation_fence, fence_value).unwrap();

            // initial sync - vulkan wait
            {
                let mut drawing = app.drawing();
                let mut gpu_ctx = &mut drawing.context;
                let mut frame = graal::Frame::new();
                frame.add_pass(
                    graal::PassBuilder::new()
                        .external_semaphore_wait(
                            self.presentation_fence_semaphore,
                            vk::PipelineStageFlags::ALL_COMMANDS,
                            graal::SemaphoreWaitKind::D3D12Fence(fence_value),
                        )
                        .name("DXGI-to-Vulkan sync"),
                );
                gpu_ctx.submit_frame(&mut (), frame, &Default::default());
            }
        }

        self.surface_acquired.set(true);
        vk_img.image
    }

    /// Submits the last acquired swap chain image for presentation.
    ///
    /// TODO: incremental present
    pub(super) fn present_image_vk(&mut self, swap_chain: &IDXGISwapChain3) {
        //let _span = trace_span!("present_and_release_surface").entered();
        //trace!("surface present");

        let app = Application::global();
        let fence_value = self.presentation_fence_value.get();
        self.presentation_fence_value.set(fence_value + 1);

        unsafe {
            {
                let mut drawing = app.drawing();
                let mut gpu_ctx = &mut drawing.context;
                let mut frame = graal::Frame::new();
                // FIXME we signal the fence on the graphics queue, but work affecting the image might have been scheduled on another in case of async compute.
                frame.add_pass(unsafe {
                    graal::PassBuilder::new()
                        .external_semaphore_signal(
                            self.presentation_fence_semaphore,
                            graal::SemaphoreSignalKind::D3D12Fence(fence_value),
                        )
                        .name("Vulkan-to-DXGI sync")
                });
                gpu_ctx.submit_frame(&mut (), frame, &Default::default());
            }

            app.backend()
                .d3d12_command_queue
                .Wait(&self.presentation_fence, fence_value)
                .unwrap();

            swap_chain.Present(1, 0).ok().expect("Present failed");
        }

        self.surface_acquired.set(false);
    }
}

impl Drop for VulkanInterop {
    /// Waits for the 3D device to be idle and destroys the vulkan images previously created with `create_interop()`.
    fn drop(&mut self) {
        let app = Application::global();
        let app_backend = app.backend();
        let app = Application::global();
        let d3d12_device = &app_backend.d3d12_device.0;

        // before releasing the buffers, we must make sure that the swap chain is not in use
        // TODO we don't bother with setting up fences around the swap chain, we just wait for all commands to complete.
        // We could use fences to avoid unnecessary waiting, but not sure that it's worth the complication.
        app_backend.wait_for_gpu_command_completion();

        // destroy fences
        app.gpu_device().destroy_semaphore(self.presentation_fence_semaphore);
        unsafe {
            CloseHandle(self.presentation_fence_shared_handle);
        }

        // destroy the vulkan imported images
        let mut frame = graal::Frame::new();
        for img in self.images.iter() {
            unsafe {
                CloseHandle(img.shared_handle);
            }
            frame.destroy_image(img.image.id);
        }

        // FIXME: app.drawing() then app.gpu_device()? these two things should be together
        let progress = app
            .drawing()
            .context
            .submit_frame(&mut (), frame, &graal::SubmitInfo::default());
        app.gpu_device()
            .wait(&progress.progress, Duration::from_secs(1))
            .unwrap();
    }
}