use kurbo::Point;
pub use kurbo::{self, Size};
pub use skia_safe as skia;
use std::future::pending;
use tokio::select;
use kyute::{application, text};
use kyute::layout::flex::Axis;
use kyute::style::{Style, StyleExt};
use kyute::text::TextStyle;
use kyute::widgets::text::Text;
use kyute::widgets::button::button;
use kyute::widgets::frame::Frame;
use kyute::widgets::text_edit::TextEdit;
use kyute::Color;
use kyute::WindowOptions;
use kyute::Window;


fn main() {
    application::run(async {
        let main_button = button("Click me!"); // &str

        // want to turn it into a sequence of AttributedRange

        let frame = Frame::new(
            Style::new()
                .background_color(Color::from_hex("211e13"))
                .direction(Axis::Vertical)
                .border_radius(8.0)
                .min_width(200.0.into())
                .min_height(50.0.into())
                .max_width(200.0.into())
                .padding_left(20.0.into())
                .padding_right(20.0.into())
                .padding_top(20.0.into())
                .padding_bottom(20.0.into())
                .border_color(Color::from_hex("5f5637"))
                .border_left(1.0.into())
                .border_right(1.0.into())
                .border_top(1.0.into())
                .border_bottom(1.0.into()),
        );

        let text_edit = TextEdit::new();
        text_edit.set_text_style(TextStyle::default().font_family("Inter").font_size(50.0));
        text_edit.set_text("Hello, world!\nMultiline".to_string());

        let text_edit2 = TextEdit::new();
        text_edit2.set_text_style(TextStyle::default().font_family("Garamond").font_size(50.0));
        text_edit2.set_text("Hello, world! Single line with ellipsis".to_string());
        text_edit2.set_single_line();
        text_edit2.set_ellipsis(true);

        let value = 450;

        // FIXME: this doesn't work because the macro, like format_args, borrows temporaries
        //let attributed_text = text!( { "Hello," i "world!\n" b "This is bold" } "This is a " { rgb(255,0,0) "red" } " word\n" "Value=" i "{value}" );
        // We could directly return `FormattedText`.

        frame.add_child(&Text::new(&text!( size(12.0) family("Inter") #EEE { "Hello," i "world!\n" b "This is bold" } "\nThis is a " { #F00 "red" } " word\n" "Value=" i "{value}" )));
        frame.add_child(&text_edit);
        frame.add_child(&text_edit2);
        frame.add_child(&main_button);

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
