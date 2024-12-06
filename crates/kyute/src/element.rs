use std::any::{Any, TypeId};
use std::cell::{Cell, Ref, RefCell, UnsafeCell};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::marker::PhantomPinned;
use std::{mem, ptr};
use std::ops::{Deref, Range};
use std::pin::Pin;
use std::ptr::addr_eq;
use std::rc::{Rc, Weak};

use crate::compositor::DrawableSurface;
use bitflags::bitflags;
use futures_util::future::LocalBoxFuture;
use futures_util::FutureExt;
use kurbo::{Affine, Point, Size, Vec2};
use pin_weak::rc::PinWeak;
use tracing::warn;

use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput, Measurements};
use crate::window::WeakWindow;
use crate::PaintCtx;
use crate::widgets::frame::Frame;

bitflags! {
    #[derive(Copy, Clone, Default)]
    pub struct ChangeFlags: u32 {
        const PAINT = 0b0001;
        const LAYOUT = 0b0010;
        const NONE = 0b0000;
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Pinned strong pointer with reference equality semantics.
#[repr(transparent)]
pub struct RcElement<T: ?Sized = dyn Element>(Pin<Rc<T>>);

impl<T> RcElement<T> {
    pub fn new(element: T) -> Self {
        RcElement(Rc::pin(element))
    }

    pub fn new_cyclic(f: impl FnOnce(WeakElement<T>) -> T) -> Self {
        unsafe {
            RcElement(
                Pin::new_unchecked(Rc::new_cyclic(move |weak| {
                    let weak = WeakElement(UnsafeCell::new(Some(weak.clone())));
                    f(weak)
                })))
        }
    }
}

impl<T: ?Sized> RcElement<T> {
    pub fn as_ptr(&self) -> *const T {
        self.0.deref()
    }

    pub fn downgrade(rc: Self) -> WeakElement<T> {
        WeakElement(UnsafeCell::new(Some(Rc::downgrade(unsafe { &Pin::into_inner_unchecked(rc.0) }))))
    }
}

impl<T: ?Sized> Deref for RcElement<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: ?Sized> Clone for RcElement<T> {
    fn clone(&self) -> Self {
        RcElement(self.0.clone())
    }
}

impl<T: ?Sized, U: ?Sized> PartialEq<RcElement<U>> for RcElement<T> {
    fn eq(&self, other: &RcElement<U>) -> bool {
        ptr::addr_eq(self.0.deref(), other.0.deref())
    }
}

impl<T: ?Sized> Eq for RcElement<T> {}

impl<T: ?Sized> PartialOrd for RcElement<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: ?Sized> Ord for RcElement<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.0.deref() as *const T).cmp(&(other.0.deref() as *const T))
    }
}

// Workaround for lack of unsized coercion
impl<T: Element + 'static> From<RcElement<T>> for RcElement {
    fn from(value: RcElement<T>) -> Self {
        RcElement(value.0)
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Pinned, nullable weak pointer.
#[repr(transparent)]
pub struct WeakElement<T: ?Sized = dyn Element>(UnsafeCell<Option<std::rc::Weak<T>>>);

impl<T: ?Sized> WeakElement<T> {
    pub fn new() -> Self {
        WeakElement(UnsafeCell::new(None))
    }

    pub fn upgrade(&self) -> Option<RcElement<T>> {
        unsafe {
            (&*self.0.get()).as_ref().and_then(std::rc::Weak::upgrade).map(|ptr| unsafe { RcElement(Pin::new_unchecked(ptr)) })
        }
    }

    pub fn set(&self, other: WeakElement<T>) {
        unsafe {
            mem::swap(&mut *self.0.get(), &mut *other.0.get());
        }
    }

    pub fn replace(&self, ptr: WeakElement<T>) -> WeakElement<T> {
        unsafe {
            mem::swap(&mut *self.0.get(), &mut *ptr.0.get());
            ptr
        }
    }

    pub fn reset(&self) {
        unsafe {
            *self.0.get() = None;
        }
    }
}

impl<T: ?Sized> Default for WeakElement<T> {
    fn default() -> Self {
        WeakElement::new()
    }
}

impl<T: ?Sized> Clone for WeakElement<T> {
    fn clone(&self) -> Self {
        WeakElement(UnsafeCell::new(unsafe { (*self.0.get()).clone() }))
    }
}

impl<T: ?Sized> PartialEq for WeakElement<T> {
    fn eq(&self, other: &Self) -> bool {
        unsafe {
            match (&*self.0.get(), &*other.0.get()) {
                (Some(ptr), Some(other)) => {
                    ptr.ptr_eq(other)
                }
                (None, None) => true,
                _ => false,
            }
        }
    }
}

impl<T: ?Sized> PartialEq<RcElement<T>> for WeakElement<T> {
    fn eq(&self, other: &RcElement<T>) -> bool {
        unsafe {
            match &*self.0.get() {
                Some(weak) => {
                    ptr::addr_eq(weak.as_ptr(), other.0.deref())
                }
                None => false,
            }
        }
    }
}

impl<T: ?Sized> Eq for WeakElement<T> {}

// workaround for lack of unsized coercion
impl<T: Element + 'static> From<WeakElement<T>> for WeakElement {
    fn from(value: WeakElement<T>) -> Self {
        // f*ck this language
        WeakElement(UnsafeCell::new(unsafe { (*value.0.get()).clone().map(|x| x as Weak<dyn Element>) }))
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
pub(crate) struct WeakNullableElemPtr(UnsafeCell<Option<WeakElement>>);

impl<'a> PartialEq<Option<&'a Node>> for WeakNullableElemPtr {
    fn eq(&self, other: &Option<&'a Node>) -> bool {
        let this = unsafe { &*self.0.get() }.as_ref();
        let other = other.map(|e| &e.weak_this);
        match (this, other) {
            (Some(this), Some(other)) => PinWeak::ptr_eq(this, other),
            (None, None) => true,
            _ => false,
        }
    }
}

impl PartialEq<Node> for WeakNullableElemPtr {
    fn eq(&self, other: &Node) -> bool {
        let this = unsafe { &*self.0.get() }.as_ref();
        let other = &other.weak_this;
        if let Some(this) = this {
            Weak::ptr_eq(this, other)
        } else {
            false
        }
    }
}*/

/*
impl PartialEq<Option<Weak<dyn Visual>>> for WeakNullableElemPtr {
    fn eq(&self, other: &Option<Weak<dyn Visual>>) -> bool {
        self.get().as_ref().map(|w| Weak::ptr_eq(w, other)).unwrap_or(false)
    }
}*/

/*
impl Default for WeakNullableElemPtr {
    fn default() -> Self {
        WeakNullableElemPtr(UnsafeCell::new(None))
    }
}

impl WeakNullableElemPtr {
    pub fn get(&self) -> Option<WeakElement> {
        unsafe { &*self.0.get() }.clone()
    }

    pub fn set(&self, other: Option<WeakElement>) {
        unsafe {
            *self.0.get() = other;
        }
    }

    pub fn replace(&self, other: Option<WeakElement>) -> Option<WeakElement> {
        unsafe { mem::replace(&mut *self.0.get(), other) }
    }

    pub fn upgrade(&self) -> Option<RcElement> {
        self.get().as_ref().and_then(PinWeak::upgrade)
    }
}*/

/*
pub struct SiblingIter {
    next: Option<Rc<dyn ElementMethods>>,
}

impl Iterator for SiblingIter {
    type Item = Rc<dyn ElementMethods>;

    fn next(&mut self) -> Option<Self::Item> {
        let r = self.next.clone();
        self.next = self.next.as_ref().and_then(|n| n.next.get());
        r
    }
}
*/

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

/// State common to all elements (the "base class" of all elements).
///
/// Concrete elements hold a field of this type, and implement the corresponding `Element` trait.
pub struct Node {
    _pin: PhantomPinned,
    //weak_this: WeakElementRef,
    parent: WeakElement,
    // Weak pointer to this element.
    weak_this: WeakElement,
    children: RefCell<Vec<RcElement>>,
    index_in_parent: Cell<usize>,

    /// Pointer to the parent owner window.
    pub(crate) window: RefCell<WeakWindow>,
    /// Layout: transform from local to parent coordinates.
    transform: Cell<Affine>,
    /// Layout: geometry (size and baseline) of this element.
    geometry: Cell<Size>,
    /// TODO unused
    change_flags: Cell<ChangeFlags>,
    // List of child elements.
    //children: RefCell<Vec<AnyVisual>>,
    /// Name of the element.
    name: RefCell<String>,
    /// Whether the element is focusable via tab-navigation.
    focusable: Cell<bool>,
    /// Map of attached properties.
    attached_properties: UnsafeCell<BTreeMap<TypeId, Box<dyn Any>>>,
}

impl Node {
    pub(crate) fn new(weak_this: WeakElement) -> Node {
        Node {
            _pin: PhantomPinned,
            weak_this,
            children: Default::default(),
            index_in_parent: Default::default(),
            window: Default::default(),
            parent: Default::default(),
            transform: Cell::new(Affine::default()),
            geometry: Cell::new(Size::default()),
            change_flags: Cell::new(ChangeFlags::LAYOUT | ChangeFlags::PAINT),
            name: RefCell::new(String::new()),
            focusable: Cell::new(false),
            attached_properties: Default::default(),
        }
    }

    /// Creates a new element with the specified type and constructor.
    pub fn new_derived<'a, T: Element + 'static>(f: impl FnOnce(Node) -> T) -> RcElement<T> {
        RcElement::new_cyclic(move |weak: WeakElement<T>| {
            let weak: WeakElement = weak.into();
            let node = Node::new(weak);
            let element = f(node);
            element
        })
    }

    /// Detaches this element from the tree.
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
    }

    fn insert_child_at(&self, at: usize, to_insert: RcElement) {
        to_insert.detach();
        to_insert.parent.set(self.weak());
        //to_insert.set_parent_window(self.window.clone());
        // SAFETY: no other references may exist to the children vector at this point,
        // provided the safety contracts of other unsafe methods are upheld.
        let mut children = self.children.borrow_mut();
        assert!(at <= children.len());
        children.insert(at, to_insert);
        for i in at..children.len() {
            children[i].index_in_parent.set(i);
        }
        self.mark_needs_relayout();
    }

    /*pub fn insert_child_at<T: Element>(&self, at: usize, to_insert: T) -> Pin<Rc<T>> {
        let pinned = Rc::pin(to_insert);
        self.insert_pinned_child_at(at, pinned.clone());
        pinned
    }*/

    /// Inserts the specified element after this element.
    pub fn insert_after(&self, to_insert: RcElement) {
        if let Some(parent) = self.parent.upgrade() {
            parent.insert_child_at(self.index_in_parent.get() + 1, to_insert)
        } else {
            panic!("tried to insert after an element with no parent");
        }
    }

    /// Inserts the specified element at the end of the children of this element.
    pub fn add_child(&self, child: impl Into<RcElement>) {
        let len = self.children.borrow().len();
        self.insert_child_at(len, child.into());
    }

    pub fn next(&self) -> Option<RcElement> {
        if let Some(parent) = self.parent() {
            let index_in_parent = self.index_in_parent.get();
            // SAFETY: no mutable references may exist to the children vector at this point,
            // provided the safety contracts of other unsafe methods are upheld.
            // There may be other shared references but that's not an issue.
            parent.children().get(index_in_parent + 1).cloned()
        } else {
            None
        }
    }

    /// Returns the number of children of this element.
    pub fn child_count(&self) -> usize {
        // SAFETY: no mutable references may exist to the children vector at this point,
        // provided the safety contracts of other unsafe methods are upheld.
        unsafe { self.children_ref().len() }
    }

    /// Returns the child element at the specified index.
    pub fn child_at(&self, index: usize) -> Option<RcElement> {
        // SAFETY: same as `child_count`
        unsafe { self.children_ref().get(index).cloned() }
    }

    /// Returns a slice of all children of this element.
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
    }

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

    /// Requests focus for the current element.
    pub fn set_focus(&self) {
        self.window.borrow().set_focus(self.weak());
    }

    pub fn set_tab_focusable(&self, focusable: bool) {
        self.focusable.set(focusable);
    }

    pub fn set_pointer_capture(&self) {
        self.window.borrow().set_pointer_capture(self.weak());
    }

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

    /// Returns the parent of this visual, if it has one.
    pub fn parent(&self) -> Option<RcElement> {
        self.parent.upgrade()
    }

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

    /// This should be called by `Visual::layout()` so this doesn't set the layout dirty flag.
    pub fn set_offset(&self, offset: Vec2) {
        self.set_transform(Affine::translate(offset));
    }

    pub fn add_offset(&self, offset: Vec2) {
        self.set_transform(self.transform.get() * Affine::translate(offset));
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

    /// Returns the list of ancestors of this visual, plus this visual itself, sorted from the root
    /// to this visual.
    pub fn ancestors_and_self(&self) -> Vec<RcElement> {
        let mut ancestors = Vec::new();
        let mut current = self.rc();
        while let Some(parent) = current.parent() {
            ancestors.push(parent.clone());
            current = parent;
        }
        ancestors.reverse();
        ancestors.push(self.rc());
        ancestors
    }

    /// Returns this visual as a reference-counted pointer.
    pub fn rc(&self) -> RcElement {
        self.weak_this.upgrade().unwrap()
    }

    pub fn weak(&self) -> WeakElement {
        self.weak_this.clone()
    }

    /*
    /// Returns the list of children.
    pub fn children(&self) -> Vec<Rc<dyn ElementMethods + 'static>> {
        // traverse the linked list
        self.iter_children().collect()
    }*/

    fn set_dirty_flags(&self, flags: ChangeFlags) {
        let flags = self.change_flags.get() | flags;
        self.change_flags.set(flags);
        if let Some(parent) = self.parent() {
            parent.set_dirty_flags(flags);
        }
        if flags.contains(ChangeFlags::PAINT) {
            // TODO: maybe don't call repaint for every widget in the hierarchy. winit should coalesce repaint requests, but still
            self.window.borrow().request_repaint()
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

/*
/// A strong reference to an element in the element tree.
#[derive(Clone)]
pub struct ElementRef {
    tree: Rc<dyn ElementTree>,
    index: u32,
    // ptr: *const T,   // guarantee that the element address is stable
}

pub struct WeakElementRef {
    tree: Weak<dyn ElementTree>,
    index: u32,
}*/
/*
pub enum ElementTreeNode<'a> {
    /// Static subtree within this tree.
    Static {
        parent: u32,
        child_range: Range<u32>,
    },
    /// Dynamic subtree
    Dynamic {
        parent: u32,
        subtree: Rc<dyn ElementTree>,
    },
}*/

/*
/// A subtree of elements.
pub trait ElementTree {
    /// Returns the root element of the tree.
    fn root(&self) -> &dyn Element;

    /// Returns the nth descendant element of the tree. 0th element is the root node.
    fn descendant(&self, index: usize) -> Option<&dyn Element>;

    /// Returns the subtree node at the specified index.
    fn subtree(&self, index: usize) -> Option<ElementTreeNode>;
}*/

// Can't implement for `Vec<T: Element>` because the elements may move around.
// However, possible to implement for `Vec<Rc<dyn Element>>`


/// Methods of elements in the element tree.
pub trait Element {
    fn rc(&self) -> RcElement {
        self.node().rc()
    }

    fn weak(&self) -> WeakElement {
        self.node().weak()
    }

    fn node(&self) -> &Node;

    /*/// Calculates the size of the widget under the specified constraints.
    fn measure(&self) -> IntrinsicSizes {
        // TODO
        IntrinsicSizes {
            min: Default::default(),
            max: Default::default(),
        }
    }*/

    /// Asks the element to measure itself under the specified constraints, but without actually laying
    /// out the children.
    fn measure(&self, children: &[RcElement], layout_input: &LayoutInput) -> Size;

    /// Specifies the size of the element, and to lays out its children.
    ///
    /// # Arguments
    /// * `children` - the children of the element.
    /// * `size` - the exact size of the element. Typically, this is one of the sizes returned by a
    /// previous call to `measure`.
    fn layout(&self, children: &[RcElement], size: Size) -> LayoutOutput {
        // The default implementation just returns the union of the geometry of the children.
        let mut output = LayoutOutput::default();
        for child in children {
            let child_output = child.do_layout(size);
            output.width = output.width.max(child_output.width);
            output.height = output.height.max(child_output.height);
            child.set_offset(Vec2::ZERO);
        }
        output
    }

    fn hit_test(&self, point: Point) -> bool {
        self.node().geometry.get().to_rect().contains(point)
    }
    #[allow(unused_variables)]
    fn paint(&self, ctx: &mut PaintCtx) {}

    #[allow(unused_variables)]
    fn event(&self, event: &mut Event)
    {}
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

impl<'a> Deref for dyn Element + 'a {
    type Target = Node;

    fn deref(&self) -> &Self::Target {
        self.node()
    }
}


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

    pub fn do_measure(&self, layout_input: &LayoutInput) -> Size {
        let children = self.children();
        self.measure(&*children, layout_input)
    }


    pub fn do_layout(&self, size: Size) -> LayoutOutput {
        let children = self.children();
        let geometry = self.layout(&*children, size);
        self.geometry.set(Size::new(geometry.width, geometry.height));
        self.mark_layout_done();
        geometry
    }

    pub fn send_event(&self, event: &mut Event) {
        // issue: allocating on every event is not great
        self.event(event);
    }

    /// Hit-tests this visual and its children.
    pub(crate) fn do_hit_test(&self, point: Point) -> Vec<RcElement> {
        // Helper function to recursively hit-test the children of a visual.
        // point: point in the local coordinate space of the visual
        // transform: accumulated transform from the local coord space of `visual` to the root coord space
        fn hit_test_rec(
            visual: &dyn Element,
            point: Point,
            transform: Affine,
            result: &mut Vec<RcElement>,
        ) -> bool {
            let mut hit = false;
            // hit-test ourselves
            if visual.hit_test(point) {
                hit = true;
                result.push(visual.rc().into());
            }

            for child in visual.children().iter() {
                let transform = transform * child.transform();
                let local_point = transform.inverse() * point;
                if hit_test_rec(&**child, local_point, transform, result) {
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

    pub fn do_paint(&self, surface: &DrawableSurface, scale_factor: f64) {
        let mut paint_ctx = PaintCtx {
            scale_factor,
            window_transform: Default::default(),
            surface,
        };

        // Recursively paint the UI tree.
        fn paint_rec(visual: &dyn Element, ctx: &mut PaintCtx) {
            visual.paint(ctx);
            for child in visual.children().iter() {
                ctx.with_transform(&child.transform(), |ctx| {
                    // TODO clipping
                    paint_rec(&**child, ctx);
                    child.mark_paint_done();
                });
            }
        }

        paint_rec(self, &mut paint_ctx);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////


pub trait ElementTree {
    /// Number of static elements in the tree.
    const LEN: usize;

    fn len(&self) -> usize {
        Self::LEN
    }

    fn child(self: Pin<&Self>, index: usize) -> Option<Pin<&dyn Element>>;
}

impl ElementTree for Vec<Pin<Rc<dyn Element>>> {
    const LEN: usize = 0; // only one statically known item in the tree

    fn len(&self) -> usize {
        <Vec<_>>::len(self)
    }

    fn child<'a>(self: Pin<&'a Self>, index: usize) -> Option<Pin<&'a dyn Element>> {
        self.get_ref().get(index).map(|e| e.as_ref())
    }
}

/*
impl<T1, T2> ElementTree for (T1, T2)
where
    T1: ElementTree,
    T2: ElementTree,
{
    const LEN: usize = T1::LEN + T2::LEN;

    fn child(self: Pin<&Self>, index: usize) -> Option<Pin<&dyn Element>> {
        let (t1, t2) = self.get_ref();
        unsafe {
            if index < T1::LEN {
                Pin::new_unchecked(t1).child(index)
            } else {
                Pin::new_unchecked(t2).child(index - T1::LEN)
            }
        }
    }
}*/

/*
macro_rules! impl_tuple_element_tree {

    (@one $($t:ident)+) => {
        #[allow(non_snake_case)]
        impl<$($t),*> ElementTree for ($($t,)*)
        where
            $($t: ElementTree),*
        {
            const LEN: usize = 0 $(+ $t::LEN)*;

            fn child(self: Pin<&Self>, index: usize) -> Option<Pin<&dyn Element>> {
                let ($($t),*) = self.get_ref();
                let mut index = index;
                unsafe {
                    $(
                        if index < $t::LEN {
                            // SAFETY: it's a pin projection of a tuple field,
                            // so it's guaranteed to be pinned as well.
                            return Pin::new_unchecked($t).child(index);
                        }
                        index -= $t::LEN;
                    )*
                }
                None
            }
        }
    };

    () => {};

    ($t0:ident $($t:ident)*) => {
        impl_tuple_element_tree!(@one $t0 $($t)*);
        impl_tuple_element_tree!($($t)*);
    };
}

impl_tuple_element_tree!(T9 T8 T7 T6 T5 T4 T3 T2 T1 T0);

pub struct ElementPtr<T: ?Sized>(Rc<T>, usize);

impl<T: ?Sized> Clone for ElementPtr<T> {
    fn clone(&self) -> Self {
        ElementPtr(self.0.clone(), self.1)
    }
}

impl<T: ?Sized> ElementPtr<T> {
    pub fn downgrade(this: &Self) -> WeakElementPtr<T> {
        WeakElementPtr(Rc::downgrade(&this.0), this.1)
    }
}

pub struct WeakElementPtr<T: ?Sized>(Weak<T>, usize);

impl<T: ?Sized> Clone for WeakElementPtr<T> {
    fn clone(&self) -> Self {
        WeakElementPtr(self.0.clone(), self.1)
    }
}

pub struct Node2 {
    parent: WeakElementPtr<dyn Element>,
    index_in_parent: usize,
}

pub struct Frame {
    node: Node,
    //children: (Rc<dyn Subtree>,
    // ...
}

impl<S> Frame<S>
where
    S: ElementTree,
{
    pub fn new(children: S) -> Self {
        Frame {
            node: Node::new(Rc::new(Self)),
            children,
        }
    }
}

impl<S> Element for Frame<S>
where
    S: ElementTree,
{}*/