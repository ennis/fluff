use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomPinned;
use std::ops::Deref;
use std::rc::Rc;

use futures_util::future::AbortHandle;
use kurbo::Size;

use crate::application::spawn;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::{Node, Element, Event, PaintCtx};

pub struct ComponentHolder<T: Component> {
    _pin: PhantomPinned,
    // The future must come first so that it is dropped first, otherwise the future may observe
    // a partially deleted component.
    future: RefCell<Option<Box<dyn Future<Output=()> + 'static>>>,
    component: T,
}

impl<T: Component + 'static> ComponentHolder<T> {
    /// Creates a new element with the specified type and constructor.
    pub fn new(f: impl FnOnce(Node) -> T) -> Rc<ComponentHolder<T>> {


        // Instantiate the component
        let component = Node::new_derived(|element| ComponentHolder {
            _pin: PhantomPinned,
            component: f(element),
            future: RefCell::new(None),
        });


        let component_clone = component.component.clone();
        let abort_handle = spawn(async move {
            component_clone.task().await;
        });

        component.abort_handle.replace(Some(abort_handle));
        component
    }
}

impl<T> Drop for ComponentHolder<T> {
    fn drop(&mut self) {
        if let Some(abort_handle) = self.abort_handle.borrow_mut().take() {
            abort_handle.abort();
        }
    }
}

impl<T> Deref for ComponentHolder<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.component
    }
}

impl<T: Component> Element for ComponentHolder<T> {
    fn node(&self) -> &Node {
        &self.component.node()
    }

    fn measure(&self, _children: &[Rc<dyn Element>], layout_input: &LayoutInput) -> Size {
        // Defer to child element
        let children = self.component.node().children();
        if !children.is_empty() {
            children[0].do_measure(layout_input)
        } else {
            Size::ZERO
        }
    }

    fn layout(&self, _children: &[Rc<dyn Element>], size: Size) -> LayoutOutput {
        // Defer to child element
        let children = self.component.node().children();
        if !children.is_empty() {
            children[0].do_layout(size)
        } else {
            LayoutOutput::default()
        }
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        for child in self.component.node().children().iter() {
            child.paint(ctx)
        }
    }

    async fn event(&self, event: &mut Event)
    where
        Self: Sized,
    {}
}

pub trait Component {
    fn node(&self) -> &Node;
    async fn task(&self) {}
}
