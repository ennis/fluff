use kurbo::Point;
pub use kurbo::{self, Size};
use kyute::layout::flex::Axis;
use kyute::text::{TextRun, TextStyle};
use kyute::widgets::button::button;
use kyute::widgets::frame::{Frame, FrameLayout, FrameStyle};
use kyute::widgets::text::Text;
use kyute::widgets::text_edit::{TextEdit, TextOverflow, WrapMode};
use kyute::{application, text, Color, Window, WindowOptions};
pub use skia_safe as skia;
use std::future::pending;
use std::rc::Rc;
use tokio::select;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};
use tracing_tree::HierarchicalLayer;
use kyute::layout::{FlexFactor, FlexMargins, FlexSize, Height, PaddingBottom, PaddingLeft, PaddingRight, PaddingTop, SizeValue, Sizing, Width};

fn frame(direction: Axis, text: &str, content: Vec<Rc<Frame>>, margin_before: FlexSize, margin_after: FlexSize) -> Rc<Frame> {
    let frame = Frame::new(FrameStyle {
        border_color: Color::from_hex("5f5637"),
        border_radius: 8.0.into(),
        background_color: Color::from_hex("211e13"),
        ..Default::default()
    });

    frame.set_layout(FrameLayout::Flex { direction, gap: Default::default(), initial_gap: Default::default(), final_gap: Default::default() });

    if !text.is_empty() {
        let text = Text::new(text![family("Inter") size(12.0) #FFF "{text}"]);
        frame.add_child(&text);
    }

    for child in content {
        frame.add_child(&child);
    }

    frame.set(FlexMargins, (margin_before, margin_after));

    frame.set(PaddingLeft, 4.0.into());
    frame.set(PaddingRight, 4.0.into());
    frame.set(PaddingBottom, 4.0.into());
    frame.set(PaddingTop, 4.0.into());
    frame
}

fn flex_frame(direction: Axis, flex: f64, content: Vec<Rc<Frame>>) -> Rc<Frame> {
    let f = frame(direction, "", content, FlexSize::NULL, FlexSize::NULL);
    f.set(FlexFactor, flex);
    f
}

fn main() {
    let subscriber = Registry::default().with(HierarchicalLayer::new(4)).with(EnvFilter::from_default_env());
    tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let no_margin = FlexSize::NULL;
        let flex_expand = FlexSize { size: 0.0, flex: 1.0 };

        let frame_root = frame(Axis::Horizontal, "", vec![
            flex_frame(Axis::Vertical, 1.0, vec![
                // items aligned on top
                frame(Axis::Horizontal, "Content", vec![], no_margin, no_margin),
                frame(Axis::Horizontal, "Content", vec![], no_margin, no_margin),
                frame(Axis::Horizontal, "Content", vec![], no_margin, flex_expand),
            ]),
            flex_frame(Axis::Vertical, 2.0, vec![
                // two items on top, last on the bottom
                frame(Axis::Horizontal, "Content Top", vec![], no_margin, no_margin),
                frame(Axis::Horizontal, "Content Top", vec![], no_margin, no_margin),
                frame(Axis::Horizontal, "Content Bottom", vec![], flex_expand, no_margin),
            ]),
            flex_frame(Axis::Vertical, 1.0, vec![
                // items aligned on bottom
                frame(Axis::Horizontal, "Content Bottom", vec![], flex_expand, no_margin),
                frame(Axis::Horizontal, "Content Bottom", vec![], no_margin, no_margin),
                frame(Axis::Horizontal, "Content Bottom", vec![], no_margin, no_margin),
            ]),
            flex_frame(Axis::Vertical, 1.0, vec![
                // centered items
                frame(Axis::Horizontal, "Content Centered", vec![], flex_expand, no_margin),
                frame(Axis::Horizontal, "Content Centered", vec![], no_margin, no_margin),
                frame(Axis::Horizontal, "Content Centered", vec![], no_margin, flex_expand),
            ]),
            flex_frame(Axis::Vertical, 1.0, vec![
                // items with space between
                frame(Axis::Horizontal, "Content Regularly Spaced", vec![], flex_expand, flex_expand),
                frame(Axis::Horizontal, "Content Regularly Spaced", vec![], flex_expand, flex_expand),
                frame(Axis::Horizontal, "Content Regularly Spaced", vec![], flex_expand, flex_expand),
            ]),
        ], no_margin, no_margin);


        ////////////////////////////////////////////////////////////////////
        let window_options = WindowOptions {
            title: "Hello, world!",
            size: Size::new(800.0, 600.0),
            background: Color::from_hex("211e13"),
            ..Default::default()
        };

        let main_window = Window::new(&window_options, &frame_root);

        loop {
            select! {
                _ = main_window.close_requested() => {
                    eprintln!("Window closed");
                    application::quit();
                    break
                }
            }
        }

        //application::quit();
    }).unwrap()
}
