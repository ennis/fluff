//! Data structures and constants shared between shaders and application code.
//!
//! **NOTE**: This file is read by the `shader-bridge` tool to generate GLSL code.
//! Only define types and constants using primitives types and types defined in [`super::types`].
//! See `build.rs` and the documentation of the `shader-bridge` crate for more information.
use glam::{Mat4, UVec2, Vec2, Vec4};

use super::types::*;

/// 3D bezier control point.
#[repr(C)]
pub struct ControlPoint {
    /// Position.
    pub pos: [f32; 3],
    /// RGB color.
    pub color: [f32; 3],
}

/// Represents a range of control points in the position buffer.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct CurveDesc {
    /// Width profile (polynomial coefficients).
    pub width_profile: Vec4,
    /// Opacity profile (polynomial coefficients).
    pub opacity_profile: Vec4,
    pub start: u32,
    /// Number of control points in the range.
    ///
    /// Should be 3N+1 for cubic Bézier curves.
    pub count: u32,
    /// parameter range
    pub param_range: Vec2,
}

/// Maximum number of line segments per tile.
pub const MAX_LINES_PER_TILE: usize = 64;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct TileLineData {
    pub coords: Vec4,
    pub param_range: Vec2,
    pub curve_index: u32,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct TileData {
    pub lines: [TileLineData; MAX_LINES_PER_TILE],
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct BinCurvesParams {
    pub control_points: BufferAddress<[ControlPoint]>,
    pub curves: BufferAddress<[CurveDesc]>,
    pub tile_line_count: BufferAddress<[u32]>,
    pub tile_data: BufferAddress<[TileData]>,
    pub view_projection_matrix: Mat4,
    pub viewport_size: UVec2,
    pub stroke_width: f32,
    pub base_curve_index: u32,
    pub curve_count: u32,
    pub tile_count_x: u32,
    pub tile_count_y: u32,
    pub frame: u32,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct TemporalAverageParams {
    pub viewport_size: UVec2,
    pub frame: u32,
    pub falloff: f32,
    pub new_frame: ImageHandle,
    pub avg_frame: ImageHandle,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct ComputeTestParams {
    pub element_count: u32,
    pub data: BufferAddress<[TileData]>,
    pub control_points: BufferAddress<[ControlPoint]>,
    pub output_image: ImageHandle,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct DrawCurvesPushConstants {
    pub view_proj: Mat4,
    /// Base index into the curve buffer.
    pub base_curve: u32,
    pub stroke_width: f32,
    /// Number of tiles in the X direction.
    pub tile_count_x: u32,
    /// Number of tiles in the Y direction.
    pub tile_count_y: u32,
    pub frame: u32,
    pub tile_data: BufferAddress<[TileData]>,
    pub tile_line_count: BufferAddress<[u32]>,
    pub output_image: ImageHandle,
}

pub const BINNING_TILE_SIZE: u32 = 16;
pub const BINNING_TASK_WORKGROUP_SIZE: u32 = 64;
pub const MAX_VERTICES_PER_CURVE: u32 = 64;
