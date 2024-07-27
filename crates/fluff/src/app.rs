use egui::{Align2, Color32, FontId, Frame, Key, Margin, Modifiers, Response, Rounding, Slider, Ui, Widget};
use egui_extras::{Column, TableBuilder};
use glam::{dvec2, mat4, uvec2, vec2, vec3, vec4, DVec2, Vec2, DVec3, Vec4Swizzles, DVec4, dvec3, Vec3Swizzles};
use graal::{prelude::*, vk::{AttachmentLoadOp, AttachmentStoreOp}, Buffer, BufferRange, ColorAttachment, ComputePipeline, ComputePipelineCreateInfo, DepthStencilAttachment, Descriptor, ImageAccess, ImageCopyBuffer, ImageCopyView, ImageDataLayout, ImageSubresourceLayers, ImageView, Point3D, Rect3D, RenderPassInfo, Barrier, Texture2DHandleRange, DeviceAddress};
use std::{
    collections::BTreeMap,
    fs, mem,
    path::{Path, PathBuf},
    ptr,
};
use tracing::{error, info, trace};

use houdinio::Geo;
use winit::{
    event::{MouseButton, TouchPhase},
    keyboard::NamedKey,
};
use rand::{random, Rng, thread_rng};

use crate::{
    camera_control::CameraControl,
    engine::{ComputePipelineDesc, Engine, Error, MeshRenderPipelineDesc},
    overlay::{CubicBezierSegment, OverlayRenderParams, OverlayRenderer},
    shaders,
    shaders::shared::{CurveDesc, DrawCurvesPushConstants, SummedAreaTableParams, TemporalAverageParams, TileData, BINNING_TILE_SIZE},
    util::resolve_file_sequence,
};
use crate::shaders::shared::ControlPoint;

/// A resizable, append-only GPU buffer. Like `Vec<T>` but stored on GPU device memory.
///
/// If the buffer is host-visible, elements can be added directly to the buffer.
/// Otherwise, elements are first added to a staging area and must be copied to the buffer on
/// the device timeline by calling [`AppendBuffer::commit`].
pub struct AppendBuffer<T> {
    buffer: Buffer<[T]>,
    len: usize,
    staging: Vec<T>,
}

impl<T: Copy> AppendBuffer<T> {
    /// Creates an append buffer with the given usage flags and default capacity.
    pub fn new(device: &Device, usage: BufferUsage, memory_location: MemoryLocation) -> AppendBuffer<T> {
        Self::with_capacity(device, usage, memory_location, 16)
    }

    /// Creates an append buffer with the given usage flags and initial capacity.
    pub fn with_capacity(device: &Device, mut usage: BufferUsage, memory_location: MemoryLocation, capacity: usize) -> Self {
        // Add TRANSFER_DST capacity if the buffer is not host-visible
        if memory_location != MemoryLocation::CpuToGpu {
            usage |= BufferUsage::TRANSFER_DST;
        }
        let buffer = device.create_array_buffer(usage, memory_location, capacity);
        Self {
            buffer,
            len: 0,
            staging: vec![],
        }
    }

    /// Returns the pointer to the buffer data in host memory.
    ///
    /// # Panics
    ///
    /// Panics if the buffer is not host-visible.
    pub fn as_mut_ptr(&self) -> *mut T {
        self.buffer.as_mut_ptr()
    }

    pub unsafe fn set_len(&mut self, len: usize) {
        assert!(self.host_visible());
        assert!(len <= self.buffer.len());
        self.len = len;
    }

    pub fn set_name(&self, name: &str) {
        self.buffer.set_name(name);
    }

    pub fn device_address(&self) -> DeviceAddress<[T]> {
        self.buffer.device_address()
    }

    /// Returns the capacity in elements of the buffer before it needs to be resized.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Returns the number of elements in the buffer (including elements in the staging area).
    pub fn len(&self) -> usize {
        // number of elements in the main buffer + pending elements
        self.len + self.staging.len()
    }

    fn needs_to_grow(&self, additional: usize) -> bool {
        self.len + additional > self.capacity()
    }

    /// Whether the buffer is host-visible (and mapped in memory).
    fn host_visible(&self) -> bool {
        self.buffer.memory_location() == MemoryLocation::CpuToGpu
    }

    fn reserve_gpu(&mut self, cmd: &mut CommandStream, additional: usize) {
        if self.needs_to_grow(additional) {
            let memory_location = self.buffer.memory_location();
            let new_capacity = (self.len + additional).next_power_of_two(); // in num of elements
            trace!(
                "AppendBuffer: reallocating {} -> {} bytes",
                self.capacity() * size_of::<T>(),
                new_capacity * size_of::<T>()
            );
            let new_buffer = self
                .buffer
                .device()
                .create_array_buffer(self.buffer.usage(), memory_location, new_capacity);
            cmd.copy_buffer(&self.buffer.untyped, 0, &new_buffer.untyped, 0, (self.len * size_of::<T>()) as u64);
            self.buffer = new_buffer;
        }
    }

    /// Reserve space for `additional` elements in the buffer, if the buffer is host-visible.
    fn reserve_cpu(&mut self, additional: usize) {
        assert!(self.host_visible());
        if self.needs_to_grow(additional) {
            let new_capacity = (self.len + additional).next_power_of_two(); // in num of elements
            trace!(
                "AppendBuffer: reallocating {} -> {} bytes",
                self.capacity() * size_of::<T>(),
                new_capacity * size_of::<T>()
            );
            let new_buffer = self
                .buffer
                .device()
                .create_array_buffer(self.buffer.usage(), self.buffer.memory_location(), new_capacity);
            // Copy the old data to the new buffer
            unsafe {
                ptr::copy_nonoverlapping(self.buffer.as_mut_ptr(), new_buffer.as_mut_ptr(), self.len);
            }
            self.buffer = new_buffer;
        }
    }

    /// Truncates the buffer to the given length.
    ///
    /// # Panics
    ///
    /// * Panics if `len` is greater than the current length of the buffer.
    /// * Panics if there are pending elements in the staging area.
    pub fn truncate(&mut self, len: usize) {
        assert!(len <= self.len);
        assert!(self.staging.is_empty());
        self.len = len;
    }

    /// Adds an element to the buffer.
    pub fn push(&mut self, elem: T) {
        if self.buffer.memory_location() == MemoryLocation::CpuToGpu {
            // the buffer is mapped in memory, we can copy the element right now
            self.reserve_cpu(1);
            unsafe {
                ptr::write(self.buffer.as_mut_ptr().add(self.len), elem);
            }
        } else {
            // add to pending list
            self.staging.push(elem);
        }
        self.len += 1;
    }

    /// Returns whether there are pending elements to be copied to the main buffer.
    pub fn has_pending(&self) -> bool {
        !self.staging.is_empty()
    }

    /// Copies pending elements to the main buffer.
    pub fn commit(&mut self, cmd: &mut CommandStream) {
        let n = self.staging.len(); // number of elements to append
        if n == 0 {
            return;
        }

        if self.host_visible() {
            // nothing to do, the elements have already been copied
            return;
        }

        self.reserve_gpu(cmd, n);
        // allocate staging buffer & copy pending elements
        let staging_buf = self
            .buffer
            .device()
            .create_array_buffer::<T>(BufferUsage::TRANSFER_SRC, MemoryLocation::CpuToGpu, n);
        unsafe {
            ptr::copy_nonoverlapping(
                self.staging.as_ptr(),
                staging_buf.as_mut_ptr(),
                n,
            );
        }
        // copy from staging to main buffer
        let elem_size = size_of::<T>() as u64;
        cmd.copy_buffer(
            &staging_buf.untyped,
            0,
            &self.buffer.untyped,
            self.len as u64 * elem_size,
            n as u64 * elem_size,
        );
        self.staging.clear();
    }

    /// Returns the underlying GPU buffer.
    pub fn buffer(&self) -> Buffer<[T]> {
        self.buffer.clone()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

fn load_brush_texture(cmd: &mut CommandStream, path: impl AsRef<Path>, usage: ImageUsage, mipmaps: bool) -> Image {
    let path = path.as_ref();
    let device = cmd.device().clone();
    let gray_image = image::open(path).expect("could not open image file").to_luma8();

    let width = gray_image.width();
    let height = gray_image.height();

    let mip_levels = graal::mip_level_count(width, height);

    // create the texture
    let image = device.create_image(&ImageCreateInfo {
        memory_location: MemoryLocation::GpuOnly,
        type_: ImageType::Image2D,
        usage: usage | ImageUsage::TRANSFER_DST,
        format: Format::R8_SRGB,
        width,
        height,
        depth: 1,
        mip_levels,
        array_layers: 1,
        samples: 1,
    });

    let byte_size = width as u64 * height as u64;

    // create a staging buffer
    let staging_buffer = device.create_buffer(BufferUsage::TRANSFER_SRC, MemoryLocation::CpuToGpu, byte_size);

    // read image data
    unsafe {
        ptr::copy_nonoverlapping(
            gray_image.as_raw().as_ptr(),
            staging_buffer.as_mut_ptr(),
            byte_size as usize,
        );
    }
    cmd.copy_buffer_to_image(
        ImageCopyBuffer {
            buffer: &staging_buffer,
            layout: ImageDataLayout {
                offset: 0,
                row_length: Some(width),
                image_height: Some(height),
            },
        },
        ImageCopyView {
            image: &image,
            mip_level: 0,
            origin: vk::Offset3D { x: 0, y: 0, z: 0 },
            aspect: vk::ImageAspectFlags::COLOR,
        },
        vk::Extent3D { width, height, depth: 1 },
    );

    image
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Geometry loaded from a `frame####.geo` file.
struct GeoFileData {
    /// The #### in `frame####.geo`.
    index: usize,
    geometry: Geo,
}


fn lagrange_interpolation(p1: glam::Vec2, p2: glam::Vec2, p3: glam::Vec2, p4: glam::Vec2) -> glam::Vec4 {
    let ys = vec4(p1.y, p2.y, p3.y, p4.y);
    let xs = vec4(p1.x, p2.x, p3.x, p4.x);
    let v = mat4(vec4(1.0, 1.0, 1.0, 1.0), xs, xs * xs, xs * xs * xs);
    let v_inv = v.inverse();
    let coeffs = v_inv * ys;
    coeffs
}

/*
/// Represents a range of control points in the position buffer.
#[derive(Copy, Clone)]
#[repr(C)]
struct CurveDesc {
    /// width profile polynomial coefficients
    width_profile: glam::Vec4,
    /// opacity profile polynomial coefficients
    opacity_profile: glam::Vec4,
    start: u32,
    /// Number of control points in the range.
    ///
    /// Should be 3N+1 for cubic Bézier curves.
    count: u32,
    /// parameter range
    param_range: glam::Vec2,
}*/

/// Represents a range of curves in the curve buffer.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct CurveRange {
    start: u32,
    count: u32,
}

/// Information about a single animation frame.
#[derive(Debug)]
struct AnimationFrame {
    /// Time of the frame in seconds.
    time: f32,
    /// Range of curves in the curve buffer.
    curve_range: CurveRange,
    /// Curve segments
    curve_segments: Vec<CubicBezierSegment>,
}

struct AnimationData {
    //point_count: usize,
    //curve_count: usize,
    frames: Vec<AnimationFrame>,
    position_buffer: AppendBuffer<ControlPoint>,
    curve_buffer: AppendBuffer<CurveDesc>,
}

/// Converts Bézier curve data from `.geo` files to a format that can be uploaded to the GPU.
///
/// Curves are represented as follows:
/// * position buffer: contains the control points of curves, all flattened into a single linear buffer.
/// * curve buffer: consists of (start, size) pairs, defining the start and number of CPs of each curve in the position buffer.
/// * animation buffer: consists of (start, size) defining the start and number of curves in the curve buffer for each animation frame.
fn convert_animation_data(device: &Device, geo_files: &[GeoFileData]) -> AnimationData {
    let mut point_count = 0;
    let mut curve_count = 0;

    // Count the number of curves and control points
    for f in geo_files.iter() {
        for prim in f.geometry.primitives.iter() {
            match prim {
                houdinio::Primitive::BezierRun(run) => match run.vertices {
                    houdinio::PrimVar::Uniform(ref u) => {
                        point_count += u.len() * run.count;
                        curve_count += (u.len() / 3) * run.count;
                    }
                    houdinio::PrimVar::Varying(ref v) => {
                        point_count += v.iter().map(|v| v.len()).sum::<usize>();
                        curve_count += v.iter().map(|v| v.len() / 3).sum::<usize>();
                    }
                },
            }
        }
    }

    // Curve buffer: contains (start, end) pairs of curves in the point buffer

    let mut position_buffer = AppendBuffer::with_capacity(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu, point_count);
    position_buffer.set_name("control point buffer");
    let mut curve_buffer = AppendBuffer::with_capacity(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu, curve_count);
    curve_buffer.set_name("curve buffer");

    let mut frames = vec![];

    // dummy width and opacity profiles
    let width_profile = lagrange_interpolation(vec2(0.0, 0.0), vec2(0.2, 0.8), vec2(0.5, 0.8), vec2(1.0, 0.0));
    let opacity_profile = lagrange_interpolation(vec2(0.0, 0.7), vec2(0.3, 1.0), vec2(0.6, 1.0), vec2(1.0, 0.0));

    // write curves
    unsafe {
        let point_data: *mut ControlPoint = position_buffer.as_mut_ptr();
        let mut point_ptr = 0;
        let curve_data: *mut CurveDesc = curve_buffer.as_mut_ptr();
        let mut curve_ptr = 0;

        for f in geo_files.iter() {
            let offset = curve_ptr;

            let mut curve_segments = vec![];
            for prim in f.geometry.primitives.iter() {
                match prim {
                    houdinio::Primitive::BezierRun(run) => {
                        for curve in run.iter() {
                            let start = point_ptr;
                            for &vertex_index in curve.vertices.iter() {
                                let pos = f.geometry.vertex_position(vertex_index);
                                let color = f.geometry.vertex_color(vertex_index).unwrap_or([0.1, 0.8, 0.1]);
                                *point_data.offset(point_ptr) = ControlPoint { pos, color };
                                point_ptr += 1;
                            }
                            // FIXME: this is wrong
                            for segment in curve.vertices.windows(4) {
                                curve_segments.push(CubicBezierSegment {
                                    p0: f.geometry.vertex_position(segment[0]).into(),
                                    p1: f.geometry.vertex_position(segment[1]).into(),
                                    p2: f.geometry.vertex_position(segment[2]).into(),
                                    p3: f.geometry.vertex_position(segment[3]).into(),
                                });
                            }

                            let num_segments = curve.vertices.len() as u32 / 3;
                            let num_segments_f = num_segments as f32;
                            for i in 0..num_segments {
                                *curve_data.offset(curve_ptr) = CurveDesc {
                                    start: start as u32 + 3 * i,
                                    count: 4,
                                    /*curve.vertices.len() as u32*/
                                    width_profile,
                                    opacity_profile,
                                    param_range: vec2(i as f32 / num_segments_f, (i + 1) as f32 / num_segments_f),
                                };
                                curve_ptr += 1;
                            }
                        }
                    }
                }
            }

            frames.push(AnimationFrame {
                time: 0.0, // TODO
                curve_range: CurveRange {
                    start: offset as u32,
                    count: curve_ptr as u32 - offset as u32,
                },
                curve_segments,
            });
        }
        position_buffer.set_len(point_count);
        curve_buffer.set_len(curve_count);
    }

    AnimationData {
        //point_count,
        //curve_count,
        frames,
        position_buffer,
        curve_buffer,
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

const CURVES_OIT_MAX_FRAGMENTS_PER_PIXEL: usize = 8;

/*#[derive(Attachments)]
struct RenderAttachments<'a> {
    #[attachment(color, format = R16G16B16A16_SFLOAT)]
    color: &'a ImageView,
    #[attachment(depth, format = D32_SFLOAT)]
    depth: &'a ImageView,
}

#[derive(Attachments)]
struct BinRastAttachments<'a> {
    #[attachment(depth, format = D32_SFLOAT)]
    depth: &'a ImageView,
}

#[derive(Attachments)]
struct CurvesOITAttachments<'a> {
    #[attachment(color, format = R16G16B16A16_SFLOAT)]
    color: &'a ImageView,
    #[attachment(depth, format = D32_SFLOAT)]
    depth: &'a ImageView,
}*/

//#[derive(Arguments)]
struct CurvesOITArguments<'a> {
    //#[argument(binding = 0, storage)]
    position_buffer: BufferRange<[ControlPoint]>,
    //#[argument(binding = 1, storage)]
    curve_buffer: BufferRange<[CurveDesc]>,
    //#[argument(binding = 2, storage)]
    fragment_buffer: BufferRange<[CurvesOITFragmentData]>,
    //#[argument(binding = 3, storage)]
    fragment_count_buffer: BufferRange<[u32]>,
    //#[argument(binding = 4, sampled_image)]
    brush_texture: &'a ImageView,
    //#[argument(binding = 5, sampler)]
    brush_sampler: &'a Sampler,
}

//#[derive(Arguments)]
struct CurvesOITResolveArguments {
    //#[argument(binding = 0, storage)]
    fragment_buffer: BufferRange<[CurvesOITFragmentData]>,
    //#[argument(binding = 1, storage)]
    fragment_count_buffer: BufferRange<[u32]>,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct CurvesOITFragmentData {
    color: glam::Vec4,
    depth: f32,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct CurvesOITPushConstants {
    view_proj: glam::Mat4,
    viewport_size: glam::UVec2,
    stroke_width: f32,
    base_curve_index: u32,
    curve_count: u32,
    frame: i32,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct CurvesOITResolvePushConstants {
    //view_proj: glam::Mat4,
    viewport_size: glam::UVec2,
    //stroke_width: f32,
    //base_curve_index: u32,
}

////////////////////////////////////////////////////////////////////////////////////////////////////
fn create_depth_buffer(device: &Device, width: u32, height: u32) -> Image {
    let image = device.create_image(&ImageCreateInfo {
        memory_location: MemoryLocation::GpuOnly,
        type_: ImageType::Image2D,
        usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
        format: Format::D32_SFLOAT,
        width,
        height,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        samples: 1,
    });
    image.set_name("depth buffer");
    image
}

fn icon_button(ui: &mut Ui, icon: &str, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::new(20.0, 20.0), egui::Sense::click());
    if response.hovered() {
        let color = ui.style().visuals.selection.bg_fill;
        ui.painter().rect_filled(rect, 0.0, color);
    }
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, FontId::proportional(16.0), color);
    response
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Default)]
struct Pipelines {
    curve_binning_pipeline: Option<GraphicsPipeline>,
    draw_curves_pipeline: Option<ComputePipeline>,
    draw_curves_oit_pipeline: Option<GraphicsPipeline>,
    draw_curves_oit_v2_pipeline: Option<GraphicsPipeline>,
    draw_curves_oit_resolve_pipeline: Option<GraphicsPipeline>,
    temporal_average_pipeline: Option<ComputePipeline>,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum RenderMode {
    BinRasterization = 0,
    CurvesOIT = 1,
    CurvesOITv2 = 2,
}

struct BrushTexture {
    name: String,
    image: Image,
    sat_image: Image,
    id: u32,
}

#[derive(Copy, Clone)]
struct PenSample {
    position: DVec2,
    pressure: f64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Tweak {
    name: String,
    value: String,
    enabled: bool,
    autofocus: bool,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct SavedSettings {
    tweaks: Vec<Tweak>,
    last_geom_file: Option<PathBuf>,
}

impl SavedSettings {
    fn save(&self) {
        fs::write("settings.json", serde_json::to_string(self).unwrap()).expect("failed to save settings");
    }

    fn load() -> Result<Self, anyhow::Error> {
        let str = fs::read_to_string("settings.json")?;
        let data = serde_json::from_str(&str)?;
        Ok(data)
    }
}

pub struct App {
    // Keep a copy of the device so we don't have to pass it around everywhere.
    device: Device,
    depth_buffer: Image,
    depth_buffer_view: ImageView,
    color_target_format: Format,
    camera_control: CameraControl,
    overlay: OverlayRenderer,
    pipelines: Pipelines,

    animation: Option<AnimationData>,

    frame: i32,
    mode: RenderMode,
    temporal_average: bool,
    temporal_average_alpha: f32,
    frame_image: Image,
    temporal_avg_image: Image,
    debug_tile_line_overflow: bool,

    // Bin rasterization
    bin_rast_stroke_width: f32,
    bin_rast_current_frame: usize,

    // Curves OIT
    oit_stroke_width: f32,
    oit_max_fragments_per_pixel: u32,
    oit_fragment_buffer: Buffer<[CurvesOITFragmentData]>,
    oit_fragment_count_buffer: Buffer<[u32]>,

    // Brushes
    reload_brush_textures: bool,
    brush_textures: Vec<BrushTexture>,
    selected_brush: usize,

    // Overlay
    overlay_line_width: f32,
    overlay_filter_width: f32,

    // UI
    /// Curve drawing mode active
    is_drawing: bool,
    last_pos: DVec2,
    pen_points: Vec<PenSample>,
    drawn_curves: AppendBuffer<CurveDesc>,
    drawn_control_points: AppendBuffer<ControlPoint>,
    settings: SavedSettings,
    tweaks_changed: bool,
    engine: Engine,
    draw_origin: glam::Vec2,
    fit_tolerance: f64,
    curve_embedding_factor: f64,

}

impl App {
    /*fn compute_sats(&mut self, cmd: &mut CommandStream) -> Result<(), Error> {

            // setup shaders
            let sat_shader = PathBuf::from("crates/fluff/shaders/sat.glsl");
            // FIXME: too verbose
            let sat_32x32 = self.engine.create_compute_pipeline(
                "sat_32x32",
                ComputePipelineDesc {
                    shader: sat_shader.clone(),
                    defines: [("SAT_LOG2_SIZE".to_string(), "6".to_string())].into(),
                },
            )?;
            let sat_64x64 = self.engine.create_compute_pipeline(
                "sat_64x64",
                ComputePipelineDesc {
                    shader: sat_shader.clone(),
                    defines: [("SAT_LOG2_SIZE".to_string(), "7".to_string())].into(),
                },
            )?;
            let sat_128x128 = self.engine.create_compute_pipeline(
                "sat_128x128",
                ComputePipelineDesc {
                    shader: sat_shader.clone(),
                    defines: [("SAT_LOG2_SIZE".to_string(), "8".to_string())].into(),
                },
            )?;

            let mut rg = self.engine.create_graph();

            for (i, brush) in self.brush_textures.iter().enumerate() {
                // TODO: don't require names
                let brush_image = rg.import_image(&brush.sat_image);
                let sat_image = rg.import_image(&brush.sat_image);

                let sat_pipeline = match brush.image.width() {
                    32 => &sat_32x32,
                    64 => &sat_64x64,
                    128 => &sat_128x128,
                    _ => panic!("unsupported brush texture size"),
                };
                assert!(brush.image.width() == brush.image.height(), "brush texture must be square");

                let mut pass = rg.record_compute_pass(&sat_pipeline);
                pass.write_image(sat_image);
                pass.set_render_func(move |encoder| {
                    // Horizontal pass
                    encoder.push_constants(&SummedAreaTableParams {
                        pass: 0,
                        input_image: brush_image.device_handle(),
                        output_image: sat_image.device_handle(),
                    });
                    encoder.dispatch(brush.image.height(), 1, 1);
                    // Vertical pass
                    encoder.barrier(MemoryScope::SHADER_STORAGE_READ | MemoryScope::SHADER_STORAGE_WRITE);
                    encoder.push_constants(&SummedAreaTableParams {
                        pass: 1,
                        input_image: sat_image.device_handle(),
                        output_image: sat_image.device_handle(),
                    });
                    // encoder.write(MemoryScope::SHADER_STORAGE_WRITE);   // signals that there are shader storage writes that are waiting to be made available
                    encoder.dispatch(brush.image.width(), 1, 1);
                });
                pass.finish();

                // FIXME: it's annoying to start a new pass because of the write/read hazard on
                // `sat_image`.
                let mut pass = rg.record_compute_pass(&sat_pipeline);
                pass.read_image(sat_image);
                pass.write_image(sat_image);
                pass.set_render_func(move |encoder| {});
                pass.finish();
            }

            //cmd.use_image(self.brush_textures[0].sat_image.clone(), ImageAccess);
        }
    */

    fn setup(&mut self, cmd: &mut CommandStream, color_target: Image, width: u32, height: u32) -> Result<(), Error> {
        let engine = &mut self.engine;

        let Some(ref animation) = self.animation else { return Ok(()) };
        let anim_frame = &animation.frames[self.bin_rast_current_frame];
        let curve_count = anim_frame.curve_range.count;
        let base_curve_index = anim_frame.curve_range.start;
        let view_proj = self.camera_control.camera().view_projection();
        let frame = self.bin_rast_current_frame as u32;
        let stroke_width = self.bin_rast_stroke_width;
        let viewport_size = [width, height];
        let temporal_average_falloff = self.temporal_average_alpha;
        let debug_tile_line_overflow = self.debug_tile_line_overflow;

        let tile_count_x = width.div_ceil(BINNING_TILE_SIZE);
        let tile_count_y = height.div_ceil(BINNING_TILE_SIZE);
        //engine.define_global("TILE_SIZE", CURVE_BINNING_TILE_SIZE.to_string());
        let camera = self.camera_control.camera();
        let scene_params = shaders::shared::SceneParams {
            view: camera.view,
            proj: camera.projection,
            view_proj: camera.view_projection(),
            eye: self.camera_control.eye().as_vec3(),
            // TODO frustum parameters
            near: 0.0,
            far: 0.0,
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        };

        //let mut rg = engine.create_graph();

        /*// set common uniform variables
        rg.set_global_constant("viewProjectionMatrix", self.camera_control.camera().view_projection());
        rg.set_global_constant("viewportSize", [width, height]);
        rg.set_global_constant("strokeWidth", self.bin_rast_stroke_width);
        rg.set_global_constant("baseCurveIndex", anim_frame.curve_range.start);
        rg.set_global_constant("curveCount", anim_frame.curve_range.count);
        rg.set_global_constant("frame", self.frame);*/

        //let control_point_buffer = rg.import_buffer(&animation.position_buffer.untyped);
        //let curve_buffer = rg.import_buffer(&animation.curve_buffer.untyped);
        //let temporal_average = rg.import_image(&self.temporal_avg_image);
        //let color_target = rg.import_image(&color_target);
        //let depth_target = rg.import_image(&self.depth_buffer);

        // FIXME: can we maybe make `STORAGE_BUFFER` a bit less screaming?
        let scene_params_buf = self.device.upload(BufferUsage::STORAGE_BUFFER, &scene_params);
        cmd.reference_resource(&scene_params_buf);

        // FIXME: importing image sets will mean finding a contiguous range of free image handles
        // or we can pass the image view handles in an array, at the cost of another indirection
        //let brush_textures = rg.import_image_set(self.brush_textures.iter().map(|b| b.image.clone()));

        ////////////////////////////////////////////////////////////
        // Curve binning
        let tile_line_count_buffer = self.device.create_array_buffer::<u32>(BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST, MemoryLocation::GpuOnly, tile_count_x as usize * tile_count_y as usize);
        let tile_buffer = self.device.create_array_buffer::<TileData>(BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST, MemoryLocation::GpuOnly, tile_count_x as usize * tile_count_y as usize);

        // TODO: consider allocating top-level image views alongside the image itself
        let color_target_view = color_target.create_top_level_view();
        let depth_target_view = self.depth_buffer.create_top_level_view();
        let temporal_avg_view = self.temporal_avg_image.create_top_level_view();

        // pipelines
        let curve_binning_pipeline = engine.create_mesh_render_pipeline(
            "curve_binning",
            // TODO: in time, all of this will be moved to hot-reloadable config files
            MeshRenderPipelineDesc {
                shader: PathBuf::from("crates/fluff/shaders/bin_curves.glsl"),
                defines: Default::default(),
                color_targets: vec![ColorTargetState {
                    format: Format::R16G16B16A16_SFLOAT,
                    ..Default::default()
                }],
                rasterization_state: Default::default(),
                depth_stencil_state: Some(DepthStencilState {
                    format: Format::D32_SFLOAT,
                    depth_write_enable: true,
                    depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
                    stencil_state: StencilState::default(),
                }),
                multisample_state: Default::default(),
            },
        )?;

        let draw_curves_pipeline = engine.create_compute_pipeline(
            "draw_curves",
            ComputePipelineDesc {
                shader: PathBuf::from("crates/fluff/shaders/draw_curves.comp"),
                defines: Default::default(),
            },
        )?;

        let temporal_average_pipeline = engine.create_compute_pipeline(
            "temporal_average",
            ComputePipelineDesc {
                shader: PathBuf::from("crates/fluff/shaders/temporal_average.comp"),
                defines: Default::default(),
            },
        )?;

        //////////////////////////////////////////
        cmd.fill_buffer(&tile_line_count_buffer.untyped.byte_range(..), 0);
        cmd.fill_buffer(&tile_buffer.untyped.byte_range(..), 0);

        cmd.barrier(Barrier::new().shader_storage_write());

        let mut encoder = cmd.begin_rendering(RenderPassInfo {
            color_attachments: &[ColorAttachment {
                image_view: &color_target_view,
                clear_value: Some([0.0, 0.0, 0.0, 1.0]),
            }],
            depth_stencil_attachment: Some(DepthStencilAttachment {
                image_view: &depth_target_view,
                depth_clear_value: Some(1.0),
                stencil_clear_value: None,
            }),
        });


        let vp_width = width as f32 / BINNING_TILE_SIZE as f32;
        let vp_height = height as f32 / BINNING_TILE_SIZE as f32;
        encoder.bind_graphics_pipeline(&curve_binning_pipeline);
        encoder.set_viewport(0.0, 0.0, vp_width, vp_height, 0.0, 1.0);
        encoder.set_scissor(0, 0, tile_count_x, tile_count_y);
        encoder.set_scissor(0, 0, tile_count_x, tile_count_y);
        encoder.push_constants(&shaders::shared::BinCurvesParams {
            scene_params: scene_params_buf.device_address(),
            viewport_size: uvec2(width, height),
            stroke_width,
            base_curve_index,
            curve_count,
            tile_count_x,
            tile_count_y,
            frame,
            control_points: animation.position_buffer.device_address(),
            curves: animation.curve_buffer.device_address(),
            tile_line_count: tile_line_count_buffer.device_address(),
            tile_data: tile_buffer.device_address(),
        });
        encoder.draw_mesh_tasks(curve_count.div_ceil(64), 1, 1);
        encoder.finish();

        cmd.barrier(Barrier::new().shader_storage_read().shader_write_image(&color_target));

        let mut encoder = cmd.begin_compute();
        encoder.bind_compute_pipeline(&draw_curves_pipeline);
        encoder.push_constants(&DrawCurvesPushConstants {
            view_proj,
            base_curve: base_curve_index,
            stroke_width,
            tile_count_x,
            tile_count_y,
            frame,
            tile_data: tile_buffer.device_address(),
            tile_line_count: tile_line_count_buffer.device_address(),
            brush_textures: Texture2DHandleRange { index: 0, count: 0 }, //brush_textures.device_handle(),
            output_image: color_target_view.device_image_handle(),
            debug_overflow: debug_tile_line_overflow as u32,
        });
        encoder.dispatch(tile_count_x, tile_count_y, 1);
        encoder.finish();

        if self.temporal_average {
            cmd.barrier(Barrier::new().shader_read_image(&color_target).shader_write_image(&self.temporal_avg_image));
            let mut encoder = cmd.begin_compute();
            encoder.bind_compute_pipeline(&temporal_average_pipeline);
            encoder.push_constants(&TemporalAverageParams {
                viewport_size: uvec2(width, height),
                frame,
                falloff: temporal_average_falloff,
                new_frame: color_target_view.device_image_handle(),
                avg_frame: temporal_avg_view.device_image_handle(),
            });
            encoder.dispatch(width.div_ceil(8), height.div_ceil(8), 1);
            encoder.finish();
            cmd.blit_full_image_top_mip_level(&self.temporal_avg_image, &color_target);
        }
        /*{
            // clear buffers
            //rg.record_fill_buffer(tile_line_count_buffer, 0);
            //rg.record_fill_buffer(tile_buffer, 0);


            let mut pass = rg.record_mesh_render_pass(&curve_binning_pipeline);

            pass.set_color_attachments([ColorAttachmentDesc {
                image: color_target,
                clear_value: Some([0.0, 0.0, 0.0, 1.0]),
            }]);
            pass.set_depth_stencil_attachment(DepthStencilAttachmentDesc {
                image: depth_target,
                depth_clear_value: Some(1.0),
                stencil_clear_value: None,
            });
            pass.read_buffer(control_point_buffer);
            pass.read_buffer(curve_buffer);
            pass.write_buffer(tile_line_count_buffer);
            pass.write_buffer(tile_buffer);

            pass.set_render_func(move |encoder| {
                let vp_width = width as f32 / BINNING_TILE_SIZE as f32;
                let vp_height = height as f32 / BINNING_TILE_SIZE as f32;
                encoder.set_viewport(0.0, 0.0, vp_width, vp_height, 0.0, 1.0);

                //eprintln!("control_point_buffer device address = 0x{:016x}", control_point_buffer.device_address());
                //eprintln!("curve_buffer device address = 0x{:016x}", curve_buffer.device_address());
                //eprintln!("base_curve_index = {}", base_curve_index);
                //eprintln!("curve_count = {}", curve_count);

                encoder.set_scissor(0, 0, tile_count_x, tile_count_y);
                encoder.push_constants(&shaders::shared::BinCurvesParams {
                    scene_params: scene_params_buf.device_address(),
                    viewport_size: uvec2(width, height),
                    stroke_width,
                    base_curve_index,
                    curve_count,
                    tile_count_x,
                    tile_count_y,
                    frame,
                    control_points: control_point_buffer.device_address(),
                    curves: curve_buffer.device_address(),
                    tile_line_count: tile_line_count_buffer.device_address(),
                    tile_data: tile_buffer.device_address(),
                });
                encoder.draw_mesh_tasks(curve_count.div_ceil(64), 1, 1);
            });

            pass.finish();
        }*/


        /*////////////////////////////////////////////////////////////
        // Temporal accumulation
        if self.temporal_average {
            let mut pass = rg.record_compute_pass(&temporal_average_pipeline);
            pass.read_image(color_target);
            pass.read_image(temporal_average);
            pass.write_image(temporal_average);
            pass.set_render_func(move |encoder| {});
            pass.finish();
            rg.record_blit(temporal_average, color_target);
        }*/

        Ok(())
    }

    fn reload_shaders(&mut self) {
        fn check<T>(name: &str, p: Result<T, graal::Error>) -> Option<T> {
            match p {
                Ok(p) => Some(p),
                Err(err) => {
                    eprintln!("Error creating `{name}`: {}", err);
                    None
                }
            }
        }
    }

    fn reload_textures(&mut self, cmd: &mut CommandStream) {
        self.brush_textures.clear();
        let files_in_directory = fs::read_dir("data/texture/").unwrap();
        for file in files_in_directory {
            let file = file.unwrap();
            if !file.file_type().unwrap().is_file() {
                continue;
            }
            let path = file.path();
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            eprintln!("Loading brush texture: `{}`", name);
            let image = load_brush_texture(cmd, path, ImageUsage::SAMPLED, false);
            self.brush_textures.push(BrushTexture {
                name,
                image: image.clone(),
                sat_image: image.clone(),
                id: self.brush_textures.len() as u32,
            });
        }
    }

    fn load_geo_file(&mut self, path: &Path) {
        let file_sequence = match resolve_file_sequence(path) {
            Ok(seq) => seq,
            Err(err) => {
                eprintln!("Error: {}", err);
                return;
            }
        };

        let mut geo_files = vec![];
        for (frame_index, file_path) in file_sequence {
            eprint!("Loading: `{}`...", file_path.display());
            match Geo::load_json(file_path) {
                Ok(geometry) => {
                    geo_files.push(GeoFileData {
                        index: frame_index,
                        geometry,
                    });
                    eprintln!("OK")
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                }
            }
        }
        self.settings.last_geom_file = Some(path.to_path_buf());
        self.settings.save();
        self.animation = Some(convert_animation_data(&self.device, &geo_files));
    }

    fn load_last_geo_file(&mut self) {}

    /*fn render_curves_oit(&mut self, cmd: &mut CommandStream) {
        let color_target = &self.frame_image;
        let color_target_view = &self.frame_image.create_top_level_view();

        let Some(animation) = self.animation.as_ref() else {
            return;
        };
        let Some(pipeline) = self.pipelines.draw_curves_oit_pipeline.as_ref() else {
            return;
        };
        let Some(pipeline_v2) = self.pipelines.draw_curves_oit_v2_pipeline.as_ref() else {
            return;
        };
        let Some(resolve_pipeline) = self.pipelines.draw_curves_oit_resolve_pipeline.as_ref() else {
            return;
        };

        /*
            let fragment_buffer = encoder.create_buffer(...);
            let fragment_count_buffer = encoder.create_buffer(...);

            encoder.fill_buffer(fragment_buffer.slice(..), 0);
            encoder.fill_buffer(fragment_count_buffer.slice(..), 0);

            let render_pass = encoder.create_rendering(&CurvesOITAttachments {
                color: &color_target_view,
                depth: &self.depth_buffer_view,
            });
        */

        self.bin_rast_current_frame = self.bin_rast_current_frame.min(animation.frames.len() - 1);

        let current_frame = &animation.frames[self.bin_rast_current_frame];

        // Allocate a big enough fragment data buffer
        let width = color_target_view.width();
        let height = color_target_view.height();

        let fragment_buffer_size = width as usize * height as usize * CURVES_OIT_MAX_FRAGMENTS_PER_PIXEL;
        if self.oit_fragment_buffer.len() < fragment_buffer_size {
            self.oit_fragment_buffer = self.device.create_array_buffer(
                BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
                MemoryLocation::GpuOnly,
                fragment_buffer_size,
            );
            self.oit_fragment_count_buffer = self.device.create_array_buffer(
                BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
                MemoryLocation::GpuOnly,
                width as usize * height as usize,
            );
        }

        unsafe {
            cmd.debug_group("clear OIT fragment buffer", |cmd| {
                let mut encoder = cmd.begin_blit();
                //encoder.fill_buffer(&self.oit_fragment_buffer.slice(..).untyped, 0);
                encoder.fill_buffer(&self.oit_fragment_count_buffer.slice(..).untyped, 0);
            });

            // Render the curves
            cmd.debug_group("OIT curves", |cmd| {
                let mut encoder = cmd.begin_rendering(RenderPassDescriptor {
                    color_attachments: &[ColorAttachment {
                        image_view: color_target_view.clone(),
                        load_op: AttachmentLoadOp::LOAD,
                        store_op: AttachmentStoreOp::STORE,
                        clear_value: [0.0, 0.0, 0.0, 0.0],
                    }],
                    depth_stencil_attachment: Some(DepthStencilAttachment {
                        image_view: self.depth_buffer_view.clone(),
                        depth_load_op: AttachmentLoadOp::LOAD,
                        depth_store_op: AttachmentLoadOp::STORE,
                        stencil_load_op: Default::default(),
                        stencil_store_op: Default::default(),
                        depth_clear_value: 0.0,
                        stencil_clear_value: 0,
                    }),
                });

                let brush_texture = &self.brush_textures[self.selected_brush].image.create_top_level_view();

                if self.mode == RenderMode::CurvesOITv2 {
                    encoder.bind_graphics_pipeline(pipeline_v2);
                } else {
                    encoder.bind_graphics_pipeline(pipeline);
                }
                encoder.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
                encoder.set_scissor(0, 0, width, height);
                encoder.push_descriptors(
                    0,
                    &[
                        (0, animation.position_buffer.slice(..).storage_descriptor()),
                        (1, animation.curve_buffer.slice(..).storage_descriptor()),
                        (2, self.oit_fragment_buffer.slice(..).storage_descriptor()),
                        (3, self.oit_fragment_count_buffer.slice(..).storage_descriptor()),
                        (4, brush_texture.texture_descriptor(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)),
                        (
                            5,
                            self.device
                                .create_sampler(&SamplerCreateInfo {
                                    address_mode_u: vk::SamplerAddressMode::REPEAT,
                                    address_mode_v: vk::SamplerAddressMode::REPEAT,
                                    address_mode_w: vk::SamplerAddressMode::REPEAT,
                                    ..Default::default()
                                })
                                .descriptor(),
                        ),
                    ],
                );

                encoder.push_constants(&CurvesOITPushConstants {
                    view_proj: self.camera_control.camera().view_projection(),
                    viewport_size: uvec2(width, height),
                    stroke_width: self.oit_stroke_width,
                    base_curve_index: current_frame.curve_range.start,
                    curve_count: current_frame.curve_range.count,
                    frame: self.frame,
                });
                if self.mode == RenderMode::CurvesOITv2 {
                    encoder.draw_mesh_tasks(current_frame.curve_range.count.div_ceil(64), 1, 1);
                } else {
                    encoder.draw_mesh_tasks(current_frame.curve_range.count, 1, 1);
                }
                encoder.finish();
            });

            // Resolve pass
            cmd.debug_group("OIT resolve", |cmd| {
                let mut encoder = cmd.begin_rendering(&CurvesOITAttachments {
                    color: &color_target_view,
                    depth: &self.depth_buffer_view,
                });

                encoder.bind_graphics_pipeline(resolve_pipeline);
                encoder.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
                encoder.set_scissor(0, 0, width, height);
                encoder.push_descriptors(
                    0,
                    &CurvesOITResolveArguments {
                        fragment_buffer: self.oit_fragment_buffer.slice(..),
                        fragment_count_buffer: self.oit_fragment_count_buffer.slice(..),
                    },
                );
                encoder.push_constants(&CurvesOITResolvePushConstants {
                    viewport_size: uvec2(width, height),
                });
                // Draw full-screen quad
                encoder.set_primitive_topology(PrimitiveTopology::TriangleList);
                encoder.draw(0..3, 0..1);
            });
        }
    }*/
}

pub struct Plane {
    pub coefs: glam::DVec4,    // a,b,c,d in ax + by + cz + d = 0
}

impl Plane {
    pub fn new(normal: DVec3, point: DVec3) -> Plane {
        let d = -normal.dot(point);
        Plane { coefs: DVec4::new(normal.x, normal.y, normal.z, d) }
    }

    pub fn intersect(&self, ray_origin: DVec3, ray_dir: DVec3) -> Option<DVec3> {
        let denom = self.coefs.xyz().dot(ray_dir);
        if denom.abs() < 1e-6 {
            return None;
        }
        let t = -(self.coefs.xyz().dot(ray_origin) + self.coefs.w) / denom;
        Some(ray_origin + ray_dir * t)
    }
}


impl App {
    /// Initializes the application.
    ///
    /// # Arguments
    ///
    /// * `color_target_format` format of the swap chain images
    pub fn new(device: &Device, width: u32, height: u32, color_target_format: Format) -> App {
        let depth_buffer = create_depth_buffer(device, width, height);
        let depth_buffer_view = depth_buffer.create_top_level_view();
        let camera_control = CameraControl::new(width, height);
        let overlay_renderer = OverlayRenderer::new(device, color_target_format, depth_buffer.format());
        let frame_image = device.create_image(&ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
            format: Format::R16G16B16A16_SFLOAT,
            width: 1,
            height: 1,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
        });
        let temporal_avg_image = device.create_image(&ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
            format: Format::R16G16B16A16_SFLOAT,
            width: 1,
            height: 1,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
        });

        // DUMMY
        let oit_fragment_buffer = device.create_array_buffer::<CurvesOITFragmentData>(
            BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
            MemoryLocation::GpuOnly,
            1,
        );
        oit_fragment_buffer.untyped.set_name("oit_fragment_buffer");
        // DUMMY
        let oit_fragment_count_buffer =
            device.create_array_buffer::<u32>(BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST, MemoryLocation::GpuOnly, 1);
        oit_fragment_count_buffer.untyped.set_name("oit_fragment_count_buffer");

        let drawn_curves = AppendBuffer::new(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu);
        let drawn_control_points = AppendBuffer::new(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu);

        // load tweaks
        let settings = SavedSettings::load().unwrap_or_default();
        let mut engine = Engine::new(device.clone());
        let tweaks = settings
            .tweaks
            .iter()
            .filter(|tweak| tweak.enabled)
            .map(|tweak| (tweak.name.clone(), tweak.value.clone()))
            .collect();
        engine.set_global_defines(tweaks);

        let mut app = App {
            device: device.clone(),
            animation: None,
            depth_buffer,
            depth_buffer_view,
            color_target_format,
            camera_control,
            overlay: overlay_renderer,
            pipelines: Default::default(),
            bin_rast_stroke_width: 1.0,
            bin_rast_current_frame: 0,
            oit_stroke_width: 0.0,
            oit_max_fragments_per_pixel: 0,
            oit_fragment_buffer,
            oit_fragment_count_buffer,
            reload_brush_textures: true,
            brush_textures: vec![],
            selected_brush: 0,
            overlay_line_width: 1.0,
            overlay_filter_width: 1.0,
            is_drawing: false,
            last_pos: Default::default(),
            pen_points: vec![],
            drawn_curves,
            mode: RenderMode::BinRasterization,
            temporal_average: false,
            temporal_avg_image,
            frame: 0,
            frame_image,
            temporal_average_alpha: 0.25,
            engine,
            drawn_control_points,
            settings,
            debug_tile_line_overflow: false,
            tweaks_changed: false,
            draw_origin: Default::default(),
            fit_tolerance: 1.0,
            curve_embedding_factor: 1.0,
        };
        app.reload_shaders();
        app
    }

    /// Called when the main window is resized.
    pub fn resize(&mut self, device: &Device, width: u32, height: u32) {
        // reallocate the depth buffer
        self.camera_control.resize(width, height);
        self.depth_buffer = create_depth_buffer(device, width, height);
        self.depth_buffer_view = self.depth_buffer.create_top_level_view();
        self.temporal_avg_image = device.create_image(&ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST | ImageUsage::COLOR_ATTACHMENT,
            format: Format::R16G16B16A16_SFLOAT,
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
        });
        self.temporal_avg_image.set_name("temporal_avg_image");
        self.frame_image = device.create_image(&ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST | ImageUsage::COLOR_ATTACHMENT,
            format: Format::R16G16B16A16_SFLOAT,
            width,
            height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
        });
        self.frame_image.set_name("frame_image");
    }

    pub fn mouse_input(&mut self, button: MouseButton, _pos: DVec2, pressed: bool) {
        self.camera_control.mouse_input(button, pressed);
    }

    pub fn cursor_moved(&mut self, pos: DVec2) {
        self.camera_control.cursor_moved(pos);
    }

    pub fn key_input(&mut self, key: &winit::keyboard::Key, pressed: bool) {
        if *key == winit::keyboard::Key::Named(NamedKey::F5) && pressed {
            self.reload_shaders();
        }
    }

    pub fn touch_event(&mut self, touch_event: &winit::event::Touch) {
        if touch_event.phase == TouchPhase::Started {
            self.pen_points.clear();
            self.is_drawing = true;
        }

        let camera = self.camera_control.camera();
        if self.is_drawing {
            let (x, y): (f64, f64) = touch_event.location.into();
            let pressure = if let Some(force) = touch_event.force {
                force.normalized()
            } else {
                1.0
            };

            const PEN_SPACING_THRESHOLD: f64 = 2.0;

            let ray = camera.screen_to_view_dir(dvec2(x, y));
            trace!("Touch event: {:?} at ({}, {}) with pressure {}; ray {}", touch_event.phase, x, y, pressure, ray);

            if self.last_pos.distance(dvec2(x, y)) >= PEN_SPACING_THRESHOLD {
                self.pen_points.push(PenSample {
                    position: dvec2(x, y),
                    pressure,
                });
                self.last_pos = dvec2(x, y);
            }

            if touch_event.phase == TouchPhase::Ended {
                self.is_drawing = false;

                // points bounding box
                let min_pos = self.pen_points.iter().map(|p| p.position).fold(dvec2(f64::INFINITY, f64::INFINITY), |a, b| a.min(b));
                let max_pos = self.pen_points.iter().map(|p| p.position).fold(dvec2(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, b| a.max(b));
                let width = max_pos.x - min_pos.x;
                let height = max_pos.y - min_pos.y;

                // project points on random plane
                let (eye, dir) = camera.screen_to_world_ray(self.pen_points.first().unwrap().position);
                let ground_plane = Plane::new(dvec3(0.0, 1.0, 0.0), dvec3(0.0, 0.0, 0.0));

                if let Some(ground_pos) = ground_plane.intersect(eye, dir) {
                    let mut apparent_distances = vec![];
                    for a in 0..10 {
                        let alpha = (a as f64 / 10.0) * std::f64::consts::PI;
                        let v = dvec3(alpha.sin(), 0.0, alpha.cos());
                        let (a, b) = camera.world_to_screen_line(ground_pos, v);
                        let d = a.xy().distance(b.xy());
                        apparent_distances.push((alpha, d));
                    }
                    let (minalpha, closest_dist) = apparent_distances.iter().fold((0.0, f64::INFINITY), |(minalpha, mindist), &(alpha, dist)| {
                        if (dist - width).abs() < mindist {
                            (alpha, (dist - width).abs())
                        } else {
                            (minalpha, mindist)
                        }
                    });

                    //let angle = thread_rng().gen_range(0.0..std::f64::consts::PI);
                    let angle = minalpha;
                    let plane = Plane::new(dvec3(angle.sin(), 0.0, angle.cos()), ground_pos);

                    let proj_points = self.pen_points.iter().filter_map(|p| {
                        let (eye, p) = camera.screen_to_world_ray(p.position);
                        plane.intersect(eye, p)
                    }).collect::<Vec<_>>();

                    // fit a curve to the pen points
                    trace!("projected points: {:#?}", proj_points);
                    let points_f64 = proj_points.iter().map(|p| p.to_array()).flatten().collect::<Vec<f64>>();
                    match curve_fit_nd::curve_fit_cubic_to_points_f64(&points_f64, 3, self.fit_tolerance, Default::default(), None) {
                        Ok(curve) => {
                            let mut control_points = curve.cubic_array.chunks_exact(3).map(|chunk| {
                                dvec3(chunk[0], chunk[1], chunk[2])
                            }).collect::<Vec<_>>();

                            for cp in control_points.chunks_exact(3) {
                                self.overlay.line(cp[1], cp[0], [255, 0, 0, 255], [255, 0, 0, 255]);
                                self.overlay.line(cp[1], cp[2], [0, 255, 0, 255], [0, 255, 0, 255]);
                            }

                            control_points.remove(0);
                            control_points.remove(control_points.len() - 1);

                            trace!("Fitted curve: {:#?}", control_points);

                            if let Some(ref mut anim) = self.animation.as_mut() {
                                //let base = anim.frames[self.bin_rast_current_frame].curve_range.start;
                                let frame = &mut anim.frames[0];
                                let mut base = anim.position_buffer.len() as u32;
                                for point in control_points.chunks_exact(4) {
                                    let p0 = point[0];
                                    let p1 = point[1];
                                    let p2 = point[2];
                                    let p3 = point[3];
                                    anim.position_buffer.push(ControlPoint {
                                        pos: p0.as_vec3().to_array(),
                                        color: [0.1, 0.3, 0.9],
                                    });
                                    anim.position_buffer.push(ControlPoint {
                                        pos: p1.as_vec3().to_array(),
                                        color: [0.1, 0.3, 0.9],
                                    });
                                    anim.position_buffer.push(ControlPoint {
                                        pos: p2.as_vec3().to_array(),
                                        color: [0.1, 0.3, 0.9],
                                    });
                                    anim.position_buffer.push(ControlPoint {
                                        pos: p3.as_vec3().to_array(),
                                        color: [0.1, 0.3, 0.9],
                                    });
                                    anim.curve_buffer.push(CurveDesc {
                                        width_profile: Default::default(),
                                        opacity_profile: Default::default(),
                                        count: 4,
                                        start: base,
                                        param_range: vec2(0.0, 1.0),
                                    });
                                    frame.curve_range.count += 1;
                                    //anim.curve_count += 1;
                                    //anim.point_count += 4;
                                    error!("append curve @ {base}");
                                    base += 4;
                                }
                            }
                        }
                        Err(e) => {
                            error!("failed to fit curve: {}", e);
                        }
                    }
                }
            }
        }
    }

    pub fn mouse_wheel(&mut self, delta: f64) {
        self.camera_control.mouse_wheel(delta);
    }

    pub fn draw_curves(&mut self) {
        /*if let Some(anim_data) = self.animation.as_ref() {
            let frame = &anim_data.frames[self.bin_rast_current_frame];
            for segment in frame.curve_segments.iter() {
                self.overlay.cubic_bezier(segment, [0, 0, 0, 255]);
            }
        }*/
    }

    pub fn draw_axes(&mut self) {
        let red = [255, 0, 0, 255];
        let green = [0, 255, 0, 255];
        let blue = [0, 0, 255, 255];

        self.overlay.line(dvec3(0.0, 0.0, 0.0), dvec3(0.95, 0.0, 0.0), red, red);
        self.overlay.line(dvec3(0.0, 0.0, 0.0), dvec3(0.0, 0.95, 0.0), green, green);
        self.overlay.line(dvec3(0.0, 0.0, 0.0), dvec3(0.0, 0.0, 0.95), blue, blue);

        self.overlay.cone(vec3(0.95, 0.0, 0.0), vec3(1.0, 0.0, 0.0), 0.02, red, red);
        self.overlay.cone(vec3(0.0, 0.95, 0.0), vec3(0.0, 1.0, 0.0), 0.02, green, green);
        self.overlay.cone(vec3(0.0, 0.0, 0.95), vec3(0.0, 0.0, 1.0), 0.02, blue, blue);

        let camera = self.camera_control.camera();
        let pen_line = self.pen_points.iter().map(|p| p.position).collect::<Vec<_>>();
        self.overlay.screen_polyline(&camera, pen_line.as_slice(), [255, 128, 0, 255]);
    }

    pub fn render(&mut self, cmd: &mut CommandStream, image: &Image) {
        if self.reload_brush_textures {
            self.reload_textures(cmd);
            self.reload_brush_textures = false;
        }

        // commit dynamic curves
        self.drawn_curves.commit(cmd);
        self.drawn_control_points.commit(cmd);

        let width = image.width();
        let height = image.height();

        self.setup(cmd, self.frame_image.clone(), width, height);

        let color_target_view = self.frame_image.create_top_level_view();
        self.draw_axes();
        self.draw_curves();

        let camera = self.camera_control.camera();
        if self.is_drawing {
            // draw a cross at the touch point
            let (x, y) = (self.last_pos.x, self.last_pos.y);
            self.overlay.screen_line(&camera, dvec2(x - 50.0, y), dvec2(x + 50.0, y), [255, 255, 0, 255], [255, 255, 0, 255]);
            self.overlay.screen_line(&camera, dvec2(x, y - 50.0), dvec2(x, y + 50.0), [255, 255, 0, 255], [255, 255, 0, 255]);
        }

        // Draw overlay
        cmd.debug_group("Overlay", |cmd| {
            self.overlay.render(
                cmd,
                OverlayRenderParams {
                    camera: self.camera_control.camera(),
                    color_target: &color_target_view,
                    depth_target: &self.depth_buffer_view,
                    line_width: self.overlay_line_width,
                    filter_width: self.overlay_filter_width,
                },
            );
        });

        // blit next frame to screen
        cmd.debug_group("blit final frame", |cmd| {
            cmd.blit_image(
                if self.temporal_average {
                    &self.temporal_avg_image
                } else {
                    &self.frame_image
                },
                ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                Rect3D {
                    min: Point3D { x: 0, y: 0, z: 0 },
                    max: Point3D {
                        x: width as i32,
                        y: height as i32,
                        z: 1,
                    },
                },
                &image,
                ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                Rect3D {
                    min: Point3D { x: 0, y: 0, z: 0 },
                    max: Point3D {
                        x: width as i32,
                        y: height as i32,
                        z: 1,
                    },
                },
                vk::Filter::NEAREST,
            );
        });

        self.frame += 1;
    }

    pub fn egui(&mut self, ctx: &egui::Context) {
        // why does `egui::Context` need Send+Sync?
        let dt = ctx.input(|input| input.unstable_dt);

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            let reload_shortcut = egui::KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, Key::O);
            if ui.input_mut(|input| input.consume_shortcut(&reload_shortcut)) {
                if let Some(path) = self.settings.last_geom_file.clone() {
                    self.load_geo_file(&path);
                }
            }

            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Load .geo...").clicked() {
                        use rfd::FileDialog;
                        let file = FileDialog::new().add_filter("Houdini JSON geometry", &["geo"]).pick_file();
                        if let Some(ref file) = file {
                            self.load_geo_file(file);
                        }
                    }
                    if egui::Button::new("Reload last geometry")
                        .shortcut_text(ui.ctx().format_shortcut(&reload_shortcut))
                        .ui(ui)
                        .clicked()
                    {
                        if let Some(path) = self.settings.last_geom_file.clone() {
                            self.load_geo_file(&path);
                        }
                    }
                })
            });
        });

        egui::Window::new("Stats")
            .frame(
                Frame::default()
                    .fill(egui::Color32::from_black_alpha(200))
                    .inner_margin(Margin::same(5.0))
                    .rounding(Rounding::same(2.0)),
            )
            .collapsible(false)
            .title_bar(false)
            .anchor(Align2::RIGHT_TOP, egui::Vec2::new(-5., 5.))
            .fixed_size(egui::Vec2::new(200., 40.)) // https://github.com/emilk/egui/issues/498 🤡
            .show(ctx, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                ui.label(format!("{:.2} ms/frame ({:.0} FPS)", dt * 1000., 1.0 / dt));
            });

        egui::Window::new("Settings").show(ctx, |ui| {
            ui.heading("Temporal average");
            //  ui.checkbox(&mut self.is_drawing, "Drawing mode");
            ui.checkbox(&mut self.temporal_average, "Enable Temporal Average");
            ui.add_enabled(
                self.temporal_average,
                egui::Slider::new(&mut self.temporal_average_alpha, 0.0..=1.).text("Alpha"),
            );

            ui.separator();
            ui.heading("Render Mode");
            ui.radio_value(&mut self.mode, RenderMode::BinRasterization, "Bin Rasterization");
            ui.radio_value(&mut self.mode, RenderMode::CurvesOIT, "Curves OIT");
            ui.radio_value(&mut self.mode, RenderMode::CurvesOITv2, "Curves OIT v2");
            ui.checkbox(&mut self.debug_tile_line_overflow, "Debug overflowing tiles")
                .on_hover_text("Show tiles which exceeded the maximum number of lines per tile");

            ui.separator();

            ui.add(egui::Slider::new(&mut self.bin_rast_stroke_width, 0.1..=40.0).text("Stroke Width"));
            ui.add(egui::Slider::new(&mut self.oit_stroke_width, 0.1..=40.0).text("OIT Stroke Width"));
            ui.add(egui::Slider::new(&mut self.overlay_line_width, 0.1..=40.0).text("Overlay Line Width"));
            ui.add(egui::Slider::new(&mut self.overlay_filter_width, 0.01..=10.0).text("Overlay Filter Width"));

            ui.separator();

            let current_brush = if self.selected_brush < self.brush_textures.len() {
                self.brush_textures[self.selected_brush].name.as_str()
            } else {
                "<choose...>"
            };
            egui::ComboBox::from_label("Brush Texture")
                .selected_text(current_brush)
                .show_ui(ui, |ui| {
                    for (i, brush) in self.brush_textures.iter().enumerate() {
                        ui.selectable_value(&mut self.selected_brush, i, &brush.name);
                    }
                });

            ui.separator();

            ui.heading("Animation");

            if let Some(ref animation) = self.animation {
                egui::DragValue::new(&mut self.bin_rast_current_frame)
                    .clamp_range(0..=(animation.frames.len() - 1))
                    .custom_formatter(|n, _| format!("Frame {} of {}", n, animation.frames.len()))
                    .ui(ui);
            }

            ui.heading("Global settings");
            TableBuilder::new(ui)
                .column(Column::auto().resizable(true))
                .column(Column::remainder())
                .column(Column::exact(16.0))
                .column(Column::exact(16.0))
                .striped(true)
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.label("Define");
                    });
                    header.col(|ui| {
                        ui.label("Value");
                    });
                })
                .body(|mut body| {
                    let mut delete = None;
                    for (i, t) in self.settings.tweaks.iter_mut().enumerate() {
                        body.row(18.0, |mut row| {
                            row.col(|ui| {
                                let resp = ui.text_edit_singleline(&mut t.name);
                                if t.autofocus {
                                    resp.request_focus();
                                    t.autofocus = false;
                                }
                                if resp.changed() {
                                    self.tweaks_changed = true;
                                }
                            });
                            row.col(|ui| {
                                if ui.text_edit_singleline(&mut t.value).changed() {
                                    self.tweaks_changed = true;
                                }
                            });
                            row.col(|ui| {
                                if ui.checkbox(&mut t.enabled, "").changed() {
                                    self.tweaks_changed = true;
                                }
                            });
                            row.col(|ui| {
                                if icon_button(ui, egui_phosphor::fill::TRASH, egui::Color32::WHITE).clicked() {
                                    delete = Some(i);
                                    self.tweaks_changed = true;
                                }
                            });
                        });
                    }
                    if let Some(i) = delete {
                        self.settings.tweaks.remove(i);
                    }
                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            if ui.button("Add tweak").clicked() {
                                self.settings.tweaks.push(Tweak {
                                    name: format!("TWEAK_VALUE_{}", self.settings.tweaks.len()),
                                    value: "0".to_string(),
                                    enabled: true,
                                    autofocus: true,
                                });
                                self.tweaks_changed = true;
                            }
                        });
                    });
                });

            if self.tweaks_changed {
                self.settings.save();
                self.tweaks_changed = false;
            }

            if ui.button("Reload shaders").clicked() {
                let defines = self
                    .settings
                    .tweaks
                    .iter()
                    .filter(|t| t.enabled)
                    .map(|t| (t.name.clone(), t.value.clone()))
                    .collect();
                info!("Will reload shaders on the next frame");
                self.engine.set_global_defines(defines);
            }

            ui.add(Slider::new(&mut self.fit_tolerance, 1.0..=40.0).text("Curve fit tolerance"));
            ui.add(Slider::new(&mut self.curve_embedding_factor, 1.0..=40.0).text("Curve fit tolerance"));
            //ui.add(Slider::new(&mut self.oit_stroke_width, 0.1..=40.0).text("OIT Stroke Width"));
            //ui.add(Slider::new(&mut self.overlay_line_width, 0.1..=40.0).text("Overlay Line Width"));
            //ui.add(Slider::new(&mut self.overlay_filter_width, 0.01..=10.0).text("Overlay Filter Width"));
        });
    }

    pub fn on_exit(&mut self) {
        self.settings.save();
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/*
fn create_curves_oit_pipeline(
    device: &Device,
    target_color_format: Format,
    target_depth_format: Format,
) -> Result<GraphicsPipeline, graal::Error> {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[CurvesOITArguments::LAYOUT],
            push_constants_size: mem::size_of::<CurvesOITPushConstants>(),
        },
        vertex_input: Default::default(),
        pre_rasterization_shaders: PreRasterizationShaders::MeshShading {
            task: None,
            mesh: ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/curve_oit.glsl"))),
                entry_point: "main",
            },
        },
        rasterization: RasterizationState {
            polygon_mode: PolygonMode::Fill,
            cull_mode: Default::default(),
            front_face: FrontFace::CounterClockwise,
            conservative_rasterization_mode: ConservativeRasterizationMode::Disabled,
            ..Default::default()
        },
        fragment_shader: ShaderEntryPoint::from_source_file(Path::new("crates/fluff/shaders/curve_oit.glsl")),
        depth_stencil: DepthStencilState {
            depth_write_enable: false,
            depth_compare_op: CompareOp::Always,
            stencil_state: StencilState::default(),
        },
        fragment_output: FragmentOutputInterfaceDescriptor {
            color_attachment_formats: &[target_color_format],
            depth_attachment_format: Some(target_depth_format),
            stencil_attachment_format: None,
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
                color_write_mask: Default::default(),
            }],
            blend_constants: [0.0; 4],
        },
    };

    device.create_graphics_pipeline(create_info)
}

fn create_curves_oit_v2_pipeline(
    device: &Device,
    target_color_format: Format,
    target_depth_format: Format,
) -> Result<GraphicsPipeline, graal::Error> {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[CurvesOITArguments::LAYOUT],
            push_constants_size: mem::size_of::<CurvesOITPushConstants>(),
        },
        vertex_input: Default::default(),
        pre_rasterization_shaders: PreRasterizationShaders::MeshShading {
            task: Some(ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/curve_oit_v2.glsl"))),
                entry_point: "main",
            }),
            mesh: ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/curve_oit_v2.glsl"))),
                entry_point: "main",
            },
        },
        rasterization: RasterizationState {
            polygon_mode: PolygonMode::Fill,
            cull_mode: Default::default(),
            front_face: FrontFace::CounterClockwise,
            conservative_rasterization_mode: ConservativeRasterizationMode::Disabled,
            ..Default::default()
        },
        fragment_shader: ShaderEntryPoint::from_source_file(Path::new("crates/fluff/shaders/curve_oit_v2.glsl")),
        depth_stencil: DepthStencilState {
            depth_write_enable: true,
            depth_compare_op: CompareOp::Less,
            stencil_state: StencilState::default(),
        },
        fragment_output: FragmentOutputInterfaceDescriptor {
            color_attachment_formats: &[target_color_format],
            depth_attachment_format: Some(target_depth_format),
            stencil_attachment_format: None,
            multisample: Default::default(),
            color_targets: &[ColorTargetState {
                blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
                color_write_mask: Default::default(),
            }],
            blend_constants: [0.0; 4],
        },
    };

    device.create_graphics_pipeline(create_info)
}
*/
