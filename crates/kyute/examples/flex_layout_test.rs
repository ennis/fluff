pub use kurbo::{self, Size};
use kyute::element::RcElement;
use kyute::elements::frame::{Frame, FrameStyle};
use kyute::elements::text::Text;
use kyute::layout::{Axis, SizeValue, SpacingAfter, SpacingBefore};
use kyute::{application, text, Color, Window, WindowOptions};
use tokio::select;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};
use tracing_tree::HierarchicalLayer;

fn frame(
    direction: Axis,
    text: &str,
    content: Vec<RcElement<Frame>>,
    margin_before: SizeValue,
    margin_after: SizeValue,
) -> RcElement<Frame> {
    let frame = Frame::new();

    frame.set_border(1.0);
    frame.set_border_radius(8.0);
    frame.set_background_color(Color::from_hex("211e13"));
    frame.set_border_color(Color::from_hex("5f5637"));
    frame.set_direction(direction);
    frame.set_gap(4.0.into());
    frame.set(SpacingBefore, margin_before);
    frame.set(SpacingAfter, margin_after);
    frame.set_padding(4.);

    if !text.is_empty() {
        let text = Text::new(text![family("Inter") size(12.0) #FFF "{text}"]);
        frame.add_child(text);
    }

    for child in content {
        frame.add_child(child.clone());
    }

    frame
}

fn flex_frame(direction: Axis, flex: f64, content: Vec<RcElement<Frame>>) -> RcElement<Frame> {
    let f = frame(direction, "", content, SizeValue::Auto, SizeValue::Auto);
    f.set_width(SizeValue::Stretch);
    f
}

fn min_flex_frame(direction: Axis, color: Color, min: f64, flex: f64) -> RcElement<Frame> {
    let frame = Frame::new();

    frame.set_background_color(color);
    frame.set_direction(direction);
    frame.set_width(SizeValue::Stretch);
    frame.set_height(SizeValue::Percentage(1.0));
    frame.set_min_height(SizeValue::Fixed(min));
    frame
}

fn main() {
    let subscriber = Registry::default()
        .with(HierarchicalLayer::new(4))
        .with(EnvFilter::from_default_env());
    tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let no_margin = SizeValue::Fixed(0.0);
        let flex_expand = SizeValue::Stretch;

        let frame_root = frame(
            Axis::Horizontal,
            "",
            vec![
                flex_frame(
                    Axis::Vertical,
                    1.0,
                    vec![
                        // items aligned on top
                        frame(Axis::Horizontal, "Content", vec![], no_margin, no_margin),
                        frame(Axis::Horizontal, "Content", vec![], no_margin, no_margin),
                        frame(Axis::Horizontal, "Content", vec![], no_margin, flex_expand),
                    ],
                ),
                flex_frame(
                    Axis::Vertical,
                    2.0,
                    vec![
                        // two items on top, last on the bottom
                        frame(Axis::Horizontal, "Content Top", vec![], no_margin, no_margin),
                        frame(Axis::Horizontal, "Content Top", vec![], no_margin, no_margin),
                        frame(Axis::Horizontal, "Content Bottom", vec![], flex_expand, no_margin),
                    ],
                ),
                flex_frame(
                    Axis::Vertical,
                    1.0,
                    vec![
                        // items aligned on bottom
                        frame(Axis::Horizontal, "Content Bottom", vec![], flex_expand, no_margin),
                        frame(Axis::Horizontal, "Content Bottom", vec![], no_margin, no_margin),
                        frame(Axis::Horizontal, "Content Bottom", vec![], no_margin, no_margin),
                    ],
                ),
                flex_frame(
                    Axis::Vertical,
                    1.0,
                    vec![
                        // centered items
                        frame(Axis::Horizontal, "Content Centered", vec![], flex_expand, no_margin),
                        frame(Axis::Horizontal, "Content Centered", vec![], no_margin, no_margin),
                        frame(Axis::Horizontal, "Content Centered", vec![], no_margin, flex_expand),
                    ],
                ),
                flex_frame(
                    Axis::Vertical,
                    1.0,
                    vec![
                        // items with space between
                        frame(
                            Axis::Horizontal,
                            "Content Regularly Spaced",
                            vec![],
                            flex_expand,
                            flex_expand,
                        ),
                        frame(
                            Axis::Horizontal,
                            "Content Regularly Spaced",
                            vec![],
                            flex_expand,
                            flex_expand,
                        ),
                        frame(
                            Axis::Horizontal,
                            "Content Regularly Spaced",
                            vec![],
                            flex_expand,
                            flex_expand,
                        ),
                    ],
                ),
                flex_frame(
                    Axis::Vertical,
                    1.0,
                    vec![
                        // boxes of color with various flex heights
                        min_flex_frame(Axis::Horizontal, Color::from_hex("f0f"), 100.0, 2.0),
                        min_flex_frame(Axis::Horizontal, Color::from_hex("ff0"), 100.0, 1.0),
                        min_flex_frame(Axis::Horizontal, Color::from_hex("f00"), 100.0, 1.0),
                        min_flex_frame(Axis::Horizontal, Color::from_hex("0f0"), 100.0, 1.0),
                    ],
                ),
            ],
            no_margin,
            no_margin,
        );

        ////////////////////////////////////////////////////////////////////
        let window_options = WindowOptions {
            title: "Hello, world!",
            size: Size::new(800.0, 600.0),
            background: Color::from_hex("211e13"),
            ..Default::default()
        };

        let main_window = Window::new(&window_options, frame_root.into());

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
    })
    .unwrap()
}
