//! Types & constants for interfacing between shader and application code.
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::marker::PhantomData;
use graal::DeviceAddress;


// Define type aliases for slang types. These are referenced in the generated bindings, which
// are just a syntactical translation of the slang declarations to Rust.
//
// WARNING: these must match the layout of the corresponding slang types in the shaders.
//          Notably, the `Texture*_Handle` types must have the same layout as `[u32;2]`
//          to match slang.
type Pointer<T> = DeviceAddress<T>;
type uint = u32;
type int = i32;
type float = f32;
type bool = u32;
type float2 = [f32; 2];
type float3 = [f32; 3];
type float4 = [f32; 4];
type uint2 = [u32; 2];
type uint3 = [u32; 3];
type uint4 = [u32; 4];
type int2 = [i32; 2];
type int3 = [i32; 3];
type int4 = [i32; 4];
type float4x4 = [[f32; 4]; 4];

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Texture2D_Handle<T> {
    handle: graal::ImageHandle,
    _unused: u32,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> From<graal::ImageHandle> for Texture2D_Handle<T> {
    fn from(handle: graal::ImageHandle) -> Self {
        Texture2D_Handle {
            handle,
            _unused: 0,
            _phantom: PhantomData,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct RWTexture2D_Handle<T> {
    handle: graal::ImageHandle,
    _unused: u32,
    _phantom: PhantomData<fn() -> T>,
}


impl<T> From<graal::ImageHandle> for RWTexture2D_Handle<T> {
    fn from(handle: graal::ImageHandle) -> Self {
        RWTexture2D_Handle {
            handle,
            _unused: 0,
            _phantom: PhantomData,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SamplerState_Handle {
    handle: graal::SamplerHandle,
    _unused: u32,
}

impl From<graal::SamplerHandle> for SamplerState_Handle {
    fn from(handle: graal::SamplerHandle) -> Self {
        SamplerState_Handle {
            handle,
            _unused: 0,
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));