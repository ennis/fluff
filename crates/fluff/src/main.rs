use egui::{FontFamily, FontId, TextStyle};

use crate::data::AppModel;
use fluff_gui::colors;
use kyute::{application, select, Size, Window};
use winit::raw_window_handle::{HasRawWindowHandle, HasWindowHandle};
use kyute::platform::WindowOptions;

mod aabb;
mod animation;
mod app;
mod camera_control;
mod data;
mod egui_backend;
mod gpu;
mod imgui;
mod overlay;
mod scene;
mod shaders;
mod ui;
mod util;

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter-Medium".to_owned(),
        egui::FontData::from_static(include_bytes!("../../../data/Inter-Medium.otf")),
    );

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter-Medium".to_owned());
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
    ]
    .into();
    ctx.set_style(style);
}

fn main() {
    tracing_subscriber::fmt::init();

    application::run(async {
        let app = AppModel::new();

        // Create the main window
        let root = ui::root_frame(app);
        let main_window = Window::new(
            &WindowOptions {
                title: "Hello, world!",
                size: Some(Size::new(800.0, 600.0)),
                background: colors::STATIC_BACKGROUND,
                ..Default::default()
            },
            root,
        );

        loop {
            select! {
                _ = main_window.close_requested() => {
                    eprintln!("Window closed");
                    application::quit();
                    break
                }
            }
        }
    }).unwrap();

    /*// Create the event loop and the main window
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let egui_ctx = egui::Context::default();
    let window = create_window(&egui_ctx, &event_loop, &ViewportBuilder::default().with_title("Fluff"))
        .expect("failed to create window");

    gpu::init();
    let device = gpu::device();

    let surface = graal::get_vulkan_surface(window.window_handle().unwrap().as_raw());
    let surface_format = vk::SurfaceFormatKHR {
        format: vk::Format::R16G16B16A16_SFLOAT,
        color_space: Default::default(),
    };
    let (init_width, init_height) = window.inner_size().into();
    let mut swapchain = unsafe { device.create_swapchain(surface, surface_format, init_width, init_height) };
    let (mut width, mut height) = window.inner_size().into();
    let mut app = App::new(width, height, surface_format.format);

    // egui stuff
    let mut egui_winit_state = egui_winit::State::new(egui_ctx, ViewportId::default(), &window, None, None);
    let mut egui_renderer = egui_backend::Renderer::new(gpu::device());

    // load egui state
    egui_winit_state.egui_ctx().memory_mut(|mem| {
        if let Ok(state) = fs::read_to_string("egui.json") {
            *mem = serde_json::from_str(&state).expect("failed to load egui state");
        }
    });

    setup_custom_fonts(egui_winit_state.egui_ctx());

    let mut last_frame_time = Instant::now();
    let mut cursor_pos = DVec2::ZERO;
    let mut delta_time = Duration::default();

    // Run the event loop and forward events to the app
    event_loop
        .run(move |event, event_loop| {
            match &event {
                Event::NewEvents(_) => {
                    // other application-specific logic
                    let now = Instant::now();
                    delta_time = now - last_frame_time;
                    last_frame_time = now;
                }
                Event::WindowEvent {
                    window_id,
                    event: window_event,
                } => {
                    let response = egui_winit_state.on_window_event(&window, &window_event);
                    if response.consumed {
                        return;
                    }

                    match window_event {
                        WindowEvent::CursorMoved {
                            position, device_id, ..
                        } => {
                            cursor_pos = dvec2(position.x, position.y);
                            app.cursor_moved(cursor_pos);
                        }
                        WindowEvent::MouseInput { button, state, .. } => {
                            app.mouse_input(*button, cursor_pos, *state == winit::event::ElementState::Pressed);
                        }
                        WindowEvent::KeyboardInput { event, .. } => {
                            app.key_input(&event.logical_key, event.state == winit::event::ElementState::Pressed);
                        }
                        WindowEvent::MouseWheel { delta, .. } => {
                            let delta = match delta {
                                MouseScrollDelta::LineDelta(x, y) => *y as f64 * 20.0,
                                MouseScrollDelta::PixelDelta(px) => px.y,
                            };
                            app.mouse_wheel(delta);
                        }
                        WindowEvent::Touch(touch) => {
                            app.touch_event(touch);
                        }
                        WindowEvent::CloseRequested => {
                            println!("The close button was pressed; stopping");
                            app.on_exit();
                            event_loop.exit();
                        }
                        WindowEvent::Resized(size) => unsafe {
                            (width, height) = (*size).into();
                            app.resize(width, height);
                            device.resize_swapchain(&mut swapchain, width, height);
                        },
                        WindowEvent::RedrawRequested => unsafe {
                            let raw_input = egui_winit_state.take_egui_input(&window);
                            let output = egui_winit_state.egui_ctx().run(raw_input, |ctx| app.egui(ctx));
                            egui_winit_state.handle_platform_output(&window, output.platform_output);

                            let (swapchain_image, swapchain_ready) = device
                                .acquire_next_swapchain_image(&swapchain, Duration::from_secs(1))
                                .unwrap();

                            // Render app
                            let mut cmd = device.create_command_stream();
                            app.render(&mut cmd, &swapchain_image.image);
                            // Update/render UI
                            let view = swapchain_image.image.create_top_level_view();
                            egui_renderer.render(
                                &mut cmd,
                                &view,
                                egui_winit_state.egui_ctx(),
                                output.textures_delta,
                                output.shapes,
                                output.pixels_per_point,
                            );
                            cmd.present(&[swapchain_ready.wait()], &swapchain_image).unwrap();
                            device.cleanup();

                            /*if quit_requested {
                                event_loop.exit();
                            }*/
                        },
                        _ => {}
                    }
                }
                Event::AboutToWait => {
                    window.request_redraw();
                }
                _ => {}
            }

            if event_loop.exiting() {
                // save egui state
                egui_winit_state.egui_ctx().memory(|mem| {
                    fs::write("egui.json", serde_json::to_string(mem).unwrap()).expect("failed to save egui state")
                });
            }
        })
        .expect("event loop run failed");*/
}
