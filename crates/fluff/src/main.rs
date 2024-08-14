use egui::{FontFamily, FontId, TextStyle, ViewportBuilder, ViewportId};
use egui_winit::create_window;
use glam::{dvec2, DVec2};
use std::{
    fs,
    time::{Duration, Instant},
};

use graal::vk;
use winit::{
    event::{Event, MouseScrollDelta, WindowEvent},
    event_loop::EventLoop,
    raw_window_handle::HasRawWindowHandle,
};

use crate::app::App;

mod aabb;
mod app;
mod camera_control;
mod egui_backend;
//mod imgui_backend;
mod overlay;
//mod ui;
mod engine;
mod util;
mod shaders;
mod point_painter;

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter-Medium".to_owned(),
        egui::FontData::from_static(include_bytes!("../../../data/Inter-Medium.otf")),
    );

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
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

    // Create the event loop and the main window
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let egui_ctx = egui::Context::default();
    let window = create_window(&egui_ctx, &event_loop, &ViewportBuilder::default().with_title("Fluff")).expect("failed to create window");

    let surface = graal::get_vulkan_surface(window.raw_window_handle().unwrap());
    let (device, mut command_stream) = unsafe { graal::create_device_and_command_stream(Some(surface)).expect("failed to create device") };
    let surface_format = vk::SurfaceFormatKHR {
        format: vk::Format::R16G16B16A16_SFLOAT,
        color_space: Default::default(),
    };
    let (init_width, init_height) = window.inner_size().into();
    let mut swapchain = unsafe { device.create_swapchain(surface, surface_format, init_width, init_height) };
    let (mut width, mut height) = window.inner_size().into();
    let mut app = App::new(&device, width, height, surface_format.format);

    // imgui stuff
    //let mut imgui = imgui::Context::create();
    //let mut platform = WinitPlatform::init(&mut imgui); // step 1
    //platform.attach_window(imgui.io_mut(), &window, HiDpiMode::Default); // step 2
    //let mut imgui_renderer = imgui_backend::Renderer::new(&mut command_stream, &mut imgui);

    // egui stuff
    let mut egui_winit_state = egui_winit::State::new(egui_ctx, ViewportId::default(), &window, None, None);
    let mut egui_renderer = egui_backend::Renderer::new(&mut command_stream);

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
                    //imgui.io_mut().update_delta_time(delta_time);
                }
                Event::WindowEvent {
                    window_id,
                    event: window_event,
                } => {
                    let response = egui_winit_state.on_window_event(&window, &window_event);
                    if response.consumed {
                        return;
                    }

                    //platform.handle_event(imgui.io_mut(), &window, &event);
                    //let want_capture_mouse = imgui.io().want_capture_mouse;
                    //let want_capture_keyboard = imgui.io().want_capture_keyboard;

                    match window_event {
                        WindowEvent::CursorMoved { position, device_id, .. } => {
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
                            app.resize(&device, width, height);
                            device.resize_swapchain(&mut swapchain, width, height);
                        },
                        WindowEvent::RedrawRequested => unsafe {
                            let raw_input = egui_winit_state.take_egui_input(&window);
                            let output = egui_winit_state.egui_ctx().run(raw_input, |ctx| app.egui(ctx));
                            egui_winit_state.handle_platform_output(&window, output.platform_output);

                            let swapchain_image = command_stream
                                .acquire_next_swapchain_image(&swapchain, Duration::from_secs(1))
                                .unwrap();
                            // Render app
                            app.render(&mut command_stream, &swapchain_image.image);
                            // Update/render UI
                            //let frame = imgui.new_frame();
                            //let quit_requested = app.ui(frame);
                            //platform.prepare_render(frame, &window);
                            //let draw_data = imgui.render();
                            let view = swapchain_image.image.create_top_level_view();
                            //imgui_renderer.render(&mut command_stream, &view, &draw_data);
                            egui_renderer.render(
                                &mut command_stream,
                                &view,
                                egui_winit_state.egui_ctx(),
                                output.textures_delta,
                                output.shapes,
                                output.pixels_per_point,
                            );
                            command_stream.present(&swapchain_image).expect("present failed");
                            device.cleanup();
                            /*if quit_requested {
                                event_loop.exit();
                            }*/
                        },
                        _ => {}
                    }
                }
                Event::AboutToWait => {
                    //platform.prepare_frame(imgui.io_mut(), &window).expect("Failed to prepare frame");
                    window.request_redraw();
                }
                event => {
                    //platform.handle_event(imgui.io_mut(), &window, &event);
                }
            }

            if event_loop.exiting() {
                // save egui state
                egui_winit_state
                    .egui_ctx()
                    .memory(|mem| fs::write("egui.json", serde_json::to_string(mem).unwrap()).expect("failed to save egui state"));
            }
        })
        .expect("event loop run failed");
}
