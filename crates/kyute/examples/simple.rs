use kurbo::Point;
pub use kurbo::{self, Size};
pub use skia_safe as skia;
use std::future::pending;
use taffy::{Dimension, LengthPercentage};
use tokio::select;
use kyute::{application, text};
use kyute::layout::flex::Axis;
use kyute::style::{Style, StyleExt};
use kyute::text::TextStyle;
use kyute::widgets::text::Text;
use kyute::widgets::button::button;
use kyute::widgets::frame::{Frame, FrameStyle, TaffyStyle};
use kyute::widgets::text_edit::{TextEdit, TextOverflow, WrapMode};
use kyute::Color;
use kyute::WindowOptions;
use kyute::Window;


fn main() {
    application::run(async {
        let main_button = button("Test"); // &str

        // want to turn it into a sequence of AttributedRange

        let frame = Frame::new(
            FrameStyle {
                border_color: Color::from_hex("5f5637"),
                border_radius: 8.0.into(),
                background_color: Color::from_hex("211e13"),
                ..Default::default()
            }
        );

        frame.set::<TaffyStyle>(taffy::Style {
            flex_direction: taffy::FlexDirection::Column,
            padding: taffy::Rect {
                left: LengthPercentage::Length(20.0),
                right: LengthPercentage::Length(20.0),
                top: LengthPercentage::Length(20.0),
                bottom: LengthPercentage::Length(20.0),
            },
            border: taffy::Rect {
                left: LengthPercentage::Length(1.0),
                right: LengthPercentage::Length(1.0),
                top: LengthPercentage::Length(1.0),
                bottom: LengthPercentage::Length(1.0),
            },
            min_size: taffy::Size {
                width: Dimension::Length(200.0),
                height: Dimension::Length(50.0),
            },
            max_size: taffy::Size {
                width: Dimension::Length(600.0),
                height: Dimension::Auto,
            },
            ..Default::default()
        });


        let text_edit = TextEdit::new();
        text_edit.set_text_style(TextStyle::default().font_family("Inter").font_size(12.0));
        text_edit.set_text("Hello, world!\nMultiline".to_string());

        let text_edit2 = TextEdit::new();
        text_edit2.set_text_style(TextStyle::default().font_family("Inter").font_size(12.0));
        text_edit2.set_text("Hello, world df sdfds fds fsdef sd fs! \n Multi line with ellipsis".to_string());
        text_edit2.set_wrap_mode(WrapMode::NoWrap);
        //text_edit2.set_single_line();
        //text_edit2.set_max_lines(2);
        //text_edit2.set_overflow(TextOverflow::Ellipsis);


        // FIXME: this doesn't work because the macro, like format_args, borrows temporaries
        //let attributed_text = text!( { "Hello," i "world!\n" b "This is bold" } "This is a " { rgb(255,0,0) "red" } " word\n" "Value=" i "{value}" );
        // We could directly return `FormattedText`.

        frame.add_child(&text_edit);
        frame.add_child(&text_edit2);

        let value = 450;
        //frame.add_child(&Text::new(text!( size(12.0) family("Inter") #EEE { "Hello," i "world!\n" b "This is bold" } "\nThis is a " { #F00 "red" } " word\n" "Value=" i "{value}" )));

        //frame.add_child(&Text::new(text![ size(40.0) family("Inter") { "طوال اليوم." } i {"الفبای فارسی"}  ]));
        //frame.add_child(&Text::new(text![ size(40.0) family("Inter")  "Sample\nSample\nSample\nSample\nSample\nSample\nSample"  ]));

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
