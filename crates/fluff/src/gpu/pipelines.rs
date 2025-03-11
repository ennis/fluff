use crate::gpu;
use crate::gpu::with_pipeline_manager;
use graal::{
    vk, ColorTargetState, ComputePipeline, ComputePipelineCreateInfo, DepthStencilState, RcDevice, FragmentState,
    GraphicsPipeline, GraphicsPipelineCreateInfo, MultisampleState, PreRasterizationShaders, RasterizationState,
    ShaderDescriptor,
};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

/// Error type for the rendering engine.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to load configuration file")]
    ConfigLoadError,
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

pub struct MeshRenderPipelineDesc2<'a> {
    pub task_shader: ShaderDescriptor<'a>,
    pub mesh_shader: ShaderDescriptor<'a>,
    pub fragment_shader: ShaderDescriptor<'a>,
    pub color_targets: Vec<ColorTargetState>,
    pub rasterization_state: RasterizationState,
    pub depth_stencil_state: Option<DepthStencilState>,
    pub multisample_state: MultisampleState,
}

pub struct PrimitiveRenderPipelineDesc2<'a> {
    pub vertex_shader: ShaderDescriptor<'a>,
    pub fragment_shader: ShaderDescriptor<'a>,
    pub color_targets: Vec<ColorTargetState>,
    pub rasterization_state: RasterizationState,
    pub depth_stencil_state: Option<DepthStencilState>,
    pub multisample_state: MultisampleState,
}

/// Responsible for creating compute & graphics pipelines on demand.
///
/// This struct is used to manage the creation of pipelines and caches the results.
/// With the `shader-hot-reload` feature enabled, the pipelines will be recompiled from source
/// files.
///
/// It also holds a set of global macro definitions that are applied to all shaders.
pub struct PipelineManager {
    global_macro_definitions: BTreeMap<String, String>,
    graphics_pipelines: BTreeMap<String, Option<GraphicsPipeline>>,
    compute_pipelines: BTreeMap<String, Option<ComputePipeline>>,
}

impl PipelineManager {
    pub fn new() -> Self {
        Self {
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

    fn reload_shader<'a>(&mut self, shader: &'a ShaderDescriptor<'a>) -> Result<Cow<'a, [u32]>, Error> {
        #[cfg(feature = "shader-hot-reload")]
        {
            if let Some(ref path) = shader.source_path {
                // recompile from path if provided
                let path = PathBuf::from(path);

                let mut macro_defs = Vec::new();
                for (k, v) in self.global_macro_definitions.iter() {
                    macro_defs.push((k.as_str(), v.as_str()));
                }
                let result = shader_bridge::compile_shader_module(&path, &[], &macro_defs, shader.entry_point);
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

        Ok(Cow::Borrowed(shader.code))
    }

    fn create_compute_pipeline_internal(
        &mut self,
        compute_shader: &ShaderDescriptor,
    ) -> Result<ComputePipeline, Error> {
        let code = self.reload_shader(compute_shader)?;
        let cpci = ComputePipelineCreateInfo {
            set_layouts: &[],
            push_constants_size: compute_shader.push_constants_size,
            shader: ShaderDescriptor {
                code: code.as_ref(),
                ..*compute_shader
            },
        };
        Ok(gpu::device().create_compute_pipeline(cpci)?)
    }

    pub fn create_compute_pipeline(
        &mut self,
        name: &str,
        compute_shader: &ShaderDescriptor,
    ) -> Result<ComputePipeline, Error> {
        if let Some(pipeline) = self.compute_pipelines.get(name) {
            match pipeline {
                Some(pipeline) => return Ok(pipeline.clone()),
                None => return Err(Error::PreviousCompilationErrors),
            }
        }
        match self.create_compute_pipeline_internal(compute_shader) {
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
        desc: &PrimitiveRenderPipelineDesc2,
    ) -> Result<GraphicsPipeline, Error> {
        let vertex = self.reload_shader(&desc.vertex_shader)?;
        let fragment = self.reload_shader(&desc.fragment_shader)?;
        let gpci = GraphicsPipelineCreateInfo {
            set_layouts: &[],
            push_constants_size: desc.vertex_shader.push_constants_size, // FIXME
            vertex_input: Default::default(),
            pre_rasterization_shaders: PreRasterizationShaders::PrimitiveShading {
                vertex: ShaderDescriptor {
                    code: vertex.as_ref(),
                    ..desc.vertex_shader
                },
            },
            rasterization: desc.rasterization_state,
            depth_stencil: desc.depth_stencil_state,
            fragment: FragmentState {
                shader: ShaderDescriptor {
                    code: fragment.as_ref(),
                    ..desc.fragment_shader
                },
                multisample: Default::default(),
                color_targets: desc.color_targets.as_slice(),
                blend_constants: [0.0, 0.0, 0.0, 0.0],
            },
        };
        Ok(gpu::device().create_graphics_pipeline(gpci)?)
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
        match self.create_primitive_pipeline_internal(desc) {
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

////////////////////////////////////////////////////////////////////////////////////////////////////

pub fn invalidate_pipelines() {
    with_pipeline_manager(|manager| manager.clear());
}

pub fn set_global_pipeline_macro_definitions(defs: BTreeMap<String, String>) {
    with_pipeline_manager(|manager| manager.set_global_macro_definitions(defs));
}

pub fn create_compute_pipeline(name: &str, compute_shader: &ShaderDescriptor) -> Result<graal::ComputePipeline, Error> {
    with_pipeline_manager(|manager| manager.create_compute_pipeline(name, compute_shader))
}

pub fn create_primitive_pipeline(
    name: &str,
    desc: &PrimitiveRenderPipelineDesc2,
) -> Result<graal::GraphicsPipeline, Error> {
    with_pipeline_manager(|manager| manager.create_primitive_pipeline(name, desc))
}
