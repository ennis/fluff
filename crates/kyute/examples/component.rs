use kurbo::Point;
pub use kurbo::{self, Size};
use kyute::handler::Handler;
use kyute::layout::{Axis, LayoutInput, LayoutOutput, PaddingLeft};
use kyute::style::{Style, StyleExt};
use kyute::text::TextStyle;
use kyute::widgets::button::button;
use kyute::widgets::frame::{Frame, FrameLayout, FrameStyle};
use kyute::widgets::text::Text;
use kyute::widgets::text_edit::{TextEdit, TextOverflow, WrapMode};
use kyute::{
    application, text, Color, Component, ComponentMethods, Node, Element, Event, PaintCtx, Window,
    WindowOptions,
};
pub use skia_safe as skia;
use std::cell::Cell;
use std::future::pending;
use std::ops::Deref;
use std::rc::Rc;
use tokio::select;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;
use tracing_tree::HierarchicalLayer;

#[allow(non_camel_case_types)]
#[derive(Clone)]
enum Events {
    // properties
    value_changed,
    // events
    editing_finished,
}

struct TestComponent {
    element: Node,
    content: Rc<Frame>,
    notifier: Handler<Events>,
    value: Cell<i32>,
    value_text: Rc<Text>,
    button_up: Rc<Frame>,
    button_down: Rc<Frame>,
}

impl TestComponent {
    pub fn new() -> Rc<Component<Self>> {
        let content = Frame::new(FrameStyle::default());
        content.set_layout(FrameLayout::Flex {
            direction: Axis::Horizontal,
            gap: 4.0.into(),
            initial_gap: 4.0.into(),
            final_gap: 4.0.into(),
        });

        let value_text = Text::new(text!["0"]);
        let button_up = button("+");
        let button_down = button("-");

        content.add_child(value_text.clone());
        content.add_child(button_up.clone());
        content.add_child(button_down.clone());

        Component::new(|element| TestComponent {
            element,
            content,
            notifier: Handler::new(),
            value: Cell::new(0),
            value_text,
            button_up,
            button_down,
        })
    }
}

impl ComponentMethods for TestComponent {
    fn element(&self) -> &Node {
        &self.element
    }

    async fn task(&self) {
        self.element().add_child(self.content.clone());

        loop {
            select! {
                _ = self.button_up.clicked() => {
                    self.value.set(self.value.get() + 1);
                    self.update_text();
                    self.notifier.emit(Events::value_changed).await;
                    self.notifier.emit(Events::editing_finished).await;
                }
                _ = self.button_down.clicked() => {
                    self.value.set(self.value.get() - 1);
                    self.update_text();
                    self.notifier.emit(Events::value_changed).await;
                    self.notifier.emit(Events::editing_finished).await;
                }
            }
        }
    }
}

impl Deref for TestComponent {
    type Target = Node;

    fn deref(&self) -> &Self::Target {
        &self.element
    }
}

impl TestComponent {
    fn update_text(&self) {
        let value = self.value.get();
        self.value_text.set_text(text!["{value}"]);
    }

    pub async fn editing_finished(&self) {
        loop {
            match self.notifier.wait().await {
                Events::editing_finished => {
                    break;
                }
                _ => {}
            }
        }
    }

    pub async fn data_changed(&self) {
        loop {}
    }
}

fn main() {
    let subscriber = Registry::default().with(HierarchicalLayer::new(2).with_indent_amount(4));
    tracing::subscriber::set_global_default(subscriber).unwrap();

    application::run(async {
        let frame = Frame::new(FrameStyle {
            border_color: Color::from_hex("5f5637"),
            border_radius: 8.0.into(),
            background_color: Color::from_hex("211e13"),
            ..Default::default()
        });

        frame.set_layout(FrameLayout::Flex {
            direction: Axis::Vertical,
            gap: 4.0.into(),
            initial_gap: 4.0.into(),
            final_gap: 4.0.into(),
        });

        let counter = TestComponent::new();
        frame.add_child(counter.clone());

        ////////////////////////////////////////////////////////////////////
        let window_options = WindowOptions {
            title: "Hello, world!",
            size: Size::new(800.0, 600.0),
            background: Color::from_hex("211e13"),
            ..Default::default()
        };

        let main_window = Window::new(&window_options, &frame);

        loop {
            select! {
                _ = main_window.close_requested() => {
                    eprintln!("Window closed");
                    application::quit();
                    break
                }
                _ = counter.editing_finished() => {
                    eprintln!("Editing finished, counter value = {}", counter.value.get());
                }
            }
        }
    })
        .unwrap()
}
