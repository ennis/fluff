use glam::{vec3, DVec2};
use graal::{
    Attachments, ClearColorValue, Device, Format, Image, ImageCreateInfo, ImageType, ImageUsage, ImageView, MemoryLocation, Point2D, Queue,
    Rect2D, Size2D,
};
use houdinio::Geo;
use winit::{
    event::MouseButton,
    keyboard::{Key, NamedKey},
};

use crate::{camera_control::CameraControl, overlay::OverlayRenderer, util::resolve_file_sequence};

struct AnimFrame {
    index: usize,
    geometry: Geo,
}

pub struct App {
    frames: Vec<AnimFrame>,
    depth_buffer: Image,
    depth_buffer_view: ImageView,
    color_target_format: Format,
    camera_control: CameraControl,
    overlay: OverlayRenderer,
}

type ControlPoint = [f32; 3];

/// Converts bezier curve data from `.geo` files to a format that can be uploaded to the GPU.
///
/// Curves are represented as follows:
/// * control point buffer: contains the control points of curves, all flattened into a single linear buffer.
/// * curve buffer: consists of (start, size) pairs, defining the start and number of CPs of each curve in the position buffer.
/// * animation buffer: consists of (start, size) defining the start and number of curves in the curve buffer for each animation frame.
fn convert_bezier_curves(geo: &Geo) {
    /*let mut control_point_count = 0;
    let mut curve_count = 0;

    // Count the number of curves and control points
    //for f in self.anim_frames.iter() {
    for prim in f.geometry.primitives.iter() {
        match prim {
            Primitive::BezierRun(run) => {
                match run.vertices {
                    PrimVar::Uniform(ref u) => {
                        // number of indices
                        control_point_count += u.len() * run.count;
                    }
                    PrimVar::Varying(ref v) => {
                        control_point_count += v.iter().map(|v| v.len()).sum::<usize>();
                    }
                }
                curve_count += run.count;
            }
        }
    }
    //point_count += f.geometry.point_count;
    //}

    // Curve buffer: contains (start, end) pairs of curves in the point buffer
    let curve_buffer_size = dbg!(curve_count * 8); // cp_count * sizeof(int2)
    let curve_buffer = Buffer::new(
        gl,
        curve_buffer_size,
        gl::DYNAMIC_STORAGE_BIT | gl::MAP_READ_BIT | gl::MAP_WRITE_BIT,
    );

    // Point buffer: contains positions of bezier control points
    let point_buffer_size = dbg!(control_point_count * 3 * 4); // point_count * sizeof(float3)
    let point_buffer = Buffer::new(
        gl,
        point_buffer_size,
        gl::DYNAMIC_STORAGE_BIT | gl::MAP_READ_BIT | gl::MAP_WRITE_BIT,
    );

    // points are unlikely to be shared between different bezier curves, so we might as well skip the indexing

    let mut frame_curve_data = vec![];

    // write curves
    unsafe {
        let curve_data = curve_buffer.map_mut(0, curve_buffer_size) as *mut [i32; 2];
        let mut curve_ptr = 0;
        let point_data = point_buffer.map_mut(0, point_buffer_size) as *mut f32;
        let mut point_ptr = 0;

        for f in self.anim_frames.iter() {
            let offset = curve_ptr;

            let position_attr = f.geometry.find_point_attribute("P").expect("no position attribute");
            assert_eq!(position_attr.size, 3);
            let positions = position_attr
                .as_f32_slice()
                .expect("position attribute should be a float");
            assert_eq!(positions.len(), f.geometry.point_count * 3);

            let mut write_curve = |indices: &[i32]| {
                let start = point_ptr;
                for &index in indices.iter() {
                    let index = f.geometry.topology[index as usize] as usize;
                    *point_data.offset(point_ptr) = positions[index * 3];
                    *point_data.offset(point_ptr + 1) = positions[index * 3 + 1];
                    *point_data.offset(point_ptr + 2) = positions[index * 3 + 2];
                    point_ptr += 3;
                }
                *curve_data.offset(curve_ptr) = [(start / 3) as i32, ((point_ptr - start) / 3) as i32];
                curve_ptr += 1;
            };

            for prim in f.geometry.primitives.iter() {
                match prim {
                    Primitive::BezierRun(run) => match run.vertices {
                        PrimVar::Uniform(ref indices) => {
                            // flatten instances
                            for _ in 0..run.count {
                                write_curve(indices);
                            }
                        }
                        PrimVar::Varying(ref indices) => {
                            for indices in indices.iter() {
                                write_curve(indices);
                            }
                        }
                    },
                }
            }

            frame_curve_data.push(FrameCurveData {
                offset,
                num_curves: curve_ptr - offset,
                num_control_points: 0,
            });
        }
    }

    curve_buffer.unmap();
    point_buffer.unmap();

    self.frame_curve_data = dbg!(frame_curve_data);
    self.curve_buffer = Some(curve_buffer);
    self.point_buffer = Some(point_buffer);*/
}

impl App {
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

            self.frames.clear();

            for (frame_index, file_path) in file_sequence {
                eprint!("Loading: `{}`...", file_path.display());
                match Geo::load_json(file_path) {
                    Ok(geometry) => {
                        self.frames.push(AnimFrame {
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

            //self.min_frame = self.anim_frames.iter().map(|a| a.frame).min().unwrap_or(0);
            //self.max_frame = self.anim_frames.iter().map(|a| a.frame).max().unwrap_or(0);
            //self.load_bezier_to_gpu();
        }
    }
}

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
        App {
            frames: vec![],
            depth_buffer,
            depth_buffer_view,
            color_target_format,
            camera_control,
            overlay: overlay_renderer,
        }
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

    pub fn reload_shaders(&mut self) {
        // TODO
    }

    pub fn ui(&mut self, ui: &mut imgui::Ui) -> bool {
        //let mut open = true;
        //ui.show_demo_window(&mut open);

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

        quit
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
        self.overlay.set_camera(self.camera_control.camera());
        self.draw_axes();
        let color_target_view = image.create_top_level_view();
        let mut command_buffer = queue.create_command_buffer();
        let mut encoder = command_buffer.begin_rendering(&RenderAttachments {
            color: &color_target_view,
            depth: &self.depth_buffer_view,
        });
        encoder.clear_color(0, ClearColorValue::Float([0.2, 0.4, 0.6, 1.0]));
        encoder.clear_depth(1.0);
        self.overlay.render(image.width(), image.height(), &mut encoder);
        encoder.finish();
        queue.submit([command_buffer]).expect("submit failed");
    }

    pub fn on_exit(&mut self) {}
}
