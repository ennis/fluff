use crate::camera_control::Camera;
use glam::uvec2;
use graal::{get_shader_compiler, shaderc, shaderc::{EnvVersion, ShaderKind, SpirvVersion, TargetEnv}, vk, vk::{AttachmentLoadOp, AttachmentStoreOp}, ArgumentsLayout, CommandStream, CompareOp, GraphicsPipeline, GraphicsPipelineCreateInfo, ImageId, ImageView, PipelineLayoutDescriptor, PreRasterizationShaders, RasterizationState, SamplerCreateInfo, ShaderCode, ShaderEntryPoint, ShaderStage, Size2D, BufferUsage, MemoryLocation, ImageUsage};
use mlua::FromLua;
use slotmap::{new_key_type, SlotMap};
use spirv_reflect::types::ReflectDescriptorType;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    ffi::c_char,
    path::{Path, PathBuf},
    ptr,
};
use std::ffi::c_void;
use tracing::{error, info, trace, warn};

mod config;
mod pipeline;
mod shader;

new_key_type! {
    struct ImageKey;
    struct BufferKey;
}

/// Location of the binding assigned to a resource.
///
/// Every resource is assigned a different binding, which means that we can create one single descriptor set
/// for all resources, bind it once, and forget about it.
#[derive(Copy, Clone, Debug, Default)]
struct BindingLocation {
    /// Descriptor set index.
    ///
    /// For now only one set is used, so this is always 0.
    set: u32,
    /// Binding index.
    binding: u32,
}

/// Describes an image resource.
///
/// # Dimensions
///
/// The dimensions of the image are either specified directly (using `width` and `height`),
/// or expressed as a divisor of the main render target dimensions (using `width_divisor` and `height_divisor`).
/// In the latter case, the dimensions are computed as follows:
///
/// ```
/// width = main_rt_width.div_ceil(width_divisor)
/// height = main_rt_height.div_ceil(height_divisor)
/// ```
struct ImageResource {
    name: String,
    /// Image format.
    format: vk::Format,
    /// Inferred usage flags.
    inferred_usage: ImageUsage,

    /// Explicit width.
    ///
    /// If specified, this takes precedence over `width_divisor`.
    width: Option<u32>,

    /// Explicit height.
    ///
    /// If specified, this takes precedence over `height_divisor`.
    height: Option<u32>,

    /// Width divisor w.r.t the width of the main render target.
    width_divisor: u32,

    /// Height divisor w.r.t the height of the main render target.
    height_divisor: u32,

    /// The allocated image resource.
    image: Option<graal::Image>,

    /// Binding to access the image as a sampled image.
    texture_binding: BindingLocation,

    /// Binding to access the image as a storage image.
    storage_binding: BindingLocation,
}

/// Describes a buffer resource.
///
/// # Size
///
/// The size of a buffer can be specified directly as a size in bytes (using `byte_size`),
/// but, like images, it can also be specified as a divisor of the main render target dimensions.
/// In this case, `element_size` must be specified.
///
/// The byte size then is computed as follows:
/// ```
/// byte_size = main_rt_width.div_ceil(width_divisor) * main_rt_height.div_ceil(height_divisor) * element_size
/// ```
struct BufferResource {
    name: String,

    /// Buffer usage flags.
    inferred_usage: BufferUsage,
    /// Inferred memory properties.
    inferred_memory_properties: vk::MemoryPropertyFlags,

    /// Explicit byte size.
    ///
    /// If specified, this takes precedence over `width_divisor` and `height_divisor`.
    byte_size: Option<usize>,

    /// Width divisor w.r.t the width of the main render target.
    width_divisor: u32,

    /// Height divisor w.r.t the height of the main render target.
    height_divisor: u32,

    /// The allocated buffer resource.
    buffer: Option<graal::BufferUntyped>,

    /// Binding to access the buffer as a uniform buffer.
    uniform_binding: BindingLocation,

    /// Binding to access the buffer as a storage buffer.
    storage_binding: BindingLocation,
}

/// `vkCmdFillBuffer` pass.
struct FillBufferPass {
    buffer_name: String,
    value: u32,
}

impl FillBufferPass {
    fn from_lua(lua: &mlua::Table) -> Result<Self, Error> {
        let buffer_name = lua.get_checked::<String>("buffer")?;
        let value = lua.get_checked::<u32>("value")?;
        Ok(FillBufferPass { buffer_name, value })
    }
}

#[derive(Clone)]
enum ShaderSource {
    File(PathBuf),
    Content(String),
}

#[derive(Clone)]
struct ShaderDescriptor {
    source: ShaderSource,
    defines: BTreeMap<String, String>,
}

impl ShaderDescriptor {
    fn from_lua(lua: &mlua::Table) -> Result<Self, Error> {
        let file = lua.get_checked::<String>("file")?;
        let defines: BTreeMap<String, String> = lua.get_checked("defines")?;
        Ok(ShaderDescriptor {
            source: ShaderSource::File(file.into()),
            defines,
        })
    }
}

pub struct ShaderModule {
    device: graal::Device,
    module: vk::ShaderModule,
    stage: ShaderStage,
}

impl Drop for ShaderModule {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_shader_module(self.module, None);
        }
    }
}

struct MeshShadingPass {
    shader: ShaderDescriptor,
    color_attachments: Vec<ColorAttachment>,
    depth_stencil_attachment: Option<DepthStencilAttachment>,
    //color_outputs: Vec<Attachment>,
    //depth_stencil: Option<Attachment>,
    pipeline: Option<graal::GraphicsPipeline>,
    draw: mlua::OwnedFunction,
}

enum Pass {
    FillBuffer(FillBufferPass),
    MeshShading(MeshShadingPass),
}

type BindingMap = BTreeMap<(String, DescriptorType), BindingLocation>;
type GlobalDefs = BTreeMap<String, String>;

/// Rendering engine instance.
///
/// # Descriptors
///
/// To simplify things, we use a single big descriptor set containing descriptors for all combinations
/// of resources and possible accesses (e.g., for each image, a descriptor for sampled access and
/// another for storage access, and for buffers, a descriptor for uniform access and another for storage).
pub struct Engine {
    /// Lua context.
    lua: mlua::Lua,

    device: graal::Device,
    config_file: PathBuf,
    images: SlotMap<ImageKey, ImageResource>,
    buffers: SlotMap<BufferKey, BufferResource>,
    images_by_name: BTreeMap<String, ImageKey>,
    buffers_by_name: BTreeMap<String, BufferKey>,
    passes: Vec<Pass>,
    global_defs: GlobalDefs,

    /// Layout of the descriptor set containing all resources.
    bindings: Vec<vk::DescriptorSetLayoutBinding>,
    /// Maps resource names to their bindings.
    binding_map: BindingMap,

    /// Size of the common parameter block (push constants)
    param_block_size: usize,

}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to load configuration file")]
    ConfigLoadError,
    #[error("Failed to execute Lua script: {0}")]
    ScriptError(#[from] mlua::Error),
    #[error("Unsupported image format: {0}")]
    UnsupportedImageFormat(String),
    #[error("Missing required property: {0}")]
    MissingProperty(&'static str),
    #[error("Resource not found: {0}")]
    ResourceNotFound(String),
    #[error("File I/O error: {0}")]
    IO(#[from] std::io::Error),
    #[error("could not read shader file `{}`: {}", .path.display(), .error)]
    ShaderReadError { path: PathBuf, error: std::io::Error },
    #[error("Shader compilation error: {0}")]
    ShaderCompilation(#[from] shaderc::Error),
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),
    #[error("Vulkan error: {0}")]
    Vulkan(#[from] vk::Result),
}

/// String to VkFormat conversion
fn vk_format_from_str(s: &str) -> Result<vk::Format, Error> {
    match s {
        // 8-bit formats
        "r8unorm" => Ok(vk::Format::R8_UNORM),
        "r8snorm" => Ok(vk::Format::R8_SNORM),
        "r8uint" => Ok(vk::Format::R8_UINT),
        "r8sint" => Ok(vk::Format::R8_SINT),

        // 16-bit formats
        "r16uint" => Ok(vk::Format::R16_UINT),
        "r16sint" => Ok(vk::Format::R16_SINT),
        "r16float" => Ok(vk::Format::R16_SFLOAT),
        "rg8unorm" => Ok(vk::Format::R8G8_UNORM),
        "rg8snorm" => Ok(vk::Format::R8G8_SNORM),
        "rg8uint" => Ok(vk::Format::R8G8_UINT),
        "rg8sint" => Ok(vk::Format::R8G8_SINT),

        // 32-bit formats
        "r32uint" => Ok(vk::Format::R32_UINT),
        "r32sint" => Ok(vk::Format::R32_SINT),
        "r32float" => Ok(vk::Format::R32_SFLOAT),
        "rg16uint" => Ok(vk::Format::R16G16_UINT),
        "rg16sint" => Ok(vk::Format::R16G16_SINT),
        "rg16float" => Ok(vk::Format::R16G16_SFLOAT),
        "rgba8unorm" => Ok(vk::Format::R8G8B8A8_UNORM),
        "rgba8unorm-srgb" => Ok(vk::Format::R8G8B8A8_SRGB),
        "rgba8snorm" => Ok(vk::Format::R8G8B8A8_SNORM),
        "rgba8uint" => Ok(vk::Format::R8G8B8A8_UINT),
        "rgba8sint" => Ok(vk::Format::R8G8B8A8_SINT),
        "bgra8unorm" => Ok(vk::Format::B8G8R8A8_UNORM),
        "bgra8unorm-srgb" => Ok(vk::Format::B8G8R8A8_SRGB),

        // 64-bit formats
        "rg32uint" => Ok(vk::Format::R32G32_UINT),
        "rg32sint" => Ok(vk::Format::R32G32_SINT),
        "rg32float" => Ok(vk::Format::R32G32_SFLOAT),
        "rgba16uint" => Ok(vk::Format::R16G16B16A16_UINT),
        "rgba16sint" => Ok(vk::Format::R16G16B16A16_SINT),
        "rgba16float" => Ok(vk::Format::R16G16B16A16_SFLOAT),

        // 128-bit formats
        "rgba32uint" => Ok(vk::Format::R32G32B32A32_UINT),
        "rgba32sint" => Ok(vk::Format::R32G32B32A32_SINT),
        "rgba32float" => Ok(vk::Format::R32G32B32A32_SFLOAT),

        // Depth formats
        "depth16unorm" => Ok(vk::Format::D16_UNORM),
        "depth24unorm" => Ok(vk::Format::X8_D24_UNORM_PACK32),
        "depth32float" => Ok(vk::Format::D32_SFLOAT),
        "depth24unorm_stencil8" => Ok(vk::Format::D24_UNORM_S8_UINT),
        "depth32float_stencil8" => Ok(vk::Format::D32_SFLOAT_S8_UINT),

        _ => Err(Error::UnsupportedImageFormat(s.to_string())),
    }
}

trait TableExt<'lua> {
    fn get_checked<T: FromLua<'lua>>(&self, key: &'static str) -> Result<T, Error>;
    fn parse_str<T>(&self, key: &str, f: impl FnMut(Option<&str>) -> T) -> T;
    fn parse_str_required<T>(&self, key: &str, f: impl FnMut(Option<&str>) -> T) -> T;
}

impl<'lua> TableExt<'lua> for mlua::Table<'lua> {
    fn get_checked<T: FromLua<'lua>>(&self, key: &'static str) -> Result<T, Error> {
        self.get::<_, T>(key).map_err(|_| Error::MissingProperty(key))
    }

    fn parse_str<T>(&self, key: &str, mut f: impl FnMut(Option<&str>) -> T) -> T {
        match self.get::<_, Option<String>>(key) {
            Ok(v) => f(v.as_deref()),
            Err(err) => {
                error!("invalid property `{key}`: `{err}`");
                f(None)
            }
        }
    }

    fn parse_str_required<T>(&self, key: &str, mut f: impl FnMut(Option<&str>) -> T) -> T {
        match self.get::<_, Option<String>>(key) {
            Ok(Some(v)) => f(Some(&v)),
            Ok(None) => {
                error!("missing required property `{key}`");
                f(None)
            }
            Err(err) => {
                error!("invalid property `{key}`: `{err}`");
                f(None)
            }
        }
    }
}

fn parse_rasterization_state(lua: &mlua::Table) -> RasterizationState {
    let polygon_mode = lua.parse_str("polygonMode", |v| match v {
        Some("fill") => graal::PolygonMode::Fill,
        Some("line") => graal::PolygonMode::Line,
        Some("point") => graal::PolygonMode::Point,
        None => graal::PolygonMode::Fill,
        Some(mode) => {
            warn!("invalid polygon mode `{}`", mode);
            graal::PolygonMode::Fill
        }
    });

    let cull_mode = lua.parse_str("cullMode", |v| match v {
        Some("none") => graal::CullMode::NONE,
        Some("front") => graal::CullMode::FRONT,
        Some("back") => graal::CullMode::BACK,
        None => graal::CullMode::NONE,
        Some(cull_mode) => {
            warn!("invalid cull mode `{}`", cull_mode);
            graal::CullMode::NONE
        }
    });

    let front_face = lua.parse_str("frontFace", |v| match v {
        Some("cw") => graal::FrontFace::Clockwise,
        Some("ccw") => graal::FrontFace::CounterClockwise,
        None => graal::FrontFace::CounterClockwise,
        Some(front_face) => {
            warn!("invalid front face `{}`", front_face);
            graal::FrontFace::CounterClockwise
        }
    });

    RasterizationState {
        polygon_mode,
        cull_mode,
        front_face,
        line_rasterization: Default::default(),
        conservative_rasterization_mode: Default::default(),
    }
}

fn parse_depth_stencil_state(lua: &mlua::Table) {}

struct ColorAttachment {
    // name of the referenced texture
    image: ImageKey,
    clear_value: [f64; 4],
    load_op: vk::AttachmentLoadOp,
    store_op: vk::AttachmentStoreOp,
    image_view: Option<ImageView>,
}

struct DepthStencilAttachment {
    image: ImageKey,
    clear_value: f64,
    depth_load_op: vk::AttachmentLoadOp,
    depth_store_op: vk::AttachmentStoreOp,
    stencil_load_op: vk::AttachmentLoadOp,
    stencil_store_op: vk::AttachmentStoreOp,
    image_view: Option<ImageView>,
}

fn parse_filter(filter: Option<&str>) -> vk::Filter {
    match filter {
        Some("nearest") => vk::Filter::NEAREST,
        Some("linear") => vk::Filter::LINEAR,
        None => vk::Filter::NEAREST,
        Some(filter) => {
            warn!("invalid filter `{}`", filter);
            vk::Filter::NEAREST
        }
    }
}

fn parse_mipmap_mode(mode: Option<&str>) -> vk::SamplerMipmapMode {
    match mode {
        Some("nearest") => vk::SamplerMipmapMode::NEAREST,
        Some("linear") => vk::SamplerMipmapMode::LINEAR,
        None => vk::SamplerMipmapMode::NEAREST,
        Some(mode) => {
            warn!("invalid mipmap mode `{}`", mode);
            vk::SamplerMipmapMode::NEAREST
        }
    }
}

fn parse_address_mode(mode: Option<&str>) -> vk::SamplerAddressMode {
    match mode {
        Some("clamp-to-edge") => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        Some("repeat") => vk::SamplerAddressMode::REPEAT,
        Some("mirror-repeat") => vk::SamplerAddressMode::MIRRORED_REPEAT,
        None => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        Some(mode) => {
            warn!("invalid address mode `{}`", mode);
            vk::SamplerAddressMode::REPEAT
        }
    }
}

fn parse_sampler(lua: &mlua::Table) -> graal::SamplerCreateInfo {
    let min_filter = lua.parse_str("minFilter", parse_filter);
    let mag_filter = lua.parse_str("magFilter", parse_filter);
    let address_mode_u = lua.parse_str("addressModeU", parse_address_mode);
    let address_mode_v = lua.parse_str("addressModeV", parse_address_mode);
    let address_mode_w = lua.parse_str("addressModeW", parse_address_mode);
    let mipmap_mode = lua.parse_str("mipmapFilter", parse_mipmap_mode);

    graal::SamplerCreateInfo {
        min_filter,
        mag_filter,
        address_mode_u,
        address_mode_v,
        address_mode_w,
        mipmap_mode,
        ..Default::default()
    }
}

struct ResourceUsageMap {
    image_usage: BTreeMap<String, vk::ImageUsageFlags>,
    buffer_usage: BTreeMap<String, vk::BufferUsageFlags>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum DescriptorType {
    UniformBuffer,
    StorageBuffer,
    SampledImage,
    StorageImage,
}

struct RenderData {
    /// Width of the main render target.
    width: u32,
    /// Height of the main render target.
    height: u32,
    /// Which animation frame are we rendering.
    frame: usize,
    /// How many curves in the current animation frame.
    curve_count: usize,
    /// Camera parameters.
    camera: Camera,
}

impl Engine {
    pub fn new(device: graal::Device, param_block_size: usize) -> Self {
        Self {
            device,
            config_file: Default::default(),
            images: Default::default(),
            buffers: Default::default(),
            images_by_name: Default::default(),
            buffers_by_name: Default::default(),
            passes: vec![],
            global_defs: Default::default(),
            bindings: vec![],
            binding_map: Default::default(),
            param_block_size,
            lua: mlua::Lua::new(),
        }
    }

    pub fn reload_config_file(&mut self) -> Result<(), Error> {
        let path = self.config_file.clone();
        self.load_config_file(&path)
    }

    /// Provides buffer data.
    fn provide_buffer_data(&mut self, buffer_id: &str, byte_size: usize, f: impl FnMut(*mut u8)) -> Result<(), Error> {
        let key = *self.buffers_by_name.get(buffer_id).ok_or_else(|| Error::ResourceNotFound(buffer_id.to_string()))?;
        let resource = self.buffers.get_mut(key).unwrap();
        let buffer = self.device.create_buffer(resource.inferred_usage, MemoryLocation::CpuToGpu,
                                               byte_size as u64);
        let ptr = buffer.mapped_data().expect("failed to map buffer");
        f(ptr);
        resource.buffer = Some(buffer);
        Ok(())
    }

    fn compile_shader(&mut self, shader: &ShaderDescriptor, shader_kind: ShaderKind) -> Result<Vec<u32>, Error> {
        // path for diagnostics
        let display_path = match shader.source {
            ShaderSource::Content(_) => "<embedded shader>".to_string(),
            ShaderSource::File(ref path) => path.display().to_string(),
        };

        // load source
        let source_content = match shader.source {
            ShaderSource::Content(ref str) => str.clone(),
            ShaderSource::File(ref path) => match std::fs::read_to_string(path) {
                Ok(content) => content,
                Err(error) => {
                    return Err(Error::ShaderReadError { path: path.clone(), error });
                }
            },
        };

        // determine include path
        // this is the current directory if the shader is embedded, otherwise it's the parent
        // directory of the shader file
        let mut base_include_path = std::env::current_dir().expect("failed to get current directory");
        match shader.source {
            ShaderSource::File(ref path) => {
                if let Some(parent) = path.parent() {
                    base_include_path = parent.to_path_buf();
                }
            }
            _ => {}
        }

        // setup CompileOptions
        let mut options = shaderc::CompileOptions::new().unwrap();
        options.set_target_env(TargetEnv::Vulkan, EnvVersion::Vulkan1_3 as u32);
        options.set_target_spirv(SpirvVersion::V1_5);
        options.set_auto_bind_uniforms(true);
        for (key, value) in self.global_defs.iter() {
            options.add_macro_definition(key, Some(value));
        }
        for (key, value) in shader.defines.iter() {
            options.add_macro_definition(key, Some(value));
        }
        options.set_include_callback(move |requested_source, _type, _requesting_source, _include_depth| {
            let mut path = base_include_path.clone();
            path.push(requested_source);
            let content = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => return Err(e.to_string()),
            };
            Ok(shaderc::ResolvedInclude {
                resolved_name: path.display().to_string(),
                content,
            })
        });
        // add stage-specific macros
        match shader_kind {
            shaderc::ShaderKind::Vertex => {
                options.add_macro_definition("__VERTEX__", None);
            }
            shaderc::ShaderKind::Fragment => {
                options.add_macro_definition("__FRAGMENT__", None);
            }
            shaderc::ShaderKind::Geometry => {
                options.add_macro_definition("__GEOMETRY__", None);
            }
            shaderc::ShaderKind::Compute => {
                options.add_macro_definition("__COMPUTE__", None);
            }
            shaderc::ShaderKind::TessControl => {
                options.add_macro_definition("__TESS_CONTROL__", None);
            }
            shaderc::ShaderKind::TessEvaluation => {
                options.add_macro_definition("__TESS_EVAL__", None);
            }
            shaderc::ShaderKind::Mesh => {
                options.add_macro_definition("__MESH__", None);
            }
            shaderc::ShaderKind::Task => {
                options.add_macro_definition("__TASK__", None);
            }
            _ => {}
        }

        // pass binding locations
        for ((name, ty), loc) in self.binding_map.iter() {
            let ty = match ty {
                DescriptorType::SampledImage => "SAMPLER",
                DescriptorType::StorageImage => "IMAGE",
                DescriptorType::UniformBuffer => "UNIFORM",
                DescriptorType::StorageBuffer => "BUFFER",
            };
            options.add_macro_definition(&format!("{}_{}", ty, name), Some(&format!("{}", loc.binding)));
        }

        let compiler = get_shader_compiler();

        let compilation_artifact = match compiler.compile_into_spirv(&source_content, shader_kind, &display_path, "main", Some(&options)) {
            Ok(artifact) => artifact,
            Err(err) => {
                error!("failed to compile shader `{display_path}`: {err}");
                return Err(err.into());
            }
        };

        // remap resource bindings
        let mut module = spirv_reflect::create_shader_module(compilation_artifact.as_binary_u8()).unwrap();
        let descriptor_bindings = module.enumerate_descriptor_bindings(Some("main")).unwrap();
        for refl in descriptor_bindings.iter() {
            let name = refl.name.clone();
            if name.is_empty() {
                warn!("`{display_path}`: not binding anonymous {ty:?} resource");
                continue;
            }

            match refl.descriptor_type {
                ReflectDescriptorType::SampledImage | ReflectDescriptorType::StorageImage => {
                    let Some(image) = self.images_by_name.get(&name) else {
                        warn!("`{display_path}`: unknown image `{name}`");
                        continue;
                    };
                    let image = self.images.get_mut(*image).unwrap();
                    match refl.descriptor_type {
                        ReflectDescriptorType::SampledImage => {
                            module
                                .change_descriptor_binding_numbers(refl, image.texture_binding.binding, Some(image.texture_binding.set))
                                .unwrap();
                            image.inferred_usage |= ImageUsage::SAMPLED;
                        }
                        ReflectDescriptorType::StorageImage => {
                            module
                                .change_descriptor_binding_numbers(refl, image.storage_binding.binding, Some(image.storage_binding.set))
                                .unwrap();
                            image.inferred_usage |= ImageUsage::STORAGE;
                        }
                        _ => {}
                    }
                }
                ReflectDescriptorType::UniformBuffer | ReflectDescriptorType::StorageBuffer => {
                    let Some(buffer) = self.buffers_by_name.get(&name) else {
                        warn!("`{display_path}`: unknown buffer `{name}`");
                        continue;
                    };
                    let buffer = self.buffers.get_mut(*buffer).unwrap();
                    match refl.descriptor_type {
                        ReflectDescriptorType::UniformBuffer => {
                            module
                                .change_descriptor_binding_numbers(refl, buffer.uniform_binding.binding, Some(buffer.uniform_binding.set))
                                .unwrap();
                            buffer.inferred_usage |= BufferUsage::UNIFORM_BUFFER;
                        }
                        ReflectDescriptorType::StorageBuffer => {
                            module
                                .change_descriptor_binding_numbers(refl, buffer.storage_binding.binding, Some(buffer.storage_binding.set))
                                .unwrap();
                            buffer.inferred_usage |= BufferUsage::STORAGE_BUFFER;
                        }
                        _ => {}
                    }
                }
                ReflectDescriptorType::Sampler => {
                    todo!()
                }
                ReflectDescriptorType::CombinedImageSampler => {
                    todo!()
                }
                _ => {
                    warn!("`{display_path}`: unsupported descriptor type {:?}", refl.descriptor_type);
                    continue;
                }
            }
        }

        Ok(module.get_code())
    }

    /// Compiles or recompiles all shaders, recreate graphics & compute pipelines.
    fn update_pipelines(&mut self) -> Result<(), Error> {
        for pass in self.passes.iter_mut() {
            match pass {
                Pass::MeshShading(ref mut pass) => {
                    // cloning the shader descriptor is unfortunately necessary to avoid borrowing headaches
                    let shader = pass.shader.clone();
                    let task_shader_spv = match self.compile_shader(&shader, ShaderKind::Task) {
                        Ok(spv) => spv,
                        Err(err) => {
                            error!("failed to compile task shader: {err}");
                            continue;
                        }
                    };
                    let mesh_shader_spv = match self.compile_shader(&shader, ShaderKind::Mesh) {
                        Ok(spv) => spv,
                        Err(err) => {
                            error!("failed to compile mesh shader: {err}");
                            continue;
                        }
                    };
                    let fragment_shader_spv = match self.compile_shader(&shader, ShaderKind::Fragment) {
                        Ok(spv) => spv,
                        Err(err) => {
                            error!("failed to compile fragment shader: {err}");
                            continue;
                        }
                    };

                    let gpci = GraphicsPipelineCreateInfo {
                        layout: PipelineLayoutDescriptor {
                            arguments: &[ArgumentsLayout {
                                // TODO we could share the same descriptor set layout
                                // for all pipelines, but the graal API doesn't expose
                                // the concept of layouts. The impact is probably negligible anyway.
                                bindings: Cow::Borrowed(&self.bindings[..]),
                            }],
                            push_constants_size: self.param_block_size,
                        },
                        vertex_input: Default::default(),
                        pre_rasterization_shaders: PreRasterizationShaders::MeshShading {
                            task: Some(ShaderEntryPoint {
                                code: ShaderCode::Spirv(&task_shader_spv),
                                entry_point: "main",
                            }),
                            mesh: ShaderEntryPoint {
                                code: ShaderCode::Spirv(&mesh_shader_spv),
                                entry_point: "main",
                            },
                        },
                        rasterization: Default::default(),
                        fragment_shader: ShaderEntryPoint {
                            code: ShaderCode::Spirv(&fragment_shader_spv),
                            entry_point: "main",
                        },
                        depth_stencil: Default::default(),
                        fragment_output: Default::default(),
                    };

                    match self.device.create_graphics_pipeline(gpci) {
                        Ok(p) => {
                            pass.pipeline = Some(p);
                        }
                        Err(err) => {
                            error!("update_pipelines: failed to create graphics pipeline: {:?}", err);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_color_attachment(&self, lua: &mlua::Table) -> Result<ColorAttachment, Error> {
        let image_ref = lua.get_checked::<String>("ref")?;
        let image = *self.images_by_name.get(&image_ref).ok_or_else(|| {
            error!("image `{}` not found", image_ref);
            Error::ResourceNotFound(image_ref)
        })?;

        let clear_value = lua.get_checked::<[f64; 4]>("clearValue").unwrap_or([0.0, 0.0, 0.0, 0.0]);

        let load_op = lua.parse_str("loadOp", |v| match v {
            Some("clear") => vk::AttachmentLoadOp::CLEAR,
            Some("load") => vk::AttachmentLoadOp::LOAD,
            None => vk::AttachmentLoadOp::CLEAR,
            Some(load_op) => {
                warn!("invalid load op `{}`", load_op);
                vk::AttachmentLoadOp::CLEAR
            }
        });
        let store_op = lua.parse_str("storeOp", |v| match v {
            Some("store") => vk::AttachmentStoreOp::STORE,
            Some("discard") => vk::AttachmentStoreOp::DONT_CARE,
            None => vk::AttachmentStoreOp::STORE,
            Some(store_op) => {
                warn!("invalid store op `{}`", store_op);
                vk::AttachmentStoreOp::STORE
            }
        });

        Ok(ColorAttachment {
            image,
            clear_value,
            load_op,
            store_op,
            image_view: None,
        })
    }

    fn parse_depth_stencil_attachment(&self, lua: &mlua::Table) -> Result<DepthStencilAttachment, Error> {
        let image_ref = lua.get_checked::<String>("ref")?;
        let image = *self.images_by_name.get(&image_ref).ok_or_else(|| {
            error!("image `{}` not found", image_ref);
            Error::ResourceNotFound(image_ref)
        })?;

        let clear_value = lua.get_checked::<f64>("clearValue").unwrap_or(1.0);

        let depth_load_op = lua.parse_str("depthLoadOp", |v| match v {
            Some("clear") => vk::AttachmentLoadOp::CLEAR,
            Some("load") => vk::AttachmentLoadOp::LOAD,
            None => vk::AttachmentLoadOp::CLEAR,
            Some(load_op) => {
                warn!("invalid depth load op `{}`", load_op);
                vk::AttachmentLoadOp::CLEAR
            }
        });
        let depth_store_op = lua.parse_str("depthStoreOp", |v| match v {
            Some("store") => vk::AttachmentStoreOp::STORE,
            Some("discard") => vk::AttachmentStoreOp::DONT_CARE,
            None => vk::AttachmentStoreOp::STORE,
            Some(store_op) => {
                warn!("invalid depth store op `{}`", store_op);
                vk::AttachmentStoreOp::STORE
            }
        });
        let stencil_load_op = lua.parse_str("stencilLoadOp", |v| match v {
            Some("clear") => vk::AttachmentLoadOp::CLEAR,
            Some("load") => vk::AttachmentLoadOp::LOAD,
            None => vk::AttachmentLoadOp::CLEAR,
            Some(load_op) => {
                warn!("invalid stencil load op `{}`", load_op);
                vk::AttachmentLoadOp::CLEAR
            }
        });
        let stencil_store_op = lua.parse_str("stencilStoreOp", |v| match v {
            Some("store") => vk::AttachmentStoreOp::STORE,
            Some("discard") => vk::AttachmentStoreOp::DONT_CARE,
            None => vk::AttachmentStoreOp::STORE,
            Some(store_op) => {
                warn!("invalid stencil store op `{}`", store_op);
                vk::AttachmentStoreOp::STORE
            }
        });

        Ok(DepthStencilAttachment {
            image,
            clear_value,
            depth_load_op,
            depth_store_op,
            stencil_load_op,
            stencil_store_op,
            image_view: None,
        })
    }

    fn assign_resource_bindings(&mut self) -> Result<(), Error> {
        let mut set = 0;
        let mut binding = 0;

        // assign image & buffer bindings sequentially
        // for now, assume that all shader stages may access all resources
        for (_key, image) in &mut self.images {
            image.texture_binding = BindingLocation { set, binding };
            image.storage_binding = BindingLocation { set, binding: binding + 1 };
            self.bindings.extend([
                vk::DescriptorSetLayoutBinding {
                    binding: image.texture_binding.binding,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::ALL,
                    ..Default::default()
                },
                vk::DescriptorSetLayoutBinding {
                    binding: image.storage_binding.binding,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::ALL,
                    ..Default::default()
                },
            ]);
            self.binding_map
                .insert((image.name.clone(), DescriptorType::SampledImage), image.texture_binding);
            self.binding_map
                .insert((image.name.clone(), DescriptorType::StorageImage), image.storage_binding);
            binding += 2;
        }
        for (_key, buffer) in &mut self.buffers {
            buffer.uniform_binding = BindingLocation { set, binding };
            buffer.storage_binding = BindingLocation { set, binding: binding + 1 };
            self.bindings.extend([
                vk::DescriptorSetLayoutBinding {
                    binding: buffer.uniform_binding.binding,
                    descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::ALL,
                    ..Default::default()
                },
                vk::DescriptorSetLayoutBinding {
                    binding: buffer.storage_binding.binding,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: 1,
                    stage_flags: vk::ShaderStageFlags::ALL,
                    ..Default::default()
                },
            ]);
            self.binding_map
                .insert((buffer.name.clone(), DescriptorType::UniformBuffer), buffer.uniform_binding);
            self.binding_map
                .insert((buffer.name.clone(), DescriptorType::StorageBuffer), buffer.storage_binding);
            binding += 2;
        }

        if tracing::enabled!(tracing::Level::TRACE) {
            for ((name, descriptor_type), binding) in self.binding_map.iter() {
                let set = binding.set;
                let binding = binding.binding;
                trace!("`{name}` ({descriptor_type:?}) bound to set={set}, binding={binding}");
            }
        }

        Ok(())
    }


    fn run(&mut self, cmd: &mut CommandStream, frame_data: impl mlua::AsChunk) -> Result<(), Error> {
        let chunk = self.lua.load(frame_data);
        let frame_data: mlua::OwnedTable = chunk.eval()?;
        self.run_inner(cmd, frame_data)
    }

    fn run_inner(&mut self, cmd: &mut CommandStream, frame_data: mlua::OwnedTable) -> Result<(), Error> {
        for pass in self.passes.iter_mut() {
            match pass {
                Pass::MeshShading(ref mut pass) => {
                    if let Some(ref pipeline) = pass.pipeline {
                        // DONE: attachments
                        let mut encoder = cmd.begin_rendering(&DynamicAttachments {
                            color_attachments: pass
                                .color_attachments
                                .iter()
                                .map(|a| DynamicColorAttachment {
                                    image_view: a.image_view.clone().unwrap(),
                                    clear_value: a.clear_value,
                                    load_op: a.load_op,
                                    store_op: a.store_op,
                                })
                                .collect(),
                            depth_attachment: pass.depth_stencil_attachment.as_ref().map(|a| DynamicDepthStencilAttachment {
                                image_view: a.image_view.clone().unwrap(),
                                clear_value: a.clear_value,
                                depth_load_op: a.depth_load_op,
                                depth_store_op: a.depth_store_op,
                                stencil_load_op: a.stencil_load_op,
                                stencil_store_op: a.stencil_store_op,
                            }),
                        });

                        // TODO: create image views
                        //let brush_texture = &self.brush_textures[self.selected_brush].image.create_top_level_view();

                        encoder.bind_graphics_pipeline(pipeline);

                        encoder.set_viewport(0.0, 0.0, render_data.width as f32, render_data.height as f32, 0.0, 1.0);
                        encoder.set_scissor(0, 0, render_data.width, render_data.height);

                        // TODO: bind resources
                        encoder.bind_arguments(
                            0,
                            &crate::app::CurvesOITArguments {
                                position_buffer: animation.position_buffer.slice(..),
                                curve_buffer: animation.curve_buffer.slice(..),
                                fragment_buffer: self.oit_fragment_buffer.slice(..),
                                fragment_count_buffer: self.oit_fragment_count_buffer.slice(..),
                                brush_texture,
                                brush_sampler: &self.device.create_sampler(&SamplerCreateInfo {
                                    address_mode_u: vk::SamplerAddressMode::REPEAT,
                                    address_mode_v: vk::SamplerAddressMode::REPEAT,
                                    address_mode_w: vk::SamplerAddressMode::REPEAT,
                                    ..Default::default()
                                }),
                            },
                        );

                        // TODO push constants
                        encoder.bind_push_constants(&crate::app::CurvesOITPushConstants {
                            view_proj: self.camera_control.camera().view_projection(),
                            viewport_size: uvec2(width, height),
                            stroke_width: self.oit_stroke_width,
                            base_curve_index: current_frame.curve_range.start,
                            curve_count: current_frame.curve_range.count,
                            frame: self.frame,
                        });

                        // DONE draw params
                        let draw_params: mlua::Table = pass.draw.call(&frame_data)?;
                        let group_count_x: u32 = draw_params.get_checked("groupCountX")?;
                        let group_count_y: u32 = draw_params.get_checked("groupCountY")?;
                        let group_count_z: u32 = draw_params.get_checked("groupCountZ")?;
                        encoder.draw_mesh_tasks(group_count_x, group_count_y, group_count_z);
                        encoder.finish();
                    }
                }
                _ => {}
            }
        }
    }

    pub fn load_config_file(&mut self, file: &Path) -> Result<(), Error> {
        info!("loading configuration file `{}`", file.display());
        info!(
            "current working directory is `{}`",
            std::env::current_dir().expect("failed to get current directory").display()
        );

        // setup mlua
        self.lua = mlua::Lua::new();
        self.lua.load(file).exec()?;

        // save config file path only if loading was successful
        self.config_file = file.to_path_buf();

        // reset state
        self.binding_map.clear();
        self.bindings.clear();
        self.images.clear();
        self.buffers.clear();
        self.passes.clear();
        self.images_by_name.clear();
        self.buffers_by_name.clear();

        // resolve resources
        let images: mlua::Table = self.lua.globals().get_checked("G_images")?;
        for pair in images.pairs::<String, mlua::Table>() {
            let (name, desc) = pair?;

            let format: String = desc.get_checked("format")?;
            let format = vk_format_from_str(&format)?;
            let width: Option<u32> = desc.get("width")?;
            let height: Option<u32> = desc.get("height")?;
            let width_divisor = desc.get::<_, Option<u32>>("width_div")?.unwrap_or(1);
            let height_divisor = desc.get::<_, Option<u32>>("height_div")?.unwrap_or(1);

            let key = self.images.insert(ImageResource {
                name: name.clone(),
                format,
                width,
                height,
                width_divisor,
                height_divisor,
                // all below determined or allocated later
                inferred_usage: Default::default(),
                image: None,
                texture_binding: Default::default(),
                storage_binding: Default::default(),
            });

            trace!(
                "load_config_file: image resource `{}`, {:?} {:?} × {:?}, width_divisor={}, height_divisor={}",
                name,
                format,
                width,
                height,
                width_divisor,
                height_divisor
            );

            self.images_by_name.insert(name, key);
        }

        let buffers: mlua::Table = self.lua.globals().get_checked("G_buffers")?;
        for pair in buffers.pairs::<String, mlua::Table>() {
            let (name, desc) = pair?;

            let byte_size: Option<usize> = desc.get("byte_size")?;
            let width_divisor = desc.get::<_, Option<u32>>("width_div")?.unwrap_or(1);
            let height_divisor = desc.get::<_, Option<u32>>("height_div")?.unwrap_or(1);

            let key = self.buffers.insert(BufferResource {
                name: name.clone(),
                byte_size,
                width_divisor,
                height_divisor,
                // all below determined or allocated later
                inferred_usage: Default::default(),
                inferred_memory_properties: Default::default(),
                buffer: None,
                uniform_binding: Default::default(),
                storage_binding: Default::default(),
            });
            trace!(
                "load_config_file: buffer resource `{}`, byte_size={:?}, width_divisor={}, height_divisor={}",
                name,
                byte_size,
                width_divisor,
                height_divisor
            );
            self.buffers_by_name.insert(name, key);
        }

        // passes
        let passes: mlua::Table = self.lua.globals().get_checked("G_passes")?;
        for pair in passes.pairs::<String, mlua::Table>() {
            let (name, desc) = pair?;
            let pass = desc.get_checked::<String>("type")?;
            match pass.as_str() {
                "fill_buffer" => {}
                "mesh_shading" => {
                    let shader = ShaderDescriptor::from_lua(&desc.get_checked("shader")?)?;

                    let color_attachments_descs = desc.get_checked::<Vec<mlua::Table>>("colorAttachments")?;
                    let depth_stencil_attachment: Option<mlua::Table> = desc.get("depthStencilAttachment")?;

                    let mut color_attachments = vec![];
                    for color_attachment_desc in color_attachments_descs {
                        let color_attachment = self.parse_color_attachment(&color_attachment_desc)?;
                        color_attachments.push(color_attachment);
                    }
                    let mut depth_stencil_attachment = match depth_stencil_attachment {
                        Some(desc) => Some(self.parse_depth_stencil_attachment(&desc)?),
                        None => None,
                    };

                    let draw: mlua::OwnedFunction = desc.get_checked("draw")?;

                    self.passes.push(Pass::MeshShading(MeshShadingPass {
                        shader,
                        color_attachments,
                        depth_stencil_attachment,
                        pipeline: None,
                        draw,
                    }));
                }
                _ => {
                    warn!("load_config_file: unknown pass type `{pass}`");
                }
            }
        }

        // TODO: allocate resources
        // self.infer_resource_usage();
        // self.update_resources()?;

        self.assign_resource_bindings()?;
        self.update_pipelines()?;

        Ok(())
    }
}
