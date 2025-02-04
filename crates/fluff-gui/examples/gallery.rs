use fluff_gui::colors;
use fluff_gui::widgets::button::Button;
use fluff_gui::widgets::spinner::{SpinnerBase, SpinnerOptions};
use kyute::elements::{Flex, Frame};
use kyute::{Size, Window, WindowOptions, application, select};
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;
use tracing_tree::HierarchicalLayer;
use fluff_gui::widgets::slider::Slider;

fn main() {
    let subscriber = Registry::default().with(HierarchicalLayer::new(2).with_indent_amount(4));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let row = Frame::new()
            .content(
                Flex::row()
                    .child(Button::new("OK"))
                    .child(Button::new("Cancel"))
                    .child(SpinnerBase::new(SpinnerOptions {
                        unit: "°C",
                        precision: 2,
                        increment: 0.10,
                        ..Default::default()
                    }))
                    .child(Slider::new(0.0, 0.0..100.0))
                    .gaps(0, 2, 0),
            )
            .padding(10.);

        let main_window = Window::new(
            &WindowOptions {
                title: "Hello, world!",
                size: Size::new(800.0, 600.0),
                background: colors::STATIC_BACKGROUND,
                ..Default::default()
            },
            row,
        );

        loop {
            select! {
                _ = main_window.close_requested() => {
                    eprintln!("Window closed");
                    application::quit();
                    break
                }
            }
        }
    })
    .unwrap()
}
