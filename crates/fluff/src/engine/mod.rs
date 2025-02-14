use std::{borrow::Cow, cell::{Cell, RefCell}, collections::BTreeMap, fs, marker::PhantomData, path::{Path, PathBuf}, rc::Rc, slice};

use graal::{
    BufferAccess, BufferRangeUntyped,
    BufferUsage,
    ColorTargetState,
    CommandStream,
    ComputeEncoder,
    ComputePipeline, ComputePipelineCreateInfo, DepthStencilAttachment, DepthStencilState, Device, FragmentState, get_shader_compiler,
    GraphicsPipeline, GraphicsPipelineCreateInfo, ImageAccess, ImageCreateInfo, ImageSubresourceLayers, ImageUsage,
    ImageView, MemoryLocation, MultisampleState, Point3D, PreRasterizationShaders, RasterizationState, Rect3D,
    RenderEncoder, RenderPassInfo, SamplerCreateInfo, shaderc, shaderc::{EnvVersion, ShaderKind, SpirvVersion, TargetEnv}, ShaderCode, ShaderEntryPoint, util::DeviceExt,
    vk, vk::{Pipeline, Viewport},
};
use scoped_tls::scoped_thread_local;
use slotmap::SlotMap;
use spirv_reflect::types::{ReflectDescriptorType, ReflectTypeFlags};
use tracing::{debug, error, warn};

use crate::engine::shader::{CompilationInfo, compile_shader_stage};
use crate::shaders::bindings::EntryPoint;

//mod bindless;
mod shader;
//mod uniform_block;

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Error type for the rendering engine.
#[derive(thiserror::Error, Debug, Clone)]
pub enum Error {
    #[error("Failed to load configuration file")]
    ConfigLoadError,
    //#[error("Failed to execute Lua script: {0}")]
    //ScriptError(#[from] mlua::Error),
    #[error("Unsupported image format: {0}")]
    UnsupportedImageFormat(String),
    #[error("Missing required property: {0}")]
    MissingProperty(&'static str),
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),
    #[error("File I/O error: {0}")]
    IO(#[from] Rc<std::io::Error>),
    #[error("could not read shader file `{}`: {}", .path.display(), .error)]
    ShaderReadError { path: PathBuf, error: Rc<std::io::Error> },
    #[error("Shader compilation error: {0}")]
    ShaderCompilation(#[from] Rc<shaderc::Error>),
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),
    #[error("Vulkan error: {0}")]
    Vulkan(#[from] vk::Result),
    #[error("Unknown field: {0}")]
    UnknownField(String),
    #[error("Invalid field type: {0}")]
    InvalidFieldType(String),
    #[error("Vulkan error: {0}")]
    VulkanError(Rc<graal::Error>),
}

pub struct MeshRenderPipelineDesc {
    pub task_shader: PathBuf,
    pub mesh_shader: PathBuf,
    pub fragment_shader: PathBuf,
    pub defines: BTreeMap<String, String>,
    pub color_targets: Vec<ColorTargetState>,
    pub rasterization_state: RasterizationState,
    pub depth_stencil_state: Option<DepthStencilState>,
    pub multisample_state: MultisampleState,
}

pub struct ComputePipelineDesc2 {
    pub entry_point: EntryPoint,
}

/// Rendering engine instance.
pub struct Engine {
    device: Device,
    /// Defines added to every compiled shader
    global_defs: BTreeMap<String, String>,
    //bindless_layout: BindlessLayout,
    /// Cached mesh render pipelines compilation results
    mesh_render_pipelines: BTreeMap<String, Result<GraphicsPipeline, Error>>,
    /// Cached compute pipelines compilation results
    compute_pipelines: BTreeMap<String, Result<ComputePipeline, Error>>,
}

impl Engine {
    pub fn new(device: Device) -> Self {
        Self {
            device,
            global_defs: Default::default(),
            mesh_render_pipelines: Default::default(),
            compute_pipelines: Default::default(),
        }
    }

    pub fn set_global_defines(&mut self, defines: BTreeMap<String, String>) {
        self.global_defs = defines;
        // recompile all shaders
        self.mesh_render_pipelines.clear();
        self.compute_pipelines.clear();
    }

    /*pub fn submit_graph(&mut self, graph: RenderGraph, cmd: &mut CommandStream) {
        // 1. allocate resources
        //let device = &self.engine.device;
        for image in graph.resources.images.iter() {
            image.ensure_allocated(&self.device);
            // Not sure we need both here
            cmd.reference_resource(&image.view());
            cmd.reference_resource(&image.image());
        }
        for buffer in graph.resources.buffers.iter() {
            buffer.ensure_allocated(&self.device);
            // It's important to reference the buffers explicitly because often we use only use
            // their addresses in push constant blocks, and we can't track those usages automatically.
            cmd.reference_resource(&buffer.buffer());
        }

        // 2. build descriptors
        // for buffers we use BDA
        let descriptors = self
            .bindless_layout
            .create_descriptors(&self.device, &graph.resources.images, &graph.samplers);
        cmd.reference_resource(&descriptors);

        RENDER_GRAPH_RESOURCES.set(&graph.resources, || {
            // 3. record passes
            let mut ctx = RecordContext { cmd, descriptors };
            let ctx = &mut ctx;

            for pass in graph.passes {
                match pass.kind {
                    PassKind::FillBuffer(mut pass) => {
                        ctx.cmd.fill_buffer(&pass.buffer.buffer().byte_range(..), pass.value);
                    }
                    PassKind::Blit(mut pass) => {
                        let src = pass.src.image();
                        let dst = pass.dst.image();
                        let width = src.width() as i32;
                        let height = src.height() as i32;
                        ctx.cmd.blit_image(
                            &src,
                            ImageSubresourceLayers {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                mip_level: 0,
                                base_array_layer: 0,
                                layer_count: 1,
                            },
                            Rect3D {
                                min: Point3D { x: 0, y: 0, z: 0 },
                                max: Point3D { x: width, y: height, z: 1 },
                            },
                            &dst,
                            ImageSubresourceLayers {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                mip_level: 0,
                                base_array_layer: 0,
                                layer_count: 1,
                            },
                            Rect3D {
                                min: Point3D { x: 0, y: 0, z: 0 },
                                max: Point3D { x: width, y: height, z: 1 },
                            },
                            vk::Filter::NEAREST,
                        );
                    }
                    PassKind::MeshRender(mut pass) => {
                        let color_attachments: Vec<_> = pass
                            .color_attachments
                            .iter()
                            .map(|ca| graal::ColorAttachment {
                                image_view: ca.image.view(),
                                clear_value: ca.clear_value.unwrap_or_default(),
                                load_op: if ca.clear_value.is_some() {
                                    vk::AttachmentLoadOp::CLEAR
                                } else {
                                    vk::AttachmentLoadOp::LOAD
                                },
                                store_op: vk::AttachmentStoreOp::STORE,
                            })
                            .collect();

                        let depth_stencil_attachment = if let Some(ref dsa) = pass.depth_stencil_attachment {
                            Some(graal::DepthStencilAttachment {
                                image_view: dsa.image.view(),
                                depth_load_op: if dsa.depth_clear_value.is_some() {
                                    vk::AttachmentLoadOp::CLEAR
                                } else {
                                    vk::AttachmentLoadOp::LOAD
                                },
                                depth_store_op: vk::AttachmentStoreOp::STORE,
                                stencil_load_op: if dsa.stencil_clear_value.is_some() {
                                    vk::AttachmentLoadOp::CLEAR
                                } else {
                                    vk::AttachmentLoadOp::LOAD
                                },
                                stencil_store_op: vk::AttachmentStoreOp::STORE,
                                depth_clear_value: dsa.depth_clear_value.unwrap_or_default(),
                                stencil_clear_value: dsa.stencil_clear_value.unwrap_or_default(),
                            })
                        } else {
                            None
                        };

                        let extent;
                        if let Some(color) = pass.color_attachments.first() {
                            extent = color.image.view().size();
                        } else if let Some(ref depth) = pass.depth_stencil_attachment {
                            extent = depth.image.view().size();
                        } else {
                            panic!("render pass has no attachments");
                        }

                        pass.tracker.transition_resources(ctx);
                        let mut encoder = ctx.cmd.begin_rendering(RenderPassInfo {
                            color_attachments: &color_attachments[..],
                            depth_stencil_attachment,
                        });
                        encoder.bind_graphics_pipeline(&pass.pipeline.0.pipeline);
                        encoder.bind_resource_descriptors(&ctx.descriptors);
                        encoder.set_viewport(0.0, 0.0, extent.width as f32, extent.height as f32, 0.0, 1.0);
                        encoder.set_scissor(0, 0, extent.width, extent.height);
                        if let Some(cb) = pass.func.take() {
                            cb(&mut encoder);
                        }
                        encoder.finish();
                    }
                    PassKind::Compute(mut pass) => {
                        pass.base.transition_resources(ctx);
                        let mut encoder = ctx.cmd.begin_compute();
                        encoder.bind_compute_pipeline(&pass.pipeline.0.pipeline);
                        encoder.bind_resource_descriptors(&ctx.descriptors);
                        if let Some(cb) = pass.func.take() {
                            cb(&mut encoder);
                        }
                        encoder.finish();
                    }
                }
            }
        });

        cmd.flush(&[], &[]).unwrap()
    }*/

    pub fn define_global(&mut self, define: &str, value: impl ToString) {
        self.global_defs.insert(define.to_string(), value.to_string());
    }

    pub fn create_compute_pipeline(&mut self, name: &str, desc: ComputePipelineDesc) -> Result<ComputePipeline, Error> {
        if let Some(pipeline) = self.compute_pipelines.get(name) {
            return pipeline.clone();
        }

        let file_path = &desc.shader;
        let gdefs = &self.global_defs;
        let defs = &desc.defines;
        let mut ci = CompilationInfo::default();

        let compute_spv = match compile_shader_stage(&file_path, &gdefs, &defs, ShaderKind::Compute, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile compute shader: {err}");
                let result = Err(err.into());
                self.compute_pipelines.insert(name.to_string(), result.clone());
                return result;
            }
        };

        let cpci = ComputePipelineCreateInfo {
            set_layouts: &[],
            push_constants_size: ci.push_cst_size,
            compute_shader: ShaderEntryPoint {
                code: ShaderCode::Spirv(&compute_spv),
                entry_point: "main",
            },
        };

        match self.device.create_compute_pipeline(cpci) {
            Ok(pipeline) => {
                self.compute_pipelines.insert(name.to_string(), Ok(pipeline.clone()));
                Ok(pipeline)
            }
            Err(err) => {
                panic!("update_pipelines: failed to create compute pipeline: {:?}", err);
            }
        }
    }

    pub fn create_mesh_render_pipeline(&mut self, name: &str, desc: MeshRenderPipelineDesc) -> Result<GraphicsPipeline, Error> {
        if let Some(pipeline) = self.mesh_render_pipelines.get(name) {
            return pipeline.clone();
        }

        let task_file_path = &desc.task_shader;
        let mesh_file_path = &desc.mesh_shader;
        let frag_file_path = &desc.fragment_shader;
        let gdefs = &self.global_defs;
        let defs = &desc.defines;
        let mut ci = CompilationInfo::default();

        let task_spv = match compile_shader_stage(&task_file_path, &gdefs, &defs, ShaderKind::Task, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile task shader: {err}");
                let result = Err(err.into());
                self.mesh_render_pipelines.insert(name.to_string(), result.clone());
                return result;
            }
        };
        let mesh_spv = match compile_shader_stage(&mesh_file_path, &gdefs, &defs, ShaderKind::Mesh, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile mesh shader: {err}");
                let result = Err(err.into());
                self.mesh_render_pipelines.insert(name.to_string(), result.clone());
                return result;
            }
        };
        let fragment_spv = match compile_shader_stage(&frag_file_path, &gdefs, &defs, ShaderKind::Fragment, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile fragment shader: {err}");
                let result = Err(err.into());
                self.mesh_render_pipelines.insert(name.to_string(), result.clone());
                return result;
            }
        };

        let gpci = GraphicsPipelineCreateInfo {
            set_layouts: &[],
            push_constants_size: ci.push_cst_size,
            vertex_input: Default::default(),
            pre_rasterization_shaders: PreRasterizationShaders::MeshShading {
                task: Some(ShaderEntryPoint {
                    code: ShaderCode::Spirv(&task_spv),
                    entry_point: "main",
                }),
                mesh: ShaderEntryPoint {
                    code: ShaderCode::Spirv(&mesh_spv),
                    entry_point: "main",
                },
            },
            rasterization: desc.rasterization_state,
            depth_stencil: desc.depth_stencil_state,
            fragment: FragmentState {
                shader: ShaderEntryPoint {
                    code: ShaderCode::Spirv(&fragment_spv),
                    entry_point: "main",
                },
                multisample: Default::default(),
                color_targets: desc.color_targets.as_slice(),
                blend_constants: [0.0, 0.0, 0.0, 0.0],
            },
        };

        match self.device.create_graphics_pipeline(gpci) {
            Ok(pipeline) => {
                self.mesh_render_pipelines.insert(name.to_string(), Ok(pipeline.clone()));
                Ok(pipeline)
            }
            Err(err) => {
                panic!("update_pipelines: failed to create mesh render pipeline: {:?}", err);
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct PipelineCache {
    device: Device,
    graphics_pipelines: BTreeMap<String, Result<GraphicsPipeline, Error>>,
    compute_pipelines: BTreeMap<String, Result<ComputePipeline, Error>>,
}

impl PipelineCache {
    
    pub fn clear(&mut self) {
        self.graphics_pipelines.clear();
        self.compute_pipelines.clear();
    }
    
    pub fn create_compute_pipeline(&mut self, name: &str, entry_point: EntryPoint) -> Result<ComputePipeline, Error> {
        if let Some(pipeline) = self.compute_pipelines.get(name) {
            return pipeline.clone();
        }
        
        let code_buf;
        let code = if let Some(path) = entry_point.path {
            // always reload from path if provided
            let path = PathBuf::from(path);
            code_buf = fs::read(&path).map_err(|err| Error::ShaderReadError { path, error: Rc::new(err) })?;
            &code_buf
        } else {
            entry_point.code
        };

        let cpci = ComputePipelineCreateInfo {
            set_layouts: &[],
            push_constants_size: ci.push_cst_size,
            compute_shader: ShaderEntryPoint {
                code: ShaderCode::Spirv(&compute_spv),
                entry_point: "main",
            },
        };

        match self.device.create_compute_pipeline(cpci) {
            Ok(pipeline) => {
                self.compute_pipelines.insert(name.to_string(), Ok(pipeline.clone()));
                Ok(pipeline)
            }
            Err(err) => {
                panic!("update_pipelines: failed to create compute pipeline: {:?}", err);
            }
        }
    }
}