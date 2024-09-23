use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell, UnsafeCell};
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
use kurbo::{Affine, Point, Vec2};

use crate::event::Event;
use crate::layout::{BoxConstraints, Geometry, IntrinsicSizes};
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

    fn set(self, item: &dyn ElementMethods, value: Self::Value)
    where
        Self: Sized,
    {
        item.set::<Self>(value);
    }

    fn get(self, item: &dyn ElementMethods) -> Option<Self::Value>
    where
        Self: Sized,
    {
        item.get::<Self>()
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

        if let Some(first_child) = next.first_child.get() {
            self.next = Some(first_child);
        } else if let Some(next) = next.next.get() {
            self.next = Some(next);
        } else {
            // go up until we find a parent with a next sibling
            let mut parent = next.parent();
            self.next = None;
            while let Some(p) = parent {
                if let Some(next) = p.next.get() {
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
    _pin: PhantomPinned,
    /// Weak pointer to this element.
    weak_this: Weak<dyn ElementMethods>,
    prev: WeakNullableElemPtr,
    next: NullableElemPtr,
    first_child: NullableElemPtr,
    last_child: WeakNullableElemPtr,
    /// Pointer to the parent owner window.
    pub(crate) window: RefCell<WeakWindow>,
    /// This element's parent.
    parent: WeakNullableElemPtr,
    /// Layout: transform from local to parent coordinates.
    transform: Cell<Affine>,
    /// Layout: geometry (size and baseline) of this element.
    geometry: Cell<Geometry>,
    /// TODO unused
    change_flags: Cell<ChangeFlags>,
    // List of child elements.
    //children: RefCell<Vec<AnyVisual>>,
    /// Name of the element.
    name: RefCell<String>,
    /// Whether the element is focusable via tab-navigation.
    focusable: Cell<bool>,
    attached_properties: RefCell<BTreeMap<TypeId, Box<dyn Any>>>,
}

impl Element {
    pub(crate) fn new(weak_this: &Weak<dyn ElementMethods>) -> Element {
        Element {
            _pin: PhantomPinned,
            weak_this: weak_this.clone(),
            prev: Default::default(),
            next: Default::default(),
            first_child: Default::default(),
            last_child: Default::default(),
            window: Default::default(),
            parent: Default::default(),
            transform: Cell::new(Affine::default()),
            geometry: Cell::new(Geometry::default()),
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
        // this.prev.next = this.next
        // OR this.parent.first_child = this.next
        if let Some(prev) = self.prev.upgrade() {
            prev.next.set(self.next.get());
        } else if let Some(parent) = self.parent() {
            parent.first_child.set(self.next.get());
        }

        // this.next.prev = this.prev
        // OR this.parent.last_child = this.prev
        if let Some(next) = self.next.get() {
            next.prev.set(self.prev.get());
        } else if let Some(parent) = self.parent() {
            parent.last_child.set(self.prev.get());
        }

        self.prev.set(None);
        self.next.set(None);

        if let Some(parent) = self.parent() {
            parent.mark_needs_relayout();
        }

        self.parent.set(None);
    }

    /// Inserts the specified element after this element.
    pub fn insert_after(&self, to_insert: &Element) {
        to_insert.detach();
        // ins.prev = this
        to_insert.prev.set(Some(self.weak()));
        // ins.next = this.next
        to_insert.next.set(self.next.get());
        // this.next.prev = ins
        // OR this.parent.last_child = ins
        if let Some(next) = self.next.get() {
            next.prev.set(Some(to_insert.weak()));
        } else if let Some(parent) = self.parent() {
            parent.last_child.set(Some(to_insert.weak()));
        }
        // this.next = ins
        self.next.set(Some(to_insert.rc()));
        // ins.parent = this.parent
        to_insert.parent.set(self.parent.get());

        if let Some(parent) = self.parent() {
            parent.mark_needs_relayout();
        }
    }

    /// Returns a cursor at this element
    pub fn cursor(&self) -> Cursor {
        Cursor { next: Some(self.rc()) }
    }

    /// Inserts the specified element at the end of the children of this element.
    pub fn add_child(&self, child: &Element) {
        child.detach();
        // child.prev = this.last_child;
        // child.next = None;
        // this.last_child.next = child;
        // this.last_child = child;
        // child.parent = this;
        child.prev.set(self.last_child.get());
        child.next.set(None);
        if let Some(last_child) = self.last_child.upgrade() {
            last_child.next.set(Some(child.rc()));
        } else {
            self.first_child.set(Some(child.rc()));
        }
        self.last_child.set(Some(child.weak()));
        child.parent.set(Some(self.weak()));
        self.mark_needs_relayout()
    }

    pub(crate) fn set_parent_window(&self, window: WeakWindow) {
        if !Weak::ptr_eq(&self.window.borrow().shared, &window.shared) {
            self.window.replace(window.clone());
            // recursively update the parent window of the children
            for child in self.iter_children() {
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

    /// Returns an iterator over this element's children.
    pub fn iter_children(&self) -> impl Iterator<Item=Rc<dyn ElementMethods>> {
        SiblingIter {
            next: self.first_child.get(),
        }
    }

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

    pub fn geometry(&self) -> Geometry {
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
        for c in self.iter_children() {
            // TODO: don't do that if there's only one reference remaining
            // detach from window
            c.window.replace(WeakWindow::default());
            // detach from parent
            c.parent.set(None);
        }
        self.first_child.set(None);
        self.last_child.set(None);
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

    /// Returns the list of children.
    pub fn children(&self) -> Vec<Rc<dyn ElementMethods + 'static>> {
        // traverse the linked list
        self.iter_children().collect()
    }

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
    pub fn set<T: AttachedProperty>(&self, value: T::Value) {
        self.attached_properties
            .borrow_mut()
            .insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Gets the value of an attached property.
    pub fn get<T: AttachedProperty>(&self) -> Option<T::Value> {
        self.attached_properties.borrow().get(&TypeId::of::<T>()).map(|v| {
            v.downcast_ref::<T::Value>()
                .expect("invalid type of attached property")
                .clone()
        })
    }
}


/// Nodes in the visual tree.
pub trait ElementMethods: EventTarget {
    fn element(&self) -> &Element;

    fn intrinsic_sizes(&self) -> IntrinsicSizes {
        // TODO
        IntrinsicSizes {
            min: Default::default(),
            max: Default::default(),
        }
    }

    // TODO: this could take a "SiblingIter"
    fn layout(&self, children: &[Rc<dyn ElementMethods>], constraints: &BoxConstraints) -> Geometry {
        // The default implementation just returns the union of the geometry of the children.
        let mut geometry = Geometry::default();
        for child in children {
            let child_geometry = child.do_layout(constraints);
            geometry.size.width = geometry.size.width.max(child_geometry.size.width);
            geometry.size.height = geometry.size.height.max(child_geometry.size.height);
            geometry.bounding_rect = geometry.bounding_rect.union(child_geometry.bounding_rect);
            geometry.paint_bounding_rect = geometry.paint_bounding_rect.union(child_geometry.paint_bounding_rect);
            child.set_offset(Vec2::ZERO);
        }
        geometry
    }

    fn hit_test(&self, point: Point) -> bool {
        self.element().geometry.get().size.to_rect().contains(point)
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

/// Implementation detail of `Visual` to get an object-safe version of `async fn event()`.
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

    pub fn do_layout(&self, constraints: &BoxConstraints) -> Geometry {
        let children = self.children();

        let geometry = self.layout(&*children, constraints);
        self.geometry.set(geometry);
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
        fn hit_test_rec(visual: &dyn ElementMethods, point: Point, transform: Affine, result: &mut Vec<AnyVisual>) -> bool {
            let mut hit = false;
            // hit-test ourselves
            if visual.hit_test(point) {
                hit = true;
                result.push(visual.rc().into());
            }

            for child in visual.iter_children() {
                let transform = transform * child.transform();
                let local_point = transform.inverse() * point;
                if hit_test_rec(&*child, local_point, transform, result) {
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
            for child in visual.iter_children() {
                ctx.with_transform(&child.transform(), |ctx| {
                    // TODO clipping
                    paint_rec(&*child, ctx);
                    child.mark_paint_done();
                });
            }
        }

        paint_rec(self, &mut paint_ctx);
    }
}
