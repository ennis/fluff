//! Scene overlay drawing utilities
use std::{f32::consts::TAU, mem, path::Path};

use glam::{vec3, Mat4, Vec3};
use graal::{
    util::DeviceExt, BufferUsage, ColorBlendEquation, ColorTargetState, CompareOp, DepthStencilState, Device, Format,
    FragmentOutputInterfaceDescriptor, FrontFace, GraphicsPipeline, GraphicsPipelineCreateInfo, IndexType, LineRasterization,
    LineRasterizationMode, PipelineBindPoint, PipelineLayoutDescriptor, Point2D, PolygonMode, PreRasterizationShaders, PrimitiveTopology,
    RasterizationState, Rect2D, RenderEncoder, ShaderCode, ShaderEntryPoint, ShaderSource, Size2D, StencilState, Vertex,
    VertexBufferDescriptor, VertexBufferLayoutDescription, VertexInputAttributeDescription, VertexInputRate, VertexInputState,
};

use crate::camera_control::Camera;

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

pub struct OverlayRenderer {
    pipeline: GraphicsPipeline,
    camera: Camera,
    target_color_format: Format,
    target_depth_format: Format,
    vertices: Vec<OverlayVertex>,
    indices: Vec<u16>,
    draws: Vec<Draw>,
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
            target_color_format,
            target_depth_format,
            vertices: vec![],
            indices: vec![],
            draws: vec![],
        }
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    pub fn line(&mut self, a: Vec3, b: Vec3, a_color: [u8; 4], b_color: [u8; 4]) {
        let start_vertex = self.vertices.len() as u32;
        self.vertices.push(OverlayVertex::new(a, a_color));
        self.vertices.push(OverlayVertex::new(b, b_color));
        self.draws.push(Draw {
            transform: Mat4::IDENTITY,
            topology: PrimitiveTopology::LineStrip,
            kind: DrawKind::Draw {
                start_vertex,
                vertex_count: 2,
            },
        })
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

    pub fn render(&mut self, width: u32, height: u32, encoder: &mut RenderEncoder) {
        if self.draws.is_empty() {
            return;
        }

        let vertex_buffer = encoder
            .device()
            .upload_array_buffer("overlay/vertices", BufferUsage::VERTEX_BUFFER, &self.vertices);
        let index_buffer = encoder
            .device()
            .upload_array_buffer("overlay/indices", BufferUsage::INDEX_BUFFER, &self.indices);

        // SAFETY: ???
        unsafe {
            encoder.bind_graphics_pipeline(&self.pipeline);
            encoder.bind_vertex_buffer(&VertexBufferDescriptor {
                binding: 0,
                buffer_range: vertex_buffer.slice(..).any(),
                stride: mem::size_of::<OverlayVertex>() as u32,
            });
            encoder.bind_index_buffer(IndexType::U16, index_buffer.slice(..).any());
            encoder.set_viewport(0.0, height as f32, width as f32, -(height as f32), 0.0, 1.0);
            encoder.set_scissor(0, 0, width, height);

            for draw in self.draws.iter() {
                encoder.bind_push_constants(
                    PipelineBindPoint::Graphics,
                    &PushConstants {
                        matrix: self.camera.view_projection() * draw.transform,
                    },
                );
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
        }

        self.vertices.clear();
        self.indices.clear();
        self.draws.clear();
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
        pre_rasterization_shaders: PreRasterizationShaders::PrimitiveShading {
            vertex: ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/overlay.vert"))),
                entry_point: "main",
            },
            tess_control: None,
            tess_evaluation: None,
            geometry: None,
        },
        rasterization: RasterizationState {
            polygon_mode: PolygonMode::Fill,
            cull_mode: Default::default(),
            front_face: FrontFace::Clockwise,
            line_rasterization: LineRasterization {
                mode: LineRasterizationMode::RectangularSmooth,
            },
        },
        fragment_shader: ShaderEntryPoint {
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/overlay.frag"))),
            entry_point: "main",
        },
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
