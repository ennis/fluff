pub use kurbo::{self, Size};
use kyute::elements::Frame;
use kyute::{application, Color, Window, WindowOptions};
use tokio::select;

fn main() {
    application::run(async {
        let root = Frame::new().background_color(Color::from_hex("413e13"));

        let main_window = Window::new(
            &WindowOptions {
                title: "System Compositor Example",
                size: Size::new(800.0, 600.0),
                background: Color::from_hex("413e13"),
                ..Default::default()
            },
            root,
        );

        loop {
            select! {
                _ = main_window.close_requested() => {
                    application::quit();
                    break
                }
            }
        }
    })
    .unwrap()
}
