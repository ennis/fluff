use std::any::{Any, TypeId};
use std::cell::{Cell, Ref, RefCell, UnsafeCell};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::marker::PhantomPinned;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr::addr_eq;
use std::rc::{Rc, UniqueRc, Weak};
use std::{mem, ptr};
use std::error::Request;
use std::hash::{Hash, Hasher};
use crate::compositor::DrawableSurface;
use bitflags::bitflags;
use futures_util::FutureExt;
use kurbo::{Affine, Point, Size, Vec2};
use winit::window::WindowId;
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::model::Model;
use crate::window::{WeakWindow, WindowInner};
use crate::{PaintCtx, Window};

bitflags! {
    #[derive(Copy, Clone, Default)]
    pub struct ChangeFlags: u32 {
        const PAINT = 0b0001;
        const LAYOUT = 0b0010;
        const NONE = 0b0000;
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
pub trait AttachedProperty: Any {
    type Value: Clone;

    fn set(self, item: &Node, value: Self::Value)
    where
        Self: Sized,
    {
        item.set(self, value);
    }

    fn get(self, item: &Node) -> Option<Self::Value>
    where
        Self: Sized,
    {
        item.get(self)
    }

    /// Returns a reference to the attached property.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the nothing mutates the value of the attached property
    /// while the returned reference is alive.
    unsafe fn get_ref(self, item: &Node) -> &Self::Value
    where
        Self: Sized,
    {
        item.get_ref(self)
    }
}

/*
/// Depth-first traversal of the visual tree.
pub struct Cursor {
    next: Option<RcElement>,
}

impl Iterator for Cursor {
    type Item = RcElement;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(next) = self.next.clone() else {
            return None;
        };
        let index = next.index_in_parent.get();
        let parent_child_count = next.parent().map(|p| p.child_count()).unwrap_or(0);
        let child_count = next.child_count();
        if child_count > 0 {
            self.next = Some(next.children.borrow()[0].clone());
        } else if index + 1 < parent_child_count {
            self.next = Some(next.parent().unwrap().children.borrow()[index + 1].clone());
        } else {
            // go up until we find a parent with a next sibling
            let mut parent = next.parent();
            self.next = None;
            while let Some(p) = parent {
                if let Some(next) = p.next() {
                    self.next = Some(next);
                    break;
                }
                parent = p.parent();
            }
        }

        Some(next)
    }
}
*/

/// Weak reference to an element in the element tree.
#[derive(Clone)]
pub struct WeakElementAny(Weak<RefCell<Node<dyn Element>>>);

impl WeakElementAny {
    pub fn upgrade(&self) -> Option<ElementAny> {
        todo!()
    }
}

impl Default for WeakElementAny {
    fn default() -> Self {
        todo!()
    }
}

// Element refs are compared by pointer equality.
impl PartialEq for WeakElementAny {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for WeakElementAny {}

impl Hash for WeakElementAny {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Weak::as_ptr(&self.0).hash(state)
    }
}

/// Strong reference to an element in the element tree.
#[derive(Clone)]
pub struct ElementAny(Rc<RefCell<Node<dyn Element>>>);

impl ElementAny {
    /// Returns a weak reference to this element.
    pub fn downgrade(&self) -> WeakElementAny {
        WeakElementAny(Rc::downgrade(&self.0))
    }

    /// Returns whether this element has a parent.
    pub fn has_parent(&self) -> bool {
        // TODO: maybe not super efficient
        self.parent().is_some()
    }

    /// Sets the parent of this element.
    pub fn set_parent(&self, parent: WeakElementAny) {
        todo!()
    }

    /// Returns the parent of this element, if it has one.
    pub fn parent(&self) -> Option<ElementAny> {
        self.0.header.borrow().parent.upgrade()
    }


    /// Marks this element as needing a repaint.
    // ISSUE: this can't be called inside an element method because it only has access
    // to the element, not the containing `Node`.
    // Most of the time, relayout & repaints should be requested by the element for the element itself.
    // So this should be a method on a context object passed to element methods instead.
    // Upward propagation is done when the method returns.

    //pub fn mark_needs_repaint(&self) {
    //    self.set_dirty_flags(ChangeFlags::PAINT);
    //}

    // Marks this element as needing a relayout.
    //pub fn mark_needs_relayout(&self) {
    //    self.set_dirty_flags(ChangeFlags::LAYOUT | ChangeFlags::PAINT);
    //}

    pub(crate) fn mark_layout_done(&self) {
        let f = &self.0.shared.change_flags;
        f.set(f.get() & !ChangeFlags::LAYOUT);
    }

    pub(crate) fn mark_paint_done(&self) {
        let f = &self.0.shared.change_flags;
        f.set(f.get() & !ChangeFlags::PAINT);
    }

    pub fn needs_relayout(&self) -> bool {
        self.0.shared.change_flags.get().contains(ChangeFlags::LAYOUT)
    }

    pub fn needs_repaint(&self) -> bool {
        self.0.shared.change_flags.get().contains(ChangeFlags::PAINT)
    }

    pub fn measure(&self, layout_input: &LayoutInput) -> Size {
        let ref mut node = *self.0.header.borrow_mut();
        let ref mut inner = *self.0.inner.borrow_mut();
        let children = &node.children[..];
        inner.measure(children, layout_input)
    }

    pub fn layout(&self, size: Size) -> LayoutOutput {
        let ref mut node = *self.0.header.borrow_mut();
        let ref mut inner = *self.0.inner.borrow_mut();
        let children = &node.children[..];
        let result = inner.layout(children, size);
        self.0.shared.change_flags.set(self.0.shared.change_flags.get().difference(ChangeFlags::LAYOUT));
        result
    }

    /*pub fn invoke<R>(&self, window_ctx: &mut WindowCtx, f: impl FnOnce(&mut dyn Element, &mut ElementCtx) -> R) -> R {
        let ref mut header = *self.0.header.borrow_mut();
        let ref mut inner = *self.0.inner.borrow_mut();
        let mut ctx = ElementCtx {
            this: self.downgrade(),
            window_ctx: window_ctx,
            header,
        };
        f(&mut inner, &mut ctx)
    }*/

    /// Hit-tests this element and its children.
    pub(crate) fn hit_test(&self, point: Point) -> Vec<ElementAny> {
        // Helper function to recursively hit-test the children of a visual.
        // point: point in the local coordinate space of the visual
        // transform: accumulated transform from the local coord space of `visual` to the root coord space
        fn hit_test_rec(
            node: &ElementAny,
            point: Point,
            transform: Affine,
            result: &mut Vec<ElementAny>,
        ) -> bool {
            let header = node.0.header.borrow_mut();
            let mut element = node.0.inner.borrow();

            let mut hit = false;
            // hit-test ourselves
            if element.hit_test(point) {
                hit = true;
                result.push(node.clone());
            }

            for child in header.children.borrow().iter() {
                let child_transform = child.0.header.transform;
                let transform = transform * child_transform;
                let local_point = transform.inverse() * point;
                if hit_test_rec(child.clone(), local_point, transform, result) {
                    hit = true;
                    break;
                }
            }

            hit
        }

        let mut path = Vec::new();
        hit_test_rec(self, point, self.transform(), &mut path);
        path
    }

    pub fn paint(&self, surface: &DrawableSurface, scale_factor: f64) {
        let size = self.0.borrow().header.geometry;
        let mut paint_ctx = PaintCtx {
            scale_factor,
            window_transform: Default::default(),
            surface,
            size,
        };

        // Recursively paint the UI tree.
        fn paint_rec(node: &Node, ctx: &mut PaintCtx) {
            node.inner.borrow_mut().paint(ctx);
            for child in node.header.children.iter() {
                ctx.with_transform(&child.transform(), |ctx| {
                    // TODO clipping
                    paint_rec(&**child, ctx);
                    child.mark_paint_done();
                });
            }
        }

        paint_rec(self, &mut paint_ctx);
    }

    pub fn add_offset(&self, offset: Vec2) {
        self.0.borrow_mut().header.add_offset(offset)
    }

    pub fn set_offset(&self, offset: Vec2) {
        self.0.borrow_mut().header.set_offset(offset)
    }

    /// Returns the list of ancestors of this visual, plus this visual itself, sorted from the root
    /// to this visual.
    pub fn ancestors_and_self(&self) -> Vec<ElementAny> {
        let mut ancestors = Vec::new();
        let mut current = self.clone();
        while let Some(parent) = current.parent() {
            ancestors.push(parent.clone());
            current = parent;
        }
        ancestors.reverse();
        ancestors.push(self.clone());
        ancestors
    }


    /// Returns a reference to the specified attached property.
    ///
    /// # Panics
    /// Panics if the attached property is not set.
    ///
    /// # Safety
    ///
    /// This function unsafely borrows the value of the attached property.
    /// The caller must ensure that the value isn't mutated (via `set`) while the reference is alive.
    ///
    /// In most cases you should prefer `get` over this function.
    pub unsafe fn get_ref<T: AttachedProperty>(&self, property: T) -> &T::Value {
        self.try_get_ref::<T>(property).expect("attached property not set")
    }

    /// Returns a reference to the specified attached property, or `None` if it is not set.
    ///
    /// # Safety
    ///
    /// Same contract as `get_ref`.
    pub unsafe fn try_get_ref<T: AttachedProperty>(&self, _property: T) -> Option<&T::Value> {
        let attached_properties = unsafe { &*self.attached_properties.get() };
        attached_properties
            .get(&TypeId::of::<T>())
            .map(|v| v.downcast_ref::<T::Value>().expect("invalid type of attached property"))
    }
}

/// Trait for elements that can be converted into a `ElementAny`.
///
/// This is implemented for all types that implement `Element`.
pub trait IntoElementAny {
    fn into_element(self, parent: WeakElementAny, index_in_parent: usize) -> ElementAny;
}

impl<T> IntoElementAny for T
where
    T: Element,
{
    fn into_element(self, parent: WeakElementAny, index_in_parent: usize) -> ElementAny {
        let mut node = UniqueRc::new(RefCell::new(Node::new(self)));
        let weak = WeakElementAny(UniqueRc::downgrade(&node));
        let header = &mut node.get_mut().header;
        header.weak_this = weak;
        header.parent = parent;
        header.index_in_parent = index_in_parent;
        ElementAny(UniqueRc::into_rc(node))
    }
}


struct NodeHeader {
    parent: WeakElementAny,
    // Weak pointer to this element.
    weak_this: WeakElementAny,
    children: Vec<ElementAny>,
    index_in_parent: usize,
    change_flags: ChangeFlags,
    /// Pointer to the parent owner window.
    pub(crate) window: WeakWindow,
    /// Layout: transform from local to parent coordinates.
    transform: Affine,
    /// Layout: geometry (size and baseline) of this element.
    geometry: Size,
    /// Name of the element.
    name: String,
    /// Whether the element is focusable via tab-navigation.
    focusable: bool,
    /// Map of attached properties.
    attached_properties: BTreeMap<TypeId, Box<dyn Any>>,
}

impl NodeHeader {
    pub fn set_offset(&mut self, offset: Vec2) {
        self.transform = Affine::translate(offset);
    }

    pub fn add_offset(&mut self, offset: Vec2) {
        self.transform *= Affine::translate(offset);
    }
}

struct Node<T: Element + ?Sized = dyn Element> {
    header: NodeHeader,
    inner: RefCell<T>,
}

impl<T> Node<T> {
    fn new(inner: T) -> Node<T> {
        Node {
            header: NodeHeader {
                weak_this: WeakElementAny::default(),
                children: Default::default(),
                index_in_parent: Default::default(),
                change_flags: Cell::new(Default::default()),
                window: Default::default(),
                parent: Default::default(),
                transform: Affine::default(),
                geometry: Size::default(),
                name: String::new(),
                focusable: false,
                attached_properties: Default::default(),
            },
            inner: RefCell::new(inner),
        }
    }

    pub fn set_parent(&mut self, parent: WeakElementAny) {
        self.header.parent = parent;
    }

    /*/// Detaches this element from the tree.
    pub fn detach(&self) {
        if let Some(parent) = self.parent() {
            // remove from parent's children
            let mut children = parent.children.borrow_mut();
            let index = self.index_in_parent.get();
            children.remove(index);
            // update the indices of the siblings
            for i in index..children.len() {
                children[i].index_in_parent.set(i);
            }
            parent.mark_needs_relayout();
        }

        self.parent.reset();
    }*/

    fn insert_child_at(&mut self, at: usize, to_insert: ElementAny) {
        assert!(!to_insert.has_parent(), "element already has a parent");

        to_insert.0.borrow_mut().header.parent = self.header.weak_this.clone();

        //to_insert.set_parent_window(self.window.clone());
        // SAFETY: no other references may exist to the children vector at this point,
        // provided the safety contracts of other unsafe methods are upheld.
        assert!(at <= self.header.children.len());
        self.header.children.insert(at, to_insert);
        for i in at..self.header.children.len() {
            self.header.children[i].0.borrow_mut().header.index_in_parent = i;
        }
        self.mark_needs_relayout();
    }

    /*pub fn insert_child_at<T: Element>(&self, at: usize, to_insert: T) -> Pin<Rc<T>> {
        let pinned = Rc::pin(to_insert);
        self.insert_pinned_child_at(at, pinned.clone());
        pinned
    }*/

    /*/// Inserts the specified element after this element.
    pub fn insert_after(&self, to_insert: RcElement) {
        if let Some(parent) = self.parent.upgrade() {
            parent.insert_child_at(self.index_in_parent.get() + 1, to_insert)
        } else {
            panic!("tried to insert after an element with no parent");
        }
    }*/

    /// Inserts the specified element at the end of the children of this element.
    pub fn add_child(&mut self, child: impl Into<ElementAny>) {
        let len = self.header.children.len();
        self.insert_child_at(len, child.into());
    }


    /*/// Returns a slice of all children of this element.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no mutable references to the children vector exist, or are created,
    /// while the returned slice is alive. In practice, this means that the caller should not:
    /// - `detach` any children of this element
    /// - call `insert_child_at` or `add_child` on this element
    pub unsafe fn children_ref(&self) -> &[RcElement] {
        &*self.children.try_borrow_unguarded().unwrap()
    }

    /// Returns a reference to the child element as the specified index.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no mutable references to the children vector exist, or are created,
    /// while the returned object is alive. In practice, this means that the caller should not:
    /// - `detach` any children of this element
    /// - call `insert_child_at` or `add_child` on this element
    pub unsafe fn child_ref_at(&self, index: usize) -> &dyn Element {
        &**self.children_ref().get(index).expect("child index out of bounds")
    }

    /// Returns a reference to the list of children of this element.
    pub fn children(&self) -> Ref<[RcElement]> {
        Ref::map(self.children.borrow(), |v| v.as_slice())
    }*/

    /// Returns a cursor at this element
    pub fn cursor(&self) -> Cursor {
        Cursor { next: Some(self.rc()) }
    }

    pub(crate) fn set_parent_window(&self, window: WeakWindow) {
        if !Weak::ptr_eq(&self.window.borrow().shared, &window.shared) {
            self.window.replace(window.clone());
            // recursively update the parent window of the children
            for child in self.children().iter() {
                child.set_parent_window(window.clone());
            }
        }
    }

    /// Returns the next focusable element.
    pub fn next_focusable_element(&self) -> Option<RcElement> {
        let mut cursor = self.cursor();
        cursor.next(); // skip self
        while let Some(node) = cursor.next() {
            if node.focusable.get() {
                return Some(node);
            }
        }
        None
    }

    /*
    /// Returns an iterator over this element's children.
    pub fn iter_children(&self) -> impl Iterator<Item=Rc<dyn ElementMethods>> {
        SiblingIter {
            next: self.first_child.get(),
        }
    }*/

    /*/// Requests focus for the current element.
    pub fn set_focus(&self) {
        self.header.window.borrow().set_focus(self.weak());
    }*/

    pub fn set_tab_focusable(&self, focusable: bool) {
        self.focusable.set(focusable);
    }

    /*pub fn set_pointer_capture(&self) {
        self.window.borrow().set_pointer_capture(self.weak());
    }*/

    /*pub fn children(&self) -> Ref<[AnyVisual]> {
        Ref::map(self.children.borrow(), |v| v.as_slice())
    }*/

    pub fn size(&self) -> Size {
        self.geometry.get()
    }

    pub fn name(&self) -> String {
        self.name.borrow().clone()
    }

    /// Returns whether this element has focus.
    pub fn has_focus(&self) -> bool {
        self.window.borrow().is_focused(self)
    }

    /// Removes all child visuals.
    pub fn clear_children(&self) {
        for c in self.children().iter() {
            // TODO: don't do that if there's only one reference remaining
            // detach from window
            c.window.replace(WeakWindow::default());
            // detach from parent
            c.parent.reset();
        }
    }

    /*/// Returns the parent of this visual, if it has one.
    pub fn parent(&self) -> Option<RcElement> {
        self.parent.upgrade()
    }*/

    /// Returns the transform of this visual relative to its parent.
    ///
    /// Shorthand for `self.element().transform.get()`.
    pub fn transform(&self) -> Affine {
        self.transform.get()
    }

    /// This should be called by `Visual::layout()` so this doesn't set the layout dirty flag.
    pub fn set_transform(&self, transform: Affine) {
        self.transform.set(transform);
    }


    /// Returns the transform from this visual's coordinate space to the coordinate space of the parent window.
    ///
    /// This walks up the parent chain and multiplies the transforms, so consider reusing the result instead
    /// of calling this function multiple times.
    pub fn window_transform(&self) -> Affine {
        let mut transform = self.transform();
        let mut parent = self.parent();
        while let Some(p) = parent {
            transform *= p.transform();
            parent = p.parent();
        }
        transform
    }


    /// Returns this visual as a reference-counted pointer.
    pub fn rc(&self) -> RcElement {
        self.weak_this.upgrade().unwrap()
    }

    pub fn weak(&self) -> WeakElement {
        self.header.weak_this.clone()
    }

    /*
    /// Returns the list of children.
    pub fn children(&self) -> Vec<Rc<dyn ElementMethods + 'static>> {
        // traverse the linked list
        self.iter_children().collect()
    }*/


    /// Sets the dirty flags on the element and propagates them upwards.
    fn set_dirty_flags(&self, flags: ChangeFlags) {
        let flags = self.shared.change_flags.get() | flags;
        self.shared.change_flags.set(flags);
        if let Some(parent) = self.shared.parent.upgrade() {
            parent.0.set_dirty_flags(flags);
        }
        if flags.contains(ChangeFlags::PAINT) {
            // TODO: maybe don't call repaint for every widget in the hierarchy. winit should coalesce repaint requests, but still
            self.header.window.borrow().request_repaint()
        }
    }

    pub fn mark_needs_repaint(&self) {
        self.set_dirty_flags(ChangeFlags::PAINT);
    }

    pub fn mark_needs_relayout(&self) {
        self.set_dirty_flags(ChangeFlags::LAYOUT | ChangeFlags::PAINT);
    }

    pub(crate) fn mark_layout_done(&self) {
        self.change_flags.set(self.change_flags.get() & !ChangeFlags::LAYOUT);
    }

    pub(crate) fn mark_paint_done(&self) {
        self.change_flags.set(self.change_flags.get() & !ChangeFlags::PAINT);
    }

    pub fn needs_relayout(&self) -> bool {
        self.change_flags.get().contains(ChangeFlags::LAYOUT)
    }

    pub fn needs_repaint(&self) -> bool {
        self.change_flags.get().contains(ChangeFlags::PAINT)
    }

    /// Returns the next sibling of this element, if it has one.
    pub fn next(&self) -> Option<ElementAny> {
        if let Some(parent) = self.parent() {
            // ISSUE: the parent node header may be inaccessible, because
            // there might be an active mutable borrow in the call stack.

            let index_in_parent = self.index_in_parent.get();
            // SAFETY: no mutable references may exist to the children vector at this point,
            // provided the safety contracts of other unsafe methods are upheld.
            // There may be other shared references but that's not an issue.
            parent.children().get(index_in_parent + 1).cloned()
        } else {
            None
        }
    }

    /// Sets the value of an attached property.
    ///
    /// This replaces the value if the property is already set.
    pub fn set<T: AttachedProperty>(&self, _property: T, value: T::Value) {
        // SAFETY: no other references to the BTreeMap exist at this point (provided
        // the safety contract of the unsafe method `get_ref` is upheld), and this method cannot
        // call itself recursively.
        let attached_properties = unsafe { &mut *self.attached_properties.get() };
        attached_properties.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Gets the value of an attached property.
    pub fn get<T: AttachedProperty>(&self, property: T) -> Option<T::Value> {
        // SAFETY: no other mutable references to the BTreeMap exists at this point (the only one
        // is localized entirely within `set`).
        unsafe { self.try_get_ref(property).cloned() }
    }
}


impl ElementCtx {
    /// Reads the value from the specified model and declares a dependency on it. The block that
    /// was passed this context will be called again when the value changes.
    pub fn read<T: Clone>(&mut self, model: Model<T>) -> T {
        //model.on_change
        todo!()
    }

    /// Marks this element as needing a relayout
    pub fn mark_needs_relayout(&mut self) {
        todo!()
    }

    /// Mounts or remounts a child element in the element tree.
    pub fn remount(&mut self, child: &ElementAny) {
        todo!()
    }

    /// Measures a child element.
    pub fn measure(&mut self, child: &ElementAny) {
        todo!()
    }
}


pub struct WindowCtx {
    request_pointer_capture: Option<WeakElementAny>,
    request_focus: Option<WeakElementAny>,
    request_repaint: bool,
    request_relayout: bool,
}


/// Context passed to mutable methods of the `Element` trait.
///
/// Allows an element to mount/unmount children, request repaints or relayouts, and declare
/// dependencies on changing `Model` values.
pub struct ElementCtx<'a> {
    this: WeakElementAny,
    window_ctx: &'a mut WindowCtx,
    node: &'a mut Node<dyn Element>,
    dirty_flags: ChangeFlags,
}


impl<'a> ElementCtx<'a> {
    /// Returns the number of children of this element.
    pub fn child_count(&self) -> usize {
        // SAFETY: no mutable references may exist to the children vector at this point,
        // provided the safety contracts of other unsafe methods are upheld.
        self.node.header.children.len()
    }

    /// Returns the child element at the specified index.
    pub fn child_at(&self, index: usize) -> Option<ElementAny> {
        // SAFETY: same as `child_count`
        self.node.header.children.get(index).cloned()
    }

    pub fn mark_needs_repaint(&mut self) {
        self.dirty_flags |= ChangeFlags::PAINT;
    }

    pub fn mark_needs_relayout(&mut self) {
        self.dirty_flags |= ChangeFlags::LAYOUT | ChangeFlags::PAINT;
    }
}

pub struct EventCtx<'a> {
    ctx: ElementCtx<'a>,
}

impl<'a> EventCtx<'a> {
    pub fn mark_needs_repaint(&mut self) {
        self.ctx.mark_needs_repaint()
    }

    pub fn mark_needs_relayout(&mut self) {
        self.ctx.mark_needs_relayout()
    }
}

/// Context passed to `Element::measure`.
pub struct MeasureCtx<'a> {
    /// Parent window.
    pub(crate) window: &'a WindowInner,
    /// The children of the element being measured.
    pub children: &'a [ElementAny],
}

/// Context passed to `Element::layout`.
pub struct LayoutCtx<'a> {
    /// The children of the element being laid out.
    pub children: &'a [ElementAny],
}

/// Methods of elements in the element tree.
pub trait Element {
    /// Asks the element to measure itself under the specified constraints, but without actually laying
    /// out the children.
    fn measure(&mut self, children: &[ElementAny], layout_input: &LayoutInput) -> Size;

    /// Specifies the size of the element, and lays out its children.
    ///
    /// # Arguments
    /// * `children` - the children of the element.
    /// * `size` - the exact size of the element. Typically, this is one of the sizes returned by a
    /// previous call to `measure`.
    fn layout(&mut self, children: &[ElementAny], size: Size) -> LayoutOutput;

    /// Called to perform hit-testing on the bounds of this element.
    fn hit_test(&self, point: Point) -> bool;

    /// Paints this element on a target surface using the specified `PaintCtx`.
    #[allow(unused_variables)]
    fn paint(&mut self, ctx: &mut PaintCtx);

    /// Called when an event is sent to this element.
    #[allow(unused_variables)]
    fn event(&mut self, ctx: &mut EventCtx, event: &mut Event)
    {}
}


/// A wrapper for elements as they are being constructed.
///
/// Basically this is a wrapper around Rc that provides a `DerefMut` impl since we know it's the
/// only strong reference to it.
pub struct ElementBuilder<T>(UniqueRc<Node<T>>);

impl<T: Default> Default for ElementBuilder<T> {
    fn default() -> Self {
        ElementBuilder::new(Default::default())
    }
}

impl<T> ElementBuilder<T> {
    /// Creates a new `ElementBuilder` instance.
    pub fn new(inner: T) -> ElementBuilder<T> {
        let mut urc = UniqueRc::new(Node::new(inner));
        urc.header.get_mut().weak_this = WeakElementAny(UniqueRc::downgrade(&urc));
        ElementBuilder(urc)
    }

    /// Assigns a name to the element, for debugging purposes.
    pub fn debug_name(mut self, name: impl Into<String>) -> Self {
        self.0.header.get_mut().name.replace(name.into());
        self
    }
}

impl<T: Element> ElementBuilder<T> {
    /// Adds a new child element.
    pub fn add_child(mut self, child: impl IntoElementAny) -> Self {
        let index = self.0.header.get_mut().children.len();
        self.insert_child_at_index(child, index)
    }

    /// Inserts a new child element at the specified index.
    pub fn insert_child_at_index(mut self, child: impl IntoElementAny, index: usize) -> Self {
        let parent = WeakElementAny(UniqueRc::downgrade(&self.0));
        let children = &mut self.0.header.get_mut().children;
        assert!(index <= children.len());
        let child = child.into_element(parent, index);
        children.insert(index, child);
        self
    }
}

impl<T> Deref for ElementBuilder<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // SAFETY:
            // The `UniqueRc` cannot be cloned so there aren't any aliasing exclusive references
            // to the inner element. The only way to obtain an exclusive reference is through the
            // `DerefMut` impl, which borrows the whole `ElementBuilder`, and thus would prevent
            // `deref` from being called at the same time.
            self.0.inner.try_borrow_unguarded().unwrap_unchecked()
        }
    }
}

impl<T> DerefMut for ElementBuilder<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // We have mutable access to the inner element, so we can safely return a mutable reference.
        self.0.inner.get_mut()
    }
}

impl<T: Element> IntoElementAny for ElementBuilder<T> {
    fn into_element(self, parent: WeakElementAny, index_in_parent: usize) -> ElementAny {
        let header = self.0.header.get_mut();
        header.weak_this = WeakElementAny(UniqueRc::downgrade(&self.0));
        header.parent = parent;
        header.index_in_parent = index_in_parent;
        ElementAny(UniqueRc::into_rc(self.0))
    }
}


/// An entry in the hit-test chain that leads to the visual that was hit.
#[derive(Clone)]
pub struct HitTestEntry {
    /// The visual in the chain.
    pub element: Rc<dyn Element>,
    // Transform from the visual's CS to the CS of the visual on which `do_hit_test` was called (usually the root visual of the window).
    //pub root_transform: Affine,
}

impl PartialEq for HitTestEntry {
    fn eq(&self, other: &Self) -> bool {
        self.element.is_same(&*other.element)
    }
}

impl Eq for HitTestEntry {}

impl dyn Element + '_ {
    /*pub fn children(&self) -> Ref<[AnyVisual]> {
        self.element().children()
    }*/

    pub fn set_name(&self, name: impl Into<String>) {
        self.node().name.replace(name.into());
    }

    /// Identity comparison.
    pub fn is_same(&self, other: &dyn Element) -> bool {
        // It's probably OK to compare the addresses directly since they should be allocated with
        // Rcs, which always allocates even with ZSTs.
        addr_eq(self, other)
    }

    /*/// Returns the number of children of this visual.
    pub fn child_count(&self) -> usize {
        self.element().children.borrow().len()
    }*/

    /*
        pub fn do_layout(&self, size: Size) -> LayoutOutput {
            let children = self.children();
            self.geometry.set(size);
            let output = self.layout(&*children, size);
            self.mark_layout_done();
            output
        }
    */
    pub fn send_event(&self, event: &mut Event) {
        self.event(event);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

