use std::cell::RefCell;
use std::future::pending;
use std::marker::PhantomPinned;
use std::ops::Deref;
use std::pin::Pin;
use std::rc::Rc;

use futures_util::future::LocalBoxFuture;
use futures_util::FutureExt;
use kurbo::Size;

use crate::layout::{LayoutInput, LayoutOutput};
use crate::{Element, Event, Node, PaintCtx};
use crate::element::RcElement;

struct ComponentInner<T> {
    _pin: PhantomPinned,
    // The future must come first so that it is dropped first, otherwise the future may observe
    // a partially deleted component.
    future: RefCell<Option<LocalBoxFuture<'static, ()>>>,
    component: T,
}

pub struct ComponentPtr<T>(Pin<Rc<ComponentInner<T>>>);

impl<T> Clone for ComponentPtr<T> {
    fn clone(&self) -> Self {
        ComponentPtr(self.0.clone())
    }
}

impl<T: Component + 'static> ComponentPtr<T> {
    /// Creates a new element with the specified type and constructor.
    pub fn new(f: impl FnOnce(Node) -> T) -> ComponentPtr<T> {
        // Instantiate the component
        let inner = Node::new_derived(|element| ComponentInner {
            _pin: PhantomPinned,
            component: f(element),
            future: RefCell::new(None),
        });

        // pin it
        let inner = unsafe { Pin::new_unchecked(inner) };
        let inner_ptr = &*inner as *const _;

        let future = async move {
            // SAFETY:
            // - the pointee is pinned, so it can't move
            // - the future is dropped before the pointee so the future won't outlive the element
            let inner = unsafe { &*inner_ptr };
            inner.component.task().await;
            pending::<()>().await;
        }.boxed_local();

        // put the future in place
        inner.future.replace(Some(future));
        inner
    }
}


impl<T> Deref for ComponentPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0.component
    }
}

impl<T: Component> Element for ComponentPtr<T> {
    fn node(&self) -> &Node {
        &self.0.component.node()
    }

    fn measure(&self, _children: &[RcElement], layout_input: &LayoutInput) -> Size {
        // Defer to child element
        let children = self.component.node().children();
        if !children.is_empty() {
            children[0].do_measure(layout_input)
        } else {
            Size::ZERO
        }
    }

    fn layout(&self, _children: &[RcElement], size: Size) -> LayoutOutput {
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

    fn event(&self, event: &mut Event)
    {}
}

pub trait Component {
    fn node(&self) -> &Node;
    async fn task(&self) {}
}
