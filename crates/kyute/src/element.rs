use crate::compositor::DrawableSurface;
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::window::{WeakWindow, WindowInner};
use crate::PaintCtx;
use bitflags::bitflags;
use futures_util::FutureExt;
use kurbo::{Affine, Point, Size, Vec2};
use std::any::{Any, TypeId};
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::{mem, ptr};
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, UniqueRc, Weak};

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

    /*fn set(self, item: &Node, value: Self::Value)
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
    }*/
}

////////////////////////////////////////////////////////////////////////////////////////////////////

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
    header: &'a NodeHeader,
    dirty_flags: ChangeFlags,
}

impl Deref for ElementCtx<'_> {
    type Target = WindowCtx;

    fn deref(&self) -> &Self::Target {
        self.window_ctx
    }
}

impl DerefMut for ElementCtx<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.window_ctx
    }
}

impl<'a> ElementCtx<'a> {
    pub fn mark_needs_repaint(&mut self) {
        self.dirty_flags |= ChangeFlags::PAINT;
    }

    pub fn mark_needs_relayout(&mut self) {
        self.dirty_flags |= ChangeFlags::LAYOUT | ChangeFlags::PAINT;
    }
}

pub struct HitTestCtx {
    pub hits: Vec<ElementAny>,
    transform: Affine,
}

impl HitTestCtx {
    pub fn new() -> HitTestCtx {
        HitTestCtx {
            hits: Vec::new(),
            transform: Affine::new(),
        }
    }
}

pub struct EventCtx<'a> {
    ctx: ElementCtx<'a>,
}

impl<'a> Deref for EventCtx<'a> {
    type Target = ElementCtx<'a>;

    fn deref(&self) -> &Self::Target {
        &self.ctx
    }
}

impl<'a> DerefMut for EventCtx<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ctx
    }
}

impl<'a> EventCtx<'a> {
    pub fn mark_needs_repaint(&mut self) {
        self.ctx.mark_needs_repaint()
    }

    pub fn mark_needs_relayout(&mut self) {
        self.ctx.mark_needs_relayout()
    }

    pub fn set_focus(&mut self) {
        todo!("set_focus")
    }

    pub fn set_pointer_capture(&mut self) {
        todo!("set_pointer_capture")
    }
}

/// Context passed to `Element::measure`.
pub struct MeasureCtx {}

// Context passed to `Element::layout`.
pub struct LayoutCtx {}

/// Methods of elements in the element tree.
pub trait Element: Any {
    /// Asks the element to measure itself under the specified constraints, but without actually laying
    /// out the children.
    fn measure(&mut self, ctx: &LayoutCtx, layout_input: &LayoutInput) -> Size;

    /// Specifies the size of the element, and lays out its children.
    ///
    /// # Arguments
    /// * `children` - the children of the element.
    /// * `size` - the exact size of the element. Typically, this is one of the sizes returned by a
    /// previous call to `measure`.
    fn layout(&mut self, ctx: &LayoutCtx, size: Size) -> LayoutOutput;

    /// Called to perform hit-testing on the bounds of this element.
    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool;

    /// Paints this element on a target surface using the specified `PaintCtx`.
    #[allow(unused_variables)]
    fn paint(&mut self, ctx: &mut PaintCtx);

    /// Called when an event is sent to this element.
    #[allow(unused_variables)]
    fn event(&mut self, ctx: &mut EventCtx, event: &mut Event) {}
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Weak reference to an element in the element tree.
pub struct WeakElement<T: ?Sized>(Weak<Node<T>>);

pub type WeakElementAny = WeakElement<dyn Element>;

impl<T: ?Sized> Clone for WeakElement<T> {
    fn clone(&self) -> Self {
        WeakElement(Weak::clone(&self.0))
    }
}

impl<T: ?Sized> WeakElement<T> {
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
pub struct ElementRc<T: ?Sized>(Rc<Node<T>>);

impl<T: ?Sized> Clone for ElementRc<T> {
    fn clone(&self) -> Self {
        ElementRc(Rc::clone(&self.0))
    }
}


impl<T: ?Sized> ElementRc<T> {
    /// Returns a weak reference to this element.
    pub fn downgrade(&self) -> WeakElement<T> {
        WeakElement(Rc::downgrade(&self.0))
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
        self.0.header.parent.upgrade()
    }

    pub(crate) fn propagate_dirty_flags(&self) {
        let flags = self.0.header.change_flags.get();
        if let Some(parent) = self.parent() {
            if parent.0.header.change_flags.get().contains(flags) {
                // the parent already has the flags, no need to propagate
                return;
            }
            parent
                .0
                .header
                .change_flags
                .set(parent.0.header.change_flags.get() | flags);
            parent.propagate_dirty_flags();
        }
    }
}

pub type ElementAny = ElementRc<dyn Element>;

// Element refs are compared by pointer equality.
impl PartialEq for ElementAny {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl PartialEq<WeakElementAny> for ElementAny {
    fn eq(&self, other: &WeakElementAny) -> bool {
        ptr::addr_eq(other.0.as_ptr(), Rc::as_ptr(&self.0))
    }
}

impl PartialEq<ElementAny> for WeakElementAny {
    fn eq(&self, other: &ElementAny) -> bool {
        ptr::addr_eq(self.0.as_ptr(), Rc::as_ptr(&other.0))
    }
}

impl Eq for ElementAny {}

impl ElementAny {
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

    /// Invokes a method on this widget.
    ///
    /// Propagates the resulting dirty flags up the tree.
    pub(crate) fn invoke<R>(
        &self,
        window_ctx: &mut WindowCtx,
        f: impl FnOnce(&mut dyn Element, &mut ElementCtx) -> R,
    ) -> R {
        let ref mut inner = *self.0.inner.borrow_mut();
        let mut ctx = ElementCtx {
            this: self.downgrade(),
            window_ctx,
            header: &self.0.header,
            dirty_flags: Default::default(),
        };
        let r = f(inner, &mut ctx);
        ctx.header.change_flags.set(ctx.dirty_flags);
        self.propagate_dirty_flags();
        r
    }

    pub fn measure(&self, layout_ctx: &LayoutCtx, layout_input: &LayoutInput) -> Size {
        let ref mut inner = *self.0.inner.borrow_mut();
        inner.measure(layout_ctx, layout_input)
    }

    /// Invokes layout on this element and its children, recursively.
    pub fn layout(&self, layout_ctx: &LayoutCtx, size: Size) -> LayoutOutput {
        self.0.header.geometry.set(size);
        let ref mut inner = *self.0.inner.borrow_mut();
        inner.layout(layout_ctx, size)
    }

    /// Hit-tests this element and its children.
    pub fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        let child_transform = self.0.header.transform.get();
        let transform = ctx.transform * child_transform;
        let local_point = transform.inverse() * point;
        let prev_transform = mem::replace(&mut ctx.transform, transform);
        let ref mut inner = *self.0.inner.borrow_mut();
        let hit = inner.hit_test(ctx, local_point);
        if hit {
            ctx.hits.push(self.clone());
        }
        ctx.transform = prev_transform;
        hit
    }

    pub fn send_event(&self, ctx: &mut WindowCtx, event: &mut Event) {
        let ref mut inner = *self.0.inner.borrow_mut();
        let mut event_ctx = EventCtx {
            ctx: ElementCtx {
                this: self.downgrade(),
                window_ctx: ctx,
                header: &self.0.header,
                dirty_flags: Default::default(),
            },
        };
        inner.event(&mut event_ctx, event);
        self.0.header.change_flags.set(event_ctx.dirty_flags);
        self.propagate_dirty_flags();
    }

    pub fn paint(&self, parent_ctx: &mut PaintCtx) {
        let size = self.0.header.geometry.get();
        let transform = self.0.header.transform.get();
        let ref mut inner = *self.0.inner.borrow_mut();

        let mut ctx = PaintCtx {
            surface: parent_ctx.surface,
            window_transform: parent_ctx.window_transform,
            scale_factor: parent_ctx.scale_factor,
            size,
            has_focus: false,
        };

        ctx.with_transform(&transform, |ctx| {
            inner.paint(ctx);
        });
    }

    pub fn add_offset(&self, offset: Vec2) {
        self.0.header.add_offset(offset);
    }

    pub fn set_offset(&self, offset: Vec2) {
        self.0.header.set_offset(offset);
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
    pub unsafe fn get_ref<A: AttachedProperty>(&self, property: A) -> &A::Value {
        todo!("get_ref")
        //self.try_get_ref::<T>(property).expect("attached property not set")
    }

    pub fn get<A: AttachedProperty>(&self, property: A) -> Option<A::Value> {
        todo!("get")
    }

    /// Returns a reference to the specified attached property, or `None` if it is not set.
    ///
    /// # Safety
    ///
    /// Same contract as `get_ref`.
    pub unsafe fn try_get_ref<A: AttachedProperty>(&self, _property: A) -> Option<&A::Value> {
        todo!("try_get_ref")
        //let attached_properties = unsafe { &*self.attached_properties.get() };
        //attached_properties
        //    .get(&TypeId::of::<T>())
        //    .map(|v| v.downcast_ref::<T::Value>().expect("invalid type of attached property"))
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
    fn into_element(self, parent: WeakElementAny, _index_in_parent: usize) -> ElementAny {
        let mut node: UniqueRc<Node<dyn Element>> = UniqueRc::new(Node::new(self));
        let weak = WeakElement(UniqueRc::downgrade(&node));
        node.header.weak_this = weak;
        node.header.parent = parent;
        ElementRc(UniqueRc::into_rc(node))
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

struct Node<T: ?Sized = dyn Element> {
    header: NodeHeader,
    inner: RefCell<T>,
}

impl<T> Node<T> {
    fn new(inner: T) -> Node<T> {
        Node {
            header: NodeHeader {
                weak_this: WeakElementAny::default(),
                change_flags: Default::default(),
                window: Default::default(),
                parent: Default::default(),
                transform: Default::default(),
                geometry: Default::default(),
                name: String::new(),
                focusable: false,
                attached_properties: Default::default(),
            },
            inner: RefCell::new(inner),
        }
    }
}

struct NodeHeader {
    parent: WeakElementAny,
    // Weak pointer to this element.
    weak_this: WeakElementAny,
    change_flags: Cell<ChangeFlags>,
    /// Pointer to the parent owner window.
    pub(crate) window: WeakWindow,
    /// Layout: transform from local to parent coordinates.
    transform: Cell<Affine>,
    /// Layout: geometry (size and baseline) of this element.
    geometry: Cell<Size>,
    /// Name of the element.
    name: String,
    /// Whether the element is focusable via tab-navigation.
    focusable: bool,
    /// Map of attached properties.
    attached_properties: BTreeMap<TypeId, Box<dyn Any>>,
}

impl NodeHeader {
    pub fn set_offset(&self, offset: Vec2) {
        self.transform.set(Affine::translate(offset));
    }

    pub fn add_offset(&self, offset: Vec2) {
        todo!("add_offset")
        //self.transform *= Affine::translate(offset);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// A wrapper for elements as they are being constructed.
///
/// Basically this is a wrapper around Rc that provides a `DerefMut` impl since we know it's the
/// only strong reference to it.
pub struct ElementBuilder<T>(UniqueRc<Node<T>>);

impl<T: Default + Element> Default for ElementBuilder<T> {
    fn default() -> Self {
        ElementBuilder::new(Default::default())
    }
}

impl<T: Element> ElementBuilder<T> {
    /// Creates a new `ElementBuilder` instance.
    pub fn new(inner: T) -> ElementBuilder<T> {
        let mut urc = UniqueRc::new(Node::new(inner));
        let weak = UniqueRc::downgrade(&urc);
        urc.header.weak_this = WeakElement(weak);
        ElementBuilder(urc)
    }

    pub fn weak(&self) -> WeakElementAny {
        let weak = UniqueRc::downgrade(&self.0);
        WeakElement(weak)
    }

    pub fn set_tab_focusable(mut self) -> Self {
        todo!("set_tab_focusable")
    }

    /// Assigns a name to the element, for debugging purposes.
    pub fn debug_name(mut self, name: impl Into<String>) -> Self {
        self.0.header.name = name.into();
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
    fn into_element(mut self, parent: WeakElementAny, _index_in_parent: usize) -> ElementAny {
        let weak = UniqueRc::downgrade(&self.0);
        let header = &mut self.0.header;
        header.weak_this = WeakElement(weak);
        header.parent = parent;
        //header.index_in_parent.set(index_in_parent);
        ElementRc(UniqueRc::into_rc(self.0))
    }
}
