use std::collections::HashMap;
use std::{mem, slice};

use crate::shaders::{EGUI_FRAG_MAIN, EGUI_VERTEX_MAIN};
use egui::epaint::Primitive;
use egui::{ClippedPrimitive, ImageData};
use graal::prelude::*;
use graal::util::CommandStreamExt;
use graal::vk::{ImageAspectFlags, Offset3D};
use graal::{Barrier, ColorAttachment, ImageCopyView, RenderPassInfo, Size3D, Vertex};

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

const _: () = assert!(size_of::<egui::epaint::Vertex>() == size_of::<EguiVertex>());
const _: () = assert!(align_of::<EguiVertex>() <= align_of::<egui::epaint::Vertex>());

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
    pub fn new(device: &RcDevice) -> Renderer {
        let pipeline = create_pipeline(device);

        let sampler = device.create_sampler(&SamplerCreateInfo {
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
                        data = slice::from_raw_parts(
                            data_buf.as_ptr() as *const u8,
                            data_buf.len() * mem::size_of::<egui::Color32>(),
                        );
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
                image.set_name("egui texture");

                let view = image.create_top_level_view();
                view.set_name("egui texture view");

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

            let (x, y) = if let Some([x, y]) = tex.pos {
                (x as i32, y as i32)
            } else {
                (0, 0)
            };

            cmd.upload_image_data(
                ImageCopyView {
                    image: &texture.image,
                    mip_level: 0,
                    origin: Offset3D { x, y, z: 0 },
                    aspect: ImageAspectFlags::COLOR,
                },
                Size3D {
                    width,
                    height,
                    depth: 1,
                },
                data,
            );

            cmd.barrier(Barrier::new().sample_read_image(&texture.image));
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

        let meshes: Vec<_> = clipped_primitives
            .iter()
            .filter_map(|ClippedPrimitive { primitive, clip_rect }| match primitive {
                Primitive::Mesh(mesh) => Some((clip_rect, mesh)),
                Primitive::Callback(_) => None,
            })
            .collect();

        // upload vertex & index data
        // TODO we could use only one vertex buffer and one index buffer for all meshes
        let mut mesh_vertex_buffers = Vec::with_capacity(meshes.len());
        let mut mesh_index_buffers = Vec::with_capacity(meshes.len());

        for (_, mesh) in meshes.iter() {
            let vertex_data: &[EguiVertex] =
                unsafe { slice::from_raw_parts(mesh.vertices.as_ptr().cast(), mesh.vertices.len()) };
            let vertex_buffer = cmd.device().upload_slice(BufferUsage::VERTEX_BUFFER, vertex_data);
            let index_buffer = cmd.device().upload_slice(BufferUsage::INDEX_BUFFER, &mesh.indices);
            vertex_buffer.set_name("egui vertex buffer");
            index_buffer.set_name("egui index buffer");
            mesh_vertex_buffers.push(vertex_buffer);
            mesh_index_buffers.push(index_buffer);
        }

        // encode draw commands
        let mut enc = cmd.begin_rendering(RenderPassInfo {
            color_attachments: &[ColorAttachment {
                image_view: color_target,
                clear_value: None,
            }],
            depth_stencil_attachment: None,
        });

        enc.bind_graphics_pipeline(&self.pipeline);

        for (i, (clip_rect, mesh)) in meshes.iter().enumerate() {
            let vertex_buffer = &mesh_vertex_buffers[i];
            let index_buffer = &mesh_index_buffers[i];
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

            enc.bind_vertex_buffer(0, vertex_buffer.slice(..).untyped);
            enc.bind_index_buffer(vk::IndexType::UINT32, index_buffer.slice(..).untyped);

            enc.push_constants(&EguiPushConstants {
                screen_size: [width as f32, height as f32],
            });
            enc.set_scissor(
                clip_min_x,
                clip_min_y,
                (clip_max_x - clip_min_x) as u32,
                (clip_max_y - clip_min_y) as u32,
            );
            enc.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);

            let texture = self.textures.get(&mesh.texture_id).expect("texture not found");
            enc.push_descriptors(
                0,
                &[
                    (
                        0,
                        texture
                            .view
                            .texture_descriptor(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
                    ),
                    (1, self.sampler.descriptor()),
                ],
            );

            enc.set_primitive_topology(vk::PrimitiveTopology::TRIANGLE_LIST);
            enc.draw_indexed(0..(index_buffer.len() as u32), 0, 0..1);
        }

        enc.finish();
    }
}

fn create_pipeline(device: &RcDevice) -> GraphicsPipeline {
    let set_layout = device.create_push_descriptor_set_layout(&[
        vk::DescriptorSetLayoutBinding {
            binding: 0,
            descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::FRAGMENT,
            ..Default::default()
        },
        vk::DescriptorSetLayoutBinding {
            binding: 1,
            descriptor_type: vk::DescriptorType::SAMPLER,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::FRAGMENT,
            ..Default::default()
        },
    ]);

    let create_info = GraphicsPipelineCreateInfo {
        set_layouts: &[set_layout],
        push_constants_size: mem::size_of::<EguiPushConstants>(),
        vertex_input: VertexInputState {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            buffers: &[VertexBufferLayoutDescription {
                binding: 0,
                stride: mem::size_of::<EguiVertex>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
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
            vertex: EGUI_VERTEX_MAIN,
        },
        rasterization: RasterizationState {
            polygon_mode: vk::PolygonMode::FILL,
            cull_mode: Default::default(),
            front_face: vk::FrontFace::CLOCKWISE,
            ..Default::default()
        },
        depth_stencil: None,
        fragment: FragmentState {
            shader: EGUI_FRAG_MAIN,
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                format: Format::R16G16B16A16_SFLOAT,
                blend_equation: Some(ColorBlendEquation {
                    src_color_blend_factor: vk::BlendFactor::ONE,
                    dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
                    color_blend_op: vk::BlendOp::ADD,
                    src_alpha_blend_factor: vk::BlendFactor::ONE_MINUS_DST_ALPHA,
                    dst_alpha_blend_factor: vk::BlendFactor::ONE,
                    alpha_blend_op: vk::BlendOp::ADD,
                }),
                ..Default::default()
            }],
            blend_constants: [0.0; 4],
        },
    };

    device
        .create_graphics_pipeline(create_info)
        .expect("failed to create pipeline")
}
