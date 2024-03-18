use graal::{prelude::*, util::CommandStreamExt};
use imgui::{internal::RawWrapper, DrawCmd, DrawCmdParams, DrawData, DrawIdx, TextureId, Textures};
use std::{mem, path::Path};

#[derive(Debug, Attachments)]
struct ImguiAttachments<'a> {
    // FIXME: we shouldn't specify attachment formats statically
    #[attachment(format=R16G16B16A16_SFLOAT)]
    color: &'a ImageView,
}

#[derive(Copy, Clone, Vertex)]
#[repr(C)]
struct ImguiDrawVert {
    position: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
}

#[derive(Copy, Clone)]
struct ImguiPushConstants {
    matrix: [[f32; 4]; 4],
}

const _: () = assert!(mem::size_of::<DrawIdx>() == 2);

#[derive(Arguments)]
struct ImguiArguments<'a> {
    #[argument(binding = 0, sampled_image)]
    tex: &'a ImageView,
    #[argument(binding = 1, sampler)]
    sampler: &'a Sampler,
}

pub struct Renderer {
    pipeline: GraphicsPipeline,
    textures: Textures<Image>,
    font_texture: Image,
    font_texture_view: ImageView,
    font_sampler: Sampler,
}

impl Renderer {
    pub fn new(cmd: &mut CommandStream, ctx: &mut imgui::Context) -> Renderer {
        let pipeline = create_pipeline(cmd.device());
        let font_texture = upload_font_texture(cmd, ctx.fonts());
        ctx.set_renderer_name(Some(format!("fluff_imgui_backend {}", env!("CARGO_PKG_VERSION"))));
        ctx.io_mut().backend_flags.insert(imgui::BackendFlags::RENDERER_HAS_VTX_OFFSET);
        let sampler = cmd.device().create_sampler(&SamplerCreateInfo {
            mag_filter: vk::Filter::LINEAR,
            min_filter: vk::Filter::LINEAR,
            mipmap_mode: vk::SamplerMipmapMode::NEAREST,
            address_mode_u: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            address_mode_v: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            address_mode_w: vk::SamplerAddressMode::CLAMP_TO_EDGE,
            ..Default::default()
        });
        let font_texture_view = font_texture.create_top_level_view();
        Renderer {
            pipeline,
            font_texture,
            font_texture_view,
            textures: Textures::new(),
            font_sampler: sampler,
        }
    }

    pub fn reload_font_texture(&mut self, cmd: &mut CommandStream, ctx: &mut imgui::Context) {
        self.font_texture = upload_font_texture(cmd, ctx.fonts());
    }

    pub fn textures(&mut self) -> &mut Textures<Image> {
        &mut self.textures
    }

    fn lookup_texture(&self, texture_id: TextureId) -> &Image {
        if texture_id.id() == usize::MAX {
            &self.font_texture
        } else if let Some(texture) = self.textures.get(texture_id) {
            texture
        } else {
            panic!("invalid texture id: {:?}", texture_id)
        }
    }

    pub fn render(&mut self, cmd: &mut CommandStream, target: &ImageView, draw_data: &DrawData) {
        let fb_width = draw_data.display_size[0] * draw_data.framebuffer_scale[0];
        let fb_height = draw_data.display_size[1] * draw_data.framebuffer_scale[1];
        if !(fb_width > 0.0 && fb_height > 0.0) {
            return;
        }

        let width = draw_data.display_size[0];
        let height = draw_data.display_size[1];
        let offset_x = draw_data.display_pos[0] / width;
        let offset_y = draw_data.display_pos[1] / height;

        let matrix = [
            [2.0 / width, 0.0, 0.0, 0.0],
            [0.0, 2.0 / height, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [-1.0 - offset_x * 2.0, -1.0 - offset_y * 2.0, 0.0, 1.0],
        ];

        let clip_off = draw_data.display_pos;
        let clip_scale = draw_data.framebuffer_scale;

        cmd.debug_group("ImGui", |cmd| {
            let mut encoder = cmd.begin_rendering(&ImguiAttachments { color: target });
            for draw_list in draw_data.draw_lists() {
                // upload vertex & index data
                let vertex_data = unsafe { draw_list.transmute_vtx_buffer::<ImguiDrawVert>() };
                let index_data = draw_list.idx_buffer();

                let vertex_buffer = encoder.device().upload_array_buffer(BufferUsage::VERTEX_BUFFER, vertex_data);
                let index_buffer = encoder.device().upload_array_buffer(BufferUsage::INDEX_BUFFER, index_data);

                encoder.bind_graphics_pipeline(&self.pipeline);

                for cmd in draw_list.commands() {
                    match cmd {
                        DrawCmd::Elements {
                            count,
                            cmd_params:
                                DrawCmdParams {
                                    clip_rect,
                                    texture_id,
                                    vtx_offset,
                                    idx_offset,
                                    ..
                                },
                        } => {
                            let clip_rect = [
                                (clip_rect[0] - clip_off[0]) * clip_scale[0],
                                (clip_rect[1] - clip_off[1]) * clip_scale[1],
                                (clip_rect[2] - clip_off[0]) * clip_scale[0],
                                (clip_rect[3] - clip_off[1]) * clip_scale[1],
                            ];

                            if clip_rect[0] < fb_width && clip_rect[1] < fb_height && clip_rect[2] >= 0.0 && clip_rect[3] >= 0.0 {
                                let texture = self.lookup_texture(texture_id);
                                let texture_view = texture.create_top_level_view();

                                // TODO: VertexBufferDescriptor from typed slice
                                encoder.bind_vertex_buffer(&VertexBufferDescriptor {
                                    binding: 0,
                                    buffer_range: vertex_buffer.slice(vtx_offset..).untyped,
                                    stride: mem::size_of::<ImguiDrawVert>() as u32,
                                });

                                // TODO: bind_index_buffer from typed slice
                                encoder.bind_index_buffer(IndexType::U16, index_buffer.slice(idx_offset..(idx_offset + count)).untyped);

                                encoder.bind_push_constants(&ImguiPushConstants { matrix });

                                encoder.set_scissor(
                                    clip_rect[0].floor() as i32,
                                    clip_rect[1].floor() as i32,
                                    (clip_rect[2] - clip_rect[0]).floor() as u32,
                                    (clip_rect[3] - clip_rect[1]).floor() as u32,
                                );

                                encoder.set_viewport(0.0, 0.0, fb_width, fb_height, 0.0, 1.0);
                                encoder.bind_arguments(
                                    0,
                                    &ImguiArguments {
                                        tex: &texture_view,
                                        sampler: &self.font_sampler,
                                    },
                                );

                                encoder.set_primitive_topology(PrimitiveTopology::TriangleList);
                                encoder.draw_indexed(0..(count as u32), 0, 0..1);
                            }
                        }
                        DrawCmd::ResetRenderState => (), // TODO
                        // TODO SAFETY
                        DrawCmd::RawCallback { callback, raw_cmd } => unsafe { callback(draw_list.raw(), raw_cmd) },
                    }
                }
            }
            encoder.finish();
        });
    }
}

fn upload_font_texture(cmd: &mut CommandStream, fonts: &mut imgui::FontAtlas) -> Image {
    let texture = fonts.build_rgba32_texture();

    let font_texture = cmd.create_image_with_data(
        &ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
            format: vk::Format::R8G8B8A8_UNORM, // ???
            width: texture.width,
            height: texture.height,
            ..Default::default()
        },
        texture.data,
    );

    fonts.tex_id = TextureId::from(usize::MAX);
    return font_texture;
}

fn create_pipeline(device: &Device) -> GraphicsPipeline {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[ImguiArguments::LAYOUT],
            push_constants_size: mem::size_of::<ImguiPushConstants>(),
        },
        vertex_input: VertexInputState {
            topology: PrimitiveTopology::TriangleList,
            buffers: &[VertexBufferLayoutDescription {
                binding: 0,
                stride: mem::size_of::<ImguiDrawVert>() as u32,
                input_rate: VertexInputRate::Vertex,
            }],
            attributes: &[
                VertexInputAttributeDescription {
                    location: 0,
                    binding: 0,
                    format: vk::Format::R32G32_SFLOAT,
                    offset: ImguiDrawVert::ATTRIBUTES[0].offset,
                },
                VertexInputAttributeDescription {
                    location: 1,
                    binding: 0,
                    format: vk::Format::R32G32_SFLOAT,
                    offset: ImguiDrawVert::ATTRIBUTES[1].offset,
                },
                VertexInputAttributeDescription {
                    location: 2,
                    binding: 0,
                    format: vk::Format::R8G8B8A8_UNORM,
                    offset: ImguiDrawVert::ATTRIBUTES[2].offset,
                },
            ],
        },
        pre_rasterization_shaders: PreRasterizationShaders::PrimitiveShading {
            vertex: ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/imgui.vert"))),
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
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/imgui.frag"))),
            entry_point: "main",
        },
        depth_stencil: DepthStencilState {
            depth_write_enable: false,
            depth_compare_op: Default::default(),
            stencil_state: StencilState::default(),
        },
        fragment_output: FragmentOutputInterfaceDescriptor {
            color_attachment_formats: <ImguiAttachments as StaticAttachments>::COLOR,
            depth_attachment_format: None,
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
