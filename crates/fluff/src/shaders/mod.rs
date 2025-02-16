//! This application's shaders and related interface types.
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

pub mod types;

#[cfg(feature = "shader-hot-reload")]
mod compiler;

use std::borrow::Cow;
use std::marker::PhantomData;
use graal::DeviceAddress;

#[cfg(feature = "shader-hot-reload")]
pub use compiler::compile_shader_module;


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
/// Represents a shader type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Compute,
    Vertex,
    Fragment,
    Geometry,
    TessellationControl,
    TessellationEvaluation,
    Mesh,
    Task,
}

/// Represents a shader entry point.
pub struct EntryPoint<'a> {
    /// Shader stage.
    pub stage: Stage,
    /// Name of the entry point in SPIR-V code.
    pub name: Cow<'a, str>,
    /// Path to the source code for the shader.
    pub source_path: Option<Cow<'a, str>>,
    /// SPIR-V code for the entry point.
    pub code: Cow<'a, [u8]>,
    /// Size of the push constants in bytes.
    pub push_constants_size: u32,
    /// Size of the local workgroup in each dimension, if applicable to the shader type.
    ///
    /// This is valid for compute, task, and mesh shaders.
    pub workgroup_size: (u32, u32, u32),
}


// include generated bindings by the build script
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

