use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::{fs, slice};

use graal::shaderc::{EnvVersion, ShaderKind, SpirvVersion, TargetEnv};
use graal::util::DeviceExt;
use graal::vk::{Pipeline, Viewport};
use graal::{
    get_shader_compiler, shaderc, vk, BufferAccess, BufferRangeUntyped, BufferUsage, ColorTargetState, CommandStream,
    ComputeEncoder, ComputePipeline, ComputePipelineCreateInfo, DepthStencilAttachment, DepthStencilState, Device,
    FragmentState, GraphicsPipeline, GraphicsPipelineCreateInfo, ImageAccess, ImageCreateInfo, ImageSubresourceLayers,
    ImageUsage, ImageView, MemoryLocation, MultisampleState, Point3D, PreRasterizationShaders, RasterizationState,
    Rect3D, RenderEncoder, RenderPassInfo, SamplerCreateInfo, ShaderCode, ShaderEntryPoint,
};
use spirv_reflect::types::{ReflectDescriptorType, ReflectTypeFlags};
use tracing::{debug, error, warn};

use crate::engine::shader::{compile_shader_stage, CompilationInfo};
use crate::shaders::{compile_shader_module, CompilationError, EntryPoint};

//mod bindless;
mod shader;
//mod uniform_block;

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Error type for the rendering engine.
#[derive(thiserror::Error, Debug)]
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
    #[error("graphics layer error: {0}")]
    GraalError(#[from] graal::Error),
    #[error("Compilation error: {0}")]
    CompilationError(String),
    #[error("the shader had previous compilation errors")]
    PreviousCompilationErrors,
}

/*
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

    pub fn create_mesh_render_pipeline(
        &mut self,
        name: &str,
        desc: MeshRenderPipelineDesc,
    ) -> Result<GraphicsPipeline, Error> {
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
                self.mesh_render_pipelines
                    .insert(name.to_string(), Ok(pipeline.clone()));
                Ok(pipeline)
            }
            Err(err) => {
                panic!("update_pipelines: failed to create mesh render pipeline: {:?}", err);
            }
        }
    }
}
*/

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct MeshRenderPipelineDesc2<'a> {
    pub task_shader: EntryPoint<'a>,
    pub mesh_shader: EntryPoint<'a>,
    pub fragment_shader: EntryPoint<'a>,
    //pub defines: BTreeMap<String, String>,
    pub color_targets: Vec<ColorTargetState>,
    pub rasterization_state: RasterizationState,
    pub depth_stencil_state: Option<DepthStencilState>,
    pub multisample_state: MultisampleState,
}

pub struct PrimitiveRenderPipelineDesc2<'a> {
    pub vertex_shader: EntryPoint<'a>,
    pub fragment_shader: EntryPoint<'a>,
    //pub defines: BTreeMap<String, String>,
    pub color_targets: Vec<ColorTargetState>,
    pub rasterization_state: RasterizationState,
    pub depth_stencil_state: Option<DepthStencilState>,
    pub multisample_state: MultisampleState,
}

pub struct PipelineCache {
    device: Device,
    global_macro_definitions: BTreeMap<String, String>,
    graphics_pipelines: BTreeMap<String, Option<GraphicsPipeline>>,
    compute_pipelines: BTreeMap<String, Option<ComputePipeline>>,
}

impl PipelineCache {
    pub fn new(device: Device) -> Self {
        Self {
            device,
            global_macro_definitions: Default::default(),
            graphics_pipelines: Default::default(),
            compute_pipelines: Default::default(),
        }
    }

    pub fn set_global_macro_definitions(&mut self, defines: BTreeMap<String, String>) {
        self.global_macro_definitions = defines;
        self.clear();
    }

    pub fn clear(&mut self) {
        self.graphics_pipelines.clear();
        self.compute_pipelines.clear();
    }

    fn reload_shader<'a>(&mut self, name: &str, entry_point: &'a EntryPoint<'a>) -> Result<Cow<'a, [u8]>, Error> {
        #[cfg(feature = "shader-hot-reload")]
        {
            if let Some(ref path) = entry_point.source_path {
                // recompile from path if provided
                let path = PathBuf::from(path.as_ref());

                let mut macro_defs = Vec::new();
                for (k, v) in self.global_macro_definitions.iter() {
                    macro_defs.push((k.as_str(), v.as_str()));
                }
                let result = compile_shader_module(&path, &[], &macro_defs, entry_point.name.as_ref());
                match result {
                    Ok(blob) => {
                        return Ok(Cow::Owned(blob));
                    }
                    Err(err) => {
                        return Err(Error::CompilationError(format!("{err}")));
                    }
                }
            }
        }

        Ok(Cow::Borrowed(entry_point.code.as_ref()))
    }

    fn create_compute_pipeline_internal(
        &mut self,
        name: &str,
        entry_point: &EntryPoint,
    ) -> Result<ComputePipeline, Error> {
        let code = self.reload_shader(name, entry_point)?;
        let cpci = ComputePipelineCreateInfo {
            set_layouts: &[],
            push_constants_size: entry_point.push_constants_size as usize,
            compute_shader: ShaderEntryPoint {
                code: ShaderCode::Spirv(bytemuck::cast_slice(&*code)),
                entry_point: entry_point.name.as_ref(),
            },
        };
        Ok(self.device.create_compute_pipeline(cpci)?)
    }

    pub fn create_compute_pipeline(&mut self, name: &str, entry_point: &EntryPoint) -> Result<ComputePipeline, Error> {
        if let Some(pipeline) = self.compute_pipelines.get(name) {
            match pipeline {
                Some(pipeline) => return Ok(pipeline.clone()),
                None => return Err(Error::PreviousCompilationErrors),
            }
        }
        match self.create_compute_pipeline_internal(name, entry_point) {
            Ok(pipeline) => {
                self.compute_pipelines.insert(name.to_string(), Some(pipeline.clone()));
                Ok(pipeline)
            }
            Err(err) => {
                self.compute_pipelines.insert(name.to_string(), None);
                Err(err)
            }
        }
    }

    fn create_primitive_pipeline_internal(
        &mut self,
        name: &str,
        desc: &PrimitiveRenderPipelineDesc2,
    ) -> Result<GraphicsPipeline, Error> {
        let vertex = self.reload_shader(name, &desc.vertex_shader)?;
        let fragment = self.reload_shader(name, &desc.fragment_shader)?;
        let gpci = GraphicsPipelineCreateInfo {
            set_layouts: &[],
            push_constants_size: 0,
            vertex_input: Default::default(),
            pre_rasterization_shaders: PreRasterizationShaders::PrimitiveShading {
                vertex: ShaderEntryPoint {
                    code: ShaderCode::Spirv(bytemuck::cast_slice(&*vertex)),
                    entry_point: desc.vertex_shader.name.as_ref(),
                },
                tess_control: None,
                tess_evaluation: None,
                geometry: None,
            },
            rasterization: desc.rasterization_state,
            depth_stencil: desc.depth_stencil_state,
            fragment: FragmentState {
                shader: ShaderEntryPoint {
                    code: ShaderCode::Spirv(bytemuck::cast_slice(&*fragment)),
                    entry_point: desc.fragment_shader.name.as_ref(),
                },
                multisample: Default::default(),
                color_targets: desc.color_targets.as_slice(),
                blend_constants: [0.0, 0.0, 0.0, 0.0],
            },
        };
        Ok(self.device.create_graphics_pipeline(gpci)?)
    }

    pub fn create_primitive_pipeline(
        &mut self,
        name: &str,
        desc: &PrimitiveRenderPipelineDesc2,
    ) -> Result<GraphicsPipeline, Error> {
        if let Some(pipeline) = self.graphics_pipelines.get(name) {
            match pipeline {
                Some(pipeline) => return Ok(pipeline.clone()),
                None => return Err(Error::PreviousCompilationErrors),
            }
        }
        match self.create_primitive_pipeline_internal(name, desc) {
            Ok(pipeline) => {
                self.graphics_pipelines.insert(name.to_string(), Some(pipeline.clone()));
                Ok(pipeline)
            }
            Err(err) => {
                self.graphics_pipelines.insert(name.to_string(), None);
                Err(err)
            }
        }
    }
}
