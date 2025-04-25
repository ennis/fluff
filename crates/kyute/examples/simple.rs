pub use kurbo::{self, Size};
use kyute::drawing::StaticImage;
use kyute::element::IntoElementAny;
use kyute::elements::button::button;
use kyute::elements::text_edit::TextEditBase;
use kyute::elements::{Flex, Frame};
use kyute::event::Model;
use kyute::text::TextStyle;
use kyute::{application, text, Color, Window, WindowOptions};
use tokio::select;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use tracing_tree::HierarchicalLayer;

pub const TEST_IMAGE: StaticImage = StaticImage::new(include_bytes!("../../../data/ui/align_center.png"));

fn myframe(content: impl IntoElementAny) -> impl IntoElementAny {
    Frame::new()
        .border_color(Color::from_hex("5f5637"))
        .border_radius(8.0)
        .background_color(Color::from_hex("211e13"))
}

fn main() {
    let subscriber = Registry::default().with(HierarchicalLayer::new(2).with_indent_amount(4));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let main_button = button("Test");

        let mut text_edit = TextEditBase::new();
        text_edit.set_text_style(TextStyle::default().font_family("Inter").font_size(12.0));
        text_edit.set_text("Hello, world!\nMultiline".to_string());
        let counter_value = Model::new(0i32);

        let increment_button = button("Increment").on_click({
            let counter_value = counter_value.clone();
            move || {
                counter_value.update(|v| *v += 1);
            }
        });

        let value = 450;
        let frame = Frame::new()
            .border_color(Color::from_hex("5f5637"))
            .border_radius(8.0)
            .background_color(Color::from_hex("211e13"))
            .content(
                Flex::new()
                    .vertical()
                    .gaps(4, 4, 4)
                    .child(main_button)
                    .child(increment_button),
            );

        let main_window = Window::new(
            &WindowOptions {
                title: "Hello, world!",
                size: Size::new(800.0, 600.0),
                background: Color::from_hex("211e13"),
                ..Default::default()
            },
            frame,
        );

        loop {
            select! {
                _ = main_window.close_requested() => {
                    eprintln!("Window closed");
                    application::quit();
                    break
                }
                size = main_window.resized() => {
                    eprintln!("Window resized to {:?}", size);
                }
            }
        }

        //application::quit();
    })
    .unwrap()
}
