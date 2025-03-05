//! Stuff related to strokes.

mod polymesh;

use crate::overlay::CubicBezierSegment;
use crate::shaders::{ControlPoint, CurveDesc, Stroke, StrokeVertex};
use crate::util::{lagrange_interpolate_4, AppendBuffer};
use alembic_ogawa::geom::{GeomParam, GeometryScope, MeshTopologyVariance, PolyMesh, XForm};
use alembic_ogawa::{DataType, ObjectReader, TypedArrayPropertyReader};
use anyhow::bail;
use glam::{vec2, DVec4, Vec3};
use graal::util::DeviceExt;
use graal::{vk, Buffer, BufferUntyped, BufferUsage, Device, MemoryLocation};
use houdinio::Geo;
use std::mem::MaybeUninit;
use std::path::Path;
use std::ptr::read;
use tracing::trace;

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

/// Vertex attribute of a 3D mesh.
///
/// Attributes can be animated over time.
struct Attribute {
    /// Time position of each animation frame of the attributes.
    ///
    /// The length of this vector corresponds to the number of animation frames of the mesh.
    /// If the mesh is not animated, this vector will contain a single element.
    time_samples: Vec<f64>,

    /// Name of the attribute.
    name: String,
    /// Attribute index in shader.
    index: u32,
    /// Attribute data on the GPU.
    device_data: BufferUntyped,
}

/// An animated 3D mesh.
pub struct Mesh3D {
    /// Number of vertices.
    vertex_count: u32,

    /// Attributes by index.
    ///
    /// The first attribute is always the position.
    attributes: Vec<Attribute>,

    /// Index buffer.
    ///
    /// Contains indices for each vertex. I.e: `[ [I_0, ... I_n], [I_0, ... I_n], ... ]` where n is
    /// the number of attributes.
    indices: graal::BufferUntyped,
}

fn triangulate_indices(face_counts: &[i32], indices: &[i32], output: &mut [MaybeUninit<u32>]) {
    let mut ii = 0;
    let mut oi = 0;
    for face_count in face_counts.iter() {
        let base_index = indices[ii];
        for j in (ii + 2)..(ii + *face_count as usize) {
            let a = indices[j - 1];
            let b = indices[j];
            output[oi].write(base_index as u32);
            output[oi + 1].write(a as u32);
            output[oi + 2].write(b as u32);
            oi += 3;
        }
        ii += *face_count as usize;
    }
}

fn read_attribute_samples<T: DataType + Copy>(
    device: &Device,
    name: &str,
    expected_count: usize,
    attribute: &TypedArrayPropertyReader<T>,
) -> Result<Attribute, anyhow::Error> {
    let sample_count = attribute.sample_count();
    let mut buffer = device.create_array_buffer::<T>(
        BufferUsage::STORAGE_BUFFER,
        MemoryLocation::CpuToGpu,
        sample_count * expected_count,
    );

    // SAFETY: TODO
    let slice = unsafe { buffer.as_mut_slice() };
    let mut time_samples = vec![];
    for i in 0..sample_count {
        assert_eq!(&attribute.dimensions(i)[..], &[expected_count]);
        time_samples.push(attribute.time_sampling().get_sample_time(i).unwrap());
        attribute.read_sample_into(i, &mut slice[i * expected_count..])?;
    }

    Ok(Attribute {
        time_samples,
        name: name.to_string(),
        index: 0,
        device_data: buffer.untyped,
    })
}

fn read_geom_param<T: DataType + Copy>(
    device: &Device,
    name: &str,
    face_vertex_count: usize,
    gp: &GeomParam<T>,
) -> Result<Attribute, anyhow::Error> {
    // number of elements
    let elem_count = match gp.scope {
        GeometryScope::Constant => 1,
        GeometryScope::Uniform => 1,
        GeometryScope::Varying => face_vertex_count,
        GeometryScope::Vertex => face_vertex_count,
        GeometryScope::FaceVarying => face_vertex_count,
        GeometryScope::Unknown => {
            return Err(anyhow::anyhow!("unknown geometry scope"));
        }
    };
    assert_eq!(gp.values.dimensions(0)[0], elem_count);

    // number of unique samples
    let unique_sample_count = if gp.is_indexed() {
        eprintln!("indexed geom param: {} unique samples", gp.indices.as_ref().unwrap().dimensions(0)[0]);
        gp.indices.as_ref().unwrap().dimensions(0)[0]
    } else {
        eprintln!("non-indexed geom param: {} samples", gp.values.sample_count());
        gp.values.sample_count()
    };

    let mut buffer = device.create_array_buffer::<T>(
        BufferUsage::STORAGE_BUFFER,
        MemoryLocation::CpuToGpu,
        unique_sample_count * elem_count,
    );

    // SAFETY: TODO
    let slice = unsafe { buffer.as_mut_slice() };
    let mut time_samples = vec![];
    for i in 0..unique_sample_count {
        assert_eq!(&gp.values.dimensions(i)[..], &[elem_count]);
        time_samples.push(gp.values.time_sampling().get_sample_time(i).unwrap());
        gp.values.read_sample_into(i, &mut slice[i * elem_count..])?;
    }

    Ok(Attribute {
        time_samples,
        name: name.to_string(),
        index: 0,
        device_data: buffer.untyped,
    })
}

/*// check that the topology is actually heterogeneous
            let face_counts = mesh.face_counts.get(0).unwrap().values;
            for i in 1..mesh.face_counts.sample_count() {
                let other_face_counts = mesh.face_counts.get(i).unwrap().values;
                if face_counts != other_face_counts {
                    return Err(anyhow::anyhow!("unsupported topology variance"));
                }
            }

            let face_indices = mesh.face_indices.get(0).unwrap().values;
            for i in 1..mesh.face_indices.sample_count() {
                let other_face_indices = mesh.face_indices.get(i).unwrap().values;
                if face_indices != other_face_indices {
                    return Err(anyhow::anyhow!("unsupported topology variance"));
                }
            }

            eprintln!(
                "*** topology is declared as heterogeneous, but is actually homogeneous ({} {}) ****",
                mesh.face_counts.sample_count(),
                mesh.face_indices.sample_count()
            );*/

impl Mesh3D {
    fn from_alembic(device: &Device, mesh: &PolyMesh) -> Result<Mesh3D, anyhow::Error> {
        if !matches!(
            mesh.topology_variance(),
            MeshTopologyVariance::Constant | MeshTopologyVariance::Homogeneous
        ) {
            bail!("unsupported topology variance");
        }

        if mesh.face_counts.sample_count() == 0 || mesh.face_indices.sample_count() == 0 {
            bail!("empty mesh (no samples)");
        }

        // triangulate indices
        let face_counts = mesh.face_counts.get(0).unwrap().values;
        let indices = mesh.face_indices.get(0).unwrap().values;
        if face_counts.is_empty() || indices.is_empty() {
            bail!("empty mesh");
        }

        let index_count = indices.len();
        let triangle_count = face_counts.iter().map(|&count| (count - 2) as usize).sum::<usize>();
        let triangulated_index_count = triangle_count * 3;
        let mut index_buffer = device.create_array_buffer::<u32>(
            BufferUsage::INDEX_BUFFER,
            MemoryLocation::CpuToGpu,
            triangulated_index_count,
        );
        //eprintln!("loading mesh: faces: {}, indices: {}", face_counts.len(), indices.len());
        triangulate_indices(&face_counts, &indices, unsafe { index_buffer.as_mut_slice() });

        // load attribute samples (positions, etc.)

        // load attribute samples
        // SAFETY: we allocate enough space in attribute buffers to hold all samples
        let mut attributes = vec![];
        let vertex_count = mesh.positions.dimensions(0)[0];
        attributes.push(read_attribute_samples(device, "P", vertex_count, &mesh.positions)?);
        if let Some(normals) = &mesh.normals {
            attributes.push(read_geom_param(device, "N", index_count, normals)?);
        }
        if let Some(uvs) = &mesh.uvs {
            attributes.push(read_geom_param(device, "uv", index_count, uvs)?);
        }
        if let Some(velocities) = &mesh.velocities {
            attributes.push(read_attribute_samples(device, "velocities", vertex_count, velocities)?);
        }

        Ok(Mesh3D {
            vertex_count: vertex_count as u32,
            attributes,
            indices: index_buffer.untyped,
        })
    }
}

struct TimeSample<T> {
    time: f64,
    value: T,
}

/// 3D scene object
struct Object {
    name: String,
    /// Transform relative to parent.
    transform: Vec<TimeSample<glam::DMat4>>,
    /// Geometry data.
    geometry: Option<Mesh3D>,
    /// Children objects.
    children: Vec<Object>,
}

impl Object {
    fn load_alembic_object_recursive(device: &Device, obj: &ObjectReader) -> Result<Object, anyhow::Error> {
        let mut transform = vec![];
        match XForm::new(obj.properties(), ".xform") {
            Ok(xform) => {
                for (time, sample) in xform.samples() {
                    transform.push(TimeSample {
                        time,
                        value: glam::DMat4::from_cols_array_2d(&sample),
                    });
                }
            }
            Err(err) => {}
        }

        let mesh = if let Ok(mesh) = PolyMesh::new(obj.properties(), ".geom") {
            match Mesh3D::from_alembic(device, &mesh) {
                Ok(mesh) => {
                    eprintln!("loaded mesh: {}", obj.name());
                    Some(mesh)
                }
                Err(err) => {
                    eprintln!("failed to load mesh `{}`: {}", obj.path(), err);
                    None
                }
            }
        } else {
            None
        };

        let mut children = vec![];
        for child in obj.children() {
            children.push(Self::load_alembic_object_recursive(device, &child)?);
        }

        //eprintln!("loaded object: {}", obj.name());

        Ok(Object {
            name: obj.name().to_string(),
            transform,
            geometry: mesh,
            children,
        })
    }
}

/// 3D scene.
pub struct Scene3D {
    root: Object,
}

impl Scene3D {
    fn load_from_alembic_inner(device: &Device, path: &Path) -> Result<Self, anyhow::Error> {
        let archive = alembic_ogawa::Archive::open(path)?;
        let root = Object::load_alembic_object_recursive(device, &archive.root()?)?;
        Ok(Self { root })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_alembic() {
        let path = Path::new("../alembic-ogawa/tests/data/ellie_animation.abc");
        let (device, _) = unsafe { graal::create_device_and_command_stream_with_surface(None).unwrap() };
        let scene = Scene3D::load_from_alembic_inner(&device, path).unwrap();
    }
}
