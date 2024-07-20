//! Types used in data structures passed between the application and the shaders.
use std::marker::PhantomData;
use std::{fmt, mem};
pub use glam::{Vec2, Vec3, Vec4, IVec2, IVec3, IVec4, UVec2, UVec3, UVec4, Mat2, Mat3, Mat4};

/// Handle to an image in a shader.
#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct ImageHandle {
    /// Index of the image in the image descriptor array.
    pub index: u32,
}

/// Handle to a sampler in a shader.
#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct SamplerHandle {
    /// Index of the image in the sampler descriptor array.
    pub index: u32,
}

/// Device address of a GPU buffer.
#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct BufferAddressUntyped {
    /// The buffer device address.
    pub address: u64,
}

/// Typed device address of a GPU buffer.
#[repr(transparent)]
pub struct BufferAddress<T: ?Sized> {
    /// The buffer device address.
    pub address: u64,
    pub _phantom: PhantomData<T>,
}

// #26925 clone impl
impl<T: ?Sized> Clone for BufferAddress<T> {
    fn clone(&self) -> Self {
        BufferAddress {
            address: self.address,
            _phantom: PhantomData,
        }
    }
}

// #26925 copy impl
impl<T: ?Sized> Copy for BufferAddress<T> {}

impl<T: ?Sized> fmt::Debug for BufferAddress<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BufferHandle({:016x})", self.address)
    }
}

