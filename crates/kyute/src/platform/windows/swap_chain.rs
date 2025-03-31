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
