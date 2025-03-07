use glam::{vec2, DVec4, Vec3};
use graal::{BufferUsage, Device, MemoryLocation};
use houdinio::Geo;
use crate::overlay::CubicBezierSegment;
use crate::shaders::{ControlPoint, CurveDesc, Stroke, StrokeVertex};
use crate::util::{lagrange_interpolate_4, AppendBuffer};

/// Represents a range of curves in the curve buffer.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct CurveRange {
    pub start: u32,
    pub count: u32,
}

/// Information about a single animation frame.
#[derive(Debug)]
pub struct AnimationFrame {
    /// Time of the frame in seconds.
    pub time: f32,
    /// Range of curves in the curve buffer.
    pub curve_range: CurveRange,
    /// Curve segments
    pub curve_segments: Vec<CubicBezierSegment>,
    pub stroke_offset: u32,
    pub stroke_count: u32,
}

pub struct Mesh {
    transform: glam::Mat4,
    start_vertex: u32,
    vertex_count: u32,
    start_index: u32,
    index_count: u32,
}

/// Scene data.
///
/// Holds the animation frames, and the buffers for strokes & curves for the entire animation.
pub struct Scene {
    //point_count: usize,
    //curve_count: usize,
    pub frames: Vec<AnimationFrame>,
    pub position_buffer: AppendBuffer<ControlPoint>,
    pub curve_buffer: AppendBuffer<CurveDesc>,
    pub stroke_vertex_buffer: AppendBuffer<StrokeVertex>,
    pub stroke_buffer: AppendBuffer<Stroke>,
}

/// Converts Bézier curve data from `.geo` files to a format that can be uploaded to the GPU.
///
/// Curves are represented as follows:
/// * position buffer: contains the control points of curves, all flattened into a single linear buffer.
/// * curve buffer: consists of (start, size) pairs, defining the start and number of CPs of each curve in the position buffer.
/// * animation buffer: consists of (start, size) defining the start and number of curves in the curve buffer for each animation frame.
pub fn load_stroke_animation_data(device: &Device, geo_files: &[Geo]) -> Scene {
    let mut point_count = 0;
    let mut curve_count = 0;

    // Count the number of curves and control points
    for f in geo_files.iter() {
        for prim in f.primitives.iter() {
            match prim {
                houdinio::Primitive::BezierRun(run) => match run.vertices {
                    houdinio::PrimVar::Uniform(ref u) => {
                        point_count += u.len() * run.count;
                        curve_count += (u.len() / 3) * run.count;
                    }
                    houdinio::PrimVar::Varying(ref v) => {
                        point_count += v.iter().map(|v| v.len()).sum::<usize>();
                        curve_count += v.iter().map(|v| v.len() / 3).sum::<usize>();
                    }
                },
            }
        }
    }

    // Curve buffer: contains (start, end) pairs of curves in the point buffer

    let mut position_buffer = AppendBuffer::with_capacity(
        device,
        BufferUsage::STORAGE_BUFFER,
        MemoryLocation::CpuToGpu,
        point_count,
    );
    position_buffer.set_name("control point buffer");
    let mut curve_buffer = AppendBuffer::with_capacity(
        device,
        BufferUsage::STORAGE_BUFFER,
        MemoryLocation::CpuToGpu,
        curve_count,
    );
    curve_buffer.set_name("curve buffer");

    let mut stroke_vertex_buffer = AppendBuffer::new(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu);
    stroke_vertex_buffer.set_name("stroke vertex buffer");
    let mut stroke_buffer = AppendBuffer::new(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu);
    stroke_buffer.set_name("stroke buffer");

    let mut frames = vec![];

    // dummy width and opacity profiles
    let width_profile = DVec4::from(lagrange_interpolate_4([0.0, 0.0], [0.2, 0.8], [0.5, 0.8], [1.0, 0.0])).as_vec4();
    let opacity_profile = DVec4::from(lagrange_interpolate_4([0.0, 0.7], [0.3, 1.0], [0.6, 1.0], [1.0, 0.0])).as_vec4();

    // write curves
    unsafe {
        let point_data: *mut ControlPoint = position_buffer.as_mut_ptr();
        let mut point_ptr = 0;
        let curve_data: *mut CurveDesc = curve_buffer.as_mut_ptr();
        let mut curve_ptr = 0;

        for f in geo_files.iter() {
            let offset = curve_ptr;

            let mut curve_segments = vec![];
            for prim in f.primitives.iter() {
                match prim {
                    houdinio::Primitive::BezierRun(run) => {
                        for curve in run.iter() {
                            let start = point_ptr;
                            for &vertex_index in curve.vertices.iter() {
                                let pos = f.vertex_position(vertex_index);
                                let color = f.vertex_color(vertex_index).unwrap_or([0.1, 0.8, 0.1]);
                                *point_data.offset(point_ptr) = ControlPoint {
                                    pos: pos.into(),
                                    color: color.into(),
                                };
                                point_ptr += 1;
                            }
                            // FIXME: this is wrong
                            for segment in curve.vertices.windows(4) {
                                curve_segments.push(CubicBezierSegment {
                                    p0: f.vertex_position(segment[0]).into(),
                                    p1: f.vertex_position(segment[1]).into(),
                                    p2: f.vertex_position(segment[2]).into(),
                                    p3: f.vertex_position(segment[3]).into(),
                                });
                            }

                            let num_segments = curve.vertices.len() as u32 / 3;
                            let num_segments_f = num_segments as f32;
                            for i in 0..num_segments {
                                *curve_data.offset(curve_ptr) = CurveDesc {
                                    start: start as u32 + 3 * i,
                                    count: 4,
                                    /*curve.vertices.len() as u32*/
                                    width_profile: width_profile.to_array(),
                                    opacity_profile: opacity_profile.to_array(),
                                    param_range: vec2(i as f32 / num_segments_f, (i + 1) as f32 / num_segments_f),
                                    brush_index: 0,
                                    //_dummy: [0; 3],
                                };
                                curve_ptr += 1;
                            }
                        }
                    }
                }
            }

            // flatten curves to polylines
            let stroke_offset = stroke_buffer.len() as u32;
            for prim in f.primitives.iter() {
                match prim {
                    houdinio::Primitive::BezierRun(run) => {
                        for curve in run.iter() {
                            let mut vertices = vec![];
                            let mut color = [1.0, 1.0, 1.0];
                            let base_vertex = stroke_vertex_buffer.len() as u32;
                            let mut control_points = vec![];
                            for &vertex_index in curve.vertices.iter() {
                                let pos = f.vertex_position(vertex_index);
                                color = f.vertex_color(vertex_index).unwrap_or([0.1, 0.8, 0.1]);
                                control_points.push(Vec3::from(pos));
                            }

                            let mut i = 0;
                            while i + 3 < control_points.len() {
                                let segment = CubicBezierSegment {
                                    p0: control_points[i],
                                    p1: control_points[i + 1],
                                    p2: control_points[i + 2],
                                    p3: control_points[i + 3],
                                };
                                segment.flatten(&mut vertices, 0.0001);
                                i += 3;
                            }

                            let mut s = 0.0;
                            for (i, v) in vertices.iter().enumerate() {
                                stroke_vertex_buffer.push(StrokeVertex {
                                    pos: (*v).into(),
                                    s,
                                    color: [
                                        (color[0] * 255.0) as u8,
                                        (color[1] * 255.0) as u8,
                                        (color[2] * 255.0) as u8,
                                        255,
                                    ],
                                    width: 255,
                                    opacity: 255,
                                });
                                if i != vertices.len() - 1 {
                                    s += v.distance(vertices[i + 1]);
                                }
                            }

                            stroke_buffer.push(Stroke {
                                base_vertex,
                                vertex_count: vertices.len() as u32,
                                brush: 0,
                                arc_length: s,
                            });
                        }
                    }
                }
            }

            frames.push(AnimationFrame {
                time: 0.0, // TODO
                curve_range: CurveRange {
                    start: offset as u32,
                    count: curve_ptr as u32 - offset as u32,
                },
                curve_segments,
                stroke_offset,
                stroke_count: stroke_buffer.len() as u32 - stroke_offset,
            });
        }
        position_buffer.set_len(point_count);
        curve_buffer.set_len(curve_count);
    }

    Scene {
        //point_count,
        //curve_count,
        frames,
        position_buffer,
        curve_buffer,
        stroke_vertex_buffer,
        stroke_buffer,
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
