mod descriptors;

use crate::engine2::descriptors::{
    ResourceDescriptors, ResourceDescriptorsBuilder, UniversalPipelineLayout, IMG_SET, SSBO_SET, TEX_SET, UBO_SET,
};
use bitflags::bitflags;
use bytemuck::cast_slice;
use graal::{
    get_shader_compiler, shaderc,
    shaderc::{EnvVersion, ShaderKind, SpirvVersion, TargetEnv},
    vk, BufferAccess, BufferUsage, CommandStream, DepthStencilState,
    GraphicsPipelineCreateInfo, ImageAccess, ImageCreateInfo, ImageUsage, ImageView, MemoryLocation, MultisampleState,
    PreRasterizationShaders, RasterizationState, RenderPassDescriptor, ShaderCode,
    ShaderEntryPoint,
};
use slotmap::SlotMap;
use spirv_reflect::types::{ReflectDescriptorType, ReflectTypeFlags};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tracing::{debug, error, warn};

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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BufferKey<'a>(pub &'a str);

impl<'a, T: Copy> From<TypedBufferKey<'a, T>> for BufferKey<'a> {
    fn from(key: TypedBufferKey<'a, T>) -> Self {
        BufferKey(key.0)
    }
}

#[derive(Copy, Clone)]
pub struct TypedBufferKey<'a, T: Copy>(pub &'a str);

impl<T: Copy> PartialEq for TypedBufferKey<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Copy> Eq for TypedBufferKey<'_, T> {}

impl<T: Copy> PartialOrd for TypedBufferKey<'_, T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other.0)
    }
}

impl<T: Copy> Ord for TypedBufferKey<'_, T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(other.0)
    }
}

impl<T: Copy> std::hash::Hash for TypedBufferKey<'_, T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T: Copy> std::fmt::Debug for TypedBufferKey<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TypedBufferKey({})", self.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageKey<'a>(pub &'a str);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PassKey<'a>(pub &'a str);

/// BTreeMap but where keys can be aliased.
struct AliasedMap<K, V> {
    map: BTreeMap<K, V>,
    aliases: BTreeMap<K, K>,
}

impl<K: Ord + Clone, V> AliasedMap<K, V> {
    fn new() -> Self {
        Self {
            map: Default::default(),
            aliases: Default::default(),
        }
    }

    fn insert(&mut self, key: K, value: V) {
        self.map.insert(key, value);
    }

    fn alias(&mut self, alias: K, real: K) -> Option<V> {
        self.aliases.remove(&alias);
        let old = self.map.remove(&alias);
        let real = self.resolve(&real);
        self.aliases.insert(alias, real);
        old
    }

    fn get(&self, key: &K) -> Option<&V> {
        let key = self.resolve(key);
        self.map.get(&key)
    }

    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let key = self.resolve(key);
        self.map.get_mut(&key)
    }

    fn resolve(&self, key: &K) -> K {
        self.aliases.get(key).cloned().unwrap_or(key.clone())
    }
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
    /// If this is an alias, contains the key to the original resource.
    alias: Option<ImageKey<'static>>,
    desc: ImageDesc,
    /// Inferred usage flags.
    inferred_usage: ImageUsage,
    /// The allocated image resource.
    image: Option<graal::Image>,
    /// Binding to access the image as a sampled image.
    texture_binding: BindingLocation,
    /// Binding to access the image as a storage image.
    storage_binding: BindingLocation,
    /// Top-level image view.
    view: Option<ImageView>,
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

    /// If this is an alias, contains the key to the original resource.
    alias: Option<BufferKey<'static>>,

    /// Buffer usage flags.
    inferred_usage: BufferUsage,

    /// Inferred memory properties.
    inferred_memory_properties: vk::MemoryPropertyFlags,

    /// Explicit byte size.
    byte_size: usize,

    /// The allocated buffer resource.
    buffer: Option<graal::BufferUntyped>,

    /// Binding to access the buffer as a uniform buffer.
    uniform_binding: BindingLocation,

    /// Binding to access the buffer as a storage buffer.
    storage_binding: BindingLocation,
}

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
    IO(#[from] std::io::Error),
    #[error("could not read shader file `{}`: {}", .path.display(), .error)]
    ShaderReadError { path: PathBuf, error: std::io::Error },
    #[error("Shader compilation error: {0}")]
    ShaderCompilation(#[from] shaderc::Error),
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),
    #[error("Vulkan error: {0}")]
    Vulkan(#[from] vk::Result),
    #[error("Unknown field: {0}")]
    UnknownField(String),
    #[error("Invalid field type: {0}")]
    InvalidFieldType(String),
}

#[derive(Copy, Clone, Debug)]
pub struct ImageDesc {
    pub width: u32,
    pub height: u32,
    pub format: vk::Format,
}

/// `vkCmdFillBuffer` pass.
struct FillBufferPass {
    buffer: BufferKey<'static>,
    value: u32,
}

pub struct ColorAttachmentDesc {
    pub image: ImageKey<'static>,
    pub clear_value: Option<[f64; 4]>,
}

pub struct DepthStencilAttachmentDesc {
    pub image: ImageKey<'static>,
    pub depth_clear_value: Option<f64>,
    pub stencil_clear_value: Option<u32>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum UniformType {
    F32,
    Vec2,
    Vec3,
    Vec4,
    Mat2,
    Mat3,
    Mat4,
}

#[derive(Copy, Clone, Debug)]
enum UniformValue {
    F32(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat2([[f32; 2]; 2]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
}

impl From<f32> for UniformValue {
    fn from(value: f32) -> Self {
        UniformValue::F32(value)
    }
}

impl From<[f32; 2]> for UniformValue {
    fn from(value: [f32; 2]) -> Self {
        UniformValue::Vec2(value)
    }
}

impl From<[f32; 3]> for UniformValue {
    fn from(value: [f32; 3]) -> Self {
        UniformValue::Vec3(value)
    }
}

impl From<[f32; 4]> for UniformValue {
    fn from(value: [f32; 4]) -> Self {
        UniformValue::Vec4(value)
    }
}

impl From<[[f32; 2]; 2]> for UniformValue {
    fn from(value: [[f32; 2]; 2]) -> Self {
        UniformValue::Mat2(value)
    }
}

impl From<[[f32; 3]; 3]> for UniformValue {
    fn from(value: [[f32; 3]; 3]) -> Self {
        UniformValue::Mat3(value)
    }
}

impl From<[[f32; 4]; 4]> for UniformValue {
    fn from(value: [[f32; 4]; 4]) -> Self {
        UniformValue::Mat4(value)
    }
}

/// Contents of a constants (uniform) buffer, with names mapped to offsets and sizes.
#[derive(Default)]
struct UniformData {
    fields: BTreeMap<String, (u32, UniformType)>,
    data: Vec<u8>,
}

impl UniformData {
    fn new(size: usize, fields: impl IntoIterator<Item=(String, (u32, UniformType))>) -> Self {
        Self {
            fields: BTreeMap::from_iter(fields),
            data: vec![0; size],
        }
    }

    fn set(&mut self, name: &str, value: impl Into<UniformValue>) -> Result<(), Error> {
        self.set_inner(name, value.into())
    }

    fn set_inner(&mut self, name: &str, value: UniformValue) -> Result<(), Error> {
        let (offset, ty) = *self.fields.get(name).ok_or(Error::UnknownField(name.to_string()))?;
        let data = &mut self.data;

        match (ty, value) {
            (UniformType::F32, UniformValue::F32(value)) => {
                // Vulkan expects values in the same byte order as the host; that's in the spec,
                // apparently. So we don't need to do anything special here.
                let bytes = value.to_ne_bytes();
                data[offset..offset + 4].copy_from_slice(&bytes);
            }
            (UniformType::Vec2, UniformValue::Vec2(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 8].copy_from_slice(bytes);
            }
            (UniformType::Vec3, UniformValue::Vec3(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 12].copy_from_slice(bytes);
            }
            (UniformType::Vec4, UniformValue::Vec4(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 16].copy_from_slice(bytes);
            }
            (UniformType::Mat2, UniformValue::Mat2(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 16].copy_from_slice(bytes);
            }
            (UniformType::Mat3, UniformValue::Mat3(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 36].copy_from_slice(bytes);
            }
            (UniformType::Mat4, UniformValue::Mat4(value)) => {
                let bytes = cast_slice(&value);
                data[offset..offset + 64].copy_from_slice(bytes);
            }
            _ => {
                return Err(Error::InvalidFieldType(name.to_string()));
            }
        }

        Ok(())
    }
}

pub struct MeshShadingPassDesc<F> {
    pub shader: PathBuf,
    pub defines: BTreeMap<String, String>,
    pub color_attachments: Vec<ColorAttachmentDesc>,
    pub depth_stencil_attachment: Option<DepthStencilAttachmentDesc>,
    pub rasterization_state: RasterizationState,
    pub depth_stencil_state: DepthStencilState,
    pub multisample_state: MultisampleState,
    //pub buffers: Vec<BufferKey<'static>>,
    //pub images: Vec<ImageKey<'static>>,
    pub color_target_states: Vec<graal::ColorTargetState>,
    pub draw: F,
}

pub struct BlitDesc {
    pub src: ImageKey<'static>,
    pub dst: ImageKey<'static>,
}

pub struct ComputePassDesc {
    pub shader: PathBuf,
    pub defines: BTreeMap<String, String>,
    pub dispatch: [u32; 3],
}

struct MeshShadingPass<C> {
    desc: MeshShadingPassDesc<Box<dyn FnMut(&mut C)>>,
    push_constant_data: UniformData,
    pipeline: Option<graal::GraphicsPipeline>,
    used_images: BTreeMap<ImageKey<'static>, ImageAccess>,
    used_buffers: BTreeMap<BufferKey<'static>, BufferAccess>,
}


struct ComputePass {
    desc: ComputePassDesc,
    push_constant_data: UniformData,
    pipeline: Option<graal::ComputePipeline>,
    used_images: BTreeMap<ImageKey<'static>, ImageAccess>,
    used_buffers: BTreeMap<BufferKey<'static>, BufferAccess>,
}

enum PassKind<C> {
    FillBuffer(FillBufferPass),
    Blit(BlitDesc),
    MeshShading(MeshShadingPass<C>),
    Compute(ComputePass),
}

struct Pass<C> {
    enabled: bool,
    kind: PassKind<C>,
}

enum Param {
    Float { value: f32, min: f32, max: f32 },
}

enum DescriptorData {
    Image(ImageKey<'static>),
    Buffer(BufferKey<'static>),
    Sampler,
}

struct Descriptor {
    binding: u32,
    descriptor_type: vk::DescriptorType,
    data: DescriptorData,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum InvalidationLevel {
    /// All clean.
    Clean = 0,
    /// Descriptors must be updated.
    Descriptors,
    /// One or more resources must be reallocated, and descriptors must be updated.
    Resources,
    /// Physical resource usages need to be recomputed.
    ResourceUsage,
    /// Pipelines must be recreated.
    Pipelines,
    /// Binding numbers have changed and must be reassigned.
    Bindings,
    All,
}

impl InvalidationLevel {
    fn merge(&mut self, level: Self) {
        *self = (*self).max(level);
    }
}

/// Rendering engine instance.
///
/// # Descriptors
///
/// To simplify things, we use a single big descriptor set containing descriptors for all combinations
/// of resources and possible accesses (e.g., for each image, a descriptor for sampled access and
/// another for storage access, and for buffers, a descriptor for uniform access and another for storage).
pub struct Engine<Context = ()> {
    device: graal::Device,
    images: BTreeMap<ImageKey<'static>, ImageResource>,
    buffers: BTreeMap<BufferKey<'static>, BufferResource>,
    passes: BTreeMap<PassKey<'static>, Pass<Context>>,
    //plan: Vec<PassKey<'static>>,
    /// Defines added to every compiled shader
    global_defs: BTreeMap<String, String>,
    /// Layout of the descriptor set containing all resources.
    bindings: Vec<vk::DescriptorSetLayoutBinding>,
    /// Parameters.
    ///
    /// Automatically bound (by name matching) to matching fields in push constant blocks inside
    /// shaders.
    params: BTreeMap<String, Param>,
    layout: UniversalPipelineLayout,
    descriptors: Option<ResourceDescriptors>,
    invalid: InvalidationLevel,
}

fn bind_params(buffer: &mut UniformData, params: &BTreeMap<String, Param>) {
    for (name, param) in params.iter() {
        match param {
            Param::Float { value, .. } => {
                buffer.set(name, *value).unwrap();
            }
        }
    }
}

#[derive(Default)]
struct CompilationInfo {
    used_images: BTreeMap<ImageKey<'static>, ImageAccess>,
    used_buffers: BTreeMap<BufferKey<'static>, BufferAccess>,
    push_cst_map: BTreeMap<String, (u32, UniformType)>,
    push_cst_size: usize,
}

impl<C> Engine<C> {
    pub fn new(device: graal::Device) -> Self {
        let layout = UniversalPipelineLayout::new(&device);
        Self {
            device,
            images: Default::default(),
            buffers: Default::default(),
            passes: Default::default(),
            global_defs: Default::default(),
            bindings: vec![],
            params: Default::default(),
            layout,
            descriptors: None,
            invalid: InvalidationLevel::All,
        }
    }

    pub fn reset(&mut self) {
        self.images.clear();
        self.buffers.clear();
        self.passes.clear();
        self.global_defs.clear();
        self.bindings.clear();
        self.params.clear();
        self.descriptors = None;
        self.invalid = InvalidationLevel::All;
    }

    pub fn define_buffer_uninit(&mut self, key: impl Into<BufferKey>, byte_size: usize) {
        let key = key.into();
        self.buffers.insert(
            key,
            BufferResource {
                name: key.0.to_string(),
                byte_size,
                inferred_usage: Default::default(),
                inferred_memory_properties: Default::default(),
                buffer: None,
                uniform_binding: Default::default(),
                storage_binding: Default::default(),
            },
        );
    }

    pub fn define_global(&mut self, define: &str, value: impl ToString) {
        self.global_defs.insert(define.to_string(), value.to_string());
    }

    pub fn define_float_param(&mut self, name: &str, init_value: f32, min: f32, max: f32) {
        self.params.insert(
            name.to_string(),
            Param::Float {
                value: init_value,
                min,
                max,
            },
        );
    }

    pub fn enable(&mut self, key: PassKey<'static>, enable: bool) {
        if let Some(pass) = self.passes.get_mut(&key) {
            pass.enabled = enable;
        } else {
            warn!("unknown pass `{}`", key.0);
        }
    }

    pub fn forward_buffer(&mut self, from: impl Into<BufferKey<'static>>, to: impl Into<BufferKey<'static>>) {
        let from = from.into();
        let to = to.into();
        self.buffers.alias(to, from);
        self.invalid.merge(InvalidationLevel::Bindings);
    }

    fn resolve_image_key(&mut self, key: ImageKey<'static>) -> ImageKey<'static> {
        let mut key = key;
        while let Some(alias) = self.images.get(&key).and_then(|r| r.alias) {
            key = alias;
        }
        key
    }

    fn resolve_buffer_key(&mut self, key: BufferKey<'static>) -> BufferKey<'static> {
        let mut key = key;
        while let Some(alias) = self.buffers.get(&key).and_then(|r| r.alias) {
            key = alias;
        }
        key
    }

    /// # Arguments
    ///
    /// * `target` the key of the concrete image to forward
    /// * `alias` the key of the alias to create
    ///
    /// The target image is not required to have been defined yet.
    pub fn alias_image(&mut self, target: impl Into<ImageKey<'static>>, alias: impl Into<ImageKey<'static>>) {

        // Does the alias exist?
        // NO => create it, invalidate bindings, done
        // YES =>
        //      Is the format the same?
        //      YES =>
        //           Are usages compatible?
        //           YES => alias, invalidate descriptors, done
        //           NO => the inferred usage of the target isn't compatible with the resource it is now aliased to;
        //                 update the usage of the source and mark it for reallocation
        //      NO =>
        //           Does the usage contain COLOR_TARGET or DEPTH_TARGET?
        //           YES => render passes depend on this image format, must invalidate pipelines
        //           NO => pipelines are not affected, invalidate descriptors




        let to = to.into();
        let from = self.resolve_image_key(from.into());
        let from_img = self.images.get(&from).expect("forwarded image not found");

        let desc = from_img.desc;
        let inferred_usage = from_img.inferred_usage;

        self.images.entry(to).and_modify(|entry| {

            entry.alias = Some(from);
            entry.image = None;
            entry.view = None;
            entry.desc.width = desc.width;
            entry.desc.height = desc.height;
            if entry.desc.format != desc.format {
                // pipelines using this image as a color attachment may need to be recompiled
                // if the format is different
                self.invalid.merge(InvalidationLevel::Pipelines);
                entry.desc.format = desc.format;
            }
            if !entry.inferred_usage.contains(inferred_usage) {
                // The inferred usage of the previous image at this key is not compatible
                // with the image that it is now aliased with.
                // It needs to be reallocated.
                self.invalid.merge(InvalidationLevel::Resources);
            }

        }).or_insert_with(|| {
            ImageResource {
                name: from.into().0.to_string(),
                alias: Some(to.into()),
                desc,
                inferred_usage: Default::default(),
                image: None,
                texture_binding: Default::default(),
                storage_binding: Default::default(),
                view: None,
            }
        }

        let from = from.into();
        let to = to.into();
        self.images.alias(to, from);
        self.invalid.merge(InvalidationLevel::Bindings);
    }

    /// Imports an existing image.
    pub fn import_image(&mut self, key: impl Into<ImageKey<'static>>, image: graal::Image) {
        let key = key.into();
        self.images
            .entry(key)
            .and_modify(|entry| {
                // FIXME: if the format changes, some shaders may need to be recompiled
                self.invalid.merge(InvalidationLevel::Descriptors);
                entry.format = image.format();
                entry.width = image.width();
                entry.height = image.height();
                entry.inferred_usage = image.usage();
                entry.view = Some(image.create_top_level_view());
                entry.image = Some(image);
            })
            .or_insert_with(|| {
                self.invalid.merge(InvalidationLevel::Bindings);
                let view = image.create_top_level_view();
                ImageResource {
                    name: key.0.to_string(),
                    format: image.format(),
                    width: image.width(),
                    height: image.height(),
                    inferred_usage: image.usage(),
                    image: Some(image),
                    texture_binding: Default::default(),
                    storage_binding: Default::default(),
                    view: Some(view),
                }
            });
    }

    pub fn import_buffer(&mut self, key: impl Into<BufferKey<'static>>, buffer: impl Into<graal::BufferUntyped>) {
        let key = key.into();

        self.buffers
            .entry(key)
            .and_modify(|entry| {
                self.invalid.merge(InvalidationLevel::Descriptors);
                entry.byte_size = buffer.size();
                entry.inferred_usage = buffer.usage();
                entry.inferred_memory_properties = buffer.memory_properties();
                entry.buffer = Some(buffer);
            })
            .or_insert_with(|| {
                self.invalid.merge(InvalidationLevel::Bindings);
                BufferResource {
                    name: key.0.to_string(),
                    byte_size: buffer.size(),
                    inferred_usage: buffer.usage(),
                    inferred_memory_properties: buffer.memory_properties(),
                    buffer: Some(buffer),
                    uniform_binding: Default::default(),
                    storage_binding: Default::default(),
                }
            });
    }

    pub fn define_image(&mut self, key: impl Into<ImageKey>, desc: ImageDesc) {
        let key = key.into();
        self.images
            .entry(key)
            .and_modify(|entry| {
                entry.format = desc.format;
                entry.width = desc.width;
                entry.height = desc.height;
                entry.image = None;
            })
            .or_insert_with(|| ImageResource {
                name: key.0.to_string(),
                alias: None,
                format: desc.format,
                width: desc.width,
                height: desc.height,
                inferred_usage: Default::default(),
                image: None,
                texture_binding: Default::default(),
                storage_binding: Default::default(),
                view: None,
            });
    }

    pub fn mesh_shading_pass(&mut self, key: PassKey<'static>, desc: MeshShadingPassDesc<C>) {
        self.passes.insert(key, Pass::MeshShading(MeshShadingPass {
            desc,
            push_constant_data: Default::default(),
            pipeline: None,
            used_images: Default::default(),
            used_buffers: Default::default(),
        }));
    }

    pub fn compute_pass(&mut self, key: PassKey<'static>, desc: ComputePassDesc) {
        self.passes.insert(key, Pass::Compute(ComputePass {
            desc,
            push_constant_data: Default::default(),
            pipeline: None,
            used_images: Default::default(),
            used_buffers: Default::default(),
        }));
    }

    pub fn blit(&mut self, key: PassKey<'static>, src: impl Into<ImageKey>, dst: impl Into<ImageKey>) {
        self.passes.insert(key, Pass::Blit(BlitDesc {
            src: src.into(),
            dst: dst.into(),
        }));
    }

    pub fn set_plan(&mut self, plan: impl IntoIterator<Item=PassKey<'static>>) {
        self.plan = plan.into_iter().collect();
    }

    fn compile_stage(
        &mut self,
        file_path: &Path,
        defines: &BTreeMap<String, String>,
        shader_kind: ShaderKind,
        info: &mut CompilationInfo,
    ) -> Result<Vec<u32>, Error> {
        // path for diagnostics
        let display_path = file_path.display().to_string();

        // load source
        let source_content = match std::fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(error) => {
                return Err(Error::ShaderReadError {
                    path: file_path.to_path_buf(),
                    error,
                });
            }
        };

        // determine include path
        // this is the current directory if the shader is embedded, otherwise it's the parent
        // directory of the shader file
        let mut base_include_path = std::env::current_dir().expect("failed to get current directory");
        if let Some(parent) = file_path.parent() {
            base_include_path = parent.to_path_buf();
        }

        // setup CompileOptions
        let mut options = shaderc::CompileOptions::new().unwrap();
        options.set_target_env(TargetEnv::Vulkan, EnvVersion::Vulkan1_3 as u32);
        options.set_target_spirv(SpirvVersion::V1_5);
        options.set_auto_bind_uniforms(true);
        for (key, value) in self.global_defs.iter() {
            options.add_macro_definition(key, Some(value));
        }
        for (key, value) in defines.iter() {
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
            ShaderKind::Vertex => {
                options.add_macro_definition("__VERTEX__", None);
            }
            ShaderKind::Fragment => {
                options.add_macro_definition("__FRAGMENT__", None);
            }
            ShaderKind::Geometry => {
                options.add_macro_definition("__GEOMETRY__", None);
            }
            ShaderKind::Compute => {
                options.add_macro_definition("__COMPUTE__", None);
            }
            ShaderKind::TessControl => {
                options.add_macro_definition("__TESS_CONTROL__", None);
            }
            ShaderKind::TessEvaluation => {
                options.add_macro_definition("__TESS_EVAL__", None);
            }
            ShaderKind::Mesh => {
                options.add_macro_definition("__MESH__", None);
            }
            ShaderKind::Task => {
                options.add_macro_definition("__TASK__", None);
            }
            _ => {}
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
            let ty = refl.descriptor_type;
            let name = refl.name.clone();
            if name.is_empty() {
                warn!("`{display_path}`: not binding anonymous {ty:?} resource");
                continue;
            }

            match refl.descriptor_type {
                ReflectDescriptorType::SampledImage | ReflectDescriptorType::StorageImage => {
                    let Some(image) = self.images.get_mut(&ImageKey(name.as_str())) else {
                        warn!("`{display_path}`: unknown image `{name}`");
                        continue;
                    };

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
                    let Some(buffer) = self.buffers.get_mut(&BufferKey(name.as_str())) else {
                        warn!("`{display_path}`: unknown buffer `{name}`");
                        continue;
                    };

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

        // reflect push constants

        let push_constants = module.enumerate_push_constant_blocks(Some("main")).unwrap();
        if push_constants.len() > 1 {
            warn!("`{display_path}`: multiple push constant blocks found; only the first one will be used");
        }

        if let Some(block) = push_constants.first() {
            if block.offset != 0 {
                warn!("`{display_path}`: push constant blocks at non-zero offset are not supported");
            } else {
                let add_constant = |name: &str, offset: u32, ty: UniformType| {
                    if let Some(c) = info.push_cst_map.insert(name.to_string(), (offset, ty)) {
                        if c != (offset, ty) {
                            warn!("`{display_path}` push constant `{name}` redefined with different offset or type");
                        }
                    }
                };

                for var in block.members.iter() {
                    let Some(tydesc) = var.type_description.as_ref() else { continue };
                    let offset = var.absolute_offset;

                    if tydesc.type_flags.contains(ReflectTypeFlags::FLOAT) {
                        if tydesc.traits.numeric.scalar.width == 32 {
                            add_constant(&var.name, offset, UniformType::F32);
                        } else {
                            warn!("`{display_path}`: unsupported float width");
                            continue;
                        }
                    } else if tydesc.type_flags.contains(ReflectTypeFlags::VECTOR) {
                        match tydesc.traits.numeric.vector.component_count {
                            2 => add_constant(&var.name, offset, UniformType::Vec2),
                            3 => add_constant(&var.name, offset, UniformType::Vec3),
                            4 => add_constant(&var.name, offset, UniformType::Vec4),
                            _ => {
                                warn!("`{display_path}`: unsupported vector component count");
                                continue;
                            }
                        }
                    } else if tydesc.type_flags.contains(ReflectTypeFlags::MATRIX) {
                        match (tydesc.traits.numeric.matrix.column_count, tydesc.traits.numeric.matrix.row_count) {
                            (2, 2) => add_constant(&var.name, offset, UniformType::Mat2),
                            (3, 3) => add_constant(&var.name, offset, UniformType::Mat3),
                            (4, 4) => add_constant(&var.name, offset, UniformType::Mat4),
                            _ => {
                                warn!("`{display_path}`: unsupported matrix shape");
                                continue;
                            }
                        }
                    } else {
                        warn!("`{display_path}`: unsupported push constant type");
                        continue;
                    }
                }
                *info.push_cst_size = (*info.push_cst_size).max(block.size as usize);
            }
        }

        Ok(module.get_code())
    }

    /// Compiles or recompiles all shaders, recreate graphics & compute pipelines.
    fn update_pipelines(&mut self) -> Result<(), Error> {
        // swap out of `self.passes` to avoid borrowing issues
        let mut passes = std::mem::take(&mut self.passes);
        for pass in passes.iter_mut() {
            match pass {
                Pass::MeshShading(ref mut pass) => {
                    // Compile shaders for each stage, and at the same time update the inferred
                    // usage flags for images and buffers (that's why we need mutable references to
                    // `self.images` and `self.buffers`), and track the push constant fields.

                    let file_path = &pass.desc.shader;
                    let defs = &pass.desc.defines;
                    let mut ci = CompilationInfo::default();

                    let task_spv = match self.compile_stage(&file_path, &defs, ShaderKind::Task, &mut ci) {
                        Ok(spv) => spv,
                        Err(err) => {
                            error!("failed to compile task shader: {err}");
                            continue;
                        }
                    };
                    let mesh_spv = match self.compile_stage(&file_path, &defs, ShaderKind::Mesh, &mut ci) {
                        Ok(spv) => spv,
                        Err(err) => {
                            error!("failed to compile mesh shader: {err}");
                            continue;
                        }
                    };
                    let fragment_spv =
                        match self.compile_stage(&file_path, &defs, ShaderKind::Fragment, &mut ci) {
                            Ok(spv) => spv,
                            Err(err) => {
                                error!("failed to compile fragment shader: {err}");
                                continue;
                            }
                        };

                    let mut color_attachment_formats = vec![];
                    for a in pass.desc.color_attachments.iter() {
                        let mut image = self.images.get_mut(&a.image).expect("unknown image");
                        image.inferred_usage |= ImageUsage::COLOR_ATTACHMENT;
                        color_attachment_formats.push(image.format);
                        ci.used_images.insert(a.image, ImageAccess::COLOR_TARGET);
                    }

                    let mut depth_attachment_format = None;
                    if let Some(a) = pass.desc.depth_stencil_attachment.as_ref() {
                        let mut image = self.images.get_mut(&a.image).expect("unknown image");
                        image.inferred_usage |= ImageUsage::DEPTH_STENCIL_ATTACHMENT;
                        depth_attachment_format = Some(image.format);
                        // FIXME: check if depth is read only
                        ci.used_images.insert(a.image, ImageAccess::DEPTH_STENCIL_WRITE);
                    }

                    let gpci = GraphicsPipelineCreateInfo {
                        set_layouts: &[
                            &self.layout.ubo,
                            &self.layout.ssbo,
                            &self.layout.texture,
                            &self.layout.image,
                            &self.layout.sampler,
                        ],
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
                        rasterization: pass.desc.rasterization_state,
                        fragment_shader: ShaderEntryPoint {
                            code: ShaderCode::Spirv(&fragment_spv),
                            entry_point: "main",
                        },
                        depth_stencil: pass.desc.depth_stencil_state,
                        fragment_output: graal::FragmentOutputInterfaceDescriptor {
                            color_attachment_formats: &color_attachment_formats[..],
                            depth_attachment_format,
                            stencil_attachment_format: None, // TODO
                            multisample: pass.desc.multisample_state,
                            color_targets: &pass.desc.color_target_states[..],
                            blend_constants: [0.0; 4], // TODO
                        },
                    };

                    match self.device.create_graphics_pipeline(gpci) {
                        Ok(p) => {
                            pass.pipeline = Some(p);
                            pass.push_constant_data = UniformData {
                                fields: ci.push_cst_map,
                                data: vec![0; ci.push_cst_size],
                            };
                            pass.used_buffers = ci.used_buffers;
                            pass.used_images = ci.used_images;
                        }
                        Err(err) => {
                            error!("update_pipelines: failed to create graphics pipeline: {:?}", err);
                        }
                    }

                    debug!("[`{}`] used buffers:", file_path.display());
                    for (key, access) in ci.used_buffers.iter() {
                        debug!("[mesh pass]   {key}: {access:?}");
                    }
                    debug!("[`{}`] used images:", file_path.display());
                    for (key, access) in ci.used_images.iter() {
                        debug!("[mesh pass]   {key}: {access:?}");
                    }
                }
                _ => {}
            }
        }
        self.passes = passes;
        Ok(())
    }

    /// Assigns resource bindings (set & binding indices) to all resources.
    ///
    /// Currently, all resources are assigned to set 0, and bindings are assigned sequentially starting from 0.
    fn assign_resource_bindings(&mut self) -> Result<(), Error> {
        self.bindings.clear();

        let mut ubo_binding = 0;
        let mut ssbo_binding = 0;
        let mut tex_binding = 0;
        let mut img_binding = 0;

        // Assign image & buffer bindings sequentially.
        // For now, assume that all shader stages may access all resources.
        // Note that we assign different bindings for aliases to the same resource, this way we don't
        // have to recompile the shaders if an alias becomes "unaliased", or vice versa.
        for (key, image) in self.images.iter_mut() {
            image.texture_binding = BindingLocation {
                set: TEX_SET,
                binding: tex_binding,
            };
            image.storage_binding = BindingLocation {
                set: IMG_SET,
                binding: img_binding,
            };
            tex_binding += 1;
            img_binding += 1;
        }
        for (key, buffer) in self.buffers.iter_mut() {
            buffer.uniform_binding = BindingLocation {
                set: UBO_SET,
                binding: ubo_binding,
            };
            buffer.storage_binding = BindingLocation {
                set: SSBO_SET,
                binding: ssbo_binding,
            };
            ubo_binding += 1;
            ssbo_binding += 1;
        }
        // TODO samplers

        Ok(())
    }

    /// Allocates resources (images, buffers) that have been defined but not yet allocated.
    fn allocate_resources(&mut self) {

        // Collect resource usages among all aliases
        let mut image_usages = BTreeMap::new();
        let mut buffer_usages = BTreeMap::new();

        for (key, image) in self.images.iter() {
            let usage = image.inferred_usage;
            *image_usages.entry(image.alias.unwrap_or(*key)).or_insert(usage) |= usage;
        }
        for (key, buffer) in self.buffers.iter() {
            let usage = buffer.inferred_usage;
            *buffer_usages.entry(buffer.alias.unwrap_or(*key)).or_insert(usage) |= usage;
        }

        // Allocate images
        for (key, res) in self.images.iter_mut() {
            if res.alias.is_some() {
                continue;
            }

            if res.image.is_none() {
                res.inferred_usage = image_usages.get(key).copied().unwrap_or_default();
                let image = self.device.create_image(&ImageCreateInfo {
                    format: res.format,
                    width: res.width,
                    height: res.height,
                    usage: res.inferred_usage,
                    ..Default::default()
                });
                res.view = Some(image.create_top_level_view());
                res.image = Some(image);
            }
        }

        // Allocate buffers
        for (key, res) in self.buffers.iter_mut() {
            if res.alias.is_some() {
                continue;
            }

            if res.buffer.is_none() {
                res.inferred_usage = buffer_usages.get(key).copied().unwrap_or_default();
                res.buffer = Some(self.device.create_buffer(
                    res.inferred_usage,
                    MemoryLocation::GpuOnly,
                    res.byte_size as u64,
                ));
            }
        }
    }

    /// Updates the bindless descriptor set with the latest resource bindings.
    fn update_descriptors(&mut self) {
        let mut builder = ResourceDescriptorsBuilder::new();
        for (_key, res) in self.images.iter() {
            let image_view =
                if let Some(alias) = res.alias {
                    self.images.get(&alias).unwrap().view.clone().unwrap()
                } else {
                    res.view.clone().unwrap()
                };

            builder.bind_sampled_image(res.texture_binding.binding, image_view.clone());
            builder.bind_storage_image(res.storage_binding.binding, image_view.clone());
        }
        for (_key, res) in self.buffers.iter() {
            let buffer = if let Some(alias) = res.alias {
                self.buffers.get(&alias).unwrap().buffer.clone().unwrap()
            } else {
                res.buffer.clone().unwrap()
            };

            builder.bind_uniform_buffer(res.uniform_binding.binding, buffer.clone().byte_range(..));
            builder.bind_storage_buffer(res.storage_binding.binding, buffer.clone().byte_range(..));
        }
        self.descriptors = Some(builder.build(&self.device, &self.layout));
        // TODO samplers
    }

    pub fn compile(&mut self) -> Result<(), Error> {
        if self.invalid >= InvalidationLevel::Bindings {
            // The number of resources has changed.
            // Reassign binding numbers to resources first,
            // we need them when we compile the shaders in the next step.
            self.assign_resource_bindings()?;
        }

        if self.invalid >= InvalidationLevel::Pipelines {
            // Compile all shaders, create pipelines, and infer resource usages by examining the
            // compiled bytecode of shaders.
            // Also change the binding numbers in the bytecode to match the assigned bindings.
            self.update_pipelines()?;
        }

        if self.invalid >= InvalidationLevel::Resources {
            // Now that we know resource usages, we can allocate them.
            self.allocate_resources()?;
        }

        if self.invalid >= InvalidationLevel::Descriptors {
            // Finally, update descriptors with the new resources.
            self.update_descriptors();
        }

        Ok(())
    }

    /// Runs the passes.
    pub fn run(&mut self, cmd: &mut CommandStream, context: &mut C) -> Result<(), Error> {
        for pass in self.passes.iter_mut() {
            match pass {
                Pass::MeshShading(ref mut pass) => {
                    let Some(ref pipeline) = pass.pipeline else {
                        continue;
                    };

                    let mut color_attachments = vec![];

                    for color_attachment in pass.desc.color_attachments.iter() {
                        let image = self.images.get(&color_attachment.image).expect("unknown image");
                        let image_view = image.view.clone().expect("image view not created");

                        color_attachments.push(graal::ColorAttachment {
                            image_view: image_view.clone(),
                            clear_value: color_attachment.clear_value.unwrap_or_default(),
                            load_op: if color_attachment.clear_value.is_some() {
                                vk::AttachmentLoadOp::CLEAR
                            } else {
                                vk::AttachmentLoadOp::LOAD
                            },
                            store_op: vk::AttachmentStoreOp::STORE,
                        });
                    }

                    let depth_stencil_attachment = if let Some(ref depth_attachment) = pass.desc.depth_stencil_attachment {
                        let image = self.images.get(&depth_attachment.image).expect("unknown image");
                        let image_view = image.view.clone().expect("image view not created");

                        Some(graal::DepthStencilAttachment {
                            image_view,
                            depth_load_op: if depth_attachment.depth_clear_value.is_some() {
                                vk::AttachmentLoadOp::CLEAR
                            } else {
                                vk::AttachmentLoadOp::LOAD
                            },
                            depth_store_op: vk::AttachmentStoreOp::STORE,
                            stencil_load_op: if depth_attachment.stencil_clear_value.is_some() {
                                vk::AttachmentLoadOp::CLEAR
                            } else {
                                vk::AttachmentLoadOp::LOAD
                            },
                            stencil_store_op: vk::AttachmentStoreOp::STORE,
                            depth_clear_value: depth_attachment.depth_clear_value.unwrap_or_default(),
                            stencil_clear_value: depth_attachment.stencil_clear_value.unwrap_or_default(),
                        })
                    } else {
                        None
                    };

                    // transition all used resources to the expected state
                    for (key, access) in pass.used_images.iter() {
                        let image = self.images.get(key).expect("unknown image");
                        cmd.use_image_view(&image.view.as_ref().unwrap(), *access);
                    }
                    for (key, access) in pass.used_buffers.iter() {
                        let buffer = self.buffers.get(key).expect("unknown buffer");
                        cmd.use_buffer(&buffer.buffer.as_ref().unwrap(), *access);
                    }


                    let mut encoder = cmd.begin_rendering(RenderPassDescriptor {
                        color_attachments: &color_attachments[..],
                        depth_stencil_attachment,
                    });

                    // bind pipeline
                    encoder.bind_graphics_pipeline(pipeline);

                    // bind resources
                    self.descriptors
                        .as_ref()
                        .expect("descriptors not created")
                        .bind_render(&mut encoder);


                    // DONE push constants
                    bind_params(&mut pass.push_constant_data, &self.params);
                    encoder.push_constants(&pass.push_constant_data.data[..]);

                    // TODO draw params
                    //let draw_params = (pass.desc.draw)(context);
                    //encoder.draw_mesh_tasks(draw_params, group_count_y, group_count_z);
                    encoder.finish();
                }
                _ => {}
            }
        }

        Ok(())
    }
}
