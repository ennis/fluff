use kurbo::Point;
pub use kurbo::{self, Size};
use kyute::layout::Axis;
use kyute::model::Model;
use kyute::text::TextStyle;
use kyute::widgets::button::button;
use kyute::widgets::draw::Draw;
use kyute::widgets::flex::Flex;
use kyute::widgets::frame::{Frame, FrameStyle};
use kyute::widgets::text::Text;
use kyute::widgets::text_edit::{TextEdit, WrapMode};
use kyute::{application, text, Color, Window, WindowOptions};
use std::future::pending;
use tokio::select;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use tracing_tree::HierarchicalLayer;

fn main() {
    let subscriber = Registry::default().with(HierarchicalLayer::new(2).with_indent_amount(4));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let main_button =
            button("Test");

        let mut text_edit = TextEdit::new();
        text_edit.set_text_style(TextStyle::default().font_family("Inter").font_size(12.0));
        text_edit.set_text("Hello, world!\nMultiline".to_string());

        /*let mut text_edit2 = TextEdit::new();
        text_edit2.set_text_style(TextStyle::default().font_family("Inter").font_size(12.0));
        text_edit2.set_text("Hello, world! \n Multi line".to_string());
        text_edit2.set_wrap_mode(WrapMode::NoWrap);*/


        let counter_value = Model::new(0i32);
        let counter_display = Frame::new().height(20).content(
            Draw::new({
                let counter_value = counter_value.clone();
                move |cx| {
                    use kyute::widgets::draw::prelude::*;
                    let value = counter_value.get();
                    cx.fill(rgb(255, 255, 255));
                    cx.draw_text(Right, Top, text!["Counter value is " b "{value}"]);
                }
            }));

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
            .content(Flex::new((
                text!( size(12.0) family("Inter") #44AE12 { "Hello," i "world!\n" b "This is bold" } "\nThis is a " { #F00 "red" } " word\n" "Value=" i "{value}" ),
                text_edit,
                main_button,
                counter_display,
                increment_button
            )).vertical().gaps(4, 4, 4));

        let main_window = Window::new(&WindowOptions {
            title: "Hello, world!",
            size: Size::new(800.0, 600.0),
            background: Color::from_hex("211e13"),
            ..Default::default()
        }, frame);

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
