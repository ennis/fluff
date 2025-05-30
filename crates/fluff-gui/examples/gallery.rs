use fluff_gui::colors;
use fluff_gui::widgets::button::Button;
use fluff_gui::widgets::menu::{MenuBar, MenuItem};
use fluff_gui::widgets::scroll::ScrollBarBase;
use fluff_gui::widgets::slider::Slider;
use fluff_gui::widgets::spinner::{SpinnerBase, SpinnerOptions};
use fluff_gui::widgets::uniform_grid::UniformGrid;
use kyute::drawing::rgb;
use kyute::elements::{Flex, Frame};
use kyute::{IntoElementAny, Size, Window, application, select};
use kyute::platform::WindowOptions;

fn random_colored_square() -> impl IntoElementAny {
    let color = rgb(rand::random(), rand::random(), rand::random());
    Frame::new()
        .background_color(color)
        .width(50.)
        .height(50.)
        .border_width(1.)
        .border_color(rgb(255, 255, 255))
}

fn main() {
    //let subscriber = Registry::default().with(HierarchicalLayer::new(2).with_indent_amount(4));
    //tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let row = Frame::new()
            .content(
                Flex::column()
                    .child(
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
                    .child(ScrollBarBase::horizontal().thumb_size(10.))
                    .child(ScrollBarBase::horizontal().thumb_size(20.))
                    .child(ScrollBarBase::horizontal().thumb_size(50.))
                    .child(ScrollBarBase::horizontal().thumb_size(100.))
                    .child(
                        UniformGrid::new(Size::new(50., 50.))
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .child(random_colored_square())
                            .v_gap(1.)
                            .h_gap(1.),
                    )
                    .gaps(0, 2, 0),
            )
            .padding(10.);

        use MenuItem::*;

        let main_menu = MenuBar::new(&[
            Submenu(
                "File",
                &[
                    Entry("New", 0),
                    Entry("Open", 1),
                    Entry("Save", 2),
                    Separator,
                    Entry("Exit", 3),
                ],
            ),
            Submenu("Edit", &[Entry("Cut", 4), Entry("Copy", 5), Entry("Paste", 6)]),
        ]);

        let top = Flex::column().child(main_menu).child(row).gaps(0, 2, 0);

        let main_window = Window::new(
            &WindowOptions {
                title: "Hello, world!",
                size: Some(Size::new(800.0, 600.0)),
                background: colors::STATIC_BACKGROUND,
                ..Default::default()
            },
            top,
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
