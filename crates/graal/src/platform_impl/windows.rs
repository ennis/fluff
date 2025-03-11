const PLATFORM_DEVICE_EXTENSIONS: &[&str] = &["VK_KHR_external_memory_win32", "VK_KHR_external_semaphore_win32"];

/// Windows-specific vulkan extensions
pub struct PlatformExtensions {
    pub khr_external_memory_win32: ash::extensions::khr::ExternalMemoryWin32,
    pub khr_external_semaphore_win32: ash::extensions::khr::ExternalSemaphoreWin32,
}

impl PlatformExtensions {
    pub(crate) fn names() -> &'static [&'static str] {
        PLATFORM_DEVICE_EXTENSIONS
    }

    pub(crate) fn load(_entry: &ash::Entry, instance: &ash::Instance, device: &ash::Device) -> PlatformExtensions {
        let khr_external_memory_win32 = ash::extensions::khr::ExternalMemoryWin32::new(instance, device);
        let khr_external_semaphore_win32 = ash::extensions::khr::ExternalSemaphoreWin32::new(instance, device);
        PlatformExtensions {
            khr_external_memory_win32,
            khr_external_semaphore_win32,
        }
    }
}
