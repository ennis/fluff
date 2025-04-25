//! DXGI swap chain wrapper
use crate::app_backend;
use windows::core::Interface;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain3, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT};

/// Default number of buffers in the swap chain.
///
/// 3 is the minimum. 2 leads to contentions on the present queue.
pub(super) const SWAP_CHAIN_BUFFER_COUNT: u32 = 3;

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

    // NOTE: using too few buffers can lead to contention on the present queue since frames
    // must wait for a buffer to be available.
    //
    // For instance:
    // - Compositor tick
    // - Render frame 1 to buffer A
    // - Compositor tick
    // - Render frame 2 to buffer B
    //    (frame 1 took longer than expected)
    //    render frame 1 finishes, present buffer A
    // - Render frame 3 to buffer A
    // ...
    //
    // 3 buffers is the minimum to avoid contention.

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
                    // FIXME this should be a parameter; DXGI_ALPHA_MODE_PREMULTIPLIED adds some latency though
                    AlphaMode: DXGI_ALPHA_MODE_IGNORE, //DXGI_ALPHA_MODE_PREMULTIPLIED,
                    Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32,
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
        //swap_chain.SetMaximumFrameLatency(1).unwrap();

        swap_chain
    }
}
