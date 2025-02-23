use crate::shaders::TemporalAverageParams;
use egui::{color_picker::{color_edit_button_rgb, color_edit_button_srgba, Alpha}, Align2, Color32, DragValue, FontId, Frame, Key, Margin, Modifiers, Response, Rounding, Slider, Ui, Widget, TextureHandle};
use egui_extras::{Column, TableBuilder};
use glam::{dvec2, dvec3, mat4, uvec2, vec2, vec3, vec4, DVec2, DVec3, DVec4, Vec2, Vec3Swizzles, Vec4Swizzles, Vec3};
use graal::{prelude::*, vk::{AttachmentLoadOp, AttachmentStoreOp}, Barrier, Buffer, BufferRange, ColorAttachment, ComputePipeline, ComputePipelineCreateInfo, DepthStencilAttachment, Descriptor, DeviceAddress, ImageAccess, ImageCopyBuffer, ImageCopyView, ImageDataLayout, ImageSubresourceLayers, ImageView, Point3D, Rect3D, RenderPassInfo, Texture2DHandleRange, ImageHandle};
use std::{
    collections::BTreeMap,
    fs, mem,
    path::{Path, PathBuf},
    ptr,
};
use std::time::Instant;
use egui::ImageData::Color;
use tracing::{error, info, trace, warn};

use houdinio::Geo;
use rand::{random, thread_rng, Rng};
use uniform_cubic_splines::{spline, spline_inverse};
use uniform_cubic_splines::basis::CatmullRom;
//use splines::Spline;
use winit::{
    event::{MouseButton, TouchPhase},
    keyboard::NamedKey,
};

use crate::{
    camera_control::CameraControl,
    engine::{Error},
    overlay::{CubicBezierSegment, OverlayRenderParams, OverlayRenderer},
    shaders,
    shaders::{
        ControlPoint, CurveDesc, DrawCurvesPushConstants, TileData,
    },
    util::resolve_file_sequence,
};
use crate::engine::PipelineCache;
use crate::util::AppendBuffer;
use crate::shaders::{Stroke, StrokeVertex, SUBGROUP_SIZE};
use crate::scene::{Scene, load_stroke_animation_data};
use crate::ui::{curve_editor_button, icon_button};


////////////////////////////////////////////////////////////////////////////////////////////////////

/// Loads a grayscale image into a R8_SRGB buffer.
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
        ptr::copy_nonoverlapping(gray_image.as_raw().as_ptr(), staging_buffer.as_mut_ptr(), byte_size as usize);
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
    image_view: ImageView,
    sat_image: Image,
    sat_image_view: ImageView,
    id: u32,
}

#[derive(Copy, Clone)]
struct PenSample {
    position: DVec2,
    pressure: f64,
    arc_length: f64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Tweak {
    name: String,
    value: String,
    enabled: bool,
    autofocus: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CubicCurve {
    knots: Vec<f64>,
    values: Vec<f64>,
}

impl CubicCurve {
    pub fn sample(&self, t: f64) -> f64 {
        let t = spline_inverse::<CatmullRom, _>(t, &self.knots, None, None).unwrap_or_default();
        spline::<CatmullRom, _, _>(t, &self.values)
    }
}

impl Default for CubicCurve {
    fn default() -> Self {
        Self {
            knots: vec![0.0, 0.0, 0.33, 0.66, 1.0, 1.0],
            values: vec![0.0, 0.0, 0.33, 0.66, 1.0, 1.0],
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedSettings {
    tweaks: Vec<Tweak>,
    last_geom_file: Option<PathBuf>,
    pressure_response_curve: CubicCurve,
}

impl Default for SavedSettings {
    fn default() -> Self {
        //use splines::Key;
        //use splines::Interpolation::CatmullRom;
        Self {
            tweaks: vec![],
            last_geom_file: None,
            pressure_response_curve: Default::default(),
        }
    }
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
    global_shader_macros: BTreeMap<String, String>,
    pcache: PipelineCache,

    animation: Option<Scene>,

    frame: i32,
    mode: RenderMode,
    temporal_average: bool,
    temporal_average_alpha: f32,
    frame_image: Image,
    temporal_avg_image: Image,
    debug_tile_line_overflow: bool,
    start_time: Instant,
    frame_start_time: Instant,

    // Bin rasterization
    bin_rast_stroke_width: f32,
    current_frame: usize,

    // Curves OIT
    oit_stroke_width: f32,
    oit_max_fragments_per_pixel: u32,

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
    draw_origin: glam::Vec2,
    fit_tolerance: f64,
    curve_embedding_factor: f64,
    stroke_bleed_exp: f32,
    stroke_color: Color32,
    background_color: Color32,
    width_profile_pos: glam::Vec4,
    width_profile: glam::Vec4,
    opacity_profile_pos: glam::Vec4,
    opacity_profile: glam::Vec4,
    opacity_response_curve: CubicCurve,

}

impl App {
    /*fn compute_sats(&mut self, cmd: &mut CommandStream) -> Result<(), Error> {
        let sat_shader = PathBuf::from("crates/fluff/shaders/sat.glsl");
        let sat_32x32 = self.pcache.create_compute_pipeline(
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
        let sat_256x256 = self.engine.create_compute_pipeline(
            "sat_256x256",
            ComputePipelineDesc {
                shader: sat_shader.clone(),
                defines: [("SAT_LOG2_SIZE".to_string(), "9".to_string())].into(),
            },
        )?;

        for (i, brush) in self.brush_textures.iter().enumerate() {
            cmd.barrier(Barrier::new()
                .shader_read_image(&brush.sat_image)
                .shader_write_image(&brush.sat_image));

            let sat_pipeline = match brush.image.width() {
                32 => &sat_32x32,
                64 => &sat_64x64,
                128 => &sat_128x128,
                256 => &sat_256x256,
                _ => {
                    warn!("`{}`: unsupported brush texture size: {}×{}", brush.name, brush.image.width(), brush.image.height());
                    continue;
                }
            };
            if brush.image.width() != brush.image.height() {
                warn!("`{}`: brush texture must be square", brush.name);
                continue;
            }

            //let image_view = brush.image.create_top_level_view();
            //let sat_image_view = brush.sat_image.create_top_level_view();
            //cmd.reference_resource(&image_view);
            //cmd.reference_resource(&sat_image_view);

            let mut encoder = cmd.begin_compute();
            encoder.bind_compute_pipeline(sat_pipeline);
            encoder.push_constants(&SummedAreaTableParams {
                pass: 0,
                input_image: brush.image_view.device_image_handle(),
                output_image: brush.sat_image_view.device_image_handle(),
            });
            encoder.dispatch(brush.image.height(), 1, 1);
            /*encoder.barrier(Barrier::new().shader_storage_read().shader_storage_write());
            encoder.push_constants(&SummedAreaTableParams {
                pass: 1,
                input_image: image_view.device_image_handle(),
                output_image: sat_image_view.device_image_handle(),
            });
            encoder.dispatch(brush.image.width(), 1, 1);*/
            encoder.finish();
            cmd.barrier(Barrier::new().shader_storage_read().shader_storage_write());
        }

        Ok(())
    }*/


    fn setup(&mut self, cmd: &mut CommandStream, color_target: Image, width: u32, height: u32) -> Result<(), Error> {
        //let engine = &mut self.engine;

        let Some(ref animation) = self.animation else { return Ok(()) };
        let anim_frame = &animation.frames[self.current_frame];
        let curve_count = anim_frame.curve_range.count;
        let base_curve_index = anim_frame.curve_range.start;
        let frame = self.current_frame as u32;
        let stroke_width = self.bin_rast_stroke_width;
        let viewport_size = [width, height];
        let temporal_average_falloff = self.temporal_average_alpha;
        let debug_tile_line_overflow = self.debug_tile_line_overflow;

        //let tile_count_x = width.div_ceil(BINNING_TILE_SIZE);
        //let tile_count_y = height.div_ceil(BINNING_TILE_SIZE);
        //engine.define_global("TILE_SIZE", CURVE_BINNING_TILE_SIZE.to_string());

        let time = (self.frame_start_time - self.start_time).as_secs_f32();

        let camera = self.camera_control.camera();
        let scene_params = shaders::SceneParams {
            view: camera.view.to_cols_array_2d(),
            proj: camera.projection.to_cols_array_2d(),
            view_proj: camera.view_projection().to_cols_array_2d(),
            eye: self.camera_control.eye().as_vec3(),
            // TODO frustum parameters
            near_clip: camera.frustum.near_plane,
            far_clip: camera.frustum.far_plane,
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
            viewport_size: viewport_size.into(),
            cursor_pos: Default::default(),
            time,
        };


        // FIXME: can we maybe make `STORAGE_BUFFER` a bit less screaming?
        let scene_params_buf = self.device.upload(BufferUsage::STORAGE_BUFFER, &scene_params);
        cmd.reference_resource(&scene_params_buf);

        // FIXME: importing image sets will mean finding a contiguous range of free image handles
        // or we can pass the image view handles in an array, at the cost of another indirection
        //let brush_textures = rg.import_image_set(self.brush_textures.iter().map(|b| b.image.clone()));

        ////////////////////////////////////////////////////////////
        // Curve binning
        //let tile_line_count_buffer = self.device.create_array_buffer::<u32>(
        //    BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
        //    MemoryLocation::GpuOnly,
        //    tile_count_x as usize * tile_count_y as usize,
        //);
        //let tile_buffer = self.device.create_array_buffer::<TileData>(
        //    BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
        //    MemoryLocation::GpuOnly,
        //    tile_count_x as usize * tile_count_y as usize,
        //);


        let brush_texture_handles: Vec<_> = self.brush_textures.iter().map(|b| b.image_view.device_image_handle()).collect();
        let brush_textures = self.device.upload_array_buffer(
            BufferUsage::STORAGE_BUFFER,
            &brush_texture_handles,
        );

        // TODO: consider allocating top-level image views alongside the image itself
        let color_target_view = color_target.create_top_level_view();
        let depth_target_view = self.depth_buffer.create_top_level_view();
        let temporal_avg_view = self.temporal_avg_image.create_top_level_view();

        // pipelines
        //let curve_binning_pipeline = engine.create_mesh_render_pipeline(
        //    "curve_binning",
        //    // TODO: in time, all of this will be moved to hot-reloadable config files
        //    MeshRenderPipelineDesc {
        //        task_shader: PathBuf::from("crates/fluff/shaders/bin_curves.task"),
        //        mesh_shader: PathBuf::from("crates/fluff/shaders/bin_curves.mesh"),
        //        fragment_shader: PathBuf::from("crates/fluff/shaders/bin_curves.frag"),
        //        defines: Default::default(),
        //        color_targets: vec![ColorTargetState {
        //            format: Format::R16G16B16A16_SFLOAT,
        //            ..Default::default()
        //        }],
        //        rasterization_state: Default::default(),
        //        depth_stencil_state: Some(DepthStencilState {
        //            format: Format::D32_SFLOAT,
        //            depth_write_enable: true,
        //            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
        //            stencil_state: StencilState::default(),
        //        }),
        //        multisample_state: Default::default(),
        //    },
        //)?;

        //let draw_curves_pipeline = engine.create_compute_pipeline(
        //    "draw_curves",
        //    ComputePipelineDesc {
        //        shader: shaders::DRAW_CURVES, //PathBuf::from("crates/fluff/shaders/draw_curves.comp"),
        //        defines: Default::default(),
        //    },
        //)?;

        let temporal_average_pipeline = self.pcache.create_compute_pipeline(
            "temporal_average",
            &shaders::TEMPORAL_AVERAGE,
        )?;

        //let draw_strokes_pipeline = engine.create_mesh_render_pipeline(
        //    "draw_strokes",
        //    MeshRenderPipelineDesc {
        //        task_shader: PathBuf::from("crates/fluff/shaders/strokes.glsl"),
        //        mesh_shader: PathBuf::from("crates/fluff/shaders/strokes.glsl"),
        //        fragment_shader: PathBuf::from("crates/fluff/shaders/strokes.glsl"),
        //        defines: Default::default(),
        //        color_targets: vec![ColorTargetState {
        //            format: Format::R16G16B16A16_SFLOAT,
        //            blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
        //            ..Default::default()
        //        }],
        //        rasterization_state: Default::default(),
        //        depth_stencil_state: Some(DepthStencilState {
        //            format: Format::D32_SFLOAT,
        //            depth_write_enable: false,
        //            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
        //            stencil_state: StencilState::default(),
        //        }),
        //        multisample_state: Default::default(),
        //    },
        //)?;

        //////////////////////////////////////////
        cmd.reference_resource(&brush_textures);

        /*match self.mode {
            RenderMode::BinRasterization => {
                cmd.fill_buffer(&tile_line_count_buffer.untyped.byte_range(..), 0);
                cmd.fill_buffer(&tile_buffer.untyped.byte_range(..), 0);

                cmd.barrier(Barrier::new().shader_storage_write());

                // curve binning
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
                encoder.draw_mesh_tasks(curve_count.div_ceil(BINPACK_SUBGROUP_SIZE), 1, 1);
                encoder.finish();

                cmd.barrier(Barrier::new().shader_storage_read().shader_write_image(&color_target));

                let mut encoder = cmd.begin_compute();
                encoder.bind_compute_pipeline(&draw_curves_pipeline);
                encoder.push_constants(&DrawCurvesPushConstants {
                    control_points: animation.position_buffer.device_address(),
                    curves: animation.curve_buffer.device_address(),
                    //view_proj,
                    scene_params: scene_params_buf.device_address(),
                    base_curve_index,
                    stroke_width,
                    tile_count_x,
                    tile_count_y,
                    frame,
                    tile_data: tile_buffer.device_address(),
                    tile_line_count: tile_line_count_buffer.device_address(),
                    brush_textures: brush_textures.device_address(),
                    output_image: color_target_view.device_image_handle(),
                    debug_overflow: debug_tile_line_overflow as u32,
                    stroke_bleed_exp: self.stroke_bleed_exp,
                });
                encoder.dispatch(tile_count_x, tile_count_y * (BINNING_TILE_SIZE / DRAW_CURVES_WORKGROUP_SIZE_Y), 1);
                encoder.finish();
            }
            RenderMode::CurvesOIT => {
                let clear_color = self.background_color.to_normalized_gamma_f32();
                let mut encoder = cmd.begin_rendering(RenderPassInfo {
                    color_attachments: &[ColorAttachment {
                        image_view: &color_target_view,
                        clear_value: Some([clear_color[0] as f64, clear_color[1] as f64, clear_color[2] as f64, clear_color[3] as f64]),
                    }],
                    depth_stencil_attachment: Some(DepthStencilAttachment {
                        image_view: &depth_target_view,
                        depth_clear_value: Some(1.0),
                        stencil_clear_value: None,
                    }),
                });
                encoder.bind_graphics_pipeline(&draw_strokes_pipeline);
                encoder.push_constants(&DrawStrokesPushConstants {
                    vertices: animation.stroke_vertex_buffer.device_address(),
                    strokes: animation.stroke_buffer.device_address().offset(anim_frame.stroke_offset as usize),
                    scene_params: scene_params_buf.device_address(),
                    brush_textures: brush_textures.device_address(),
                    stroke_count: anim_frame.stroke_count,
                    width: stroke_width,
                    filter_width: self.overlay_filter_width,
                    brush: self.selected_brush as u32,
                });
                encoder.draw_mesh_tasks(anim_frame.stroke_count.div_ceil(SUBGROUP_SIZE), 1, 1);
                encoder.finish();
            }
            _ => {}
        }*/


        if self.temporal_average {
            cmd.reference_resource(&temporal_avg_view);
            cmd.barrier(
                Barrier::new()
                    .shader_read_image(&color_target)
                    .shader_write_image(&self.temporal_avg_image),
            );
            let mut encoder = cmd.begin_compute();
            encoder.bind_compute_pipeline(&temporal_average_pipeline);
            encoder.push_constants(&TemporalAverageParams {
                viewport_size: uvec2(width, height),
                frame,
                falloff: temporal_average_falloff,
                new_frame: color_target_view.device_image_handle().into(),
                avg_frame: temporal_avg_view.device_image_handle().into(),
            });
            encoder.dispatch(width.div_ceil(8), height.div_ceil(8), 1);
            encoder.finish();
            cmd.blit_full_image_top_mip_level(&self.temporal_avg_image, &color_target);
        }

        Ok(())
    }

    /*
    fn recompile_shaders(&mut self) {
        let dir = crate::shaders::bindings::SHADER_DIRECTORY;
        let macro_definitions = self.global_shader_macros.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>();
        info!("recompiling shaders in {dir}...");
        shader_bridge::recompile_shaders(dir, &macro_definitions).unwrap();
        self.pipeline_cache.clear();
    }*/

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
            let image = load_brush_texture(cmd, path, ImageUsage::SAMPLED | ImageUsage::STORAGE, false);
            let sat_image = cmd.device().create_image(&ImageCreateInfo {
                memory_location: MemoryLocation::GpuOnly,
                type_: ImageType::Image2D,
                usage: ImageUsage::STORAGE,
                format: Format::R32_SFLOAT,
                width: image.width(),
                height: image.height(),
                depth: 1,
                ..Default::default()
            });
            let image_view = image.create_top_level_view();
            let sat_image_view = sat_image.create_top_level_view();
            self.brush_textures.push(BrushTexture {
                name,
                image,
                sat_image,
                image_view,
                sat_image_view,
                id: self.brush_textures.len() as u32,
            });
        }
        //let _ = self.compute_sats(cmd);
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
        let geoms: Vec<_> = geo_files.into_iter().map(|g| g.geometry).collect();
        self.animation = Some(load_stroke_animation_data(&self.device, &geoms));
        self.current_frame = 0;
    }
}

pub struct Plane {
    pub coefs: glam::DVec4, // a,b,c,d in ax + by + cz + d = 0
}

impl Plane {
    pub fn new(normal: DVec3, point: DVec3) -> Plane {
        let d = -normal.dot(point);
        Plane {
            coefs: DVec4::new(normal.x, normal.y, normal.z, d),
        }
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

        let drawn_curves = AppendBuffer::new(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu);
        let drawn_control_points = AppendBuffer::new(device, BufferUsage::STORAGE_BUFFER, MemoryLocation::CpuToGpu);

        // load tweaks
        let mut pcache = PipelineCache::new(device.clone());
        let settings = SavedSettings::load().unwrap_or_default();
        let tweaks: BTreeMap<String,String> = settings
            .tweaks
            .iter()
            .filter(|tweak| tweak.enabled)
            .map(|tweak| (tweak.name.clone(), tweak.value.clone()))
            .collect();
        pcache.set_global_macro_definitions(tweaks.clone());

        let mut app = App {
            device: device.clone(),
            animation: None,
            depth_buffer,
            depth_buffer_view,
            color_target_format,
            camera_control,
            overlay: overlay_renderer,
            global_shader_macros: Default::default(),
            bin_rast_stroke_width: 1.0,
            current_frame: 0,
            oit_stroke_width: 0.0,
            oit_max_fragments_per_pixel: 0,
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
            drawn_control_points,
            settings,
            debug_tile_line_overflow: false,
            start_time: Instant::now(),
            tweaks_changed: false,
            draw_origin: Default::default(),
            fit_tolerance: 1.0,
            curve_embedding_factor: 1.0,
            stroke_bleed_exp: 1.0,
            stroke_color: Color32::from_rgb(129, 212, 250),
            background_color: Default::default(),
            width_profile_pos: vec4(0.0, 0.333, 0.666, 1.0),
            width_profile: vec4(0.0, 0.8, 0.5, 0.3),
            opacity_profile_pos: vec4(0.0, 0.333, 0.666, 1.0),
            opacity_profile: vec4(1.0, 1.0, 0.7, 0.),
            opacity_response_curve: Default::default(),
            frame_start_time: Instant::now(),
            pcache,
        };
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
            self.pcache.clear();
        }
    }

    pub fn add_stroke(&mut self) {
        let camera = self.camera_control.camera();

        // project points on screen-aligned plane
        let Some(first_point) = self.pen_points.first().cloned() else { return };
        let (eye, dir) = camera.screen_to_world_ray(first_point.position);
        let ground_plane = Plane::new(dvec3(0.0, 1.0, 0.0), dvec3(0.0, 0.0, 0.0));

        if let Some(ground_pos) = ground_plane.intersect(eye, dir) {
            let plane = Plane::new(-dir, ground_pos);
            let proj_points = self
                .pen_points
                .iter()
                .filter_map(|p| {
                    let (eye, pos) = camera.screen_to_world_ray(p.position);
                    if let Some(pos) = plane.intersect(eye, pos) {
                        Some((pos, p.pressure, p.arc_length))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            /*let width_profile = DVec4::from(lagrange_interpolate_4(
                [self.width_profile_pos.x as f64, self.width_profile.x as f64],
                [self.width_profile_pos.y as f64, self.width_profile.y as f64],
                [self.width_profile_pos.z as f64, self.width_profile.z as f64],
                [self.width_profile_pos.w as f64, self.width_profile.w as f64],
            )).as_vec4();
            let opacity_profile = DVec4::from(lagrange_interpolate_4(
                [self.opacity_profile_pos.x as f64, self.opacity_profile.x as f64],
                [self.opacity_profile_pos.y as f64, self.opacity_profile.y as f64],
                [self.opacity_profile_pos.z as f64, self.opacity_profile.z as f64],
                [self.opacity_profile_pos.w as f64, self.opacity_profile.w as f64],
            )).as_vec4();*/
            let color = *egui::Rgba::from(self.stroke_color).to_array().first_chunk::<4>().unwrap();

            //trace!("projected points: {:#?}", proj_points);

            if let Some(ref mut anim) = self.animation.as_mut() {
                let base_vertex = anim.stroke_vertex_buffer.len() as u32;
                let mut arc_length = 0.0;
                let mut prev_pt = proj_points.first().unwrap().0;
                for (point, pressure, screen_space_arc_length) in proj_points.iter() {
                    arc_length += prev_pt.distance(*point) as f32;
                    let mapped_pressure = self.settings.pressure_response_curve.sample(*pressure);
                    let mapped_opacity = self.opacity_response_curve.sample(*pressure);
                    anim.stroke_vertex_buffer.push(StrokeVertex {
                        pos: point.as_vec3(),
                        s: *screen_space_arc_length as f32,
                        color: [(color[0] * 255.) as u8, (color[1] * 255.) as u8, (color[2] * 255.) as u8, (color[3] * 255.) as u8],
                        width: (mapped_pressure * 255.) as u8,
                        opacity: (mapped_opacity * 255.) as u8,
                    });
                    prev_pt = *point;
                }
                anim.stroke_buffer.push(Stroke {
                    base_vertex,
                    vertex_count: proj_points.len() as u32,
                    brush: self.selected_brush as u8,
                    arc_length,
                });
                anim.frames[0].stroke_count += 1;
            }
        }
    }

    pub fn touch_event(&mut self, touch_event: &winit::event::Touch) {
        let (x, y): (f64, f64) = touch_event.location.into();
        if touch_event.phase == TouchPhase::Started {
            self.pen_points.clear();
            self.is_drawing = true;
            self.last_pos = dvec2(x, y);
        }

        if self.is_drawing {
            let mut pressure = if let Some(force) = touch_event.force {
                force.normalized()
            } else {
                1.0
            };


            const PEN_SPACING_THRESHOLD: f64 = 40.0;

            let pos = dvec2(x, y);
            let delta = self.last_pos.distance(pos);
            trace!(
                "Touch event: {:?} at ({}, {}) with pressure {}; delta = {}",
                touch_event.phase,
                x,
                y,
                pressure,
                delta
            );

            if delta >= PEN_SPACING_THRESHOLD + 1.0 {
                // stabilization
                let new_pos = dvec2(x, y).lerp(self.last_pos, PEN_SPACING_THRESHOLD / delta);
                if touch_event.phase != TouchPhase::Ended {
                    let prev_arc_length = self.pen_points.last().map(|p| p.arc_length).unwrap_or(0.0);
                    self.pen_points.push(PenSample {
                        position: new_pos,
                        pressure,
                        arc_length: prev_arc_length + new_pos.distance(self.last_pos),
                    });
                }
                self.last_pos = new_pos;
            }

            if touch_event.phase == TouchPhase::Ended {
                self.is_drawing = false;
                self.add_stroke();
            }
        }
    }

    pub fn mouse_wheel(&mut self, delta: f64) {
        self.camera_control.mouse_wheel(delta);
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
        if self.frame == 0 {
            self.start_time = Instant::now();
        }
        self.frame_start_time = Instant::now();

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
        
        cmd.clear_image(&self.frame_image, ClearColorValue::Float([0.0, 0.0, 0.0, 1.0]));
        cmd.clear_depth_image(&self.depth_buffer, 1.0);

        let color_target_view = self.frame_image.create_top_level_view();
        self.draw_axes();

        let camera = self.camera_control.camera();
        if self.is_drawing {
            // draw a cross at the touch point
            let (x, y) = (self.last_pos.x, self.last_pos.y);
            self.overlay.screen_line(
                &camera,
                dvec2(x - 50.0, y),
                dvec2(x + 50.0, y),
                [255, 255, 0, 255],
                [255, 255, 0, 255],
            );
            self.overlay.screen_line(
                &camera,
                dvec2(x, y - 50.0),
                dvec2(x, y + 50.0),
                [255, 255, 0, 255],
                [255, 255, 0, 255],
            );
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
                if let Some(anim) = &self.animation {
                    let curve_count = anim.frames[self.current_frame].curve_range.count;
                    let point_count = anim.position_buffer.len();
                    let stroke_count = anim.frames[self.current_frame].stroke_count;
                    let stroke_vertex_count = anim.stroke_vertex_buffer.len();
                    //ui.label(format!("{} points", point_count));
                    ui.label(format!("{} curves (current frame), {} points (all frames)", curve_count, point_count));
                    ui.label(format!("{} strokes (current frame), {} stroke vertices (all frames)", stroke_count, stroke_vertex_count));
                }
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

            ui.add(egui::Slider::new(&mut self.bin_rast_stroke_width, 0.1..=256.0).text("Stroke Width"));
            ui.add(egui::Slider::new(&mut self.oit_stroke_width, 0.1..=256.0).text("OIT Stroke Width"));
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
                egui::DragValue::new(&mut self.current_frame)
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
                                    //toggled_tweak = true;
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

            if ui.button("Reload shaders").clicked() || self.tweaks_changed {
                let defines = self
                    .settings
                    .tweaks
                    .iter()
                    .filter(|t| t.enabled)
                    .map(|t| (t.name.clone(), t.value.clone()))
                    .collect();
                info!("Will reload shaders on the next frame");
                self.pcache.set_global_macro_definitions(defines);
            }

            if self.tweaks_changed {
                self.settings.save();
                self.tweaks_changed = false;
            }

            ui.add(Slider::new(&mut self.fit_tolerance, 1.0..=40.0).text("Curve fit tolerance"));
            ui.add(Slider::new(&mut self.curve_embedding_factor, 1.0..=40.0).text("Curve embedding factor"));
            ui.add(Slider::new(&mut self.stroke_bleed_exp, 1.0..=40.0).text("Stroke bleeding exponent"));
            ui.horizontal(|ui| {
                ui.label("Stroke color");
                color_edit_button_srgba(ui, &mut self.stroke_color, Alpha::BlendOrAdditive);
            });
            ui.horizontal(|ui| {
                ui.label("Background color");
                color_edit_button_srgba(ui, &mut self.background_color, Alpha::BlendOrAdditive);
            });
            ui.label("Stroke width profile");
            ui.horizontal(|ui| {
                ui.add(DragValue::new(&mut self.width_profile_pos.x).speed(0.01).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.width_profile_pos.y).speed(0.01).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.width_profile_pos.z).speed(0.01).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.width_profile_pos.w).speed(0.01).clamp_range(0.0..=1.0));
            });
            ui.horizontal(|ui| {
                ui.add(DragValue::new(&mut self.width_profile.x).speed(0.1).clamp_range(0.0..=40.0));
                ui.add(DragValue::new(&mut self.width_profile.y).speed(0.1).clamp_range(0.0..=40.0));
                ui.add(DragValue::new(&mut self.width_profile.z).speed(0.1).clamp_range(0.0..=40.0));
                ui.add(DragValue::new(&mut self.width_profile.w).speed(0.1).clamp_range(0.0..=40.0));
            });
            ui.label("Stroke opacity profile");
            ui.horizontal(|ui| {
                ui.add(DragValue::new(&mut self.opacity_profile_pos.x).speed(0.01).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.opacity_profile_pos.y).speed(0.01).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.opacity_profile_pos.z).speed(0.01).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.opacity_profile_pos.w).speed(0.01).clamp_range(0.0..=1.0));
            });
            ui.horizontal(|ui| {
                ui.add(DragValue::new(&mut self.opacity_profile.x).speed(0.1).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.opacity_profile.y).speed(0.1).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.opacity_profile.z).speed(0.1).clamp_range(0.0..=1.0));
                ui.add(DragValue::new(&mut self.opacity_profile.w).speed(0.1).clamp_range(0.0..=1.0));
            });

            ui.horizontal(|ui| {
                ui.label("Width response");
                curve_editor_button(ui, &mut self.settings.pressure_response_curve.knots, &mut self.settings.pressure_response_curve.values);
                if ui.button("Load default").clicked() {
                    self.settings.pressure_response_curve = Default::default();
                }
            });

            ui.horizontal(|ui| {
                ui.label("Opacity response");
                curve_editor_button(ui, &mut self.opacity_response_curve.knots, &mut self.opacity_response_curve.values);
                if ui.button("Load default").clicked() {
                    self.opacity_response_curve = Default::default();
                }
            });

            //ui.add(Slider::new(&mut self.oit_stroke_width, 0.1..=40.0).text("OIT Stroke Width"));
            //ui.add(Slider::new(&mut self.overlay_line_width, 0.1..=40.0).text("Overlay Line Width"));
            //ui.add(Slider::new(&mut self.overlay_filter_width, 0.01..=10.0).text("Overlay Filter Width"));
        });
    }

    pub fn on_exit(&mut self) {
        self.settings.save();
    }
}
