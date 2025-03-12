use std::io;
use std::io::Read;
use crate::error::{invalid_data, Error};
use crate::Result;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PodType {
    Bool = 0,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F16,
    F32,
    F64,
    String,
    WideString,
}

impl PodType {
    pub fn byte_size(self) -> usize {
        match self {
            Self::Bool => 1,
            Self::U8 => 1,
            Self::I8 => 1,
            Self::U16 => 2,
            Self::I16 => 2,
            Self::U32 => 4,
            Self::I32 => 4,
            Self::U64 => 8,
            Self::I64 => 8,
            Self::F16 => 2,
            Self::F32 => 4,
            Self::F64 => 8,
            Self::String => 0,
            Self::WideString => 0,
        }
    }
}

impl TryFrom<u32> for PodType {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::Bool),
            1 => Ok(Self::U8),
            2 => Ok(Self::I8),
            3 => Ok(Self::U16),
            4 => Ok(Self::I16),
            5 => Ok(Self::U32),
            6 => Ok(Self::I32),
            7 => Ok(Self::U64),
            8 => Ok(Self::I64),
            9 => Ok(Self::F16),
            10 => Ok(Self::F32),
            11 => Ok(Self::F64),
            12 => Ok(Self::String),
            13 => Ok(Self::WideString),
            _ => Err(invalid_data("invalid PodType")),
        }
    }
}

/// Trait implemented by scalar sample data types.
pub unsafe trait DataType {
    /// The equivalent type in the Alembic file format.
    const ELEMENT_TYPE: PodType;

    /// Extent of the data type. `None` for variable-length data types.
    const EXTENT: Option<usize>;

    fn from_bytes(data: &[u8]) -> Result<Self>
    where
        Self: Sized;
}

/*
#[repr(C)]
pub struct Box3D {
    pub min: [f64; 3],
    pub max: [f64; 3],
}*/

macro_rules! impl_data_type {
    ($ty:ty, $variant:ident) => {
        unsafe impl DataType for $ty {
            const ELEMENT_TYPE: PodType = PodType::$variant;
            const EXTENT: Option<usize> = Some(1);

            fn from_bytes(data: &[u8]) -> Result<Self> {
                let mut value = [0; size_of::<Self>()];
                let mut reader = io::Cursor::new(data);
                reader.read_exact(&mut value)?;
                Ok(Self::from_le_bytes(value))
            }
        }
    };
}

impl_data_type!(u8, U8);
impl_data_type!(i8, I8);
impl_data_type!(u16, U16);
impl_data_type!(i16, I16);
impl_data_type!(u32, U32);
impl_data_type!(i32, I32);
impl_data_type!(u64, U64);
impl_data_type!(i64, I64);
impl_data_type!(f32, F32);
impl_data_type!(f64, F64);

unsafe impl DataType for bool {
    const ELEMENT_TYPE: PodType = PodType::Bool;
    const EXTENT: Option<usize> = Some(1);

    fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 1 {
            return Err(invalid_data("data too short"));
        }
        Ok(data[0] != 0)
    }
}

unsafe impl<T: DataType + Copy + Default, const N: usize> DataType for [T; N] {
    const ELEMENT_TYPE: PodType = T::ELEMENT_TYPE;
    const EXTENT: Option<usize> = Some(N);

    fn from_bytes(data: &[u8]) -> Result<Self> {
        let sz = size_of::<T>();
        let mut array = [T::default(); N];
        for i in 0..N {
            array[i] = T::from_bytes(&data[i * sz..(i + 1) * sz])?;
        }
        Ok(array)
    }
}
