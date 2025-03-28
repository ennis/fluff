//! Windows implementation details
use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::rc::Rc;
use std::time::Duration;

pub use compositor::{DrawableSurface, Layer};
use skia_safe::gpu::Protected;
use threadbound::ThreadBound;
use windows::core::{IUnknown, Interface, Owned};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12CreateDevice, ID3D12CommandAllocator, ID3D12CommandQueue, ID3D12Device, ID3D12Fence,
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_FENCE_FLAG_NONE,
};
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice3, IDCompositionDesktopDevice};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter1, IDXGIFactory3, DXGI_CREATE_FACTORY_FLAGS};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime;

mod compositor;

/////////////////////////////////////////////////////////////////////////////
// COM wrappers
/////////////////////////////////////////////////////////////////////////////

// COM thread safety notes: some interfaces are thread-safe, some are not, and for some we don't know due to poor documentation.
// Additionally, some interfaces should only be called on the thread in which they were created.
//
// - For thread-safe interfaces: wrap them in a `Send+Sync` newtype
// - For interfaces bound to a thread: wrap them in `ThreadBound`
// - For interfaces not bound to a thread but with unsynchronized method calls:
//      wrap them in a `Send` newtype, and if you actually need to call the methods from multiple threads, `Mutex`.

/// Defines a send+sync wrapper over a windows interface type.
///
/// This signifies that it's OK to call the interface's methods from multiple threads simultaneously:
/// the object itself should synchronize the calls.
macro_rules! sync_com_ptr_wrapper {
    ($wrapper:ident ( $iface:ident ) ) => {
        #[derive(Clone)]
        pub(crate) struct $wrapper(pub(crate) $iface);
        unsafe impl Sync for $wrapper {} // ok to send &I across threads
        unsafe impl Send for $wrapper {} // ok to send I across threads
        impl ::std::ops::Deref for $wrapper {
            type Target = $iface;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

/*/// Defines a send wrapper over a windows interface type.
///
/// This signifies that it's OK to call an interface's methods from a different thread than that in which it was created.
/// However, you still have to synchronize the method calls yourself (with, e.g., a `Mutex`).
macro_rules! send_com_ptr_wrapper {
    ($wrapper:ident ( $iface:ident ) ) => {
        #[derive(Clone)]
        pub(crate) struct $wrapper(pub(crate) $iface);
        unsafe impl Send for $wrapper {} // ok to send I across threads
        impl ::std::ops::Deref for $wrapper {
            type Target = $iface;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}*/

// TODO: the wrappers are not necessary anymore since ApplicationBackend is not accessible from
//       threads other than the main thread. We can just use the raw interfaces directly.
sync_com_ptr_wrapper! { D3D12Device(ID3D12Device) }
sync_com_ptr_wrapper! { DXGIFactory3(IDXGIFactory3) }
sync_com_ptr_wrapper! { D3D12CommandQueue(ID3D12CommandQueue) }
//sync_com_ptr_wrapper! { DWriteFactory(IDWriteFactory) }
//sync_com_ptr_wrapper! { D3D12Fence(ID3D12Fence) }
//sync_com_ptr_wrapper! { D3D11Device(ID3D11Device5) }
//sync_com_ptr_wrapper! { WICImagingFactory2(IWICImagingFactory2) }
//sync_com_ptr_wrapper! { D2D1Factory1(ID2D1Factory1) }
//sync_com_ptr_wrapper! { D2D1Device(ID2D1Device) }
//send_com_ptr_wrapper! { D2D1DeviceContext(ID2D1DeviceContext) }

/////////////////////////////////////////////////////////////////////////////
// AppBackend
/////////////////////////////////////////////////////////////////////////////

struct GpuFenceData {
    fence: ID3D12Fence,
    event: Owned<HANDLE>,
    value: Cell<u64>,
}

pub struct ApplicationBackend {
    //pub(crate) dispatcher_queue_controller: DispatcherQueueController,
    adapter: IDXGIAdapter1,
    pub(crate) d3d12_device: D3D12Device,        // thread safe
    pub(crate) command_queue: D3D12CommandQueue, // thread safe
    pub(crate) command_allocator: ThreadBound<ID3D12CommandAllocator>,
    dxgi_factory: DXGIFactory3,
    //dwrite_factory: DWriteFactory,
    /// Fence data used to synchronize GPU and CPU (see `wait_for_gpu`).
    sync: GpuFenceData,
    // Windows compositor instance (Windows.UI.Composition).
    //compositor: Compositor,
    //debug: IDXGIDebug1,
    direct_context: RefCell<skia_safe::gpu::DirectContext>,
    pub(crate) composition_device: IDCompositionDesktopDevice,
}

impl ApplicationBackend {
    pub(crate) fn new() -> ApplicationBackend {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap() };

        // DirectWrite factory
        //let dwrite_factory = unsafe {
        //    let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
        //    DWriteFactory(dwrite)
        //};

        //=========================================================
        // DXGI Factory and adapter enumeration

        // SAFETY: the paramters are valid
        let dxgi_factory =
            unsafe { DXGIFactory3(CreateDXGIFactory2::<IDXGIFactory3>(DXGI_CREATE_FACTORY_FLAGS::default()).unwrap()) };

        // --- Enumerate adapters
        let mut adapters = Vec::new();
        unsafe {
            let mut i = 0;
            while let Ok(adapter) = dxgi_factory.EnumAdapters1(i) {
                adapters.push(adapter);
                i += 1;
            }
        };

        let mut chosen_adapter = None;
        for adapter in adapters.iter() {
            let desc = unsafe { adapter.GetDesc1().unwrap() };

            use std::os::windows::ffi::OsStringExt;

            let name = &desc.Description[..];
            let name_len = name.iter().take_while(|&&c| c != 0).count();
            let name = OsString::from_wide(&desc.Description[..name_len])
                .to_string_lossy()
                .into_owned();
            tracing::info!(
                "DXGI adapter: name={}, LUID={:08x}{:08x}",
                name,
                desc.AdapterLuid.HighPart,
                desc.AdapterLuid.LowPart,
            );
            /*if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0) != 0 {
                continue;
            }*/
            if chosen_adapter.is_none() {
                chosen_adapter = Some(adapter.clone())
            }
        }
        let adapter = chosen_adapter.expect("no suitable video adapter found");

        //=========================================================
        // D3D12 stuff

        //let debug = unsafe { DXGIGetDebugInterface1(0).unwrap() };

        let d3d12_device = unsafe {
            let mut d3d12_device: Option<ID3D12Device> = None;
            D3D12CreateDevice(
                // pAdapter:
                &adapter.cast::<IUnknown>().unwrap(),
                // MinimumFeatureLevel:
                D3D_FEATURE_LEVEL_12_0,
                // ppDevice:
                &mut d3d12_device,
            )
            .expect("D3D12CreateDevice failed");
            D3D12Device(d3d12_device.unwrap())
        };

        let command_queue = unsafe {
            let cqdesc = D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                Priority: 0,
                Flags: Default::default(),
                NodeMask: 0,
            };
            let cq: ID3D12CommandQueue = d3d12_device
                .0
                .CreateCommandQueue(&cqdesc)
                .expect("CreateCommandQueue failed");
            D3D12CommandQueue(cq)
        };

        let command_allocator = unsafe {
            let command_allocator = d3d12_device
                .0
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .unwrap();
            ThreadBound::new(command_allocator)
        };

        //=========================================================
        // Compositor

        let direct_context = unsafe {
            // SAFETY: backend_context is valid I guess?
            skia_safe::gpu::DirectContext::new_d3d(
                &skia_safe::gpu::d3d::BackendContext {
                    adapter: adapter.clone(),
                    device: d3d12_device.0.clone(),
                    queue: command_queue.0.clone(),
                    memory_allocator: None,
                    protected_context: Protected::No,
                },
                None,
            )
            .expect("failed to create skia context")
        };

        //let compositor = Compositor::new().expect("failed to create compositor");

        let composition_device: IDCompositionDesktopDevice =
            unsafe { DCompositionCreateDevice3(None).expect("failed to create composition device") };

        //let composition_device_debug : IDCompositionDeviceDebug = composition_device.cast().unwrap();
        //unsafe {
        //    composition_device_debug.EnableDebugCounters();
        //}

        let sync = {
            let fence = unsafe {
                d3d12_device
                    .CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE)
                    .expect("CreateFence failed")
            };
            let event = unsafe { Owned::new(CreateEventW(None, false, false, None).unwrap()) };

            GpuFenceData {
                fence,
                event,
                value: Cell::new(0),
            }
        };

        ApplicationBackend {
            d3d12_device,
            command_queue,
            command_allocator,
            dxgi_factory,
            adapter,
            composition_device,
            sync,
            direct_context: RefCell::new(direct_context),
        }
    }

    /// Waits for submitted GPU commands to complete.
    pub(crate) fn wait_for_gpu(&self) {
        //let _span = span!("wait_for_gpu_command_completion");
        unsafe {
            let mut val = self.sync.value.get();
            val += 1;
            self.sync.value.set(val);
            self.command_queue
                .Signal(&self.sync.fence, val)
                .expect("ID3D12CommandQueue::Signal failed");
            if self.sync.fence.GetCompletedValue() < val {
                self.sync
                    .fence
                    .SetEventOnCompletion(val, *self.sync.event)
                    .expect("SetEventOnCompletion failed");
                WaitForSingleObject(*self.sync.event, 0xFFFFFFFF);
            }
        }
    }

    /// Returns the system double click time in milliseconds.
    pub(crate) fn double_click_time(&self) -> Duration {
        unsafe {
            let ms = GetDoubleClickTime();
            Duration::from_millis(ms as u64)
        }
    }

    /// Returns the system caret blink time.
    pub(crate) fn get_caret_blink_time(&self) -> Duration {
        unsafe {
            let ms = GetCaretBlinkTime();
            // TODO it may return INFINITE, which should be treated as no blinking
            Duration::from_millis(ms as u64)
        }
    }

    pub(crate) fn teardown(&self) {
        self.wait_for_gpu();
    }
}
