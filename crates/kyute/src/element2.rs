use std::any::{Any, TypeId};
use std::cell::UnsafeCell;
use std::collections::BTreeMap;
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::ops::Deref;
use kurbo::{Affine, Point, Size};
use crate::element::ChangeFlags;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::{Event, PaintCtx};
use crate::tree::{NodeChildrenRefMut, NodeKey, NodeRef, NodeRefMut, Tree};
use crate::window::WeakWindow;

pub struct ElementState {
    /// Pointer to the parent owner window.
    pub(crate) window: WeakWindow,
    /// Layout: transform from local to parent coordinates.
    transform: Affine,
    /// Layout: geometry (size and baseline) of this element.
    geometry: Size,
    /// TODO unused
    change_flags: ChangeFlags,
    // List of child elements.
    //children: RefCell<Vec<AnyVisual>>,
    /// Name of the element.
    name: String,
    /// Whether the element is focusable via tab-navigation.
    focusable: bool,
    /// Map of attached properties.
    attached_properties: UnsafeCell<BTreeMap<TypeId, Box<dyn Any>>>,
}


impl ElementState {
    pub fn new() -> Self {
        Self {
            window: WeakWindow::default(),
            transform: Affine::default(),
            geometry: Size::ZERO,
            change_flags: ChangeFlags::empty(),
            name: "".to_string(),
            focusable: false,
            attached_properties: UnsafeCell::new(BTreeMap::new()),
        }
    }

    pub fn mark_needs_relayout(&mut self) {
        self.change_flags |= ChangeFlags::LAYOUT;
    }

    pub fn mark_needs_repaint(&mut self) {
        self.change_flags |= ChangeFlags::PAINT;
    }
}

pub trait Element2: Any {
    /// Asks the element to measure itself under the specified constraints, but without actually laying
    /// out the children.
    fn measure(&mut self, children: &[&mut dyn Element2], layout_input: &LayoutInput) -> Size;

    /// Specifies the size of the element, and to lays out its children.
    ///
    /// # Arguments
    /// * `children` - the children of the element.
    /// * `size` - the exact size of the element. Typically, this is one of the sizes returned by a
    /// previous call to `measure`.
    fn layout(&self, children: &[&mut dyn Element2], size: Size) -> LayoutOutput {
        todo!()
    }

    /// Hit-tests the element.
    fn hit_test(&self, point: Point) -> bool {
        todo!()
    }

    /// Paints the element.
    #[allow(unused_variables)]
    fn paint(&self, ctx: &mut PaintCtx) {}

    /// Handles an event.
    #[allow(unused_variables)]
    fn event(&mut self, event: &mut Event)
    {}

    fn as_any_mut(&mut self) -> &mut dyn Any;
}


/// A key to an element in the element tree.
#[repr(transparent)]
pub struct ElementKey<E: ?Sized>(NodeKey, PhantomData<*const E>);

impl<E: ?Sized> Clone for ElementKey<E> {
    fn clone(&self) -> Self {
        Self(self.0, PhantomData)
    }
}

impl<E: ?Sized> Copy for ElementKey<E> {}

impl<E: ?Sized> Debug for ElementKey<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ElementKey").field(&self.0).finish()
    }
}


////////////////////////////////////////////////////////////////////////////////////////////////////
struct Node {
    state: ElementState,
    element: Box<dyn Element2>,
}

impl Node {
    fn new(inner: Box<dyn Element2>) -> Self {
        Self {
            state: ElementState::new(),
            element: inner,
        }
    }
}

pub struct ElementTree {
    tree: Tree<Node>,
    root: NodeKey,
}

impl ElementTree {
    /// Creates a new element tree along with the root element.
    pub fn new<E: Element2>(root: E) -> (Self, ElementKey<E>) {
        let mut tree = Self {
            tree: Tree::new(),
            root: Default::default(),
        };
        let (key, _) = tree.create(root);
        tree.root = key.0;
        (tree, key)
    }

    /*/// Returns the root key of the tree.
    pub fn root(&self) -> ElementKey<dyn Element2> {
        ElementKey(self.root, PhantomData)
    }*/

    /// Returns a modification context on the root element.
    pub fn root_ctx(&mut self) -> TreeCtx {
        TreeCtx { current: self.tree.get_mut(self.root).unwrap() }
    }

    /// Invokes a function on an element.
    pub fn with_element<E: Element2>(&mut self, key: ElementKey<E>, f: impl FnOnce(ElementMut<E>)) {
        let (node, children) = self.tree.get_mut(key.0).unwrap().split_children();
        let state = &mut node.state;
        let element = node.element.as_any_mut().downcast_mut::<E>().expect("invalid element type");
        f(ElementMut {
            state,
            element,
            children,
        });
        // Propagate change flags up the tree.
        let change_flags = state.change_flags;
        let mut current = self.tree.get_mut(key.0).unwrap().parent();
        while let Some(mut parent) = current {
            if parent.state.change_flags.contains(change_flags) {
                break;
            }
            parent.state.change_flags |= change_flags;
            current = parent.parent();
        }
    }

    fn create<E: Element2>(&mut self, element: E) -> (ElementKey<E>, TreeCtx) {
        let refmut = self.tree.create(Node::new(Box::new(element)));
        let key = ElementKey(refmut.key(), PhantomData);
        (key, TreeCtx { current: refmut })
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Context for modifying the element tree.
pub struct TreeCtx<'a> {
    current: NodeRefMut<'a, Node>,
}

impl<'a> TreeCtx<'a> {
    /// Adds a child to the current element.
    fn add_child<E: Element2>(&mut self, element: E) -> (ElementKey<E>, TreeCtx) {
        let child_ref = self.current.insert_child(Node::new(Box::new(element)));
        let key = ElementKey(child_ref.key(), PhantomData);
        (key, TreeCtx { current: child_ref })
    }
}


////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct ElementChildrenMut<'a> {
    children: NodeChildrenRefMut<'a, Node>,
}

impl<'a> ElementChildrenMut<'a> {
    /// Adds a child to the current element.
    pub fn push<T: Element2>(&mut self, element: T) -> (ElementKey<T>, ElementMut<T>) {
        let (node, children) = self.children.push(Node::new(Box::new(element)));
        let key = ElementKey(children.key(), PhantomData);
        let element = node.element.as_any_mut().downcast_mut::<T>().unwrap();
        let state = &mut node.state;
        (key, ElementMut { element, state, children: ElementChildrenMut { children } })
    }

    /// Removes a child from the current element.
    pub fn remove<E: ?Sized>(&mut self, key: ElementKey<E>) {
        self.children.remove(key.0);
    }
}

/// A mutable reference to an element in the tree.
pub struct ElementMut<'a, E: ?Sized> {
    pub element: &'a mut E,
    pub state: &'a mut ElementState,
    pub children: ElementChildrenMut<'a>,
}

impl<'a, E: ?Sized> ElementMut<'a, E> {}

////////////////////////////////////////////////////////////////////////////////////////////////////


#[cfg(test)]
mod tests {
    use std::any::Any;
    use kurbo::Size;
    use crate::element2::{Element2, ElementMut, ElementTree, TreeCtx};
    use crate::layout::LayoutInput;

    struct TestElement(&'static str);

    impl Element2 for TestElement {
        fn measure(&mut self, children: &[&mut dyn Element2], layout_input: &LayoutInput) -> Size {
            Size::ZERO
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    impl TestElement {
        pub fn set_text(this: ElementMut<Self>, text: &'static str) {
            eprintln!("set_text: {}", text);
            this.element.0 = text;
            this.state.mark_needs_relayout();
        }

        pub fn set_color(this: ElementMut<Self>, color: u32) {
            eprintln!("set_color: {}", color);
            this.state.mark_needs_repaint();
        }
    }


    #[test]
    fn build_tree() {
        let (mut tree, root) = ElementTree::new(TestElement("root"));

        let child_1;
        let child_2;
        let child_21;
        let child_3;

        let mut cx = tree.root_ctx();

        {
            let (key, mut cx) = cx.add_child(TestElement("child_1"));
            child_1 = key;
        }
        {
            let (key, mut cx) = cx.add_child(TestElement("child_2"));
            child_2 = key;
            {
                let (key, mut cx) = cx.add_child(TestElement("child_21"));
                child_21 = key;
            }
        }
        {
            let (key, mut cx) = cx.add_child(TestElement("child_3"));
            child_3 = key;
        }
    }

    #[test]
    fn change_flag_propagation() {
        let (mut tree, root) = ElementTree::new(TestElement("root"));

        let child;
        let grandchild;
        {
            let mut cx = tree.root_ctx();

            {
                let (key, mut cx) = cx.add_child(TestElement("child"));
                child = key;
                {
                    let (key, mut cx) = cx.add_child(TestElement("grandchild"));
                    grandchild = key;
                }
            }
        }

        tree.with_element(root, |root| {
            TestElement::set_text(root, "new text");
        });

        tree.with_element(grandchild, |grandchild| {
            TestElement::set_color(grandchild, 0xff00ff);
        });
    }
}
