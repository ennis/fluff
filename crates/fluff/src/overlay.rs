//! Scene overlay drawing utilities
use std::{f32::consts::TAU, mem, path::Path};

use glam::{vec3, Mat4, Vec3};
use graal::{prelude::*, BufferRange};

use crate::camera_control::Camera;

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Copy, Clone, Debug)]
pub struct CubicBezierSegment {
    pub p0: Vec3,
    pub p1: Vec3,
    pub p2: Vec3,
    pub p3: Vec3,
}

fn point_line_dist(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let d = (p - a).dot(ab) / ab.dot(ab);
    //d = clamp(d, 0.0, 1.0);
    let p0 = a + d * ab;
    (p - p0).length()
}

impl CubicBezierSegment {
    fn subdivide(&self, t: f32) -> (Self, Self) {
        let q0 = self.p0.lerp(self.p1, t);
        let q1 = self.p1.lerp(self.p2, t);
        let q2 = self.p2.lerp(self.p3, t);
        let r0 = q0.lerp(q1, t);
        let r1 = q1.lerp(q2, t);
        let p = r0.lerp(r1, t);

        (
            Self {
                p0: self.p0,
                p3: p,
                p1: q0,
                p2: r0,
            },
            Self {
                p0: p,
                p3: self.p3,
                p1: r1,
                p2: q2,
            },
        )
    }

    fn is_flat(&self, tolerance: f32) -> bool {
        let p0 = self.p0;
        let p1 = self.p1;
        let p2 = self.p2;
        let p3 = self.p3;
        let t = tolerance * tolerance;
        (0.5 * (p0 + p2) - p1).length_squared() <= t && (0.5 * (p1 + p3) - p2).length_squared() <= t
    }

    fn flatten_inner(&self, points: &mut Vec<Vec3>, tolerance: f32) {
        //points.push(self.start);
        if self.is_flat(tolerance) {
            points.push(self.p3);
        } else {
            let (a, b) = self.subdivide(0.5);
            a.flatten_inner(points, tolerance);
            b.flatten_inner(points, tolerance);
        }
    }

    fn flatten(&self, points: &mut Vec<Vec3>, tolerance: f32) -> Vec<Vec3> {
        points.clear();
        points.push(self.p0);
        self.flatten_inner(points, tolerance);
        points.clone()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Copy, Clone, Vertex, Default)]
#[repr(C)]
struct OverlayVertex {
    position: [f32; 3],
    color: [u8; 4],
}

impl OverlayVertex {
    fn new(position: Vec3, color: [u8; 4]) -> Self {
        Self {
            position: position.to_array(),
            color,
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct PushConstants {
    matrix: Mat4,
    line_count: u32,
    width: f32,
    filter_width: f32,
    screen_width: f32,
    screen_height: f32,
}

struct Draw {
    topology: PrimitiveTopology,
    transform: Mat4,
    kind: DrawKind,
}

enum DrawKind {
    Indexed {
        base_vertex: u32,
        start_index: u32,
        index_count: u32,
    },
    Draw {
        start_vertex: u32,
        vertex_count: u32,
    },
}

/*
#[derive(Attachments)]
struct OverlayAttachments<'a> {
    #[attachment(color, format=R8G8B8A8_UNORM)]
    color: &'a ImageView,
    #[attachment(depth, format=D32_SFLOAT)]
    depth: &'a ImageView,
}*/

#[derive(Copy, Clone, Default)]
#[repr(C)]
struct LineVertex {
    position: [f32; 3],
    color: [u8; 4],
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct Polyline {
    start_vertex: u32,
    vertex_count: u32,
}

/*

// Line vertex buffer
layout(std430,set=0,binding=0) buffer PositionBuffer {
    float vertices[];
};

layout(std430,set=0,binding=1) buffer LineBuffer {
    Polyline polylines[];
};
*/

#[derive(Arguments)]
struct LineRenderingArguments<'a> {
    #[argument(binding = 0, storage, read_only)]
    vertex_buffer: BufferRange<'a, [LineVertex]>,
    #[argument(binding = 1, storage, read_only)]
    line_buffer: BufferRange<'a, [Polyline]>,
}

pub struct OverlayRenderer {
    pipeline: GraphicsPipeline,
    line_pipeline: GraphicsPipeline,

    camera: Camera,
    target_color_format: Format,
    target_depth_format: Format,
    vertices: Vec<OverlayVertex>,
    indices: Vec<u16>,
    draws: Vec<Draw>,

    // Line data
    line_vertices: Vec<LineVertex>,
    polylines: Vec<Polyline>,
}

impl OverlayRenderer {
    /// Creates a new overlay instance.
    ///
    /// # Arguments
    ///
    /// * `format` format of the output image
    pub fn new(device: &Device, target_color_format: Format, target_depth_format: Format) -> Self {
        Self {
            camera: Camera::default(),
            pipeline: compile_shaders(device, target_color_format, target_depth_format),
            line_pipeline: compile_line_shaders(device, target_color_format, target_depth_format),
            target_color_format,
            target_depth_format,
            vertices: vec![],
            indices: vec![],
            draws: vec![],
            line_vertices: vec![],
            polylines: vec![],
        }
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    pub fn line(&mut self, a: Vec3, b: Vec3, a_color: [u8; 4], b_color: [u8; 4]) {
        /*let start_vertex = self.vertices.len() as u32;
        self.vertices.push(OverlayVertex::new(a, a_color));
        self.vertices.push(OverlayVertex::new(b, b_color));
        self.draws.push(Draw {
            transform: Mat4::IDENTITY,
            topology: PrimitiveTopology::LineStrip,
            kind: DrawKind::Draw {
                start_vertex,
                vertex_count: 2,
            },
        })*/

        let start_vertex = self.line_vertices.len() as u32;
        self.line_vertices.push(LineVertex {
            position: a.to_array(),
            color: a_color,
        });
        self.line_vertices.push(LineVertex {
            position: b.to_array(),
            color: b_color,
        });
        self.polylines.push(Polyline {
            start_vertex,
            vertex_count: 2,
        });
    }

    pub fn cubic_bezier(&mut self, segment: &CubicBezierSegment, color: [u8; 4]) {
        let mut points = vec![];
        segment.flatten(&mut points, 0.0001);
        let start_vertex = self.line_vertices.len() as u32;
        for p in points.iter() {
            self.line_vertices.push(LineVertex {
                position: p.to_array(),
                color,
            });
        }
        self.polylines.push(Polyline {
            start_vertex,
            vertex_count: points.len() as u32,
        });
    }

    pub fn cylinder(&mut self, a: Vec3, b: Vec3, diameter: f32, a_color: [u8; 4], b_color: [u8; 4]) {
        const D: usize = 8;

        let base_vertex = self.vertices.len() as u32;
        let start_index = self.indices.len() as u32;
        let height = (b - a).length();

        for i in 0..D {
            let angle = (i as f32 / D as f32) * TAU;
            let x = angle.cos() * diameter;
            let y = angle.sin() * diameter;
            self.vertices.push(OverlayVertex::new(vec3(x, y, 0.0), a_color));
            self.vertices.push(OverlayVertex::new(vec3(x, y, height), b_color));
        }

        for i in 0..D {
            let a = i as u16 * 2;
            let b = ((i + 1) % D) as u16 * 2;
            self.indices.extend([a, b, b + 1, a, b + 1, a + 1]);
        }

        let rot = glam::Quat::from_rotation_arc(vec3(0.0, 0.0, 1.0), (b - a).normalize());

        let depth = (self.camera.view * a.extend(1.0)).z;
        let scale = Vec3::splat(depth);

        let index_count = self.indices.len() as u32 - start_index;
        self.draws.push(Draw {
            topology: PrimitiveTopology::TriangleList,
            transform: Mat4::from_scale_rotation_translation(scale, rot, a),
            kind: DrawKind::Indexed {
                base_vertex,
                start_index,
                index_count,
            },
        })
    }

    pub fn cone(&mut self, base: Vec3, apex: Vec3, radius: f32, base_color: [u8; 4], apex_color: [u8; 4]) {
        const D: usize = 8;

        let base_vertex = self.vertices.len() as u32;
        let start_index = self.indices.len() as u32;

        //let mut base_vertices = [OverlayVertex::default(); DIVISIONS];
        for i in 0..D {
            let angle = (i as f32 / D as f32) * TAU;
            let x = angle.cos() * radius;
            let y = angle.sin() * radius;
            self.vertices.push(OverlayVertex::new(vec3(x, y, 0.0), base_color));
        }

        let h = (base - apex).length();
        self.vertices.push(OverlayVertex::new(vec3(0.0, 0.0, 0.0), base_color));
        self.vertices.push(OverlayVertex::new(vec3(0.0, 0.0, h), apex_color));

        let base_index = D as u16;
        let apex_index = D as u16 + 1;

        for i in 0..D {
            let a = i as u16;
            let b = ((i + 1) % D) as u16;
            self.indices.extend([a, b, base_index, a, b, apex_index]);
        }

        let rot = glam::Quat::from_rotation_arc(vec3(0.0, 0.0, 1.0), (apex - base).normalize());

        let depth = (self.camera.view * base.extend(1.0)).z;
        let scale = Vec3::splat(-depth);

        let index_count = self.indices.len() as u32 - start_index;
        self.draws.push(Draw {
            topology: PrimitiveTopology::TriangleList,
            transform: Mat4::from_scale_rotation_translation(scale, rot, base),
            kind: DrawKind::Indexed {
                base_vertex,
                start_index,
                index_count,
            },
        })
    }

    pub fn render(&mut self, width: u32, height: u32, line_width: f32, filter_width: f32, encoder: &mut RenderEncoder) {
        if self.draws.is_empty() {
            return;
        }

        let vertex_buffer = encoder.device().upload_array_buffer(BufferUsage::VERTEX_BUFFER, &self.vertices);
        let index_buffer = encoder.device().upload_array_buffer(BufferUsage::INDEX_BUFFER, &self.indices);

        // SAFETY: ???
        encoder.set_viewport(0.0, height as f32, width as f32, -(height as f32), 0.0, 1.0);
        encoder.set_scissor(0, 0, width, height);

        // Lines
        if !self.polylines.is_empty() {
            let line_vertex_buffer = encoder
                .device()
                .upload_array_buffer(BufferUsage::STORAGE_BUFFER, &self.line_vertices);
            let line_buffer = encoder.device().upload_array_buffer(BufferUsage::STORAGE_BUFFER, &self.polylines);

            encoder.bind_graphics_pipeline(&self.line_pipeline);
            encoder.bind_push_constants(&PushConstants {
                matrix: self.camera.view_projection(),
                width: line_width,
                filter_width,
                line_count: self.polylines.len() as u32,
                screen_width: width as f32,
                screen_height: height as f32,
            });
            encoder.bind_arguments(
                0,
                &LineRenderingArguments {
                    vertex_buffer: line_vertex_buffer.slice(..),
                    line_buffer: line_buffer.slice(..),
                },
            );
            encoder.draw_mesh_tasks(((self.polylines.len() + 31) as u32) / 32, 1, 1);
        }

        // Polygons
        encoder.bind_graphics_pipeline(&self.pipeline);
        encoder.bind_vertex_buffer(&VertexBufferDescriptor {
            binding: 0,
            buffer_range: vertex_buffer.slice(..).untyped,
            stride: mem::size_of::<OverlayVertex>() as u32,
        });
        encoder.bind_index_buffer(IndexType::U16, index_buffer.slice(..).untyped);

        for draw in self.draws.iter() {
            encoder.bind_push_constants(&PushConstants {
                matrix: self.camera.view_projection() * draw.transform,
                width: 1.0,
                filter_width,
                line_count: 0,
                screen_width: width as f32,
                screen_height: height as f32,
            });
            encoder.set_primitive_topology(draw.topology);

            match draw.kind {
                DrawKind::Indexed {
                    base_vertex,
                    start_index,
                    index_count,
                } => {
                    encoder.draw_indexed(start_index..(start_index + index_count), base_vertex as i32, 0..1);
                }
                DrawKind::Draw {
                    start_vertex,
                    vertex_count,
                } => {
                    encoder.draw(start_vertex..(start_vertex + vertex_count), 0..1);
                }
            }
        }

        self.vertices.clear();
        self.indices.clear();
        self.draws.clear();
        self.line_vertices.clear();
        self.polylines.clear();
    }
}

fn compile_shaders(device: &Device, target_color_format: Format, target_depth_format: Format) -> GraphicsPipeline {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[],
            push_constants_size: mem::size_of::<PushConstants>(),
        },
        vertex_input: VertexInputState {
            topology: PrimitiveTopology::LineStrip,
            buffers: &[VertexBufferLayoutDescription {
                binding: 0,
                stride: mem::size_of::<OverlayVertex>() as u32,
                input_rate: VertexInputRate::Vertex,
            }],
            attributes: &[
                // Position
                VertexInputAttributeDescription {
                    location: 0,
                    binding: 0,
                    format: Format::R32G32B32_SFLOAT,
                    offset: OverlayVertex::ATTRIBUTES[0].offset,
                },
                // Color
                VertexInputAttributeDescription {
                    location: 1,
                    binding: 0,
                    format: Format::R8G8B8A8_UNORM,
                    offset: OverlayVertex::ATTRIBUTES[1].offset,
                },
            ],
        },
        pre_rasterization_shaders: PreRasterizationShaders::vertex_shader_from_source_file(Path::new(
            "crates/fluff/shaders/overlay/overlay.vert",
        )),
        rasterization: RasterizationState {
            polygon_mode: PolygonMode::Fill,
            cull_mode: Default::default(),
            front_face: FrontFace::Clockwise,
            line_rasterization: LineRasterization {
                mode: LineRasterizationMode::RectangularSmooth,
            },
            ..Default::default()
        },
        fragment_shader: ShaderEntryPoint::from_source_file(Path::new("crates/fluff/shaders/overlay/overlay.frag")),
        depth_stencil: DepthStencilState {
            depth_write_enable: true,
            depth_compare_op: CompareOp::LessOrEqual,
            stencil_state: StencilState::default(),
        },
        fragment_output: FragmentOutputInterfaceDescriptor {
            color_attachment_formats: &[target_color_format],
            depth_attachment_format: Some(target_depth_format),
            stencil_attachment_format: None,
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
                color_write_mask: Default::default(),
            }],
            blend_constants: [0.0; 4],
        },
    };

    device.create_graphics_pipeline(create_info).expect("failed to create pipeline")
}

fn compile_line_shaders(device: &Device, target_color_format: Format, target_depth_format: Format) -> GraphicsPipeline {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[LineRenderingArguments::LAYOUT],
            push_constants_size: mem::size_of::<PushConstants>(),
        },
        vertex_input: VertexInputState::default(),
        pre_rasterization_shaders: PreRasterizationShaders::mesh_shading_from_source_file(Path::new(
            "crates/fluff/shaders/overlay/lines.glsl",
        )),
        rasterization: RasterizationState {
            polygon_mode: PolygonMode::Fill,
            cull_mode: Default::default(),
            front_face: FrontFace::Clockwise,
            line_rasterization: LineRasterization {
                mode: LineRasterizationMode::RectangularSmooth,
            },
            ..Default::default()
        },
        fragment_shader: ShaderEntryPoint::from_source_file(Path::new("crates/fluff/shaders/overlay/lines.glsl")),
        depth_stencil: DepthStencilState {
            depth_write_enable: true,
            depth_compare_op: CompareOp::LessOrEqual,
            stencil_state: StencilState::default(),
        },
        fragment_output: FragmentOutputInterfaceDescriptor {
            color_attachment_formats: &[target_color_format],
            depth_attachment_format: Some(target_depth_format),
            stencil_attachment_format: None,
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
                color_write_mask: Default::default(),
            }],
            blend_constants: [0.0; 4],
        },
    };

    device.create_graphics_pipeline(create_info).expect("failed to create pipeline")
}
