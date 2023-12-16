use glam::{dvec2, DVec2};
use std::time::{Duration, Instant};

use graal::vk;
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use winit::{
    event::{Event, MouseScrollDelta, WindowEvent},
    event_loop::EventLoop,
    raw_window_handle::HasRawWindowHandle,
    window::WindowBuilder,
};

use crate::app::App;

mod aabb;
mod app;
mod camera_control;
mod imgui_backend;
mod overlay;
mod shaders;
mod util;

fn main() {
    // Create the event loop and the main window
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let surface = graal::get_vulkan_surface(window.raw_window_handle().unwrap());
    let (device, mut queue) = unsafe { graal::create_device_and_queue(Some(surface)).expect("failed to create device") };
    let surface_format = vk::SurfaceFormatKHR {
        format: vk::Format::B8G8R8A8_UNORM,
        color_space: Default::default(),
    }; // unsafe { device.get_preferred_surface_format(surface) };
    let (init_width, init_height) = window.inner_size().into();
    let mut swapchain = unsafe { device.create_swapchain(surface, surface_format, init_width, init_height) };
    let (mut width, mut height) = window.inner_size().into();
    let mut app = App::new(&device, width, height, surface_format.format);

    // imgui stuff
    let mut imgui = imgui::Context::create();
    let mut platform = WinitPlatform::init(&mut imgui); // step 1
    platform.attach_window(imgui.io_mut(), &window, HiDpiMode::Default); // step 2
    let mut imgui_renderer = imgui_backend::Renderer::new(&mut queue, &mut imgui);

    let mut last_frame = Instant::now();
    let mut cursor_pos = DVec2::ZERO;

    // Run the event loop and forward events to the app
    event_loop
        .run(move |event, event_loop| {
            match &event {
                Event::NewEvents(_) => {
                    // other application-specific logic
                    let now = Instant::now();
                    imgui.io_mut().update_delta_time(now - last_frame);
                    last_frame = now;
                }
                Event::WindowEvent {
                    window_id,
                    event: window_event,
                } => {
                    platform.handle_event(imgui.io_mut(), &window, &event);

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
                        WindowEvent::CloseRequested => {
                            println!("The close button was pressed; stopping");
                            event_loop.exit();
                        }
                        WindowEvent::Resized(size) => unsafe {
                            (width, height) = (*size).into();
                            app.resize(&device, width, height);
                            device.resize_swapchain(&mut swapchain, width, height);
                        },
                        WindowEvent::RedrawRequested => unsafe {
                            let swapchain_image = queue.acquire_next_swapchain_image(&swapchain, Duration::from_secs(1)).unwrap();
                            // Render app
                            app.render(&mut queue, &swapchain_image.image);
                            // Update/render UI
                            let frame = imgui.new_frame();
                            let quit_requested = app.ui(frame);
                            platform.prepare_render(frame, &window);
                            let draw_data = imgui.render();
                            let view = swapchain_image.image.create_top_level_view();
                            imgui_renderer.render(&mut queue, &view, &draw_data);
                            queue.present(&swapchain_image).expect("present failed");
                            queue.end_frame().expect("end_frame failed");
                            if quit_requested {
                                event_loop.exit();
                            }
                        },
                        _ => {}
                    }
                }
                Event::AboutToWait => {
                    platform.prepare_frame(imgui.io_mut(), &window).expect("Failed to prepare frame");
                    window.request_redraw();
                }
                event => {
                    platform.handle_event(imgui.io_mut(), &window, &event);
                }
            }
        })
        .expect("event loop run failed");
}
