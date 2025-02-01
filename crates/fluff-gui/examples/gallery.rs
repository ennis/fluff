use fluff_gui::colors;
use fluff_gui::widgets::button::button;
use fluff_gui::widgets::spinner::spinner_buttons;
use kyute::elements::Flex;
use kyute::{Size, Window, WindowOptions, application, select};
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;
use tracing_tree::HierarchicalLayer;

fn main() {
    let subscriber = Registry::default().with(HierarchicalLayer::new(2).with_indent_amount(4));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let row = Flex::row()
        .child(button("Test"))
        .child(button("Test2"))
        .child(spinner_buttons())
        .gaps(10, 2, 10);

    application::run(async {
        let main_window = Window::new(
            &WindowOptions {
                title: "Hello, world!",
                size: Size::new(800.0, 600.0),
                background: colors::DISPLAY_BACKGROUND,
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
