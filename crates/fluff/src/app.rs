use glam::{vec3, DVec2};
use graal::{
    prelude::*, vk::ImageLayout, Buffer, ComputePipeline, ComputePipelineCreateInfo, ConservativeRasterizationMode, ImageSubresourceLayers,
    Point3D, ReadOnlyStorageBuffer, ReadWriteStorageBuffer, ReadWriteStorageImage, Rect3D,
};
use std::{mem, path::Path};

use houdinio::Geo;
use winit::{
    event::MouseButton,
    keyboard::{Key, NamedKey},
};

use crate::{
    camera_control::CameraControl,
    overlay::{CubicBezierSegment, OverlayRenderer},
    util::resolve_file_sequence,
};

/// Geometry loaded from a `frame####.geo` file.
struct GeoFileData {
    /// The #### in `frame####.geo`.
    index: usize,
    geometry: Geo,
}

/// 3D bezier control point.
type ControlPoint = [f32; 3];

/// Represents a range of control points in the position buffer.
#[derive(Copy, Clone)]
#[repr(C)]
struct ControlPointRange {
    start: u32,
    /// Number of control points in the range.
    ///
    /// Should be 3N+1 for cubic bezier curves.
    count: u32,
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
    position_buffer: TypedBuffer<[ControlPoint]>,
    curve_buffer: TypedBuffer<[ControlPointRange]>,
}

/// Converts bezier curve data from `.geo` files to a format that can be uploaded to the GPU.
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
                houdinio::Primitive::BezierRun(run) => {
                    match run.vertices {
                        houdinio::PrimVar::Uniform(ref u) => {
                            point_count += u.len() * run.count;
                        }
                        houdinio::PrimVar::Varying(ref v) => {
                            point_count += v.iter().map(|v| v.len()).sum::<usize>();
                        }
                    }
                    curve_count += run.count;
                }
            }
        }
    }

    // Curve buffer: contains (start, end) pairs of curves in the point buffer

    let position_buffer = device.create_array_buffer::<ControlPoint>(
        "curve position buffer",
        BufferUsage::STORAGE_BUFFER,
        MemoryLocation::CpuToGpu,
        point_count,
    );
    let curve_buffer =
        device.create_array_buffer::<ControlPointRange>("curve buffer", BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu, curve_count);

    let mut frames = vec![];

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
                                *point_data.offset(point_ptr) = f.geometry.vertex_position(vertex_index);
                                point_ptr += 1;
                            }
                            for segment in curve.vertices.windows(4) {
                                curve_segments.push(CubicBezierSegment {
                                    p0: f.geometry.vertex_position(segment[0]).into(),
                                    p1: f.geometry.vertex_position(segment[1]).into(),
                                    p2: f.geometry.vertex_position(segment[2]).into(),
                                    p3: f.geometry.vertex_position(segment[3]).into(),
                                });
                            }

                            *curve_data.offset(curve_ptr) = ControlPointRange {
                                start: start as u32,
                                count: curve.vertices.len() as u32,
                            };
                            curve_ptr += 1;
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

const BIN_RAST_SAMPLE_COUNT: u32 = 32;
const BIN_RAST_TILE_SIZE: u32 = 8;
const BIN_RAST_MAX_CURVES_PER_TILE: usize = 16;

type CurveIndex = u32;

#[derive(Copy, Clone)]
struct BinRastPushConstants {
    view_proj: glam::Mat4,
    /// Base index into the curve buffer.
    base_curve: u32,
    stroke_width: f32,
    /// Number of tiles in the X direction.
    tile_count_x: u32,
    /// Number of tiles in the Y direction.
    tile_count_y: u32,
}

#[derive(Copy, Clone)]
struct BinRastTile {
    curves: [CurveIndex; BIN_RAST_MAX_CURVES_PER_TILE],
}

#[derive(Arguments)]
struct BinRastArguments<'a> {
    #[argument(binding = 0)]
    position_buffer: ReadOnlyStorageBuffer<'a, [ControlPoint]>,
    #[argument(binding = 1)]
    curve_buffer: ReadOnlyStorageBuffer<'a, [ControlPointRange]>,
    #[argument(binding = 2)]
    tiles_curve_count_image: ReadWriteStorageImage<'a>,
    #[argument(binding = 3)]
    tiles_buffer: ReadWriteStorageBuffer<'a, [BinRastTile]>,
}

#[derive(Arguments)]
struct DrawCurvesArguments<'a> {
    #[argument(binding = 0)]
    position_buffer: ReadOnlyStorageBuffer<'a, [ControlPoint]>,
    #[argument(binding = 1)]
    curve_buffer: ReadOnlyStorageBuffer<'a, [ControlPointRange]>,
    #[argument(binding = 2)]
    tiles_curve_count_image: ReadWriteStorageImage<'a>,
    #[argument(binding = 3)]
    tiles_buffer: ReadWriteStorageBuffer<'a, [BinRastTile]>,
    #[argument(binding = 4)]
    output_image: ReadWriteStorageImage<'a>,
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
}

////////////////////////////////////////////////////////////////////////////////////////////////////
fn create_depth_buffer(device: &Device, width: u32, height: u32) -> Image {
    device.create_image(
        "depth buffer",
        &ImageCreateInfo {
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
        },
    )
}

#[derive(Attachments)]
struct RenderAttachments<'a> {
    #[attachment(color, format=R8G8B8A8_UNORM)]
    color: &'a ImageView,
    #[attachment(depth, format=D32_SFLOAT)]
    depth: &'a ImageView,
}

#[derive(Attachments)]
struct BinRastAttachments<'a> {
    #[attachment(depth, format=D32_SFLOAT)]
    depth: &'a ImageView,
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Default)]
struct Pipelines {
    bin_rast_pipeline: Option<GraphicsPipeline>,
    draw_curves_pipeline: Option<ComputePipeline>,
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
    bin_rast_stroke_width: f32,
    bin_rast_current_frame: usize,

    bin_rast_tiles_x: u32,
    bin_rast_tiles_y: u32,
    bin_rast_tile_curve_count_image: Image,
    bin_rast_tile_buffer: TypedBuffer<[BinRastTile]>,
    curves_image: Image,

    overlay_line_width: f32,
    overlay_filter_width: f32,
}

impl App {
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

        self.pipelines.bin_rast_pipeline = check(
            "bin_rast_pipeline",
            create_bin_rast_pipeline(&self.device, self.color_target_format, self.depth_buffer.format()),
        );
        self.pipelines.draw_curves_pipeline = check("draw_curves_pipeline", create_draw_curves_pipeline(&self.device));
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

    fn render_curves(&mut self, queue: &mut Queue, color_target: &Image, color_target_view: &ImageView) {
        let Some(animation) = self.animation.as_ref() else {
            return;
        };
        let Some(bin_rast_pipeline) = self.pipelines.bin_rast_pipeline.as_ref() else {
            return;
        };

        let frame = &animation.frames[0];

        let width = color_target_view.width();
        let height = color_target_view.height();

        let tile_count_x = (width + BIN_RAST_TILE_SIZE - 1) / BIN_RAST_TILE_SIZE;
        let tile_count_y = (height + BIN_RAST_TILE_SIZE - 1) / BIN_RAST_TILE_SIZE;

        if self.bin_rast_tiles_x != tile_count_x || self.bin_rast_tiles_y != tile_count_y {
            self.bin_rast_tiles_x = tile_count_x;
            self.bin_rast_tiles_y = tile_count_y;
            self.bin_rast_tile_buffer = self.device.create_array_buffer(
                "bin_rast_tile_buffer",
                BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
                MemoryLocation::GpuOnly,
                tile_count_x as usize * tile_count_y as usize,
            );

            self.bin_rast_tile_curve_count_image = self.device.create_image(
                "bin_rast_tile_curve_count",
                &ImageCreateInfo {
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
                },
            );
        }

        let bin_rast_tile_curve_count_image_view = self.bin_rast_tile_curve_count_image.create_top_level_view();

        if width != self.curves_image.width() || height != self.curves_image.height() {
            self.curves_image = self.device.create_image(
                "curves_image",
                &ImageCreateInfo {
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
                },
            );
        }

        unsafe {
            let mut cmdbuf = queue.create_command_buffer();
            cmdbuf.push_debug_group("clear tile buffer");

            // Clear the tile curve count image and the tile buffer
            {
                let mut encoder = cmdbuf.begin_blit();
                encoder.clear_image(&self.bin_rast_tile_curve_count_image, ClearColorValue::Uint([0, 0, 0, 0]));
                encoder.clear_image(&self.curves_image, ClearColorValue::Float([0.0, 0.0, 0.0, 0.0]));
                encoder.fill_buffer(&self.bin_rast_tile_buffer.slice(..).any(), 0);
            }
            cmdbuf.pop_debug_group();

            cmdbuf.push_debug_group("render curves");

            // Render the curves
            {
                let mut encoder = cmdbuf.begin_rendering(&RenderAttachments {
                    color: &color_target_view,
                    depth: &self.depth_buffer_view,
                });

                encoder.bind_graphics_pipeline(bin_rast_pipeline);
                encoder.set_viewport(0.0, 0.0, tile_count_x as f32, tile_count_y as f32, 0.0, 1.0);
                encoder.set_scissor(0, 0, tile_count_x, tile_count_y);
                encoder.bind_arguments(
                    0,
                    &BinRastArguments {
                        position_buffer: animation.position_buffer.as_read_only_storage_buffer(),
                        curve_buffer: animation.curve_buffer.as_read_only_storage_buffer(),
                        tiles_curve_count_image: bin_rast_tile_curve_count_image_view.as_read_write_storage(),
                        tiles_buffer: self.bin_rast_tile_buffer.as_read_write_storage_buffer(),
                    },
                );

                encoder.bind_push_constants(&BinRastPushConstants {
                    view_proj: self.camera_control.camera().view_projection(),
                    base_curve: animation.frames[self.bin_rast_current_frame].curve_range.start,
                    stroke_width: self.bin_rast_stroke_width,
                    tile_count_x,
                    tile_count_y,
                });

                encoder.draw_mesh_tasks(frame.curve_range.count, 1, 1);
            }

            cmdbuf.pop_debug_group();

            cmdbuf.push_debug_group("draw curves");

            {
                let mut encoder = cmdbuf.begin_compute();
                encoder.bind_compute_pipeline(self.pipelines.draw_curves_pipeline.as_ref().unwrap());
                encoder.bind_arguments(
                    0,
                    &DrawCurvesArguments {
                        position_buffer: animation.position_buffer.as_read_only_storage_buffer(),
                        curve_buffer: animation.curve_buffer.as_read_only_storage_buffer(),
                        tiles_curve_count_image: bin_rast_tile_curve_count_image_view.as_read_write_storage(),
                        tiles_buffer: self.bin_rast_tile_buffer.as_read_write_storage_buffer(),
                        output_image: self.curves_image.create_top_level_view().as_read_write_storage(),
                    },
                );
                encoder.bind_push_constants(&DrawCurvesPushConstants {
                    view_proj: self.camera_control.camera().view_projection(),
                    base_curve: animation.frames[self.bin_rast_current_frame].curve_range.start,
                    stroke_width: self.bin_rast_stroke_width,
                    tile_count_x,
                    tile_count_y,
                });
                encoder.dispatch(tile_count_x, tile_count_y, 1);
            }

            cmdbuf.pop_debug_group();

            cmdbuf.push_debug_group("blit curves to screen");

            {
                let mut encoder = cmdbuf.begin_blit();
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
            }

            cmdbuf.pop_debug_group();
            queue.submit([cmdbuf]).expect("submit failed");
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
        let bin_rast_tile_buffer = device.create_array_buffer::<BinRastTile>(
            "bin_rast_tile_buffer",
            BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
            MemoryLocation::GpuOnly,
            1,
        );
        // DUMMY
        let bin_rast_tile_curve_count_image = device.create_image(
            "bin_rast_tile_curve_count",
            &ImageCreateInfo {
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
            },
        );
        // DUMMY
        let curves_image = device.create_image(
            "curves_image",
            &ImageCreateInfo {
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
            },
        );
        let mut app = App {
            device: device.clone(),
            animation: None,
            depth_buffer,
            depth_buffer_view,
            color_target_format,
            camera_control,
            overlay: overlay_renderer,
            pipelines: Default::default(),
            bin_rast_stroke_width: 0.001,
            bin_rast_current_frame: 0,
            bin_rast_tiles_x: 0,
            bin_rast_tiles_y: 0,
            bin_rast_tile_curve_count_image,
            bin_rast_tile_buffer,
            curves_image,
            overlay_line_width: 1.0,
            overlay_filter_width: 1.0,
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

    pub fn ui(&mut self, ui: &mut imgui::Ui) -> bool {
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

        if let Some(ref animation) = self.animation {
            ui.window("Animation").build(|| {
                ui.slider("Stroke width", 0.001, 0.20, &mut self.bin_rast_stroke_width);
                ui.slider("Overlay line width", 0.1, 40.0, &mut self.overlay_line_width);
                ui.slider("Overlay filter width", 0.01, 10.0, &mut self.overlay_filter_width);
                imgui::Drag::new("Frame")
                    .display_format("Frame %d")
                    .range(0, animation.frames.len() - 1)
                    .build(ui, &mut self.bin_rast_current_frame);
            });
        }

        ui.show_metrics_window(&mut true);

        quit
    }

    pub fn draw_curves(&mut self) {
        if let Some(anim_data) = self.animation.as_ref() {
            let frame = &anim_data.frames[self.bin_rast_current_frame];
            for segment in frame.curve_segments.iter() {
                self.overlay.cubic_bezier(segment, [0, 0, 0, 255]);
            }
        }
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

    pub fn render(&mut self, queue: &mut Queue, image: &Image) {
        let color_target_view = image.create_top_level_view();

        // Clear attachments
        {
            let mut command_buffer = queue.create_command_buffer();
            command_buffer.push_debug_group("Clear attachments");
            let mut encoder = command_buffer.begin_rendering(&RenderAttachments {
                color: &color_target_view,
                depth: &self.depth_buffer_view,
            });
            encoder.clear_color(0, ClearColorValue::Float([0.2, 0.4, 0.6, 1.0]));
            encoder.clear_depth(1.0);
            drop(encoder);
            command_buffer.pop_debug_group();
            queue.submit([command_buffer]).expect("submit failed");
        }

        // Render the curves
        self.render_curves(queue, &image, &color_target_view);

        // Draw overlay
        {
            self.overlay.set_camera(self.camera_control.camera());
            self.draw_axes();
            self.draw_curves();

            let mut command_buffer = queue.create_command_buffer();
            command_buffer.push_debug_group("Overlay");
            let mut encoder = command_buffer.begin_rendering(&RenderAttachments {
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
            drop(encoder);
            command_buffer.pop_debug_group();
            queue.submit([command_buffer]).expect("submit failed");
        }

        // 16x16 bin => 8100 tiles
        // 16 curves per tile => 129600 curves
        // 1 curve segment = 1 index into the curve buffer = 4 bytes
    }

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
            task: None,
            mesh: ShaderEntryPoint {
                code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/bin_rast.mesh"))),
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
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/bin_rast.frag"))),
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
            code: ShaderCode::Source(ShaderSource::File(Path::new("crates/fluff/shaders/draw_curves.comp"))),
            entry_point: "main",
        },
    };
    device.create_compute_pipeline(create_info)
}
