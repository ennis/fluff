use egui::{epaint::Primitive, ClippedPrimitive, ImageData, TexturesDelta};
use graal::{
    prelude::*,
    util::CommandStreamExt,
    vk::{ImageAspectFlags, Offset3D},
    BlendFactor, BlendOp, ImageCopyView, Size3D, Vertex,
};
use std::{collections::HashMap, mem, path::Path, slice};

#[derive(Debug, Attachments)]
struct EguiAttachments<'a> {
    // FIXME: we shouldn't specify attachment formats statically
    #[attachment(format=R16G16B16A16_SFLOAT)]
    color: &'a ImageView,
}

#[derive(Copy, Clone, Vertex)]
#[repr(C)]
struct EguiVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
}

#[derive(Copy, Clone)]
struct EguiPushConstants {
    screen_size: [f32; 2],
}

#[derive(Arguments)]
struct EguiArguments<'a> {
    #[argument(binding = 0, sampled_image)]
    tex: &'a ImageView,
    #[argument(binding = 1, sampler)]
    sampler: &'a Sampler,
}

struct Texture {
    image: Image,
    view: ImageView,
    sampler: Sampler,
}

pub struct Renderer {
    pipeline: GraphicsPipeline,
    sampler: Sampler,
    textures: HashMap<egui::TextureId, Texture>,
}

impl Renderer {
    pub fn new(cmd: &mut CommandStream) -> Renderer {
        let pipeline = create_pipeline(cmd.device());

        let sampler = cmd.device().create_sampler(&SamplerCreateInfo {
            mag_filter: vk::Filter::LINEAR,
            min_filter: vk::Filter::LINEAR,
            mipmap_mode: vk::SamplerMipmapMode::NEAREST,
            address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            ..Default::default()
        });

        Renderer {
            pipeline,
            textures: HashMap::new(),
            sampler,
        }
    }

    fn update_textures(&mut self, cmd: &mut CommandStream, textures_delta: egui::TexturesDelta) {
        let convert_filter = |min_filter: egui::TextureFilter| -> vk::Filter {
            match min_filter {
                egui::TextureFilter::Nearest => vk::Filter::NEAREST,
                egui::TextureFilter::Linear => vk::Filter::LINEAR,
            }
        };

        for (id, tex) in textures_delta.set {
            let width = tex.image.width() as u32;
            let height = tex.image.height() as u32;

            // Get format and pointer to texture data
            let format;
            let data;
            let data_buf;

            unsafe {
                match tex.image {
                    ImageData::Color(color_image) => {
                        format = Format::R8G8B8A8_SRGB;
                        data = slice::from_raw_parts(
                            color_image.pixels.as_ptr() as *const u8,
                            color_image.pixels.len() * mem::size_of::<egui::Color32>(),
                        );
                    }
                    ImageData::Font(font_image) => {
                        format = Format::R8G8B8A8_SRGB;
                        data_buf = font_image.srgba_pixels(None).collect::<Vec<_>>();
                        data = slice::from_raw_parts(data_buf.as_ptr() as *const u8, data_buf.len() * mem::size_of::<egui::Color32>());
                    }
                }
            };

            // Get or create texture
            let texture = self.textures.entry(id).or_insert_with(|| {
                let image = cmd.create_image_with_data(
                    &ImageCreateInfo {
                        memory_location: MemoryLocation::GpuOnly,
                        type_: ImageType::Image2D,
                        usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
                        format,
                        width,
                        height,
                        ..Default::default()
                    },
                    data,
                );

                let view = image.create_top_level_view();

                let sampler = cmd.device().create_sampler(&SamplerCreateInfo {
                    mag_filter: convert_filter(tex.options.magnification),
                    min_filter: convert_filter(tex.options.minification),
                    mipmap_mode: vk::SamplerMipmapMode::NEAREST,
                    address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                    address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                    address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
                    ..Default::default()
                });

                Texture { image, view, sampler }
            });

            let (x, y) = if let Some([x, y]) = tex.pos { (x as i32, y as i32) } else { (0, 0) };

            cmd.upload_image_data(
                ImageCopyView {
                    image: &texture.image,
                    mip_level: 0,
                    origin: Offset3D { x, y, z: 0 },
                    aspect: ImageAspectFlags::COLOR,
                },
                Size3D { width, height, depth: 1 },
                data,
            );
        }
    }

    pub fn render(
        &mut self,
        cmd: &mut CommandStream,
        color_target: &ImageView,
        ctx: &egui::Context,
        textures_delta: egui::TexturesDelta,
        shapes: Vec<egui::epaint::ClippedShape>,
        pixels_per_point: f32,
    ) {
        self.update_textures(cmd, textures_delta);

        let clipped_primitives = ctx.tessellate(shapes, pixels_per_point);
        let mut encoder = cmd.begin_rendering(&EguiAttachments { color: color_target });

        for ClippedPrimitive { clip_rect, primitive } in clipped_primitives.iter() {
            match primitive {
                Primitive::Mesh(mesh) => {
                    // upload vertex & index data
                    assert_eq!(mem::size_of::<egui::epaint::Vertex>(), mem::size_of::<EguiVertex>(),);
                    assert!(mem::align_of::<EguiVertex>() <= mem::align_of::<egui::epaint::Vertex>());
                    let vertex_data: &[EguiVertex] = unsafe { slice::from_raw_parts(mesh.vertices.as_ptr().cast(), mesh.vertices.len()) };
                    let vertex_buffer = encoder.device().upload_array_buffer(BufferUsage::VERTEX_BUFFER, vertex_data);
                    let index_buffer = encoder.device().upload_array_buffer(BufferUsage::INDEX_BUFFER, &mesh.indices);
                    encoder.bind_graphics_pipeline(&self.pipeline);

                    let width = color_target.width();
                    let height = color_target.height();

                    // Transform clip rect to physical pixels:
                    let clip_min_x = pixels_per_point * clip_rect.min.x;
                    let clip_min_y = pixels_per_point * clip_rect.min.y;
                    let clip_max_x = pixels_per_point * clip_rect.max.x;
                    let clip_max_y = pixels_per_point * clip_rect.max.y;

                    let clip_min_x = clip_min_x.round() as i32;
                    let clip_min_y = clip_min_y.round() as i32;
                    let clip_max_x = clip_max_x.round() as i32;
                    let clip_max_y = clip_max_y.round() as i32;

                    let clip_min_x = clip_min_x.clamp(0, width as i32);
                    let clip_min_y = clip_min_y.clamp(0, height as i32);
                    let clip_max_x = clip_max_x.clamp(clip_min_x, width as i32);
                    let clip_max_y = clip_max_y.clamp(clip_min_y, height as i32);

                    // TODO: VertexBufferDescriptor from typed slice
                    encoder.bind_vertex_buffer(&VertexBufferDescriptor {
                        binding: 0,
                        buffer_range: vertex_buffer.slice(..).untyped,
                        stride: mem::size_of::<EguiVertex>() as u32,
                    });

                    // TODO: bind_index_buffer from typed slice
                    encoder.bind_index_buffer(IndexType::U32, index_buffer.slice(..).untyped);

                    let texture = self.textures.get(&mesh.texture_id).expect("texture not found");

                    encoder.bind_push_constants(&EguiPushConstants {
                        screen_size: [width as f32, height as f32],
                    });
                    encoder.set_scissor(
                        clip_min_x,
                        clip_min_y,
                        (clip_max_x - clip_min_x) as u32,
                        (clip_max_y - clip_min_y) as u32,
                    );
                    encoder.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
                    encoder.bind_arguments(
                        0,
                        &EguiArguments {
                            tex: &texture.view,
                            sampler: &texture.sampler,
                        },
                    );

                    encoder.set_primitive_topology(PrimitiveTopology::TriangleList);
                    encoder.draw_indexed(0..(index_buffer.len() as u32), 0, 0..1);
                }
                Primitive::Callback(_) => {
                    // TODO
                }
            }
        }

        encoder.finish();
    }
}

fn create_pipeline(device: &Device) -> GraphicsPipeline {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[EguiArguments::LAYOUT],
            push_constants_size: mem::size_of::<EguiPushConstants>(),
        },
        vertex_input: VertexInputState {
            topology: PrimitiveTopology::TriangleList,
            buffers: &[VertexBufferLayoutDescription {
                binding: 0,
                stride: mem::size_of::<EguiVertex>() as u32,
                input_rate: VertexInputRate::Vertex,
            }],
            attributes: &[
                VertexInputAttributeDescription {
                    location: 0,
                    binding: 0,
                    format: vk::Format::R32G32_SFLOAT,
                    offset: EguiVertex::ATTRIBUTES[0].offset,
                },
                VertexInputAttributeDescription {
                    location: 1,
                    binding: 0,
                    format: vk::Format::R32G32_SFLOAT,
                    offset: EguiVertex::ATTRIBUTES[1].offset,
                },
                VertexInputAttributeDescription {
                    location: 2,
                    binding: 0,
                    format: vk::Format::R8G8B8A8_UINT,
                    offset: EguiVertex::ATTRIBUTES[2].offset,
                },
            ],
        },
        pre_rasterization_shaders: PreRasterizationShaders::PrimitiveShading {
            vertex: ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/egui.vert"))),
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
            line_rasterization: Default::default(),
            ..Default::default()
        },
        fragment_shader: ShaderEntryPoint {
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/egui.frag"))),
            entry_point: "main",
        },
        depth_stencil: DepthStencilState {
            depth_write_enable: false,
            depth_compare_op: Default::default(),
            stencil_state: StencilState::default(),
        },
        fragment_output: FragmentOutputInterfaceDescriptor {
            color_attachment_formats: <EguiAttachments as StaticAttachments>::COLOR,
            depth_attachment_format: None,
            stencil_attachment_format: None,
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                blend_equation: Some(ColorBlendEquation {
                    src_color_blend_factor: BlendFactor::One,
                    dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
                    color_blend_op: BlendOp::Add,
                    src_alpha_blend_factor: BlendFactor::OneMinusDstAlpha,
                    dst_alpha_blend_factor: BlendFactor::One,
                    alpha_blend_op: BlendOp::Add,
                }),
                color_write_mask: Default::default(),
            }],
            blend_constants: [0.0; 4],
        },
    };

    device.create_graphics_pipeline(create_info).expect("failed to create pipeline")
}
