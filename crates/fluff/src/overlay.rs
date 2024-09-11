//! Scene overlay drawing utilities
use std::{f32::consts::TAU, mem, path::Path};

use glam::{DVec2, DVec3, Mat4, vec3, Vec3};
use graal::{ColorAttachment, DepthStencilAttachment, prelude::*, RenderPassInfo};
use graal::vk::{AttachmentLoadOp, AttachmentStoreOp};

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

    pub fn flatten(&self, points: &mut Vec<Vec3>, tolerance: f32) {
        if points.is_empty() {
            points.push(self.p0);
        }
        self.flatten_inner(points, tolerance);
    }
}

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
struct OverlayPolygonsPushConstants {
    matrix: Mat4,
    width: f32,
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct OverlayLinesPushConstants {
    matrix: Mat4,
    vertex_count: u32,
    width: f32,
    filter_width: f32,
    screen_width: f32,
    screen_height: f32,
}

struct Draw {
    topology: vk::PrimitiveTopology,
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


const LINE_VERTEX_FLAG_FIRST: u32 = 1;
const LINE_VERTEX_FLAG_LAST: u32 = 2;

#[derive(Copy, Clone, Default)]
#[repr(C)]
struct LineVertex {
    position: [f32; 3],
    color: [u8; 4],
    flags: u32,
}

/*
#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct Polyline {
    start_vertex: u32,
    vertex_count: u32,
}*/

struct Pipelines {
    polygon_pipeline: GraphicsPipeline,
    line_pipeline: GraphicsPipeline,
}

fn create_pipelines(device: &Device, target_color_format: Format, target_depth_format: Format) -> Pipelines {
    // Polygon pipeline
    let create_info = GraphicsPipelineCreateInfo {
        set_layouts: &[],
        push_constants_size: mem::size_of::<OverlayPolygonsPushConstants>(),
        vertex_input: VertexInputState {
            topology: vk::PrimitiveTopology::LINE_STRIP,
            buffers: &[VertexBufferLayoutDescription {
                binding: 0,
                stride: mem::size_of::<OverlayVertex>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
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
            "crates/fluff/shaders/overlay_polygons.vert",
        )),
        rasterization: RasterizationState {
            polygon_mode: vk::PolygonMode::FILL,
            cull_mode: Default::default(),
            front_face: vk::FrontFace::CLOCKWISE,
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilState {
            format: target_depth_format,
            depth_write_enable: true,
            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
            stencil_state: StencilState::default(),
        }),
        fragment: FragmentState {
            shader: ShaderEntryPoint::from_source_file(Path::new("crates/fluff/shaders/overlay_polygons.frag")),
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                format: target_color_format,
                blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
                ..Default::default()
            }],
            blend_constants: [0.; 4],
        },
    };

    let polygon_pipeline = device.create_graphics_pipeline(create_info).expect("failed to create pipeline");

    // Line pipeline
    let descriptor_set_layout = device.create_push_descriptor_set_layout(&[
        vk::DescriptorSetLayoutBinding {
            binding: 0,
            descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::MESH_EXT | vk::ShaderStageFlags::TASK_EXT,
            ..Default::default()
        },
        /*vk::DescriptorSetLayoutBinding {
            binding: 1,
            descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::MESH_EXT | vk::ShaderStageFlags::TASK_EXT,
            ..Default::default()
        },*/
    ]);

    let create_info = GraphicsPipelineCreateInfo {
        set_layouts: &[descriptor_set_layout.clone()],
        push_constants_size: mem::size_of::<OverlayLinesPushConstants>(),
        vertex_input: VertexInputState::default(),
        pre_rasterization_shaders: PreRasterizationShaders::mesh_shading_from_source_file(Path::new(
            "crates/fluff/shaders/overlay_lines.glsl",
        )),
        rasterization: RasterizationState {
            polygon_mode: vk::PolygonMode::FILL,
            cull_mode: Default::default(),
            front_face: vk::FrontFace::CLOCKWISE,
            depth_clamp_enable: true,
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilState {
            format: target_depth_format,
            depth_write_enable: true,
            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
            stencil_state: StencilState::default(),
        }),
        fragment: FragmentState {
            shader: ShaderEntryPoint::from_source_file(Path::new("crates/fluff/shaders/overlay_lines.glsl")),
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                format: target_color_format,
                blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
                ..Default::default()
            }],
            blend_constants: [0.0; 4],
        },
    };

    let line_pipeline = device.create_graphics_pipeline(create_info).expect("failed to create pipeline");

    Pipelines {
        polygon_pipeline,
        line_pipeline,
    }
}

#[derive(Clone)]
pub struct OverlayRenderParams<'a> {
    pub camera: Camera,
    pub color_target: &'a ImageView,
    pub depth_target: &'a ImageView,
    pub line_width: f32,
    pub filter_width: f32,
}

pub struct OverlayRenderer {
    polygon_pipeline: GraphicsPipeline,
    line_pipeline: GraphicsPipeline,
    //camera: Camera,
    target_color_format: Format,
    target_depth_format: Format,
    vertices: Vec<OverlayVertex>,
    indices: Vec<u16>,
    draws: Vec<Draw>,
    line_vertices: Vec<LineVertex>,
    //polylines: Vec<Polyline>,
}

impl OverlayRenderer {
    /// Creates a new overlay instance.
    ///
    /// # Arguments
    ///
    /// * `format` format of the output image
    pub fn new(device: &Device, target_color_format: Format, target_depth_format: Format) -> Self {
        let Pipelines {
            polygon_pipeline,
            line_pipeline,
        } = create_pipelines(device, target_color_format, target_depth_format);

        Self {
            //camera: Camera::default(),
            polygon_pipeline,
            line_pipeline,
            target_color_format,
            target_depth_format,
            vertices: vec![],
            indices: vec![],
            draws: vec![],
            line_vertices: vec![],
        }
    }

    pub fn line(&mut self, a: DVec3, b: DVec3, a_color: [u8; 4], b_color: [u8; 4]) {
        //let start_vertex = self.line_vertices.len() as u32;
        self.line_vertices.push(LineVertex {
            position: a.as_vec3().to_array(),
            color: a_color,
            flags: LINE_VERTEX_FLAG_FIRST,
        });
        self.line_vertices.push(LineVertex {
            position: b.as_vec3().to_array(),
            color: b_color,
            flags: LINE_VERTEX_FLAG_LAST,
        });
    }

    pub fn screen_line(&mut self, camera: &Camera, a: DVec2, b: DVec2, a_color: [u8; 4], b_color: [u8; 4]) {
        // TODO: dedicated screen-space line shader
        let a = camera.screen_to_world(a.extend(0.0));
        let b = camera.screen_to_world(b.extend(0.0));
        self.line(a, b, a_color, b_color);
    }

    pub fn screen_polyline(&mut self, camera: &Camera, vertices: &[DVec2], color: [u8; 4]) {
        for (i, &v) in vertices.iter().enumerate() {
            let p = camera.screen_to_world(v.extend(0.0));
            self.line_vertices.push(LineVertex {
                position: p.as_vec3().to_array(),
                color,
                flags: if i == 0 {
                    LINE_VERTEX_FLAG_FIRST
                } else if i == vertices.len() - 1 {
                    LINE_VERTEX_FLAG_LAST
                } else {
                    0
                },
            });
        }
    }

    pub fn cubic_bezier(&mut self, segment: &CubicBezierSegment, color: [u8; 4]) {
        let mut points = vec![];
        segment.flatten(&mut points, 0.0001);
        //let start_vertex = self.line_vertices.len() as u32;
        for (i, p) in points.iter().enumerate() {
            self.line_vertices.push(LineVertex {
                position: p.to_array(),
                color,
                flags: if i == 0 {
                    LINE_VERTEX_FLAG_FIRST
                } else if i == points.len() - 1 {
                    LINE_VERTEX_FLAG_LAST
                } else {
                    0
                },
            });
        }
        /*
        self.polylines.push(Polyline {
            start_vertex,
            vertex_count: points.len() as u32,
        });*/
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

        //let depth = (self.camera.view * a.extend(1.0)).z;
        //let scale = Vec3::splat(depth);

        let index_count = self.indices.len() as u32 - start_index;
        self.draws.push(Draw {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            transform: Mat4::from_rotation_translation(rot, a),
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

        //let depth = (self.camera.view * base.extend(1.0)).z;
        //let scale = Vec3::splat(-depth);

        let index_count = self.indices.len() as u32 - start_index;
        self.draws.push(Draw {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            transform: Mat4::from_rotation_translation(rot, base),
            kind: DrawKind::Indexed {
                base_vertex,
                start_index,
                index_count,
            },
        })
    }

    pub fn render(&mut self,
                  cmd: &mut CommandStream,
                  params: OverlayRenderParams)
    {
        if self.draws.is_empty() {
            return;
        }

        let mut encoder = cmd.begin_rendering(RenderPassInfo {
            color_attachments: &[ColorAttachment {
                image_view: params.color_target,
                clear_value: None,
            }],
            depth_stencil_attachment: Some(DepthStencilAttachment {
                image_view: params.depth_target,
                depth_clear_value: None,
                stencil_clear_value: None,
            }),
        });

        let width = params.color_target.width();
        let height = params.color_target.height();

        let vertex_buffer = encoder.device().upload_array_buffer(BufferUsage::VERTEX_BUFFER, &self.vertices);
        vertex_buffer.set_name("overlay vertex buffer");
        let index_buffer = encoder.device().upload_array_buffer(BufferUsage::INDEX_BUFFER, &self.indices);
        index_buffer.set_name("overlay index buffer");

        encoder.set_viewport(0.0, height as f32, width as f32, -(height as f32), 0.0, 1.0);
        encoder.set_scissor(0, 0, width, height);

        // Draw polylines
        if !self.line_vertices.is_empty() {
            let line_vertex_buffer = encoder
                .device()
                .upload_array_buffer(BufferUsage::STORAGE_BUFFER, &self.line_vertices);
            line_vertex_buffer.set_name("overlay line vertex buffer");
            //let line_buffer = encoder.device().upload_array_buffer(BufferUsage::STORAGE_BUFFER, &self.polylines);
            //line_buffer.set_name("overlay line buffer");

            encoder.bind_graphics_pipeline(&self.line_pipeline);
            encoder.push_constants(&OverlayLinesPushConstants {
                matrix: params.camera.view_projection(),
                width: params.line_width,
                filter_width: params.filter_width,
                vertex_count: self.line_vertices.len() as u32,
                screen_width: width as f32,
                screen_height: height as f32,
            });
            encoder.push_descriptors(
                0,
                &[
                    (0, line_vertex_buffer.slice(..).storage_descriptor()),
                ],
            );
            encoder.draw_mesh_tasks(self.line_vertices.len().div_ceil(32) as u32, 1, 1);
        }

        // Draw polygons
        encoder.bind_graphics_pipeline(&self.polygon_pipeline);
        encoder.bind_vertex_buffer(0, vertex_buffer.slice(..).untyped);
        encoder.bind_index_buffer(vk::IndexType::UINT16, index_buffer.slice(..).untyped);

        for draw in self.draws.iter() {
            // Scale meshes based on depth to keep a constant screen size
            let depth = (params.camera.view * draw.transform.transform_point3(Vec3::splat(0.0)).extend(1.0)).z;
            let scale = Mat4::from_scale(Vec3::splat(-depth));

            encoder.push_constants(&OverlayPolygonsPushConstants {
                matrix: params.camera.view_projection() * draw.transform * scale,
                width: 1.0,
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
        //self.polylines.clear();
    }
}
