mod bindless;
mod shader;
mod uniform_block;

use crate::engine2::{
    bindless::{BindlessLayout, ResourceDescriptorBindExt, ResourceDescriptors, IMG_SET, TEX_SET},
    shader::{compile_shader_stage, CompilationInfo},
    uniform_block::{UniformBlock, UniformType, UniformValue},
};
use bitflags::bitflags;
use bytemuck::cast_slice;
use graal::{
    get_shader_compiler, shaderc,
    shaderc::{EnvVersion, ShaderKind, SpirvVersion, TargetEnv},
    vk,
    vk::{Pipeline, Viewport},
    BufferAccess, BufferRangeUntyped, BufferUsage, ColorTargetState, CommandStream, ComputeEncoder, ComputePipelineCreateInfo,
    DepthStencilAttachment, DepthStencilState, Device, FragmentState, GraphicsPipelineCreateInfo, ImageAccess, ImageCreateInfo,
    ImageSubresourceLayers, ImageUsage, ImageView, MemoryLocation, MultisampleState, Point3D, PreRasterizationShaders, RasterizationState,
    Rect3D, RenderEncoder, RenderPassDescriptor, SamplerCreateInfo, ShaderCode, ShaderEntryPoint,
};
use scoped_tls::scoped_thread_local;
use slotmap::SlotMap;
use spirv_reflect::types::{ReflectDescriptorType, ReflectTypeFlags};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
};
use tracing::{debug, error, warn};

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
    #[error("Vulkan error: {0}")]
    VulkanError(graal::Error),
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
/*

slotmap::new_key_type! {
    pub struct BufferKey;
    pub struct ImageKey;
}
*/

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BufferKey(pub &'static str);

/*
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
}*/

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageKey(pub &'static str);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PassKey(pub &'static str);

#[derive(Copy, Clone, Debug)]
pub struct ImageDesc {
    pub width: u32,
    pub height: u32,
    pub format: vk::Format,
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
#[derive(Clone, Debug)]
struct ImageResource {
    name: String,
    desc: ImageDesc,
    /// Inferred usage flags.
    inferred_usage: Cell<ImageUsage>,
    /// Index in the descriptor arrays.
    descriptor_index: Cell<u32>,
    /// The allocated or imported image resource.
    image: RefCell<Option<graal::Image>>,
    /// Top-level image view.
    view: RefCell<Option<ImageView>>,
}

/// Handle to an image resource.
#[derive(Clone, Debug)]
pub struct Image(Rc<ImageResource>);

impl Image {
    fn add_usage(&self, usage: ImageUsage) {
        self.0.inferred_usage.set(self.0.inferred_usage.get() | usage);
    }

    fn descriptor_index(&self) -> u32 {
        self.0.descriptor_index.get()
    }

    fn ensure_allocated(&self, device: &Device) {
        if self.0.image.borrow().is_none() {
            let desc = self.0.desc;
            let image = device.create_image(&ImageCreateInfo {
                format: desc.format,
                width: desc.width,
                height: desc.height,
                usage: self.0.inferred_usage.get(),
                ..Default::default()
            });
            image.set_name(&self.0.name);
            self.0.view.replace(Some(image.create_top_level_view()));
            self.0.image.replace(Some(image));
        }
    }

    fn view(&self) -> ImageView {
        self.0.view.borrow().clone().expect("image view not created")
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn image(&self) -> graal::Image {
        self.0.image.borrow().clone().expect("image not created")
    }

    pub fn width(&self) -> u32 {
        self.0.desc.width
    }

    pub fn height(&self) -> u32 {
        self.0.desc.height
    }
}

impl PartialOrd for Image {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Rc::as_ptr(&self.0).partial_cmp(&Rc::as_ptr(&other.0))
    }
}

impl Ord for Image {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Rc::as_ptr(&self.0).cmp(&Rc::as_ptr(&other.0))
    }
}

// Referential equality for images
impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Image {}

impl std::hash::Hash for Image {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

/// Describes a buffer resource.
///
/// # Size
///
///
#[derive(Clone, Debug)]
struct BufferResource {
    name: String,
    /// Buffer usage flags.
    inferred_usage: Cell<BufferUsage>,
    /// Inferred memory properties.
    inferred_memory_properties: Cell<vk::MemoryPropertyFlags>,
    /// Explicit byte size.
    byte_size: usize,
    /// The allocated buffer resource.
    buffer: RefCell<Option<graal::BufferUntyped>>,
    descriptor_index: Cell<u32>,
}

/// Handle to a buffer resource.
#[derive(Clone, Debug)]
pub struct Buffer(Rc<BufferResource>);

impl Buffer {
    fn add_usage(&self, usage: BufferUsage) {
        self.0.inferred_usage.set(self.0.inferred_usage.get() | usage);
    }

    fn descriptor_index(&self) -> u32 {
        self.0.descriptor_index.get()
    }

    fn ensure_allocated(&self, device: &Device) {
        if self.0.buffer.borrow().is_none() {
            let buffer = device.create_buffer(self.0.inferred_usage.get(), MemoryLocation::GpuOnly, self.0.byte_size as u64);
            buffer.set_name(&self.0.name);
            self.0.buffer.replace(Some(buffer));
        }
    }

    fn buffer(&self) -> graal::BufferUntyped {
        self.0.buffer.borrow().clone().expect("buffer not created")
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn device_address(&self) -> vk::DeviceAddress {
        self.buffer().device_address()
    }
}

impl PartialOrd for Buffer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Rc::as_ptr(&self.0).partial_cmp(&Rc::as_ptr(&other.0))
    }
}

impl Ord for Buffer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        Rc::as_ptr(&self.0).cmp(&Rc::as_ptr(&other.0))
    }
}

impl PartialEq for Buffer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Buffer {}

impl std::hash::Hash for Buffer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

struct PipelinePassBase {
    used_images: BTreeMap<ImageHandle, ImageAccess>,
    used_buffers: BTreeMap<BufferHandle, BufferAccess>,
    push_constant_data: UniformBlock,
}

impl PipelinePassBase {
    /*fn set_texture(&mut self, name: &str, image: Image) {
        let handle = image.descriptor_index();
        self.push_constant_data.set(name, handle).unwrap();
        *self.used_images.entry(image.clone()).or_insert(ImageAccess::SAMPLED_READ) |= ImageAccess::SAMPLED_READ;
        image.add_usage(ImageUsage::SAMPLED);
    }

    fn set_storage_image(&mut self, name: &str, image: Image) {
        let handle = image.descriptor_index();
        self.push_constant_data.set(name, handle).unwrap();
        *self.used_images.entry(image.clone()).or_insert(ImageAccess::IMAGE_READ_WRITE) |= ImageAccess::IMAGE_READ_WRITE;
        image.add_usage(ImageUsage::STORAGE);
    }

    fn set_buffer_address(&mut self, name: &str, buffer: Buffer) {
        let address = buffer.buffer().device_address();
        self.push_constant_data.set(name, UniformValue::DeviceAddress(address)).unwrap();
        *self.used_buffers.entry(buffer.clone()).or_insert(BufferAccess::STORAGE_READ_WRITE) |= BufferAccess::STORAGE_READ_WRITE;
        buffer.add_usage(BufferUsage::STORAGE_BUFFER);
    }*/

    fn sample_image(&mut self, image: ImageHandle) {
        *self.used_images.entry(image).or_insert(ImageAccess::SAMPLED_READ) |= ImageAccess::SAMPLED_READ;
    }

    fn read_image(&mut self, image: ImageHandle) {
        *self.used_images.entry(image).or_insert(ImageAccess::IMAGE_READ) |= ImageAccess::IMAGE_READ;
    }

    fn write_image(&mut self, image: ImageHandle) {
        *self.used_images.entry(image).or_insert(ImageAccess::IMAGE_READ_WRITE) |= ImageAccess::IMAGE_READ_WRITE;
    }

    fn read_buffer(&mut self, buffer: BufferHandle) {
        *self.used_buffers.entry(buffer).or_insert(BufferAccess::STORAGE_READ_WRITE) |= BufferAccess::STORAGE_READ_WRITE;
    }

    fn write_buffer(&mut self, buffer: BufferHandle) {
        *self.used_buffers.entry(buffer).or_insert(BufferAccess::STORAGE_READ_WRITE) |= BufferAccess::STORAGE_READ_WRITE;
    }

    fn transition_resources(&self, ctx: &mut RecordContext) {
        for (image, access) in self.used_images.iter() {
            ctx.cmd.use_image_view(&image.view(), *access);
        }
        for (buffer, access) in self.used_buffers.iter() {
            ctx.cmd.use_buffer(&buffer.buffer(), *access);
        }
    }

    fn update_constants(&mut self, ctx: &mut RecordContext) {
        for (name, param) in ctx.constants.iter() {
            // ignore errors here, it just means that the shader doesn't use this constant
            self.push_constant_data.set(name, param.clone());
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct ColorAttachmentDesc {
    pub image: ImageHandle,
    pub clear_value: Option<[f64; 4]>,
}

pub struct DepthStencilAttachmentDesc {
    pub image: ImageHandle,
    pub depth_clear_value: Option<f64>,
    pub stencil_clear_value: Option<u32>,
}

pub struct MeshRenderPipelineDesc {
    pub shader: PathBuf,
    pub defines: BTreeMap<String, String>,
    pub color_targets: Vec<ColorTargetState>,
    pub rasterization_state: RasterizationState,
    pub depth_stencil_state: Option<DepthStencilState>,
    pub multisample_state: MultisampleState,
}

type UniformBlockLayout = BTreeMap<String, (u32, UniformType)>;

struct MeshRenderPipelineInner {
    desc: MeshRenderPipelineDesc,
    push_constants_layout: UniformBlockLayout,
    push_constants_size: usize,
    pipeline: graal::GraphicsPipeline,
}

#[derive(Clone)]
pub struct MeshRenderPipeline(Rc<MeshRenderPipelineInner>);

struct MeshRenderPass {
    base: PipelinePassBase,
    pipeline: MeshRenderPipeline,
    color_attachments: Vec<ColorAttachmentDesc>,
    depth_stencil_attachment: Option<DepthStencilAttachmentDesc>,
    //group_count: (u32, u32, u32),
    //viewport: Option<[f32; 6]>,
    //scissor: Option<[i32; 4]>,
    func: Option<Box<dyn FnOnce(&mut graal::RenderEncoder)>>,
}

struct RecordContext<'a> {
    cmd: &'a mut CommandStream,
    descriptors: ResourceDescriptors,
    constants: &'a BTreeMap<String, UniformValue>,
}

impl MeshRenderPass {
    fn record(&mut self, ctx: &mut RecordContext) {
        let mut color_attachments = vec![];
        for color_attachment in self.color_attachments.iter() {
            //let image = self.images.get(&color_attachment.image).expect("unknown image");
            //let image_view = image.view.clone().expect("image view not created");
            color_attachments.push(graal::ColorAttachment {
                image_view: color_attachment.image.view(),
                clear_value: color_attachment.clear_value.unwrap_or_default(),
                load_op: if color_attachment.clear_value.is_some() {
                    vk::AttachmentLoadOp::CLEAR
                } else {
                    vk::AttachmentLoadOp::LOAD
                },
                store_op: vk::AttachmentStoreOp::STORE,
            });
        }

        let depth_stencil_attachment = if let Some(ref depth_attachment) = self.depth_stencil_attachment {
            Some(graal::DepthStencilAttachment {
                image_view: depth_attachment.image.view(),
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

        self.base.transition_resources(ctx);
        self.base.update_constants(ctx);

        let extent;
        if let Some(color) = self.color_attachments.first() {
            extent = color.image.view().size();
        } else if let Some(ref depth) = self.depth_stencil_attachment {
            extent = depth.image.view().size();
        } else {
            panic!("render pass has no attachments");
        }

        let mut encoder = ctx.cmd.begin_rendering(RenderPassDescriptor {
            color_attachments: &color_attachments[..],
            depth_stencil_attachment,
        });
        encoder.bind_graphics_pipeline(&self.pipeline.0.pipeline);
        encoder.bind_resource_descriptors(&ctx.descriptors);
        encoder.set_viewport(0.0, 0.0, extent.width as f32, extent.height as f32, 0.0, 1.0);
        encoder.set_scissor(0, 0, extent.width, extent.height);

        if let Some(cb) = self.func.take() {
            cb(&mut encoder);
        }

        //encoder.push_constants_slice(self.base.push_constant_data.data());
        //encoder.draw_mesh_tasks(self.group_count.0, self.group_count.1, self.group_count.2);

        encoder.finish();
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct ComputePipelineDesc {
    pub shader: PathBuf,
    pub defines: BTreeMap<String, String>,
}

struct ComputePipelineInner {
    desc: ComputePipelineDesc,
    pipeline: graal::ComputePipeline,
    push_constants_layout: UniformBlockLayout,
    push_constants_size: usize,
}

#[derive(Clone)]
pub struct ComputePipeline(Rc<ComputePipelineInner>);

struct ComputePass {
    base: PipelinePassBase,
    pipeline: ComputePipeline,
    //group_count: (u32, u32, u32),
    func: Option<Box<dyn FnOnce(&mut ComputeEncoder)>>,
}

impl ComputePass {
    fn record(&mut self, ctx: &mut RecordContext) {
        let pipeline = &self.pipeline.0.pipeline;
        self.base.transition_resources(ctx);
        self.base.update_constants(ctx);
        let mut encoder = ctx.cmd.begin_compute();
        encoder.bind_compute_pipeline(pipeline);
        encoder.bind_resource_descriptors(&ctx.descriptors);
        if let Some(cb) = self.func.take() {
            cb(&mut encoder);
        }
        //encoder.push_constants_slice(self.base.push_constant_data.data());
        //encoder.dispatch(self.group_count.0, self.group_count.1, self.group_count.2);
        encoder.finish();
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// `vkCmdBlitImage` pass.
struct BlitPass {
    src: ImageHandle,
    dst: ImageHandle,
}

impl BlitPass {
    fn record(&self, ctx: &mut RecordContext) {
        let src = self.src.image();
        let dst = self.dst.image();
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
}

/// `vkCmdFillBuffer` pass.
struct FillBufferPass {
    buffer: BufferHandle,
    value: u32,
}

impl FillBufferPass {
    fn record(&self, ctx: &mut RecordContext) {
        let buffer = self.buffer.buffer();
        ctx.cmd.fill_buffer(&buffer.byte_range(..), self.value);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
enum PassKind {
    FillBuffer(FillBufferPass),
    Blit(BlitPass),
    MeshRender(MeshRenderPass),
    Compute(ComputePass),
}

struct Pass {
    name: String,
    kind: PassKind,
}

enum Param {
    Float { value: f32, min: f32, max: f32 },
}

enum DescriptorData {
    Image(ImageKey),
    Buffer(BufferKey),
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

struct RenderGraphResources {
    buffers: Vec<Buffer>,
    images: Vec<Image>,
}

scoped_thread_local!(static RENDER_GRAPH_RESOURCES: RenderGraphResources);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct SamplerHandle(pub u32);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BufferHandle(u32);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageHandle(u32);

impl BufferHandle {
    pub fn device_address(self) -> u64 {
        RENDER_GRAPH_RESOURCES.with(|resources| resources.buffers[self.0 as usize].device_address())
    }

    pub fn buffer(self) -> graal::BufferUntyped {
        RENDER_GRAPH_RESOURCES.with(|resources| resources.buffers[self.0 as usize].buffer())
    }
}

impl ImageHandle {
    pub fn device_handle(self) -> u32 {
        self.0
    }

    pub fn view(self) -> ImageView {
        RENDER_GRAPH_RESOURCES.with(|resources| resources.images[self.0 as usize].view())
    }

    pub fn image(self) -> graal::Image {
        RENDER_GRAPH_RESOURCES.with(|resources| resources.images[self.0 as usize].image())
    }
}

impl SamplerHandle {
    pub fn as_raw(self) -> u32 {
        self.0
    }
}

/// Builder for a frame.
///
/// Re-created on every frame.
pub struct RenderGraph {
    device: Device,
    passes: Vec<Pass>,
    resources: RenderGraphResources,
    named_buffers: BTreeMap<BufferKey, Buffer>,
    named_images: BTreeMap<ImageKey, Image>,
    samplers: Vec<graal::Sampler>,
    constants: BTreeMap<String, UniformValue>,
}

impl RenderGraph {
    pub fn set_global_constant(&mut self, name: impl Into<String>, value: impl Into<UniformValue>) {
        self.constants.insert(name.into(), value.into());
    }

    pub fn add_buffer_usage(&mut self, buffer: BufferHandle, usage: BufferUsage) {
        self.resources.buffers[buffer.0 as usize].add_usage(usage);
    }

    pub fn add_image_usage(&mut self, image: ImageHandle, usage: ImageUsage) {
        self.resources.images[image.0 as usize].add_usage(usage);
    }

    pub fn record_compute_pass(&mut self, name: impl Into<String>, pipeline: ComputePipeline) -> ComputePassBuilder<'_> {
        ComputePassBuilder {
            rg: self,
            name: name.into(),
            pass: ComputePass {
                base: PipelinePassBase {
                    used_images: Default::default(),
                    used_buffers: Default::default(),
                    push_constant_data: UniformBlock::new(pipeline.0.push_constants_size, pipeline.0.push_constants_layout.clone()),
                },
                pipeline,
                func: None,
            },
        }
    }

    pub fn record_mesh_render_pass(&mut self, name: impl Into<String>, pipeline: MeshRenderPipeline) -> MeshRenderBuilder<'_> {
        MeshRenderBuilder {
            rg: self,
            name: name.into(),
            pass: MeshRenderPass {
                base: PipelinePassBase {
                    used_images: Default::default(),
                    used_buffers: Default::default(),
                    push_constant_data: UniformBlock::new(pipeline.0.push_constants_size, pipeline.0.push_constants_layout.clone()),
                },
                pipeline,
                color_attachments: Default::default(),
                depth_stencil_attachment: Default::default(),
                func: None,
            },
        }
    }

    pub fn record_blit(&mut self, name: impl Into<String>, src: ImageHandle, dst: ImageHandle) {
        self.add_image_usage(src, ImageUsage::TRANSFER_SRC);
        self.add_image_usage(dst, ImageUsage::TRANSFER_DST);
        self.passes.push(Pass {
            name: name.into(),
            kind: PassKind::Blit(BlitPass { src, dst }),
        });
    }

    pub fn record_fill_buffer(&mut self, name: impl Into<String>, buffer: BufferHandle, value: u32) {
        self.add_buffer_usage(buffer, BufferUsage::TRANSFER_DST);
        self.passes.push(Pass {
            name: name.into(),
            kind: PassKind::FillBuffer(FillBufferPass { buffer, value }),
        });
    }

    pub fn import_buffer(&mut self, name: &str, buffer: graal::BufferUntyped) -> BufferHandle {
        let descriptor_index = self.resources.buffers.len() as u32;
        let buffer_inner = BufferResource {
            name: name.to_string(),
            inferred_usage: Cell::new(buffer.usage()),
            inferred_memory_properties: Default::default(),
            byte_size: buffer.byte_size() as usize,
            buffer: RefCell::new(Some(buffer)),
            descriptor_index: Cell::new(descriptor_index),
        };
        let buffer = Buffer(Rc::new(buffer_inner));
        self.resources.buffers.push(buffer.clone());
        BufferHandle(descriptor_index)
    }

    /*
    pub fn import_named_buffer(&mut self, key: BufferKey, buffer: graal::BufferUntyped) -> Buffer {
        let buffer = self.import_buffer(key.0, buffer);
        self.named_buffers.insert(key, buffer.clone());
        buffer
    }*/

    /*
    pub fn import_named_image(&mut self, key: ImageKey, image: graal::Image) -> Image {
        let image = self.import_image(key.0, image);
        self.named_images.insert(key, image.clone());
        image
    }*/

    pub fn import_image(&mut self, name: &str, image: graal::Image) -> ImageHandle {
        let descriptor_index = self.resources.images.len() as u32;
        let image_inner = ImageResource {
            name: name.to_string(),
            desc: ImageDesc {
                width: image.width(),
                height: image.height(),
                format: image.format(),
            },
            inferred_usage: Cell::new(Default::default()),
            descriptor_index: Cell::new(descriptor_index),
            image: RefCell::new(Some(image.clone())),
            view: RefCell::new(Some(image.create_top_level_view())),
        };
        let image = Image(Rc::new(image_inner));
        self.resources.images.push(image.clone());
        ImageHandle(descriptor_index)
    }

    pub fn create_image(&mut self, name: &str, desc: ImageDesc) -> ImageHandle {
        let descriptor_index = self.resources.images.len() as u32;
        let image_inner = ImageResource {
            name: name.to_string(),
            desc,
            inferred_usage: Cell::new(Default::default()),
            descriptor_index: Cell::new(descriptor_index),
            image: RefCell::new(None),
            view: RefCell::new(None),
        };
        let image = Image(Rc::new(image_inner));
        self.resources.images.push(image.clone());
        ImageHandle(descriptor_index)
    }

    /*
    pub fn create_named_image(&mut self, key: ImageKey, desc: ImageDesc) -> Image {
        let image = self.create_image(key.0, desc);
        self.named_images.insert(key, image.clone());
        image
    }

    pub fn create_named_buffer(&mut self, key: BufferKey, byte_size: usize) -> Buffer {
        let buffer = self.create_buffer(key.0, byte_size);
        self.named_buffers.insert(key, buffer.clone());
        buffer
    }*/

    pub fn create_buffer(&mut self, name: &str, byte_size: usize) -> BufferHandle {
        let descriptor_index = self.resources.buffers.len() as u32;
        let buffer_inner = BufferResource {
            name: name.to_string(),
            inferred_usage: Cell::new(Default::default()),
            inferred_memory_properties: Cell::new(Default::default()),
            byte_size,
            buffer: RefCell::new(None),
            descriptor_index: Cell::new(descriptor_index),
        };
        let buffer = Buffer(Rc::new(buffer_inner));
        self.resources.buffers.push(buffer.clone());
        BufferHandle(descriptor_index)
    }

    pub fn create_sampler(&mut self, create_info: SamplerCreateInfo) -> SamplerHandle {
        let sampler = self.device.create_sampler(&create_info);
        self.samplers.push(sampler);
        SamplerHandle(self.samplers.len() as u32 - 1)
    }

    /*
    pub fn get_named_image(&self, key: ImageKey) -> Result<Image, Error> {
        Ok(self.named_images.get(&key).ok_or(Error::ResourceNotFound(key.0.to_string()))?.clone())
    }

    pub fn get_named_buffer(&self, key: BufferKey) -> Result<Buffer, Error> {
        Ok(self.named_buffers.get(&key).ok_or(Error::ResourceNotFound(key.0.to_string()))?.clone())
    }*/
}

/*
pub struct PassBuilder<'a> {
    name: String,
    used_images: BTreeMap<Image, ImageAccess>,
    used_buffers: BTreeMap<Buffer, BufferAccess>,
    push_constant_data: UniformBlock,
}

impl<'a> PassBuilder<'a> {
    fn new(rg: &'a mut RenderGraph, name: String) -> Self {
        Self {
            rg,
            name,
            used_images: Default::default(),
            used_buffers: Default::default(),
            push_constant_data: Default::default(),
        }
    }

    pub fn set_texture(&mut self, name: &str, image: Image) {
        let handle = image.descriptor_index();
        self.push_constant_data.set(name, handle).unwrap();
        *self.used_images.entry(image.clone()).or_insert(ImageAccess::SAMPLED_READ) |= ImageAccess::SAMPLED_READ;
        image.add_usage(ImageUsage::SAMPLED);
    }

    pub fn set_texture_named(&mut self, name: &str, image_key: ImageKey) -> Result<(), Error> {
        let image = self.rg.named_images.get(&image_key)?;
        self.set_texture(name, image.clone());
        Ok(())
    }

    pub fn set_storage_image(&mut self, name: &str, image: Image) {
        let handle = image.descriptor_index();
        self.push_constant_data.set(name, handle).unwrap();
        *self.used_images.entry(image.clone()).or_insert(ImageAccess::IMAGE_READ_WRITE) |= ImageAccess::IMAGE_READ_WRITE;
        image.add_usage(ImageUsage::STORAGE);
    }

    pub fn set_storage_image_named(&mut self, name: &str, image_key: ImageKey) -> Result<(), Error> {
        let image = self.rg.named_images.get(&image_key)?;
        self.set_storage_image(name, image.clone());
        Ok(())
    }

    pub fn set_buffer_address(&mut self, name: &str, buffer: Buffer) {
        let address = buffer.buffer().device_address();
        self.push_constant_data.set(name, UniformValue::DeviceAddress(address)).unwrap();
        *self.used_buffers.entry(buffer.clone()).or_insert(BufferAccess::STORAGE_READ_WRITE) |= BufferAccess::STORAGE_READ_WRITE;
        buffer.add_usage(BufferUsage::STORAGE_BUFFER);
    }

    pub fn set_buffer_address_named(&mut self, name: &str, buffer_key: BufferKey) -> Result<(), Error> {
        let buffer = self.rg.named_buffers.get(&buffer_key)?;
        self.set_buffer_address(name, buffer.clone());
        Ok(())
    }
}*/

pub struct MeshRenderBuilder<'a> {
    rg: &'a mut RenderGraph,
    name: String,
    pass: MeshRenderPass,
}

impl<'a> MeshRenderBuilder<'a> {
    pub fn set_color_attachments(&mut self, desc: impl IntoIterator<Item=ColorAttachmentDesc>) {
        self.pass.color_attachments = desc.into_iter().collect();
        for desc in self.pass.color_attachments.iter() {
            self.pass.base.used_images.insert(desc.image.clone(), ImageAccess::COLOR_TARGET);
            self.rg.add_image_usage(desc.image, ImageUsage::COLOR_ATTACHMENT);
        }
    }

    pub fn set_depth_stencil_attachment(&mut self, desc: DepthStencilAttachmentDesc) {
        self.pass.base.used_images.insert(
            desc.image.clone(),
            ImageAccess::DEPTH_STENCIL_READ | ImageAccess::DEPTH_STENCIL_WRITE,
        );
        self.rg.add_image_usage(desc.image, ImageUsage::DEPTH_STENCIL_ATTACHMENT);
        self.pass.depth_stencil_attachment = Some(desc);
    }

    pub fn set_sampler(&mut self, name: &str, sampler_index: SamplerHandle) {
        self.pass.base.push_constant_data.set(name, sampler_index.0).unwrap();
    }

    pub fn set_sampler_immediate(&mut self, name: &str, desc: SamplerCreateInfo) {
        let sampler = self.rg.create_sampler(desc);
        self.pass.base.push_constant_data.set(name, sampler.0).unwrap();
    }

    pub fn set_render_func(&mut self, f: impl FnOnce(&mut RenderEncoder) + 'static) {
        self.pass.func = Some(Box::new(f));
    }

    pub fn sample_image(&mut self, image: ImageHandle) {
        self.rg.add_image_usage(image, ImageUsage::SAMPLED);
        self.pass.base.read_image(image);
    }

    pub fn read_image(&mut self, image: ImageHandle) {
        self.rg.add_image_usage(image, ImageUsage::STORAGE);
        self.pass.base.read_image(image);
    }

    pub fn write_image(&mut self, image: ImageHandle) {
        self.rg.add_image_usage(image, ImageUsage::STORAGE);
        self.pass.base.write_image(image);
    }

    pub fn read_buffer(&mut self, buffer: BufferHandle) {
        self.rg.add_buffer_usage(buffer, BufferUsage::STORAGE_BUFFER);
        self.pass.base.read_buffer(buffer);
    }

    pub fn write_buffer(&mut self, buffer: BufferHandle) {
        self.rg.add_buffer_usage(buffer, BufferUsage::STORAGE_BUFFER);
        self.pass.base.write_buffer(buffer);
    }

    /*
    pub fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32, min_depth: f32, max_depth: f32) {
        self.pass.viewport = Some([x, y, width, height, min_depth, max_depth]);
    }

    pub fn set_scissor(&mut self, x: i32, y: i32, width: u32, height: u32) {
        self.pass.scissor = Some([x, y, width as i32, height as i32]);
    }

    pub fn set_group_count(&mut self, x: u32, y: u32, z: u32) {
        self.pass.group_count = (x, y, z);
    }*/

    /*pub fn set_constant(&mut self, name: &str, value: impl Into<UniformValue>) {
        self.pass.base.push_constant_data.set(name, value.into()).unwrap();
    }*/

    pub fn finish(self) {
        assert!(self.pass.func.is_some(), "render pass must have a render function");
        self.rg.passes.push(Pass {
            name: self.name,
            kind: PassKind::MeshRender(self.pass),
        })
    }
}

pub struct ComputePassBuilder<'a> {
    rg: &'a mut RenderGraph,
    name: String,
    pass: ComputePass,
}

impl<'a> ComputePassBuilder<'a> {
    /*pub fn set_texture(&mut self, name: &str, image: Image) {
        self.pass.base.set_texture(name, image);
    }

    pub fn set_storage_image(&mut self, name: &str, image: Image) {
        self.pass.base.set_storage_image(name, image);
    }

    pub fn set_buffer_address(&mut self, name: &str, buffer: Buffer) {
        self.pass.base.set_buffer_address(name, buffer);
    }*/

    pub fn sample_image(&mut self, image: ImageHandle) {
        self.rg.add_image_usage(image, ImageUsage::SAMPLED);
        self.pass.base.read_image(image);
    }

    pub fn read_image(&mut self, image: ImageHandle) {
        self.rg.add_image_usage(image, ImageUsage::STORAGE);
        self.pass.base.read_image(image);
    }

    pub fn write_image(&mut self, image: ImageHandle) {
        self.rg.add_image_usage(image, ImageUsage::STORAGE);
        self.pass.base.write_image(image);
    }

    pub fn read_buffer(&mut self, buffer: BufferHandle) {
        self.rg.add_buffer_usage(buffer, BufferUsage::STORAGE_BUFFER);
        self.pass.base.read_buffer(buffer);
    }

    pub fn write_buffer(&mut self, buffer: BufferHandle) {
        self.rg.add_buffer_usage(buffer, BufferUsage::STORAGE_BUFFER);
        self.pass.base.write_buffer(buffer);
    }

    pub fn set_constant(&mut self, name: &str, value: impl Into<UniformValue>) {
        self.pass.base.push_constant_data.set(name, value.into()).unwrap();
    }

    pub fn set_render_func(&mut self, f: impl FnOnce(&mut ComputeEncoder) + 'static) {
        self.pass.func = Some(Box::new(f));
    }

    pub fn finish(self) {
        assert!(self.pass.func.is_some(), "compute pass must have a render function");
        self.rg.passes.push(Pass {
            name: self.name,
            kind: PassKind::Compute(self.pass),
        })
    }
}

#[derive(Clone, Debug)]
enum Resource {
    Image(Image),
    Buffer(Buffer),
}

/// Rendering engine instance.
///
/// # Descriptors
///
/// To simplify things, we use a single big descriptor set containing descriptors for all combinations
/// of resources and possible accesses (e.g., for each image, a descriptor for sampled access and
/// another for storage access, and for buffers, a descriptor for uniform access and another for storage).
pub struct Engine {
    device: graal::Device,
    /// Defines added to every compiled shader
    global_defs: BTreeMap<String, String>,
    /// Parameters.
    ///
    /// Automatically bound (by name matching) to matching fields in push constant blocks inside
    /// shaders.
    params: BTreeMap<String, Param>,
    bindless_layout: BindlessLayout,
    mesh_render_pipelines: BTreeMap<String, MeshRenderPipeline>,
    compute_pipelines: BTreeMap<String, ComputePipeline>,
}

impl Engine {
    pub fn new(device: graal::Device) -> Self {
        let layout = BindlessLayout::new(&device);
        Self {
            device,
            global_defs: Default::default(),
            params: Default::default(),
            bindless_layout: layout,
            mesh_render_pipelines: Default::default(),
            compute_pipelines: Default::default(),
        }
    }

    pub fn create_graph(&mut self) -> RenderGraph {
        RenderGraph {
            device: self.device.clone(),
            passes: Default::default(),
            resources: RenderGraphResources {
                buffers: Default::default(),
                images: Default::default(),
            },
            named_buffers: Default::default(),
            named_images: Default::default(),
            samplers: vec![],
            constants: Default::default(),
        }
    }

    pub fn submit_graph(&mut self, graph: RenderGraph, cmd: &mut CommandStream) {
        // 1. allocate resources
        //let device = &self.engine.device;
        for image in graph.resources.images.iter() {
            image.ensure_allocated(&self.device);
        }
        for buffer in graph.resources.buffers.iter() {
            buffer.ensure_allocated(&self.device);
        }

        // 2. build descriptors
        // for buffers we use BDA
        let descriptors = self
            .bindless_layout
            .create_descriptors(&self.device, &graph.resources.images, &graph.samplers);
        cmd.reference_resource(&descriptors);

        RENDER_GRAPH_RESOURCES.set(&graph.resources, || {
            // 3. record passes
            let mut ctx = RecordContext {
                cmd,
                descriptors,
                constants: &graph.constants,
            };

            for pass in graph.passes {
                match pass.kind {
                    PassKind::FillBuffer(mut fill_buffer) => {
                        fill_buffer.record(&mut ctx);
                    }
                    PassKind::Blit(mut blit) => {
                        blit.record(&mut ctx);
                    }
                    PassKind::MeshRender(mut mesh_render_pass) => {
                        mesh_render_pass.record(&mut ctx);
                    }
                    PassKind::Compute(mut compute_pass) => {
                        compute_pass.record(&mut ctx);
                    }
                }
            }
        });

        cmd.flush(&[], &[]).unwrap()
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

    pub fn create_compute_pipeline(&mut self, name: &str, desc: ComputePipelineDesc) -> Result<ComputePipeline, Error> {
        if let Some(pipeline) = self.compute_pipelines.get(name) {
            return Ok(pipeline.clone());
        }

        let file_path = &desc.shader;
        let gdefs = &self.global_defs;
        let defs = &desc.defines;
        let mut ci = CompilationInfo::default();

        let compute_spv = match compile_shader_stage(&file_path, &gdefs, &defs, ShaderKind::Compute, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile compute shader: {err}");
                return Err(err).into();
            }
        };

        let cpci = ComputePipelineCreateInfo {
            set_layouts: &[
                self.bindless_layout.textures.clone(),
                self.bindless_layout.images.clone(),
                self.bindless_layout.samplers.clone(),
            ],
            push_constants_size: ci.push_cst_size,
            compute_shader: ShaderEntryPoint {
                code: ShaderCode::Spirv(&compute_spv),
                entry_point: "main",
            },
        };

        match self.device.create_compute_pipeline(cpci) {
            Ok(pipeline) => {
                let pipeline = ComputePipeline(Rc::new(ComputePipelineInner {
                    desc,
                    pipeline,
                    push_constants_layout: ci.push_cst_map,
                    push_constants_size: ci.push_cst_size,
                }));
                self.compute_pipelines.insert(name.to_string(), pipeline.clone());
                Ok(pipeline)
            }
            Err(err) => {
                panic!("update_pipelines: failed to create compute pipeline: {:?}", err);
            }
        }
    }

    pub fn create_mesh_render_pipeline(&mut self, name: &str, desc: MeshRenderPipelineDesc) -> Result<MeshRenderPipeline, Error> {
        if let Some(pipeline) = self.mesh_render_pipelines.get(name) {
            return Ok(pipeline.clone());
        }

        let file_path = &desc.shader;
        let gdefs = &self.global_defs;
        let defs = &desc.defines;
        let mut ci = CompilationInfo::default();

        let task_spv = match compile_shader_stage(&file_path, &gdefs, &defs, ShaderKind::Task, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile task shader: {err}");
                return Err(err).into();
            }
        };
        let mesh_spv = match compile_shader_stage(&file_path, &gdefs, &defs, ShaderKind::Mesh, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile mesh shader: {err}");
                return Err(err).into();
            }
        };
        let fragment_spv = match compile_shader_stage(&file_path, &gdefs, &defs, ShaderKind::Fragment, &mut ci) {
            Ok(spv) => spv,
            Err(err) => {
                error!("failed to compile fragment shader: {err}");
                return Err(err).into();
            }
        };

        let gpci = GraphicsPipelineCreateInfo {
            set_layouts: &[
                self.bindless_layout.textures.clone(),
                self.bindless_layout.images.clone(),
                self.bindless_layout.samplers.clone(),
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
                let pipeline = MeshRenderPipeline(Rc::new(MeshRenderPipelineInner {
                    desc,
                    push_constants_layout: ci.push_cst_map,
                    push_constants_size: ci.push_cst_size,
                    pipeline,
                }));
                self.mesh_render_pipelines.insert(name.to_string(), pipeline.clone());
                Ok(pipeline)
            }
            Err(err) => {
                panic!("update_pipelines: failed to create mesh render pipeline: {:?}", err);
            }
        }
    }
}
