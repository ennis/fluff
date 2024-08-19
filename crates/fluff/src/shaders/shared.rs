//! Data structures and constants shared between shaders and application code.
//!
//! **NOTE**: This file is read by the `shader-bridge` tool to generate GLSL code.
//! Only define types and constants using primitives types and types defined in [`super::types`].
//! See `build.rs` and the documentation of the `shader-bridge` crate for more information.
use glam::{Mat4, UVec2, Vec2, Vec4};

use super::types::*;

/// Scene camera parameters.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct SceneParams {
    /// View matrix.
    pub view: Mat4,
    /// Projection matrix.
    pub proj: Mat4,
    /// View-projection matrix.
    pub view_proj: Mat4,
    /// Position of the camera in world space.
    pub eye: Vec3,
    /// Near clip plane position in view space.
    pub near_clip: f32,
    /// far clip plane position in view space.
    pub far_clip: f32,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub viewport_size: UVec2,
    pub cursor_pos: Vec2,
    pub time: f32,
}

/// 3D bezier control point.
#[derive(Copy, Clone)]
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
    pub width_profile: [f32; 4],
    /// Opacity profile (polynomial coefficients).
    pub opacity_profile: [f32; 4],
    pub start: u32,
    /// Number of control points in the range.
    ///
    /// Should be 3N+1 for cubic Bézier curves.
    pub count: u32,
    /// parameter range
    pub param_range: Vec2,
    pub brush_index: u32,
}

/// Stroke vertex.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct StrokeVertex {
    pub pos: [f32; 3],
    /// Arc length along the curve.
    pub s: f32,
    pub color: [u8; 4],
    pub width: u8,  // unorm8
    pub opacity: u8,  // unorm8
}

/// Stroke vertex.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Stroke {
    pub base_vertex: u32,
    pub vertex_count: u32,
    pub brush: u8,
    pub arc_length: f32,
}


/// Maximum number of line segments per tile.
pub const MAX_LINES_PER_TILE: usize = 32;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct TileLineData {
    pub line_coords: Vec4,
    pub param_range: Vec2,
    pub curve_id: u32,
    pub depth: f32,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct TileData {
    pub lines: [TileLineData; MAX_LINES_PER_TILE],
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct BinCurvesParams {
    pub control_points: DeviceAddress<[ControlPoint]>,
    pub curves: DeviceAddress<[CurveDesc]>,
    pub tile_line_count: DeviceAddress<[u32]>,
    pub tile_data: DeviceAddress<[TileData]>,
    pub scene_params: DeviceAddress<SceneParams>,
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
    pub data: DeviceAddress<[TileData]>,
    pub control_points: DeviceAddress<[ControlPoint]>,
    pub output_image: ImageHandle,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct DrawCurvesPushConstants {
    pub control_points: DeviceAddress<[ControlPoint]>,
    pub curves: DeviceAddress<[CurveDesc]>,
    pub scene_params: DeviceAddress<SceneParams>,
    //pub view_proj: Mat4,
    /// Base index into the curve buffer.
    pub base_curve_index: u32,
    pub stroke_width: f32,
    /// Number of tiles in the X direction.
    pub tile_count_x: u32,
    /// Number of tiles in the Y direction.
    pub tile_count_y: u32,
    pub frame: u32,
    pub tile_data: DeviceAddress<[TileData]>,
    pub tile_line_count: DeviceAddress<[u32]>,
    pub brush_textures: DeviceAddress<[ImageHandle]>,
    pub output_image: ImageHandle,
    pub debug_overflow: u32,
    pub stroke_bleed_exp: f32,
}


pub const BINNING_TILE_SIZE: u32 = 16;
pub const DRAW_CURVES_WORKGROUP_SIZE_X: u32 = 16;
pub const DRAW_CURVES_WORKGROUP_SIZE_Y: u32 = 2;

pub const BINPACK_SUBGROUP_SIZE: u32 = 32;
pub const SUBGROUP_SIZE: u32 = 32;
pub const MAX_VERTICES_PER_CURVE: u32 = 64;


#[derive(Copy, Clone)]
#[repr(C)]
pub struct SummedAreaTableParams {
    pub pass: u32,      // 0: horizontal, 1: vertical
    pub input_image: ImageHandle,
    pub output_image: ImageHandle,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct Particle {
    /// Position relative to cluster. 16-bit fixed point.
    pub pos: [u16; 3],
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct ParticleCluster {
    pub pos: [f32; 3],
    pub size: f32,
    pub count: u32,
}


#[derive(Copy, Clone)]
#[repr(C)]
pub struct DrawStrokesPushConstants {
    pub vertices: DeviceAddress<[StrokeVertex]>,
    pub strokes: DeviceAddress<[Stroke]>,
    pub scene_params: DeviceAddress<SceneParams>,
    pub brush_textures: DeviceAddress<[ImageHandle]>,
    pub stroke_count: u32,
    pub width: f32,
    pub filter_width: f32,
    pub brush: u32,
}



