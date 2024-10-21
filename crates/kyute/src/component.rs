use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use futures_util::future::AbortHandle;
use kurbo::Size;

use crate::application::spawn;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::{Node, Element, Event, PaintCtx};

pub struct Component<T> {
    pub component: Rc<T>,
    abort_handle: RefCell<Option<AbortHandle>>,
}

impl<T: ComponentMethods + 'static> Component<T> {
    /// Creates a new element with the specified type and constructor.
    pub fn new(f: impl FnOnce(Node) -> T) -> Rc<Component<T>> {
        let component = Node::new_derived(|element| Component {
            component: Rc::new(f(element)),
            abort_handle: RefCell::new(None),
        });

        let component_clone = component.component.clone();
        let abort_handle = spawn(async move {
            component_clone.task().await;
        });

        component.abort_handle.replace(Some(abort_handle));
        component
    }
}

impl<T> Drop for Component<T> {
    fn drop(&mut self) {
        if let Some(abort_handle) = self.abort_handle.borrow_mut().take() {
            abort_handle.abort();
        }
    }
}

impl<T> Deref for Component<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.component
    }
}

impl<T: ComponentMethods> Element for Component<T> {
    fn node(&self) -> &Node {
        &self.component.element()
    }

    fn measure(&self, _children: &[Rc<dyn Element>], layout_input: &LayoutInput) -> Size {
        // Defer to child element
        let children = self.component.element().children();
        if !children.is_empty() {
            children[0].do_measure(layout_input)
        } else {
            Size::ZERO
        }
    }

    fn layout(&self, _children: &[Rc<dyn Element>], size: Size) -> LayoutOutput {
        // Defer to child element
        let children = self.component.element().children();
        if !children.is_empty() {
            children[0].do_layout(size)
        } else {
            LayoutOutput::default()
        }
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        for child in self.component.element().children().iter() {
            child.paint(ctx)
        }
    }

    async fn event(&self, event: &mut Event)
    where
        Self: Sized,
    {}
}

pub trait ComponentMethods {
    fn element(&self) -> &Node;
    async fn task(&self) {}
}
