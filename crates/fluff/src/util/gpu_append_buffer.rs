use std::ptr;
use graal::{Buffer, BufferUsage, CommandStream, Device, DeviceAddress, MemoryLocation};
use graal::util::DeviceExt;
use tracing::trace;

/// A resizable, append-only GPU buffer. Like `Vec<T>` but stored on GPU device memory.
///
/// If the buffer is host-visible, elements can be added directly to the buffer.
/// Otherwise, elements are first added to a staging area and must be copied to the buffer on
/// the device timeline by calling [`AppendBuffer::commit`].
pub struct AppendBuffer<T> {
    buffer: Buffer<[T]>,
    len: usize,
    staging: Vec<T>,
}

impl<T: Copy> AppendBuffer<T> {
    /// Creates an append buffer with the given usage flags and default capacity.
    pub fn new(device: &Device, usage: BufferUsage, memory_location: MemoryLocation) -> AppendBuffer<T> {
        Self::with_capacity(device, usage, memory_location, 16)
    }

    /// Creates an append buffer with the given usage flags and initial capacity.
    pub fn with_capacity(device: &Device, mut usage: BufferUsage, memory_location: MemoryLocation, capacity: usize) -> Self {
        // Add TRANSFER_DST capacity if the buffer is not host-visible
        if memory_location != MemoryLocation::CpuToGpu {
            usage |= BufferUsage::TRANSFER_DST;
        }
        let buffer = device.create_array_buffer(usage, memory_location, capacity);
        Self {
            buffer,
            len: 0,
            staging: vec![],
        }
    }

    /// Returns the pointer to the buffer data in host memory.
    ///
    /// # Panics
    ///
    /// Panics if the buffer is not host-visible.
    pub fn as_mut_ptr(&self) -> *mut T {
        self.buffer.as_mut_ptr()
    }

    pub unsafe fn set_len(&mut self, len: usize) {
        assert!(self.host_visible());
        assert!(len <= self.buffer.len());
        self.len = len;
    }

    pub fn set_name(&self, name: &str) {
        self.buffer.set_name(name);
    }

    pub fn device_address(&self) -> DeviceAddress<[T]> {
        self.buffer.device_address()
    }

    /// Returns the capacity in elements of the buffer before it needs to be resized.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the number of elements in the buffer (including elements in the staging area).
    pub fn len(&self) -> usize {
        // number of elements in the main buffer + pending elements
        self.len + self.staging.len()
    }

    fn needs_to_grow(&self, additional: usize) -> bool {
        self.len + additional > self.capacity()
    }

    /// Whether the buffer is host-visible (and mapped in memory).
    fn host_visible(&self) -> bool {
        self.buffer.memory_location() == MemoryLocation::CpuToGpu
    }

    fn reserve_gpu(&mut self, cmd: &mut CommandStream, additional: usize) {
        if self.needs_to_grow(additional) {
            let memory_location = self.buffer.memory_location();
            let new_capacity = (self.len + additional).next_power_of_two(); // in num of elements
            trace!(
                "AppendBuffer: reallocating {} -> {} bytes",
                self.capacity() * size_of::<T>(),
                new_capacity * size_of::<T>()
            );
            let new_buffer = self
                .buffer
                .device()
                .create_array_buffer(self.buffer.usage(), memory_location, new_capacity);
            cmd.copy_buffer(&self.buffer.untyped, 0, &new_buffer.untyped, 0, (self.len * size_of::<T>()) as u64);
            self.buffer = new_buffer;
        }
    }

    /// Reserve space for `additional` elements in the buffer, if the buffer is host-visible.
    fn reserve_cpu(&mut self, additional: usize) {
        assert!(self.host_visible());
        if self.needs_to_grow(additional) {
            let new_capacity = (self.len + additional).next_power_of_two(); // in num of elements
            trace!(
                "AppendBuffer: reallocating {} -> {} bytes",
                self.capacity() * size_of::<T>(),
                new_capacity * size_of::<T>()
            );
            let new_buffer = self
                .buffer
                .device()
                .create_array_buffer(self.buffer.usage(), self.buffer.memory_location(), new_capacity);
            // Copy the old data to the new buffer
            unsafe {
                ptr::copy_nonoverlapping(self.buffer.as_mut_ptr(), new_buffer.as_mut_ptr(), self.len);
            }
            self.buffer = new_buffer;
        }
    }

    /// Truncates the buffer to the given length.
    ///
    /// # Panics
    ///
    /// * Panics if `len` is greater than the current length of the buffer.
    /// * Panics if there are pending elements in the staging area.
    pub fn truncate(&mut self, len: usize) {
        assert!(len <= self.len);
        assert!(self.staging.is_empty());
        self.len = len;
    }

    /// Adds an element to the buffer.
    pub fn push(&mut self, elem: T) {
        if self.buffer.memory_location() == MemoryLocation::CpuToGpu {
            // the buffer is mapped in memory, we can copy the element right now
            self.reserve_cpu(1);
            unsafe {
                ptr::write(self.buffer.as_mut_ptr().add(self.len), elem);
            }
        } else {
            // add to pending list
            self.staging.push(elem);
        }
        self.len += 1;
    }

    /// Returns whether there are pending elements to be copied to the main buffer.
    pub fn has_pending(&self) -> bool {
        !self.staging.is_empty()
    }

    /// Copies pending elements to the main buffer.
    pub fn commit(&mut self, cmd: &mut CommandStream) {
        let n = self.staging.len(); // number of elements to append
        if n == 0 {
            return;
        }

        if self.host_visible() {
            // nothing to do, the elements have already been copied
            return;
        }

        self.reserve_gpu(cmd, n);
        // allocate staging buffer & copy pending elements
        let staging_buf = self
            .buffer
            .device()
            .create_array_buffer::<T>(BufferUsage::TRANSFER_SRC, MemoryLocation::CpuToGpu, n);
        unsafe {
            ptr::copy_nonoverlapping(self.staging.as_ptr(), staging_buf.as_mut_ptr(), n);
        }
        // copy from staging to main buffer
        let elem_size = size_of::<T>() as u64;
        cmd.copy_buffer(
            &staging_buf.untyped,
            0,
            &self.buffer.untyped,
            self.len as u64 * elem_size,
            n as u64 * elem_size,
        );
        self.staging.clear();
    }

    /// Returns the underlying GPU buffer.
    pub fn buffer(&self) -> Buffer<[T]> {
        self.buffer.clone()
    }
}