use egui::{Align2, Frame, Margin, Rounding, Widget};
use glam::{mat4, uvec2, vec2, vec3, vec4, DVec2};
use graal::{
    prelude::*,
    vk::{AttachmentLoadOp, AttachmentStoreOp},
    Buffer, BufferRange, ColorAttachment, ComputePipeline, ComputePipelineCreateInfo, DepthStencilAttachment, Descriptor, ImageCopyBuffer,
    ImageCopyView, ImageDataLayout, ImageSubresourceLayers, ImageView, Point3D, Rect3D, RenderPassDescriptor,
};
use std::{
    fs, mem,
    path::{Path, PathBuf},
    ptr,
};

use houdinio::Geo;
use winit::{
    event::MouseButton,
    keyboard::{Key, NamedKey},
};

use crate::{
    camera_control::CameraControl,
    engine::{ColorAttachmentDesc, ComputePipelineDesc, DepthStencilAttachmentDesc, Engine, Error, MeshRenderPipelineDesc},
    overlay::{CubicBezierSegment, OverlayRenderParams, OverlayRenderer},
    shaders,
    util::resolve_file_sequence,
};
use crate::shaders::shared::{BINNING_TILE_SIZE, TemporalAverageParams};

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
            staging_buffer.mapped_data().unwrap(),
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

/// 3D bezier control point.
#[derive(Copy, Clone)]
#[repr(C)]
struct ControlPoint {
    pos: [f32; 3],
    color: [f32; 3],
}

fn lagrange_interpolation(p1: glam::Vec2, p2: glam::Vec2, p3: glam::Vec2, p4: glam::Vec2) -> glam::Vec4 {
    let ys = vec4(p1.y, p2.y, p3.y, p4.y);
    let xs = vec4(p1.x, p2.x, p3.x, p4.x);
    let v = mat4(vec4(1.0, 1.0, 1.0, 1.0), xs, xs * xs, xs * xs * xs);
    let v_inv = v.inverse();
    let coeffs = v_inv * ys;
    coeffs
}

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
}

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
    point_count: usize,
    curve_count: usize,
    frames: Vec<AnimationFrame>,
    position_buffer: Buffer<[ControlPoint]>,
    curve_buffer: Buffer<[CurveDesc]>,
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

    let position_buffer = device.create_array_buffer::<ControlPoint>(BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu, point_count);
    position_buffer.set_name("control point buffer");
    let curve_buffer = device.create_array_buffer::<CurveDesc>(BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu, curve_count);
    curve_buffer.set_name("curve buffer");

    let mut frames = vec![];

    // dummy width and opacity profiles
    let width_profile = lagrange_interpolation(vec2(0.0, 0.0), vec2(0.2, 0.8), vec2(0.5, 0.8), vec2(1.0, 0.0));
    let opacity_profile = lagrange_interpolation(vec2(0.0, 0.7), vec2(0.3, 1.0), vec2(0.6, 1.0), vec2(1.0, 0.0));

    // write curves
    unsafe {
        let point_data = position_buffer.mapped_data().unwrap();
        let mut point_ptr = 0;
        let curve_data = curve_buffer.mapped_data().unwrap();
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
    }

    AnimationData {
        point_count,
        curve_count,
        frames,
        position_buffer,
        curve_buffer,
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

const BIN_RAST_SAMPLE_COUNT: u32 = 16;
const CURVE_BINNING_TILE_SIZE: u32 = 16;
const CURVE_BINNING_MAX_LINES_PER_TILE: usize = 64;
const CURVES_OIT_MAX_FRAGMENTS_PER_PIXEL: usize = 8;

type CurveIndex = u32;

#[derive(Copy, Clone)]
struct BinRastPushConstants {
    view_proj: glam::Mat4,
    viewport_size: glam::UVec2,
    stroke_width: f32,
    base_curve_index: u32,
    curve_count: u32,
    /// Number of tiles in the X direction.
    tile_count_x: u32,
    /// Number of tiles in the Y direction.
    tile_count_y: u32,
    frame: i32,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct TileEntry {
    line: [f32; 4],
    param_range: [f32; 2],
    curve_index: CurveIndex,
}

const _: () = assert!(mem::size_of::<TileEntry>() == 28);

#[derive(Copy, Clone)]
#[repr(C)]
struct BinRastTile {
    lines: [TileEntry; CURVE_BINNING_MAX_LINES_PER_TILE],
}

const _: () = assert!(mem::size_of::<BinRastTile>() == 28 * CURVE_BINNING_MAX_LINES_PER_TILE);

struct BinRastArguments<'a> {
    //#[argument(binding = 0, storage, read_only)]
    position_buffer: BufferRange<[ControlPoint]>,
    //#[argument(binding = 1, storage, read_only)]
    curve_buffer: BufferRange<[CurveDesc]>,
    //#[argument(binding = 2, storage_image, read_write)]
    tile_line_count_image: &'a ImageView,
    //#[argument(binding = 3, storage, read_write)]
    tile_buffer: BufferRange<[BinRastTile]>,
}

//#[derive(Arguments)]
struct DrawCurvesArguments<'a> {
    //#[argument(binding = 0, storage, read_only)]
    position_buffer: BufferRange<[ControlPoint]>,
    //#[argument(binding = 1, storage, read_only)]
    curve_buffer: BufferRange<[CurveDesc]>,
    //#[argument(binding = 2, storage_image, read_write)]
    tile_line_count_image: &'a ImageView,
    //#[argument(binding = 3, storage)]
    tile_buffer: BufferRange<[BinRastTile]>,
    //#[argument(binding = 4, storage_image, read_write)]
    output_image: &'a ImageView,
}


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

//////////////////////////////////////////////////////


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
    id: u32,
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

    // Bin rasterization
    bin_rast_stroke_width: f32,
    bin_rast_current_frame: usize,

    // Curves OIT
    oit_stroke_width: f32,
    oit_max_fragments_per_pixel: u32,
    oit_fragment_buffer: Buffer<[CurvesOITFragmentData]>,
    oit_fragment_count_buffer: Buffer<[u32]>,

    // Resources
    reload_brush_textures: bool,
    brush_textures: Vec<BrushTexture>,
    selected_brush: usize,

    // Overlay
    overlay_line_width: f32,
    overlay_filter_width: f32,

    engine: Engine,
}

/*
layout(push_constant) uniform PushConstants {
    mat4 viewProjectionMatrix;
    uvec2 viewportSize;
    float strokeWidth;
    int baseCurveIndex;
    int curveCount;
    int tilesCountX;
    int tilesCountY;
    int frame;

    CurveControlPoints b_controlPoints;
    CurveBuffer b_curveData;
    TileLineCountData b_tileLineCount;
    TileData b_tileData;


};
*/
#[derive(Copy, Clone)]
#[repr(C)]
struct CurveBinningConstants {
    control_points: vk::DeviceAddress,
    curve_data: vk::DeviceAddress,
    tile_line_count: vk::DeviceAddress,
    tile_data: vk::DeviceAddress,
    view_projection_matrix: glam::Mat4,
    viewport_size: glam::UVec2,
    stroke_width: f32,
    base_curve_index: u32,
    curve_count: u32,
    tile_count_x: u32,
    tile_count_y: u32,
    frame: u32,
}

/*
const CONTROL_POINT_BUFFER: BufferKey = BufferKey("b_controlPoints");
const CURVE_BUFFER: BufferKey = BufferKey("b_curves");
const TILE_LINE_COUNT_BUFFER: BufferKey = BufferKey("b_tileLineCount");
const TILE_BUFFER: BufferKey = BufferKey("b_tiles");

const COLOR_TARGET: ImageKey = ImageKey("i_color");
const DEPTH_TARGET: ImageKey = ImageKey("i_depth");
const TEMPORAL_AVERAGE_TARGET: ImageKey = ImageKey("i_temporalAvg");

const PASS_CURVE_BINNING: &str = "curve_binning";
const PASS_DRAW_CURVES: &str = "draw_curves";
const PASS_TEMPORAL_AVERAGE: &str = "temporal_average";*/

impl App {
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

        let tile_count_x = width.div_ceil(CURVE_BINNING_TILE_SIZE);
        let tile_count_y = height.div_ceil(CURVE_BINNING_TILE_SIZE);
        //engine.define_global("TILE_SIZE", CURVE_BINNING_TILE_SIZE.to_string());

        let mut rg = engine.create_graph();

        /*// set common uniform variables
        rg.set_global_constant("viewProjectionMatrix", self.camera_control.camera().view_projection());
        rg.set_global_constant("viewportSize", [width, height]);
        rg.set_global_constant("strokeWidth", self.bin_rast_stroke_width);
        rg.set_global_constant("baseCurveIndex", anim_frame.curve_range.start);
        rg.set_global_constant("curveCount", anim_frame.curve_range.count);
        rg.set_global_constant("frame", self.frame);*/

        let control_point_buffer = rg.import_buffer("control_points", animation.position_buffer.untyped.clone());
        let curve_buffer = rg.import_buffer("curves", animation.curve_buffer.untyped.clone());
        let temporal_average = rg.import_image("temporal_average", self.temporal_avg_image.clone());
        let color_target = rg.import_image("color_target", color_target);
        let depth_target = rg.import_image("depth_buffer", self.depth_buffer.clone());

        ////////////////////////////////////////////////////////////
        // Curve binning test
        let tile_line_count_buffer = rg.create_buffer(
            "TILE_LINE_COUNT_BUFFER",
            tile_count_x as usize * tile_count_y as usize * size_of::<u32>(),
        );
        let tile_buffer = rg.create_buffer(
            "TILE_BUFFER",
            tile_count_x as usize * tile_count_y as usize * size_of::<BinRastTile>(),
        );

        {
            // clear buffers
            rg.record_fill_buffer("clear_tile_line_count", tile_line_count_buffer, 0);
            rg.record_fill_buffer("clear_tile_buffer", tile_buffer, 0);

            let curve_binning_pipeline = engine
                .create_mesh_render_pipeline(
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
                )
                .unwrap();

            let mut rp = rg.record_mesh_render_pass("bin_curves", curve_binning_pipeline);

            rp.set_color_attachments([ColorAttachmentDesc {
                image: color_target,
                clear_value: Some([0.0, 0.0, 0.0, 1.0]),
            }]);
            rp.set_depth_stencil_attachment(DepthStencilAttachmentDesc {
                image: depth_target,
                depth_clear_value: Some(1.0),
                stencil_clear_value: None,
            });
            rp.read_buffer(control_point_buffer);
            rp.read_buffer(curve_buffer);
            rp.write_buffer(tile_line_count_buffer);
            rp.write_buffer(tile_buffer);

            rp.set_render_func(move |encoder| {
                let vp_width = width as f32 / BINNING_TILE_SIZE as f32;
                let vp_height = height as f32 / BINNING_TILE_SIZE as f32;
                encoder.set_viewport(0.0, 0.0, vp_width, vp_height, 0.0, 1.0);

                //eprintln!("control_point_buffer device address = 0x{:016x}", control_point_buffer.device_address());
                //eprintln!("curve_buffer device address = 0x{:016x}", curve_buffer.device_address());
                //eprintln!("base_curve_index = {}", base_curve_index);
                //eprintln!("curve_count = {}", curve_count);

                encoder.set_scissor(0, 0, tile_count_x, tile_count_y);
                encoder.push_constants(&shaders::shared::BinCurvesParams {
                    view_projection_matrix: view_proj,
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

            rp.finish();
        }

        ////////////////////////////////////////////////////////////
        // resolve curves
        {
            let draw_curves_pipeline = engine.create_compute_pipeline(
                "draw_curves",
                ComputePipelineDesc {
                    shader: PathBuf::from("crates/fluff/shaders/draw_curves.comp"),
                    defines: Default::default(),
                },
            )?;

            let mut pass = rg.record_compute_pass("draw_curves", draw_curves_pipeline);
            pass.read_buffer(tile_line_count_buffer);
            pass.read_buffer(tile_buffer);
            pass.write_image(color_target);
            pass.set_render_func(move |encoder| {
                //eprintln!("tile_line_count_buffer device address = 0x{:016x}", tile_line_count_buffer.device_address());
                //eprintln!("number of tiles = {}x{}", tile_count_x, tile_count_y);
                //eprintln!("tile_buffer device address = 0x{:016x}", tile_buffer.device_address());

                encoder.push_constants(&crate::shaders::shared::DrawCurvesPushConstants {
                    view_proj,
                    base_curve: base_curve_index,
                    stroke_width,
                    tile_count_x,
                    tile_count_y,
                    frame,
                    tile_data: tile_buffer.device_address(),
                    tile_line_count: tile_line_count_buffer.device_address(),
                    output_image: color_target.device_handle(),
                });
                encoder.dispatch(tile_count_x, tile_count_y, 1);
            });
            pass.finish();
        }

        /*////////////////////////////////////////////////////////////
        // Temporal accumulation
        {
            let compute_test = engine.create_compute_pipeline(
                "compute_test",
                ComputePipelineDesc {
                    shader: PathBuf::from("../shaders/compute_test.comp"),
                    defines: Default::default(),
                },
            )?;
            let mut pass = rg.record_compute_pass("compute_test", compute_test);
            pass.read_buffer(curve_buffer);
            pass.read_buffer(control_point_buffer);
            pass.write_image(temporal_average);
            pass.set_render_func(move |encoder| {
                encoder.push_constants(&ComputeTestPushConstants {
                    element_count: curve_count,
                    data: curve_buffer.device_address(),
                    control_points: control_point_buffer.device_address(),
                    output_image: temporal_average.device_handle(),
                });
                encoder.dispatch(curve_count.div_ceil(64), 1, 1);
            });
            pass.finish();
            //rg.record_blit("blit_temporal_avg", temporal_average.clone(), color_target.clone());
        }*/

        ////////////////////////////////////////////////////////////
        // Temporal accumulation
        if self.temporal_average {
            let temporal_average_pipeline = engine.create_compute_pipeline(
                "temporal_average",
                ComputePipelineDesc {
                    shader: PathBuf::from("../shaders/temporal_average.comp"),
                    defines: Default::default(),
                },
            )?;
            let mut pass = rg.record_compute_pass("temporal_average", temporal_average_pipeline.clone());
            pass.read_image(color_target);
            pass.read_image(temporal_average);
            pass.write_image(temporal_average);
            pass.set_render_func(move |encoder| {
                encoder.push_constants(&TemporalAverageParams {
                    viewport_size: uvec2(width, height),
                    frame,
                    falloff: temporal_average_falloff,
                    new_frame: color_target.device_handle(),
                    avg_frame: temporal_average.device_handle(),
                });
                encoder.dispatch(width.div_ceil(8), height.div_ceil(8), 1);
            });
            pass.finish();
            rg.record_blit("blit_temporal_avg", temporal_average.clone(), color_target.clone());
        }

        ////////////////////////////////////////////////////////////
        engine.submit_graph(rg, cmd);
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

        /*self.pipelines.curve_binning_pipeline = check(
            "bin_rast_pipeline",
            create_bin_rast_pipeline(&self.device, self.color_target_format, self.depth_buffer.format()),
        );
        self.pipelines.draw_curves_pipeline = check("draw_curves_pipeline", create_draw_curves_pipeline(&self.device));
        self.pipelines.draw_curves_oit_pipeline = check(
            "draw_curves_oit_pipeline",
            create_curves_oit_pipeline(&self.device, self.color_target_format, self.depth_buffer.format()),
        );
        self.pipelines.draw_curves_oit_v2_pipeline = check(
            "draw_curves_oit_v2_pipeline",
            create_curves_oit_v2_pipeline(&self.device, self.color_target_format, self.depth_buffer.format()),
        );
        self.pipelines.draw_curves_oit_resolve_pipeline = check(
            "draw_curves_oit_resolve_pipeline",
            create_curves_oit_resolve_pipeline(&self.device, self.color_target_format, self.depth_buffer.format()),
        );*/
        //self.pipelines.temporal_average_pipeline = check("temporal_average_pipeline", create_temporal_avg_pipeline(&self.device));

        /*compile_pass_pipeline(
            &self.device,
            &PipelineDescriptor {
                mode: PipelineMode::MeshShading,
                unified_shader_file: "crates/fluff/shaders/curve_binning.glsl",
                defines: vec![
                    ("TILE_SIZE", Some(CURVE_BINNING_TILE_SIZE.to_string())),
                    ("MAX_LINES_PER_TILE", Some(CURVE_BINNING_MAX_LINES_PER_TILE.to_string())),
                ],
            },
        )
        .unwrap()*/
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
                image,
                id: self.brush_textures.len() as u32,
            });
        }
    }

    fn load_geo(&mut self) {
        use rfd::FileDialog;
        let file = FileDialog::new().add_filter("Houdini JSON geometry", &["geo"]).pick_file();

        if let Some(ref file) = file {
            let file_sequence = match resolve_file_sequence(file) {
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
            self.animation = Some(convert_animation_data(&self.device, &geo_files));
        }
    }

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
            mode: RenderMode::BinRasterization,
            temporal_average: false,
            temporal_avg_image,
            frame: 0,
            frame_image,
            temporal_average_alpha: 0.25,
            engine: Engine::new(device.clone()),
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

    pub fn key_input(&mut self, key: &Key, pressed: bool) {
        if *key == Key::Named(NamedKey::F5) && pressed {
            self.reload_shaders();
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

        self.overlay.line(vec3(0.0, 0.0, 0.0), vec3(0.95, 0.0, 0.0), red, red);
        self.overlay.line(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.95, 0.0), green, green);
        self.overlay.line(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.95), blue, blue);

        self.overlay.cone(vec3(0.95, 0.0, 0.0), vec3(1.0, 0.0, 0.0), 0.02, red, red);
        self.overlay.cone(vec3(0.0, 0.95, 0.0), vec3(0.0, 1.0, 0.0), 0.02, green, green);
        self.overlay.cone(vec3(0.0, 0.0, 0.95), vec3(0.0, 0.0, 1.0), 0.02, blue, blue);
    }

    pub fn render(&mut self, cmd: &mut CommandStream, image: &Image) {
        if self.reload_brush_textures {
            self.reload_textures(cmd);
            self.reload_brush_textures = false;
        }

        let width = image.width();
        let height = image.height();

        self.setup(cmd, self.frame_image.clone(), width, height);

        let color_target_view = self.frame_image.create_top_level_view();

        // Draw overlay
        self.overlay.set_camera(self.camera_control.camera());
        self.draw_axes();
        self.draw_curves();
        cmd.debug_group("Overlay", |cmd| {
            self.overlay.render(
                cmd,
                OverlayRenderParams {
                    color_target: color_target_view.clone(),
                    depth_target: self.depth_buffer_view.clone(),
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
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Load .geo...").clicked() {
                        self.load_geo()
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
        });
    }

    pub fn on_exit(&mut self) {}
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

fn create_curves_oit_resolve_pipeline(
    device: &Device,
    target_color_format: Format,
    target_depth_format: Format,
) -> Result<GraphicsPipeline, graal::Error> {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[CurvesOITResolveArguments::LAYOUT],
            push_constants_size: mem::size_of::<CurvesOITResolvePushConstants>(),
        },
        vertex_input: Default::default(),
        pre_rasterization_shaders: PreRasterizationShaders::vertex_shader_from_source_file(Path::new(
            "crates/fluff/shaders/curve_oit_resolve.glsl",
        )),
        rasterization: RasterizationState {
            polygon_mode: PolygonMode::Fill,
            cull_mode: Default::default(),
            front_face: FrontFace::CounterClockwise,
            conservative_rasterization_mode: ConservativeRasterizationMode::Disabled,
            ..Default::default()
        },
        fragment_shader: ShaderEntryPoint::from_source_file(Path::new("crates/fluff/shaders/curve_oit_resolve.glsl")),
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

fn create_temporal_avg_pipeline(device: &Device) -> Result<ComputePipeline, graal::Error> {
    let create_info = ComputePipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[TemporalAverageArguments::LAYOUT],
            push_constants_size: mem::size_of::<TemporalAveragePushConstants>(),
        },
        compute_shader: ShaderEntryPoint {
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/temporal_average.comp"))),
            entry_point: "main",
        },
    };
    device.create_compute_pipeline(create_info)
}
*/
