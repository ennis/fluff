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
    application, text, Color, ComponentInner, Component, Node, Element, Event, PaintCtx, Window,
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

// (PAIN) boilerplate code for events.
// NOTE: using callbacks instead of futures would simplify things, at the cost of being able to ergonomically
// access local state in the element task.
#[allow(non_camel_case_types)]
#[derive(Clone)]
enum Events {
    // properties
    value_changed,
    // events
    editing_finished,
}

struct TestComponent {
    // (PAIN) boilerplate
    node: Node,
    content: Rc<Frame>,
    // (PAIN) boilerplate code for events.
    notifier: Handler<Events>,

    // (PAIN) interior mutability is necessary for everything that needs to be accessible from the
    // parent.
    value: Cell<i32>,
    value_text: Rc<Text>,
    button_up: Rc<Frame>,
    button_down: Rc<Frame>,
}

impl TestComponent {
    // (PAIN) having to type `Rc<ComponentHolder<Self>>` is boilerplate and unintuitive.
    // Problem: there are really two interfaces: one that the ComponentHolder sees and the task sees,
    // and the interface that users of the component see.
    // Users of the component see a wrapper type, which can be annoying to work with.
    // It must also reimplement the methods, unless it derefs to the inner type.
    //
    // Solution: ComponentHolder<T> can be seen as a smart pointer type. Combine with Rc and rename to
    // something shorter.
    pub fn new() -> Rc<ComponentInner<Self>> {

        // (PAIN) building the element tree is verbose (albeit flexible), and most importantly
        // doesn't visually reflect the tree structure of the UI at a glance.
        let content = Frame::new();

        // (PAIN) setting styles is roundabout: need to call `set_style`, create the `FrameStyle`,
        // and set the corresponding field, instead of just having one line like `padding: 4px`.
        content.set_style(FrameStyle::default());
        content.set_layout(FrameLayout::Flex {
            direction: Axis::Horizontal,
            gap: 4.0.into(),
            initial_gap: 4.0.into(),
            final_gap: 4.0.into(),
        });

        let value_text = Text::new(text!["0"]);
        let button_up = button("+");
        let button_down = button("-");

        // (PAIN) tree building boilerplate (clone, add_child).
        content.add_child(value_text.clone());
        content.add_child(button_up.clone());
        content.add_child(button_down.clone());

        // (PAIN) ComponentHolder boilerplate.
        ComponentInner::new(|node| TestComponent {
            node,
            content,
            notifier: Handler::new(),
            value: Cell::new(0),
            value_text,
            button_up,
            button_down,
        })
    }
}

impl TestComponent {
    pub fn set_value(&self, value: i32) {
        // (PAIN) if the async task somehow needed to be notified of the value change,
        // `set_value` would need to be async.
        // (PAIN) no access to the node here
        self.value.set(value)
    }
}


// (PAIN) having to implement a trait, far away from the constructor.
impl Component for TestComponent {
    async fn task(&self) {

        // (PAIN) easy to forget to add the content to the node.
        self.node().add_child(self.content.clone());


        loop {
            select! {
                // (PAIN) handling events is located too far away from where the source of the event
                // is defined. This should be next to the element declaration.
                // (PAIN) too much `self`
                _ = self.button_up.clicked() => {
                    self.value.set(self.value.get() + 1);
                    self.update_text();
                    // (PAIN) signalling an event is verbose
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

// (PAIN) deref impl
impl Deref for TestComponent {
    type Target = Node;

    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

impl TestComponent {
    fn update_text(&self) {
        let value = self.value.get();
        self.value_text.set_text(text!["{value}"]);
    }

    // (PAIN) boilerplate code for events.
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


// 18 pain points
// * Boilerplate code for events.
// * Node boilerplate.
// * ComponentHolder boilerplate.
// * Interior mutability.
// * Deref impl.
// * Signalling an event is verbose.
// * Handling events is located too far away from where the source of the event is defined.
// * Too much `self`.
// * `Component::node()` boilerplate.
// * async setters
// * Building the element tree is verbose.


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
