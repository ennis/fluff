use crate::pass::PassError::ReflectionError;
use egui::{TextBuffer, Ui};
use graal::{
    compile_shader, shaderc, shaderc::CompileOptions, vk, BufferUntyped, ComputePipeline, Device, Format, GraphicsPipeline,
    GraphicsPipelineCreateInfo, Image, ShaderSource, ShaderStage,
};
use spirv_reflect::types::{ReflectBlockVariable, ReflectDescriptorBinding};
use winit::event::DeviceId;

mod bin_curves;

/// Describes an image resource required by a pass.
#[derive(Debug, Clone)]
pub struct ImageDescriptor {
    /// Name of the uniform in the shader.
    ///
    /// It is matched against the name of the uniform in the shader.
    pub name: &'static str,
    /// Allowed formats for the image.
    ///
    /// If none are specified, it will allocate an image with the same format as the main swap chain.
    pub formats: &'static [Format],
    /// Base width of the image.
    ///
    /// If unspecified, it will use the width of the main swap chain.
    /// The final width will be `width.div_ceil(width_divisor)`.
    pub width: Option<u32>,
    /// Height of the image.
    ///
    /// If unspecified, it will use the height of the main swap chain.
    /// The final height will be `height.div_ceil(height_divisor)`.
    pub height: Option<u32>,

    /// Width divisor.
    pub width_divisor: Option<u32>,

    /// Height divisor.
    pub height_divisor: Option<u32>,
}

impl Default for ImageDescriptor {
    fn default() -> Self {
        Self {
            name: "",
            formats: &[],
            width: None,
            height: None,
            width_divisor: None,
            height_divisor: None,
        }
    }
}

/// Describes a buffer resource required by a pass.
#[derive(Debug, Clone)]
pub struct BufferDescriptor {
    /// Name of the buffer block in the shader.
    ///
    /// It is matched against the name of the uniform in the shader.
    pub name: &'static str,

    /// Size of the buffer, in **number of elements**.
    ///
    /// The byte size is inferred from the type of the buffer in the reflected SPIR-V.
    ///
    /// `None` means that the count will be `width * height`, where `width` and `height` are the dimensions of the main swap chain.
    /// The final count will be `count.div_ceil(width_divisor*height_divisor)`.
    pub count: Option<u64>,

    /// Width divisor.
    ///
    /// See `count`.
    pub width_divisor: Option<u32>,

    /// Height divisor.
    ///
    /// See `count`.
    pub height_divisor: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub enum PipelineMode {
    PrimitiveShading, // VS + FS
    MeshShading,      // TS + MS + FS
    Compute,          // CS
}

/// Describes a render pipeline.
#[derive(Debug, Clone)]
pub struct PipelineDescriptor {
    pub mode: PipelineMode,
    pub unified_shader_file: &'static str,
    pub defines: Vec<(&'static str, Option<String>)>,
}

impl Default for PipelineDescriptor {
    fn default() -> Self {
        Self {
            mode: PipelineMode::PrimitiveShading,
            unified_shader_file: "",
            defines: vec![],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PassError {
    #[error("Reflection error: {0}")]
    ReflectionError(&'static str),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Graphics API error: {0}")]
    GraphicsApiError(#[from] graal::Error),
}

/// The prefix to prepend to the shader source code.
///
/// Declares the GLSL version (460 core), enables some common extensions,
/// and sets the default block layout for buffer & uniform blocks to the scalar block layout.
const SHADER_PREFIX: &str = r#"
#version 460 core
#extension GL_EXT_scalar_block_layout : require
#extension GL_EXT_shader_explicit_arithmetic_types : require
layout(scalar) buffer;
layout(scalar) uniform;
"#;

struct CollectedReflectionInfo {
    push_constant_block: Option<ReflectBlockVariable>,
    bindings: Vec<(vk::ShaderStageFlags, ReflectDescriptorBinding)>,
}

impl CollectedReflectionInfo {
    fn new() -> Self {
        Self {
            push_constant_block: None,
            bindings: vec![],
        }
    }

    fn compile_and_reflect_shader(
        &mut self,
        stage: ShaderStage,
        source: ShaderSource,
        entry_point: &str,
        compile_options: &CompileOptions,
    ) -> Result<Vec<u32>, PassError> {
        let stage_flag = stage.to_vk_shader_stage();
        let shader_spv = compile_shader(stage, source, entry_point, SHADER_PREFIX, compile_options.clone().unwrap())?;
        let reflect_module = spirv_reflect::ShaderModule::load_u32_data(&shader_spv).map_err(ReflectionError)?;

        // push constants
        let push_constant_blocks = reflect_module
            .enumerate_push_constant_blocks(Some("main"))
            .map_err(ReflectionError)?;
        if push_constant_blocks.len() > 1 {
            return Err(ReflectionError("Only at most one push constant block is supported"));
        }

        // we expect the push constant blocks to be the same between stages
        if let Some(pcb) = push_constant_blocks.get(0) {
            if self.push_constant_block.is_none() {
                self.push_constant_block = Some(pcb.clone());
            }
        }

        // uniforms
        let descriptor_sets = reflect_module.enumerate_descriptor_sets(Some("main")).map_err(ReflectionError)?;
        if descriptor_sets.len() > 1 {
            return Err(ReflectionError("Only at most one descriptor set is supported"));
        }

        if let Some(descriptor_set) = descriptor_sets.into_iter().next() {
            for binding in descriptor_set.bindings {
                if let Some((stages, _binding)) = self.bindings.iter_mut().find(|(_, b)| b.binding == binding.binding) {
                    *stages |= stage_flag;
                } else {
                    self.bindings.push((stage_flag, binding));
                }
            }
        }

        Ok(shader_spv)
    }
}

pub fn compile_pass_pipeline(device: &Device, desc: &PipelineDescriptor) -> Result<(), PassError> {
    use PassError::ReflectionError;

    let mut compile_options = shaderc::CompileOptions::new().unwrap();
    compile_options.set_auto_bind_uniforms(true);
    compile_options.set_auto_map_locations(true);

    for (define, value) in desc.defines.iter() {
        compile_options.add_macro_definition(define, value.as_deref());
    }

    let unified_shader_file = ShaderSource::File(desc.unified_shader_file.as_ref());
    let mut refl = CollectedReflectionInfo::new();

    match desc.mode {
        PipelineMode::PrimitiveShading => {
            let vs = refl.compile_and_reflect_shader(ShaderStage::Vertex, unified_shader_file, "main", &compile_options)?;
            let fs = refl.compile_and_reflect_shader(ShaderStage::Fragment, unified_shader_file, "main", &compile_options)?;
        }
        PipelineMode::MeshShading => {
            let ts = refl.compile_and_reflect_shader(ShaderStage::Task, unified_shader_file, "main", &compile_options)?;
            let ms = refl.compile_and_reflect_shader(ShaderStage::Mesh, unified_shader_file, "main", &compile_options)?;
            let fs = refl.compile_and_reflect_shader(ShaderStage::Fragment, unified_shader_file, "main", &compile_options)?;
        }
        PipelineMode::Compute => {
            let cs = refl.compile_and_reflect_shader(ShaderStage::Compute, unified_shader_file, "main", &compile_options)?;
        }
    }

    dbg!(refl.push_constant_block);
    dbg!(refl.bindings);

    Ok(())
}

pub struct PassResourceCtx {
    // TODO
}

pub struct PassResourcesDescriptor {
    // TODO
    pub images: Vec<ImageDescriptor>,
    pub buffers: Vec<BufferDescriptor>,
}

//device: Device,/ Describes a graphics pipeline.
pub trait PassDelegate {
    fn declare_pipeline(&self) -> PipelineDescriptor;
    fn declare_resources(&self, ctx: &PassResourceCtx) -> PassResourcesDescriptor;
    fn ui(&mut self, ui: &mut Ui) -> bool;
}

// On every frame, check which passes need to be recompiled, or that have resources that need to be reallocated.

struct PassBuffer {
    name: String,
    desc: BufferDescriptor,
    buffer: Option<BufferUntyped>,
    elem_size: usize,
}

struct PassImage {
    name: String,
    desc: ImageDescriptor,
    image: Option<Image>,
}

enum Pipeline {
    MeshShading {
        pipeline: GraphicsPipeline,
        task_shader_spv: Vec<u32>,
        mesh_shader_spv: Vec<u32>,
        fragment_shader_spv: Vec<u32>,
    },
    PrimitiveShading {
        pipeline: GraphicsPipeline,
        vertex_shader_spv: Vec<u32>,
        fragment_shader_spv: Vec<u32>,
    },
    Compute {
        pipeline: ComputePipeline,
        compute_shader_spv: Vec<u32>,
    },
}

struct Pass {
    delegate: Option<Box<dyn PassDelegate>>,
    buffers: Vec<PassBuffer>,
    images: Vec<PassImage>,
    pipeline: Option<Pipeline>,
}

struct PassManager {
    passes: Vec<Pass>,
}

impl PassManager {
    fn new() -> Self {
        Self { passes: vec![] }
    }

    fn add_pass(&mut self, pass: impl PassDelegate) {}

    fn run(&mut self) {}
}
