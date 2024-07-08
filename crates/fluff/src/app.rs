use egui::{Align2, Frame, Margin, Rounding, Widget};
use glam::{mat4, uvec2, vec2, vec3, vec4, DVec2, Vec2, Vec4};
use graal::{prelude::*, vk::{AttachmentLoadOp, AttachmentStoreOp}, Buffer, BufferRange, ColorAttachment, ComputePipeline, ComputePipelineCreateInfo, DepthStencilAttachment, ImageCopyBuffer, ImageCopyView, ImageDataLayout, ImageSubresourceLayers, ImageView, Point3D, Rect3D, RenderPassDescriptor, Descriptor};
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
    engine2::{BufferKey, Engine, ImageDesc, ImageKey, MeshShadingPassDesc},
    overlay::{CubicBezierSegment, OverlayRenderer},
    ui,
    ui::{resources_window, test_ui},
    util::resolve_file_sequence,
};
use crate::engine2::ComputePassDesc;

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

        let mut encoder = cmd.begin_blit();
        encoder.copy_buffer_to_image(
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
    }

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
    /// Should be 3N+1 for cubic bezier curves.
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
    let curve_buffer = device.create_array_buffer::<CurveDesc>(BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu, curve_count);

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

#[derive(Arguments)]
struct BinRastArguments<'a> {
    #[argument(binding = 0, storage, read_only)]
    position_buffer: BufferRange<[ControlPoint]>,
    #[argument(binding = 1, storage, read_only)]
    curve_buffer: BufferRange<[CurveDesc]>,
    #[argument(binding = 2, storage_image, read_write)]
    tile_line_count_image: &'a ImageView,
    #[argument(binding = 3, storage, read_write)]
    tile_buffer: BufferRange<[BinRastTile]>,
}

#[derive(Arguments)]
struct DrawCurvesArguments<'a> {
    #[argument(binding = 0, storage, read_only)]
    position_buffer: BufferRange<[ControlPoint]>,
    #[argument(binding = 1, storage, read_only)]
    curve_buffer: BufferRange<[CurveDesc]>,
    #[argument(binding = 2, storage_image, read_write)]
    tile_line_count_image: &'a ImageView,
    #[argument(binding = 3, storage)]
    tile_buffer: BufferRange<[BinRastTile]>,
    #[argument(binding = 4, storage_image, read_write)]
    output_image: &'a ImageView,
}

#[derive(Copy, Clone)]
struct DrawCurvesPushConstants {
    view_proj: glam::Mat4,
    /// Base index into the curve buffer.
    base_curve: u32,
    stroke_width: f32,
    /// Number of tiles in the X direction.
    tile_count_x: u32,
    /// Number of tiles in the Y direction.
    tile_count_y: u32,
    frame: i32,
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

#[derive(Arguments)]
struct CurvesOITArguments<'a> {
    #[argument(binding = 0, storage)]
    position_buffer: BufferRange<[ControlPoint]>,
    #[argument(binding = 1, storage)]
    curve_buffer: BufferRange<[CurveDesc]>,
    #[argument(binding = 2, storage)]
    fragment_buffer: BufferRange<[CurvesOITFragmentData]>,
    #[argument(binding = 3, storage)]
    fragment_count_buffer: BufferRange<[u32]>,
    #[argument(binding = 4, sampled_image)]
    brush_texture: &'a ImageView,
    #[argument(binding = 5, sampler)]
    brush_sampler: &'a Sampler,
}

#[derive(Arguments)]
struct CurvesOITResolveArguments<'a> {
    #[argument(binding = 0, storage)]
    fragment_buffer: BufferRange<[CurvesOITFragmentData]>,
    #[argument(binding = 1, storage)]
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

/*
#[derive(Attachments)]
struct TemporalAverageAttachments<'a> {
    #[attachment(color, format = R16G16B16A16_SFLOAT)]
    color: &'a ImageView,
    #[attachment(depth, format = D32_SFLOAT)]
    depth: &'a ImageView,
}*/

#[derive(Arguments)]
struct TemporalAverageArguments<'a> {
    #[argument(binding = 0, storage_image)]
    new_frame: &'a ImageView,
    #[argument(binding = 1, storage_image, read_write)]
    accum: &'a ImageView,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct TemporalAveragePushConstants {
    viewport_size: glam::UVec2,
    frame: i32,
    falloff: f32,
}

////////////////////////////////////////////////////////////////////////////////////////////////////
fn create_depth_buffer(device: &Device, width: u32, height: u32) -> Image {
    device.create_image(&ImageCreateInfo {
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
    })
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
    tile_count_x: u32,
    tile_count_y: u32,
    tile_line_count_image: Image,
    tile_buffer: Buffer<[BinRastTile]>,
    curves_image: Image,

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

const CONTROL_POINT_BUFFER: BufferKey = BufferKey("b_controlPoints");
const CURVE_BUFFER: BufferKey = BufferKey("b_curves");
const TILE_LINE_COUNT_IMAGE: ImageKey = ImageKey("i_tileLineCount");
const DRAW_CURVES_OUTPUT_IMAGE: ImageKey = ImageKey("i_drawCurvesOutput");
const TEMPORAL_AVG_OUTPUT_IMAGE: ImageKey = ImageKey("i_temporalAvg");

const PASS_CURVE_BINNING: &str = "curve_binning";
const PASS_DRAW_CURVES: &str = "draw_curves";
const PASS_TEMPORAL_AVERAGE: &str = "temporal_average";

impl App {
    fn setup(&mut self, screen_width: u32, screen_height: u32) {
        let engine = &mut self.engine;
        engine.reset();

        let tile_count_x = screen_width.div_ceil(CURVE_BINNING_TILE_SIZE);
        let tile_count_y = screen_height.div_ceil(CURVE_BINNING_TILE_SIZE);

        engine.define_global("TILE_SIZE", CURVE_BINNING_TILE_SIZE.to_string());

        if let Some(ref animation) = self.animation {
            engine.import_buffer(CONTROL_POINT_BUFFER, animation.position_buffer.clone());
            engine.import_buffer(CURVE_BUFFER, animation.curve_buffer.clone());
        }

        engine.define_image(
            TILE_LINE_COUNT_IMAGE,
            ImageDesc {
                format: Format::R32_SINT,
                width: tile_count_x,
                height: tile_count_y,
            },
        );

        if self.temporal_average {
            engine.define_image(
                "i_temporalAvg",
                ImageDesc {
                    format: Format::R16G16B16A16_SFLOAT,
                    width: screen_width,
                    height: screen_height,
                },
            );
        }

        engine.mesh_shading_pass(
            PASS_CURVE_BINNING,
            MeshShadingPassDesc {
                shader: PathBuf::from("crates/fluff/shaders/curve_binning_stripped.glsl"),
                defines: Default::default(),
                color_attachments: vec![],
                depth_stencil_attachment: None,
                rasterization_state: Default::default(),
                depth_stencil_state: Default::default(),
                multisample_state: Default::default(),
                color_target_states: vec![],
                draw: (),
            },
        );

        engine.compute_pass(
            PASS_DRAW_CURVES,
            ComputePassDesc {
                shader: PathBuf::from("crates/fluff/shaders/draw_curves.glsl"),
                defines: Default::default(),
                dispatch: (tile_count_x, tile_count_y, 1),
            },
        );

        // temporal average
        engine.compute_pass(
            PASS_TEMPORAL_AVERAGE,
            ComputePassDesc {
                shader: PathBuf::from("crates/fluff/shaders/temporal_average.glsl"),
                defines: Default::default(),
                dispatch: (screen_width.div_ceil(16), screen_height.div_ceil(16), 1),
            },
        );


        //engine.forward_image(DRAW_CURVES_OUTPUT_IMAGE, TEMPORAL_AVG_OUTPUT_IMAGE);


        // split pass definition from pass order?
        // this way it's possible to enable/disable passes without recompiling everything
        // alternatively, enable/disable passes instead of respecifying the order
        // problem: not enough to disable passes, must also forward resources
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

        self.pipelines.curve_binning_pipeline = check(
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
        );
        self.pipelines.temporal_average_pipeline = check("temporal_average_pipeline", create_temporal_avg_pipeline(&self.device));

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

    fn load_config(&mut self) {
        use rfd::FileDialog;
        let file = FileDialog::new().add_filter("Render configuration file", &["lua"]).pick_file();
        if let Some(ref file) = file {
            let r = self.engine.load_config_file(file);
            match r {
                Ok(_) => {
                    eprintln!("Loaded config file: `{}`", file.display());
                }
                Err(err) => {
                    eprintln!("Error loading config file: {}", err);
                }
            }
        }
    }

    fn reload_config(&mut self) {
        let r = self.engine.reload_config_file();
        match r {
            Ok(_) => {
                eprintln!("Reloaded config file");
            }
            Err(err) => {
                eprintln!("Error loading config file: {}", err);
            }
        }
    }

    fn render_curve_bins(&mut self, cmd: &mut CommandStream) {
        let color_target = &self.frame_image;
        let color_target_view = &self.frame_image.create_top_level_view();

        let Some(animation) = self.animation.as_ref() else {
            return;
        };
        let Some(bin_rast_pipeline) = self.pipelines.curve_binning_pipeline.as_ref() else {
            return;
        };

        let frame = &animation.frames[self.bin_rast_current_frame];
        let width = color_target_view.width();
        let height = color_target_view.height();

        let tile_count_x = (width + CURVE_BINNING_TILE_SIZE - 1) / CURVE_BINNING_TILE_SIZE;
        let tile_count_y = (height + CURVE_BINNING_TILE_SIZE - 1) / CURVE_BINNING_TILE_SIZE;

        if self.tile_count_x != tile_count_x || self.tile_count_y != tile_count_y {
            self.tile_count_x = tile_count_x;
            self.tile_count_y = tile_count_y;
            self.tile_buffer = self.device.create_array_buffer(
                BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
                MemoryLocation::GpuOnly,
                tile_count_x as usize * tile_count_y as usize,
            );

            self.tile_line_count_image = self.device.create_image(&ImageCreateInfo {
                memory_location: MemoryLocation::GpuOnly,
                type_: ImageType::Image2D,
                usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_DST | ImageUsage::TRANSFER_SRC,
                format: Format::R32_SINT,
                width: tile_count_x,
                height: tile_count_y,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                samples: 1,
            });
        }

        let tile_line_count_image_view = self.tile_line_count_image.create_top_level_view();

        if width != self.curves_image.width() || height != self.curves_image.height() {
            self.curves_image = self.device.create_image(&ImageCreateInfo {
                memory_location: MemoryLocation::GpuOnly,
                type_: ImageType::Image2D,
                usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_DST | ImageUsage::TRANSFER_SRC,
                format: Format::R8G8B8A8_UNORM,
                width,
                height,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                samples: 1,
            });
        }

        unsafe {
            cmd.debug_group("clear tile buffer", |cmd| {
                let mut encoder = cmd.begin_blit();
                encoder.clear_image(&self.tile_line_count_image, ClearColorValue::Uint([0, 0, 0, 0]));
                encoder.clear_image(&self.curves_image, ClearColorValue::Float([0.0, 0.0, 0.0, 0.0]));
                encoder.fill_buffer(&self.tile_buffer.slice(..).untyped, 0);
            });

            cmd.debug_group("render curves", |cmd| {
                let mut encoder = cmd.begin_rendering(
                    &[ColorAttachment {
                        image_view: color_target_view.clone(),
                        load_op: AttachmentLoadOp::LOAD,
                        store_op: AttachmentStoreOp::STORE,
                        clear_value: [0.0, 0.0, 0.0, 0.0],
                    }],
                    Some(DepthStencilAttachment {
                        image_view: self.depth_buffer_view.clone(),
                        depth_load_op: AttachmentLoadOp::LOAD,
                        depth_store_op: AttachmentLoadOp::STORE,
                        stencil_load_op: Default::default(),
                        stencil_store_op: Default::default(),
                        depth_clear_value: 0.0,
                        stencil_clear_value: 0,
                    }),
                );

                encoder.bind_graphics_pipeline(bin_rast_pipeline);
                encoder.set_viewport(
                    0.0,
                    0.0,
                    width as f32 / CURVE_BINNING_TILE_SIZE as f32,
                    height as f32 / CURVE_BINNING_TILE_SIZE as f32,
                    0.0,
                    1.0,
                );
                encoder.set_scissor(0, 0, tile_count_x, tile_count_y);
                encoder.push_descriptors(
                    0,
                    &BinRastArguments {
                        position_buffer: animation.position_buffer.slice(..),
                        curve_buffer: animation.curve_buffer.slice(..),
                        tile_line_count_image: &tile_line_count_image_view,
                        tile_buffer: self.tile_buffer.slice(..),
                    },
                );

                encoder.push_constants(&BinRastPushConstants {
                    view_proj: self.camera_control.camera().view_projection(),
                    base_curve_index: animation.frames[self.bin_rast_current_frame].curve_range.start,
                    stroke_width: self.bin_rast_stroke_width,
                    tile_count_x,
                    tile_count_y,
                    viewport_size: uvec2(width, height),
                    curve_count: frame.curve_range.count,
                    frame: self.frame,
                });

                encoder.draw_mesh_tasks(frame.curve_range.count.div_ceil(64), 1, 1);
                encoder.finish();
            });

            cmd.debug_group("draw curves", |cmd| {
                let mut encoder = cmd.begin_compute();
                encoder.bind_compute_pipeline(self.pipelines.draw_curves_pipeline.as_ref().unwrap());
                encoder.bind_arguments(
                    0,
                    &DrawCurvesArguments {
                        position_buffer: animation.position_buffer.slice(..),
                        curve_buffer: animation.curve_buffer.slice(..),
                        tile_line_count_image: &tile_line_count_image_view,
                        tile_buffer: self.tile_buffer.slice(..),
                        output_image: &self.curves_image.create_top_level_view(),
                    },
                );
                encoder.bind_push_constants(&DrawCurvesPushConstants {
                    view_proj: self.camera_control.camera().view_projection(),
                    base_curve: animation.frames[self.bin_rast_current_frame].curve_range.start,
                    stroke_width: self.bin_rast_stroke_width,
                    tile_count_x,
                    tile_count_y,
                    frame: self.frame,
                });
                encoder.dispatch(tile_count_x, tile_count_y, 1);
            });

            cmd.debug_group("blit curves to screen", |cmd| {
                let mut encoder = cmd.begin_blit();
                encoder.blit_image(
                    &self.curves_image,
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
                    &color_target,
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
        }
    }

    fn render_curves_oit(&mut self, cmd: &mut CommandStream) {
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
                },
                );

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
                        (5, self.device.create_sampler(&SamplerCreateInfo {
                            address_mode_u: vk::SamplerAddressMode::REPEAT,
                            address_mode_v: vk::SamplerAddressMode::REPEAT,
                            address_mode_w: vk::SamplerAddressMode::REPEAT,
                            ..Default::default()
                        }).descriptor())
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
        // DUMMY
        let bin_rast_tile_buffer =
            device.create_array_buffer::<BinRastTile>(BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST, MemoryLocation::GpuOnly, 1);
        // DUMMY
        let bin_rast_tile_curve_count_image = device.create_image(&ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_DST,
            format: Format::R32_SINT,
            width: 1,
            height: 1,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
        });
        // DUMMY
        let curves_image = device.create_image(&ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC | ImageUsage::TRANSFER_DST,
            format: Format::R8G8B8A8_SRGB,
            width: 1,
            height: 1,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
        });
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
        oit_fragment_buffer.untyped.set_label("oit_fragment_buffer");
        // DUMMY
        let oit_fragment_count_buffer =
            device.create_array_buffer::<u32>(BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST, MemoryLocation::GpuOnly, 1);
        oit_fragment_count_buffer.untyped.set_label("oit_fragment_count_buffer");

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
            tile_count_x: 0,
            tile_count_y: 0,
            tile_line_count_image: bin_rast_tile_curve_count_image,
            tile_buffer: bin_rast_tile_buffer,
            curves_image,
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
            engine: Engine::new(device.clone(), mem::size_of::<BinRastPushConstants>()),
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

    fn render_temporal_average(&mut self, cmd: &mut CommandStream) {
        let width = self.frame_image.width();
        let height = self.frame_image.height();

        let Some(ref pipeline) = self.pipelines.temporal_average_pipeline else {
            return;
        };

        // Resolve pass
        cmd.debug_group("temporal average", |cmd| {
            let tile_count_x = width.div_ceil(16);
            let tile_count_y = height.div_ceil(16);

            let mut encoder = cmd.begin_compute();

            encoder.bind_compute_pipeline(pipeline);
            encoder.bind_arguments(
                0,
                &TemporalAverageArguments {
                    new_frame: &self.frame_image.create_top_level_view(),
                    accum: &self.temporal_avg_image.create_top_level_view(),
                },
            );
            encoder.bind_push_constants(&TemporalAveragePushConstants {
                viewport_size: uvec2(width, height),
                frame: self.frame,
                falloff: self.temporal_average_alpha,
            });

            encoder.dispatch(tile_count_x, tile_count_y, 1);
        });
    }

    pub fn render(&mut self, cmd: &mut CommandStream, image: &Image) {
        engine.import_image(FRAME_IMAGE, &image);

        let width = image.width();
        let height = image.height();

        if self.reload_brush_textures {
            self.reload_textures(cmd);
            self.reload_brush_textures = false;
        }

        let color_target_view = self.frame_image.create_top_level_view();

        // Clear attachments
        cmd.debug_group("Clear attachments", |cmd| {
            let mut encoder = cmd.begin_rendering(&RenderAttachments {
                color: &color_target_view,
                depth: &self.depth_buffer_view,
            });
            encoder.clear_color(0, ClearColorValue::Float([0.0, 0.0, 0.0, 1.0]));
            encoder.clear_depth(1.0);
            encoder.finish();
        });

        // Render the curves
        match self.mode {
            RenderMode::BinRasterization => self.render_curve_bins(cmd),
            RenderMode::CurvesOIT | RenderMode::CurvesOITv2 => self.render_curves_oit(cmd),
        }

        self.overlay.set_camera(self.camera_control.camera());
        self.draw_axes();
        self.draw_curves();
        cmd.debug_group("Overlay", |cmd| {
            let mut encoder = cmd.begin_rendering(&RenderAttachments {
                color: &color_target_view,
                depth: &self.depth_buffer_view,
            });
            self.overlay.render(
                image.width(),
                image.height(),
                self.overlay_line_width,
                self.overlay_filter_width,
                &mut encoder,
            );
            encoder.finish();
        });

        if self.temporal_average {
            self.render_temporal_average(cmd);
        }

        // blit next frame to screen
        cmd.debug_group("blit final frame", |cmd| {
            let mut encoder = cmd.begin_blit();
            encoder.blit_image(
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
                    if ui.button("Load render config").clicked() {
                        self.load_config()
                    }
                    if ui.button("Reload render config").clicked() {
                        self.reload_config()
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

        resources_window(ctx);

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

            ui::test_ui(ui);
        });
    }

    /*pub fn ui(&mut self, ui: &mut imgui::Ui) -> bool {
        ui.show_demo_window(&mut true);

        let mut quit = false;
        ui.main_menu_bar(|| {
            ui.menu("File", || {
                if ui.menu_item("Load .geo...") {
                    self.load_geo()
                }

                if ui.menu_item("Quit") {
                    quit = true;
                }
            });
        });

        if ui.button("Reload brush textures") {
            self.reload_brush_textures = true;
        }

        ui.checkbox("Temporal average", &mut self.temporal_average);
        if self.temporal_average {
            ui.slider("Alpha", 0.0, 1.0, &mut self.temporal_average_alpha);
        }

        if let Some(ref animation) = self.animation {
            ui.window("Animation").build(|| {
                ui.slider("Stroke width", 0.1, 40.0, &mut self.bin_rast_stroke_width);
                ui.slider("OIT Stroke width", 0.1, 40.0, &mut self.oit_stroke_width);
                ui.slider("Overlay line width", 0.1, 40.0, &mut self.overlay_line_width);
                ui.slider("Overlay filter width", 0.01, 10.0, &mut self.overlay_filter_width);
                let mut mode = match self.mode {
                    RenderMode::BinRasterization => 0,
                    RenderMode::CurvesOIT => 1,
                    RenderMode::CurvesOITv2 => 2,
                };
                ui.combo_simple_string("Render mode", &mut mode, &["Bin rasterization", "Curves OIT", "Curves OIT v2"]);
                self.mode = match mode {
                    0 => RenderMode::BinRasterization,
                    1 => RenderMode::CurvesOIT,
                    2 => RenderMode::CurvesOITv2,
                    _ => unreachable!(),
                };

                let current_brush = self
                    .brush_textures
                    .get(self.selected_brush)
                    .map(|b| b.name.as_str())
                    .unwrap_or("<choose...>");
                if let Some(cb) = ui.begin_combo("Brush texture", current_brush) {
                    for (i, brush) in self.brush_textures.iter().enumerate() {
                        let clicked = ui.selectable_config(&brush.name).selected(self.selected_brush == i).build();
                        if self.selected_brush == i {
                            ui.set_item_default_focus();
                        }
                        if clicked {
                            self.selected_brush = i;
                        }
                    }
                    cb.end();
                }

                imgui::Drag::new("Frame")
                    .display_format("Frame %d")
                    .range(0, animation.frames.len() - 1)
                    .build(ui, &mut self.bin_rast_current_frame);

                test_ui(ui);
            });
        }

        ui.show_metrics_window(&mut true);

        quit
    }*/

    pub fn on_exit(&mut self) {}
}

////////////////////////////////////////////////////////////////////////////////////////////////////

fn create_bin_rast_pipeline(
    device: &Device,
    target_color_format: Format,
    target_depth_format: Format,
) -> Result<GraphicsPipeline, graal::Error> {
    let create_info = GraphicsPipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[BinRastArguments::LAYOUT],
            push_constants_size: mem::size_of::<BinRastPushConstants>(),
        },
        vertex_input: Default::default(),
        pre_rasterization_shaders: PreRasterizationShaders::MeshShading {
            task: Some(ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/curve_binning.glsl"))),
                entry_point: "main",
            }),
            mesh: ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/curve_binning.glsl"))),
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
        fragment_shader: ShaderEntryPoint {
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/curve_binning.glsl"))),
            entry_point: "main",
        },
        depth_stencil: DepthStencilState {
            depth_write_enable: true,
            depth_compare_op: CompareOp::LessOrEqual,
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

fn create_draw_curves_pipeline(device: &Device) -> Result<ComputePipeline, graal::Error> {
    let create_info = ComputePipelineCreateInfo {
        layout: PipelineLayoutDescriptor {
            arguments: &[DrawCurvesArguments::LAYOUT],
            push_constants_size: mem::size_of::<DrawCurvesPushConstants>(),
        },
        compute_shader: ShaderEntryPoint {
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/curve_binning_render.comp"))),
            entry_point: "main",
        },
    };
    device.create_compute_pipeline(create_info)
}

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
