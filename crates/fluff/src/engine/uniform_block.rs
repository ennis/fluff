//! Uniform data blocks to be sent to the GPU.
use std::collections::BTreeMap;
use bytemuck::cast_slice;
use crate::engine::Error;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum UniformType {
    I32,
    U32,
    F32,
    Vec2,
    Vec3,
    Vec4,
    Mat2,
    Mat3,
    Mat4,
    Texture2DHandle,
    SamplerHandle,
    ImageHandle,
    DeviceAddress,
}

#[derive(Copy, Clone, Debug)]
pub(super) enum UniformValue {
    I32(i32),
    U32(u32),
    F32(f32),
    UVec2([u32; 2]),
    UVec3([u32; 3]),
    UVec4([u32; 4]),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat2([[f32; 2]; 2]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
    Texture2DHandle(u32),
    SamplerHandle(u32),
    ImageHandle(u32),
    DeviceAddress(u64),
}

impl From<i32> for UniformValue {
    fn from(value: i32) -> Self {
        UniformValue::I32(value)
    }
}

impl From<u32> for UniformValue {
    fn from(value: u32) -> Self {
        UniformValue::U32(value)
    }
}

impl From<f32> for UniformValue {
    fn from(value: f32) -> Self {
        UniformValue::F32(value)
    }
}

impl From<[f32; 2]> for UniformValue {
    fn from(value: [f32; 2]) -> Self {
        UniformValue::Vec2(value)
    }
}

impl From<[f32; 3]> for UniformValue {
    fn from(value: [f32; 3]) -> Self {
        UniformValue::Vec3(value)
    }
}

impl From<[f32; 4]> for UniformValue {
    fn from(value: [f32; 4]) -> Self {
        UniformValue::Vec4(value)
    }
}

impl From<[[f32; 2]; 2]> for UniformValue {
    fn from(value: [[f32; 2]; 2]) -> Self {
        UniformValue::Mat2(value)
    }
}

impl From<[[f32; 3]; 3]> for UniformValue {
    fn from(value: [[f32; 3]; 3]) -> Self {
        UniformValue::Mat3(value)
    }
}

impl From<[[f32; 4]; 4]> for UniformValue {
    fn from(value: [[f32; 4]; 4]) -> Self {
        UniformValue::Mat4(value)
    }
}

impl From<[u32; 2]> for UniformValue {
    fn from(value: [u32; 2]) -> Self {
        UniformValue::UVec2(value)
    }
}

impl From<[u32; 3]> for UniformValue {
    fn from(value: [u32; 3]) -> Self {
        UniformValue::UVec3(value)
    }
}

impl From<[u32; 4]> for UniformValue {
    fn from(value: [u32; 4]) -> Self {
        UniformValue::UVec4(value)
    }
}

impl From<glam::Mat4> for UniformValue {
    fn from(value: glam::Mat4) -> Self {
        UniformValue::Mat4(value.to_cols_array_2d())
    }
}

/// Contents of a constants (uniform) buffer, with names mapped to offsets and sizes.
#[derive(Default)]
pub(super) struct UniformBlock {
    fields: BTreeMap<String, (u32, UniformType)>,
    data: Vec<u8>,
}

impl UniformBlock {
    pub(super) fn new(size: usize, fields: impl IntoIterator<Item=(String, (u32, UniformType))>) -> Self {
        Self {
            fields: BTreeMap::from_iter(fields),
            data: vec![0; size],
        }
    }

    pub(super) fn data(&self) -> &[u8] {
        &self.data
    }

    pub(super) fn set(&mut self, name: &str, value: impl Into<UniformValue>) -> Result<(), Error> {
        self.set_inner(name, value.into())
    }

    fn set_inner(&mut self, name: &str, value: UniformValue) -> Result<(), Error> {
        let (offset, ty) = *self.fields.get(name).ok_or(Error::UnknownField(name.to_string()))?;
        let data = &mut self.data;
        let offset = offset as usize;

        match (ty, value) {
            (UniformType::U32, UniformValue::U32(value)) => {
                let bytes = value.to_ne_bytes();
                data[offset..offset + 4].copy_from_slice(&bytes);
            }
            (UniformType::I32, UniformValue::I32(value)) => {
                let bytes = value.to_ne_bytes();
                data[offset..offset + 4].copy_from_slice(&bytes);
            }
            (UniformType::F32, UniformValue::F32(value)) => {
                // Vulkan expects values in the same byte order as the host; that's in the spec,
                // apparently. So we don't need to do anything special here.
                let bytes = value.to_ne_bytes();
                data[offset..offset + 4].copy_from_slice(&bytes);
            }
            (UniformType::Vec2, UniformValue::Vec2(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 8].copy_from_slice(bytes);
            }
            (UniformType::Vec3, UniformValue::Vec3(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 12].copy_from_slice(bytes);
            }
            (UniformType::Vec4, UniformValue::Vec4(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 16].copy_from_slice(bytes);
            }
            (UniformType::Mat2, UniformValue::Mat2(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 16].copy_from_slice(bytes);
            }
            (UniformType::Mat3, UniformValue::Mat3(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 36].copy_from_slice(bytes);
            }
            (UniformType::Mat4, UniformValue::Mat4(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 64].copy_from_slice(bytes);
            }
            (UniformType::DeviceAddress, UniformValue::DeviceAddress(value)) => {
                let bytes = value.to_ne_bytes();
                data[offset..offset + 8].copy_from_slice(&bytes);
            }
            _ => {
                return Err(Error::InvalidFieldType(name.to_string()));
            }
        }

        Ok(())
    }
}
