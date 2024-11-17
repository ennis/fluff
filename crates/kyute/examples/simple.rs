use std::future::pending;
pub use kurbo::{self, Size};
use kurbo::Point;
use kyute::layout::Axis;
use kyute::text::TextStyle;
use kyute::widgets::button::button;
use kyute::widgets::frame::{Frame, FrameLayout, FrameStyle};
use kyute::widgets::text::Text;
use kyute::widgets::text_edit::{TextEdit, WrapMode};
use kyute::{application, text, Color, Window, WindowOptions};
use tokio::select;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use tracing_tree::HierarchicalLayer;

fn main() {
    let subscriber = Registry::default().with(HierarchicalLayer::new(2).with_indent_amount(4));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let main_button = button("Test"); // &str
        let frame = Frame::new();

        frame.set_style(FrameStyle {
            border_color: Color::from_hex("5f5637"),
            border_radius: 8.0.into(),
            background_color: Color::from_hex("211e13"),
            ..Default::default()
        });

        frame.set_layout(FrameLayout::Flex { direction: Axis::Vertical, gap: 4.0.into(), initial_gap: 4.0.into(), final_gap: 4.0.into() });

        let text_edit = TextEdit::new();
        text_edit.set_text_style(TextStyle::default().font_family("Inter").font_size(12.0));
        text_edit.set_text("Hello, world!\nMultiline".to_string());

        let text_edit2 = TextEdit::new();
        text_edit2.set_text_style(TextStyle::default().font_family("Inter").font_size(12.0));
        text_edit2.set_text("Hello, world df sdfds fds fsdef sd fs! \n Multi line with ellipsis".to_string());
        text_edit2.set_wrap_mode(WrapMode::NoWrap);

        let value = 450;
        frame.add_child(Text::new(text!( size(12.0) family("Inter") #EEE { "Hello," i "world!\n" b "This is bold" } "\nThis is a " { #F00 "red" } " word\n" "Value=" i "{value}" )));

        //frame.add_child(&Text::new(text![ size(40.0) family("Inter") { "طوال اليوم." } i {"الفبای فارسی"}  ]));
        //frame.add_child(&Text::new(text![ size(40.0) family("Inter")  "Sample\nSample\nSample\nSample\nSample\nSample\nSample"  ]));
        frame.add_child(main_button.clone());

        let window_options = WindowOptions {
            title: "Hello, world!",
            size: Size::new(800.0, 600.0),
            background: Color::from_hex("211e13"),
            ..Default::default()
        };

        let main_window = Window::new(&window_options, &frame);
        let mut popup: Option<Window> = None;

        loop {
            select! {
                _ = main_button.clicked() => {
                    if let Some(_popup) = popup.take() {
                        // drop popup window
                        eprintln!("Popup closing");
                    } else {
                        // create popup
                        let popup_options = WindowOptions {
                            title: "Popup",
                            size: Size::new(400.0, 300.0),
                            parent: Some(main_window.raw_window_handle()),
                            decorations: false,
                            no_focus: true,
                            position: Some(Point::new(100.0, 100.0)),
                            ..Default::default()
                        };
                        let button = button("Close me");
                        let p = Window::new(&popup_options, &button);
                        main_window.set_popup(&p);
                        popup = Some(p);
                    }
                    eprintln!("Button clicked");
                }
                focus = async {
                    if let Some(ref popup) = popup {
                        popup.focus_changed().await
                    } else {
                        pending().await
                    }
                } => {
                    if !focus {
                        popup = None;
                    }
                }
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
