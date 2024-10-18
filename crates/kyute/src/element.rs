use std::any::{Any, TypeId};
use std::cell::{Cell, Ref, RefCell, UnsafeCell};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::marker::PhantomPinned;
use std::mem;
use std::ops::Deref;
use std::ptr::addr_eq;
use std::rc::{Rc, Weak};

use crate::compositor::DrawableSurface;
use bitflags::bitflags;
use futures_util::future::LocalBoxFuture;
use futures_util::FutureExt;
use kurbo::{Affine, Point, Size, Vec2};
use tracing::warn;

use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::window::WeakWindow;
use crate::PaintCtx;

bitflags! {
    #[derive(Copy, Clone, Default)]
    pub struct ChangeFlags: u32 {
        const PAINT = 0b0001;
        const LAYOUT = 0b0010;
        const NONE = 0b0000;
    }
}

pub trait AttachedProperty: Any {
    type Value: Clone;

    fn set(self, item: &Element, value: Self::Value)
    where
        Self: Sized,
    {
        item.set(self, value);
    }

    fn get(self, item: &Element) -> Option<Self::Value>
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
    unsafe fn get_ref(self, item: &Element) -> &Self::Value
    where
        Self: Sized,
    {
        item.get_ref(self)
    }
}

/// Wrapper over Rc<dyn Visual> that has PartialEq impl.
#[derive(Clone)]
#[repr(transparent)]
pub struct AnyVisual(pub(crate) Rc<dyn ElementMethods>);

impl PartialOrd for AnyVisual {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AnyVisual {
    fn cmp(&self, other: &Self) -> Ordering {
        Rc::as_ptr(&self.0).cast::<()>().cmp(&Rc::as_ptr(&other.0).cast::<()>())
    }
}

impl Eq for AnyVisual {}

impl From<Rc<dyn ElementMethods>> for AnyVisual {
    fn from(rc: Rc<dyn ElementMethods>) -> Self {
        AnyVisual(rc)
    }
}

impl Deref for AnyVisual {
    type Target = dyn ElementMethods;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl PartialEq for AnyVisual {
    fn eq(&self, other: &Self) -> bool {
        self.0.is_same(&*other.0)
    }
}

pub(crate) struct NullableElemPtr(UnsafeCell<Option<Rc<dyn ElementMethods>>>);

impl Default for NullableElemPtr {
    fn default() -> Self {
        NullableElemPtr(UnsafeCell::new(None))
    }
}

impl NullableElemPtr {
    pub fn get(&self) -> Option<Rc<dyn ElementMethods>> {
        unsafe { &*self.0.get() }.as_ref().cloned()
    }

    pub fn set(&self, other: Option<Rc<dyn ElementMethods>>) {
        unsafe {
            *self.0.get() = other;
        }
    }
}

impl<'a> From<&'a Element> for NullableElemPtr {
    fn from(element: &'a Element) -> Self {
        NullableElemPtr(UnsafeCell::new(Some(element.rc())))
    }
}

pub(crate) struct WeakNullableElemPtr(UnsafeCell<Option<Weak<dyn ElementMethods>>>);

impl<'a> PartialEq<Option<&'a Element>> for WeakNullableElemPtr {
    fn eq(&self, other: &Option<&'a Element>) -> bool {
        let this = unsafe { &*self.0.get() }.as_ref();
        let other = other.map(|e| &e.weak_this);
        match (this, other) {
            (Some(this), Some(other)) => Weak::ptr_eq(this, other),
            (None, None) => true,
            _ => false,
        }
    }
}

impl PartialEq<Element> for WeakNullableElemPtr {
    fn eq(&self, other: &Element) -> bool {
        let this = unsafe { &*self.0.get() }.as_ref();
        let other = &other.weak_this;
        if let Some(this) = this {
            Weak::ptr_eq(this, other)
        } else {
            false
        }
    }
}

/*
impl PartialEq<Option<Weak<dyn Visual>>> for WeakNullableElemPtr {
    fn eq(&self, other: &Option<Weak<dyn Visual>>) -> bool {
        self.get().as_ref().map(|w| Weak::ptr_eq(w, other)).unwrap_or(false)
    }
}*/

impl Default for WeakNullableElemPtr {
    fn default() -> Self {
        WeakNullableElemPtr(UnsafeCell::new(None))
    }
}

impl WeakNullableElemPtr {
    pub fn get(&self) -> Option<Weak<dyn ElementMethods>> {
        unsafe { &*self.0.get() }.clone()
    }

    pub fn set(&self, other: Option<Weak<dyn ElementMethods>>) {
        unsafe {
            *self.0.get() = other;
        }
    }

    pub fn replace(&self, other: Option<Weak<dyn ElementMethods>>) -> Option<Weak<dyn ElementMethods>> {
        unsafe { mem::replace(&mut *self.0.get(), other) }
    }

    pub fn upgrade(&self) -> Option<Rc<dyn ElementMethods>> {
        self.get().as_ref().and_then(Weak::upgrade)
    }
}

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
    next: Option<Rc<dyn ElementMethods>>,
}

impl Iterator for Cursor {
    type Item = Rc<dyn ElementMethods>;

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
/// Concrete elements hold a field of this type, and implement the corresponding `ElementMethods` trait.
pub struct Element {
    // TODO remove, this is a relic of a previous implementation
    _pin: PhantomPinned,
    parent: WeakNullableElemPtr,
    // Weak pointer to this element.
    weak_this: Weak<dyn ElementMethods>,
    children: RefCell<Vec<Rc<dyn ElementMethods>>>,
    index_in_parent: Cell<usize>,

    //prev: WeakNullableElemPtr,
    //next: NullableElemPtr,
    //first_child: NullableElemPtr,
    //last_child: WeakNullableElemPtr,
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

impl Element {
    pub(crate) fn new(weak_this: &Weak<dyn ElementMethods>) -> Element {
        Element {
            _pin: PhantomPinned,
            weak_this: weak_this.clone(),
            //prev: Default::default(),
            //next: Default::default(),
            //first_child: Default::default(),
            //last_child: Default::default(),
            children: Default::default(),
            index_in_parent: Default::default(),
            window: Default::default(),
            parent: Default::default(),
            transform: Cell::new(Affine::default()),
            geometry: Cell::new(Size::default()),
            change_flags: Cell::new(ChangeFlags::LAYOUT | ChangeFlags::PAINT),
            name: RefCell::new(format!("{:p}", weak_this.as_ptr())),
            focusable: Cell::new(false),
            attached_properties: Default::default(),
        }
    }

    /// Creates a new element with the specified type and constructor.
    pub fn new_derived<'a, T: ElementMethods + 'static>(f: impl FnOnce(Element) -> T) -> Rc<T> {
        Rc::new_cyclic(move |weak: &Weak<T>| {
            let weak: Weak<dyn ElementMethods> = weak.clone();
            let element = Element::new(&weak);
            let visual = f(element);
            visual
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

        self.parent.set(None);
    }

    pub fn insert_child_at(&self, at: usize, to_insert: &Element) {
        to_insert.detach();
        to_insert.parent.set(Some(self.weak()));
        //to_insert.set_parent_window(self.window.clone());
        // SAFETY: no other references may exist to the children vector at this point,
        // provided the safety contracts of other unsafe methods are upheld.
        let mut children = self.children.borrow_mut();
        assert!(at <= children.len());
        children.insert(at, to_insert.rc());
        for i in at..children.len() {
            children[i].index_in_parent.set(i);
        }
        self.mark_needs_relayout();
    }

    /// Inserts the specified element after this element.
    pub fn insert_after(&self, to_insert: &Element) {
        if let Some(parent) = self.parent.upgrade() {
            parent.insert_child_at(self.index_in_parent.get() + 1, to_insert);
        } else {
            warn!("tried to insert after an element with no parent");
        }
    }

    /// Inserts the specified element at the end of the children of this element.
    pub fn add_child(&self, child: &Element) {
        child.detach();
        // SAFETY: no other references may exist to the children vector at this point,
        // provided the safety contracts of other unsafe methods are upheld.
        let mut children = self.children.borrow_mut();
        child.parent.set(Some(self.weak()));
        child.index_in_parent.set(children.len());
        children.push(child.rc());
        self.mark_needs_relayout();
    }

    pub fn next(&self) -> Option<Rc<dyn ElementMethods>> {
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
    pub fn child_at(&self, index: usize) -> Option<Rc<dyn ElementMethods>> {
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
    pub unsafe fn children_ref(&self) -> &[Rc<dyn ElementMethods>] {
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
    pub unsafe fn child_ref_at(&self, index: usize) -> &dyn ElementMethods {
        &**self.children_ref().get(index).expect("child index out of bounds")
    }

    /// Returns a reference to the list of children of this element.
    pub fn children(&self) -> Ref<[Rc<dyn ElementMethods>]> {
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
    pub fn next_focusable_element(&self) -> Option<Rc<dyn ElementMethods>> {
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
    pub async fn set_focus(&self) {
        self.window.borrow().set_focus(Some(self)).await;
    }

    pub fn set_tab_focusable(&self, focusable: bool) {
        self.focusable.set(focusable);
    }

    pub fn set_pointer_capture(&self) {
        self.window.borrow().set_pointer_capture(self);
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
            c.parent.set(None);
        }
    }

    /// Returns the parent of this visual, if it has one.
    pub fn parent(&self) -> Option<Rc<dyn ElementMethods>> {
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
    pub fn ancestors_and_self(&self) -> Vec<Rc<dyn ElementMethods>> {
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
    pub fn rc(&self) -> Rc<dyn ElementMethods + 'static> {
        self.weak_this.upgrade().unwrap()
    }

    pub fn weak(&self) -> Weak<dyn ElementMethods + 'static> {
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

/// Methods of elements in the element tree.
pub trait ElementMethods: EventTarget {
    fn element(&self) -> &Element;

    /*/// Calculates the size of the widget under the specified constraints.
    fn measure(&self) -> IntrinsicSizes {
        // TODO
        IntrinsicSizes {
            min: Default::default(),
            max: Default::default(),
        }
    }*/

    /// Asks the widget to measure itself under the specified constraints, but without actually laying
    /// out the children.
    fn measure(&self, children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> Size;

    /// Lays out the children of this widget under the specified constraints.
    fn layout(&self, children: &[Rc<dyn ElementMethods>], size: Size) -> LayoutOutput {
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
        self.element().geometry.get().to_rect().contains(point)
    }
    #[allow(unused_variables)]
    fn paint(&self, ctx: &mut PaintCtx) {}

    // Why async? this is because the visual may transfer control to async event handlers
    // before returning.
    #[allow(unused_variables)]
    async fn event(&self, event: &mut Event)
    where
        Self: Sized,
    {}
}

/// Implementation detail of `ElementMethods` to get an object-safe version of `async fn event()`.
#[doc(hidden)]
pub trait EventTarget {
    fn event_future<'a>(&'a self, event: &'a mut Event) -> LocalBoxFuture<'a, ()>;
}

impl<T> EventTarget for T
where
    T: ElementMethods,
{
    fn event_future<'a>(&'a self, event: &'a mut Event) -> LocalBoxFuture<'a, ()> {
        self.event(event).boxed_local()
    }
}

/// An entry in the hit-test chain that leads to the visual that was hit.
#[derive(Clone)]
pub struct HitTestEntry {
    /// The visual in the chain.
    pub element: Rc<dyn ElementMethods>,
    // Transform from the visual's CS to the CS of the visual on which `do_hit_test` was called (usually the root visual of the window).
    //pub root_transform: Affine,
}

impl PartialEq for HitTestEntry {
    fn eq(&self, other: &Self) -> bool {
        self.element.is_same(&*other.element)
    }
}

impl Eq for HitTestEntry {}

impl<'a> Deref for dyn ElementMethods + 'a {
    type Target = Element;

    fn deref(&self) -> &Self::Target {
        self.element()
    }
}

impl dyn ElementMethods + '_ {
    /*pub fn children(&self) -> Ref<[AnyVisual]> {
        self.element().children()
    }*/

    pub fn set_name(&self, name: impl Into<String>) {
        self.element().name.replace(name.into());
    }

    /// Identity comparison.
    pub fn is_same(&self, other: &dyn ElementMethods) -> bool {
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

    pub async fn send_event(&self, event: &mut Event) {
        // issue: allocating on every event is not great
        self.event_future(event).await;
    }

    /// Hit-tests this visual and its children.
    pub(crate) fn do_hit_test(&self, point: Point) -> Vec<AnyVisual> {
        // Helper function to recursively hit-test the children of a visual.
        // point: point in the local coordinate space of the visual
        // transform: accumulated transform from the local coord space of `visual` to the root coord space
        fn hit_test_rec(
            visual: &dyn ElementMethods,
            point: Point,
            transform: Affine,
            result: &mut Vec<AnyVisual>,
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
        fn paint_rec(visual: &dyn ElementMethods, ctx: &mut PaintCtx) {
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
