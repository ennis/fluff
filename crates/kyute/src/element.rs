use crate::application::{run_after, run_queued};
use crate::compositor::DrawableSurface;
use crate::elements::{ActivatedEvent, ClickedEvent, HoveredEvent};
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::model::{
    watch_multi_once, watch_multi_once_with_location, with_tracking_scope, EventSource, SubscriptionKey,
};
use crate::util::rc_cell::{RcCell, RcRef, RcRefMut};
use crate::window::WeakWindow;
use crate::{ElementState, PaintCtx};
use bitflags::bitflags;
use kurbo::{Affine, Point, Rect, Size, Vec2};
use rc_borrow::RcBorrow;
use std::any::{Any, TypeId};
use std::cell::{BorrowMutError, Cell, Ref, RefCell, RefMut, UnsafeCell};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::ptr::NonNull;
use std::rc::{Rc, UniqueRc, Weak};
use std::time::Duration;
use std::{fmt, mem, ptr};
use tracing::warn;

pub mod prelude {
    pub use crate::element::{
        Element, ElementAny, ElementBuilder, ElementCtx, ElementCtxAny, HitTestCtx, IntoElementAny, WeakElementAny,
        WindowCtx,
    };
    pub use crate::event::Event;
    pub use crate::layout::{LayoutInput, LayoutOutput, SizeConstraint, SizeValue};
    pub use crate::PaintCtx;
}

bitflags! {
    #[derive(Copy, Clone, Default)]
    pub struct ChangeFlags: u32 {
        const PAINT = 0b0001;
        const LAYOUT = 0b0010;
        const STRUCTURE = 0b0100;
        const NONE = 0b0000;
    }
}

/// Represents the element that has the keyboard focus.
#[derive(Clone)]
pub struct FocusedElement {
    /// The window in which the element is located.
    pub window: WeakWindow,
    /// The element that has focus.
    pub element: ElementAny,
}

thread_local! {
    /// The element that has keyboard focus (unique among all windows).
    static FOCUSED_ELEMENT: RefCell<Option<FocusedElement>> = RefCell::new(None);

    /// The element that is capturing the pointer.
    static POINTER_CAPTURING_ELEMENT: RefCell<Option<ElementAny>> = RefCell::new(None);
}

/// Returns the element that has keyboard focus.
pub fn get_keyboard_focus() -> Option<FocusedElement> {
    FOCUSED_ELEMENT.with(|f| f.borrow().clone())
}

/// Called to set the global keyboard focus to the specified element.
pub fn set_keyboard_focus(target: ElementAny) {
    run_queued(move || {
        let parent_window = target.get_parent_window();
        let prev_focus = FOCUSED_ELEMENT.take();
        if let Some(prev_focus) = prev_focus {
            if prev_focus.element == target {
                // Element already has focus. This should be handled earlier.
                warn!("{:?} already focused", target);
                FOCUSED_ELEMENT.replace(Some(prev_focus));
                return;
            }

            // Send a FocusLost event to the previously focused element.
            prev_focus.element.borrow_mut().ctx_mut().focused = false;
            prev_focus.element.send_event(&mut WindowCtx {}, &mut Event::FocusLost);
        }

        // Send a FocusGained event to the newly focused element.
        target.borrow_mut().ctx_mut().focused = true;
        target.send_event(&mut WindowCtx {}, &mut Event::FocusGained);

        // If necessary, activate the target window.
        if let Some(_parent_window) = parent_window.shared.upgrade() {
            //parent_window.
            //war!("activate window")
        }

        // Update the global focus.
        FOCUSED_ELEMENT.replace(Some(FocusedElement {
            window: parent_window,
            element: target,
        }));
    });
}

////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct WindowCtx {}

pub struct HitTestCtx {
    pub hits: Vec<WeakElementAny>,
    transform: Affine,
}

impl HitTestCtx {
    pub fn new() -> HitTestCtx {
        HitTestCtx {
            hits: Vec::new(),
            transform: Affine::default(),
        }
    }
}

/// Methods of elements in the element tree.
pub trait Element: Any {
    /// Returns the list of children of this element.
    fn children(&self) -> Vec<ElementAny> {
        Vec::new()
    }

    /// Asks the element to measure itself under the specified constraints, but without actually laying
    /// out the children.
    ///
    /// NOTE: implementations shouldn't add/remove children, or otherwise change the dirty flags
    /// in ElementCtx.
    fn measure(&mut self, layout_input: &LayoutInput) -> Size;

    /// Specifies the size of the element, and lays out its children.
    ///
    /// # Arguments
    /// * `children` - the children of the element.
    /// * `size` - the exact size of the element. Typically, this is one of the sizes returned by a
    /// previous call to `measure`.
    ///
    /// NOTE: implementations shouldn't add/remove children, or otherwise change the dirty flags
    /// in ElementCtx.
    fn layout(&mut self, size: Size) -> LayoutOutput;

    /// Called to perform hit-testing on the bounds of this element.
    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool;

    /// Paints this element on a target surface using the specified `PaintCtx`.
    #[allow(unused_variables)]
    fn paint(&mut self, ctx: &mut PaintCtx);

    /// Called when an event is sent to this element.
    #[allow(unused_variables)]
    fn event(&mut self, ctx: &mut WindowCtx, event: &mut Event) {}
}

/// Exposes the child elements of container elements.
pub trait Container<Item>: Element {
    /// The type of the internal container for child elements.
    type Elements;
    // FIXME: the container can't react to individual changes (e.g. to create or invalidate associated elements)
    // => when this method is called, the container must invalidate any data associated with its
    // children
    fn elements(&mut self) -> &mut Self::Elements;
}

impl dyn Element + 'static {
    /// Downcasts the element to a concrete type.
    pub fn downcast<T: 'static>(&self) -> Option<&T> {
        if (*self).type_id() == TypeId::of::<T>() {
            unsafe {
                // SAFETY: we just checked that the type matches
                let raw = self as *const dyn Element as *const T;
                Some(&*raw)
            }
        } else {
            None
        }
    }

    /// Downcasts the element to a concrete type.
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        // (*self) because of https://users.rust-lang.org/t/calling-the-any-traits-type-id-on-a-mutable-reference-causes-a-weird-compiler-error/84658/2
        if (*self).type_id() == TypeId::of::<T>() {
            unsafe {
                // SAFETY: we just checked that the type matches
                let raw = self as *mut dyn Element as *mut T;
                Some(&mut *raw)
            }
        } else {
            None
        }
    }
}

pub struct ElemBox<T: ?Sized> {
    pub ctx: ElementCtxAny,
    pub element: T,
}

impl<T> ElemBox<T> {
    fn new(element: T) -> ElemBox<T> {
        ElemBox {
            ctx: ElementCtxAny::new(),
            element,
        }
    }
}

impl<T: ?Sized> Deref for ElemBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.element
    }
}

impl<T: ?Sized> DerefMut for ElemBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.element
    }
}

pub struct ElementRef<'a, T: ?Sized>(RcRef<'a, ElemBox<T>>);

impl<'a, T: ?Sized> Deref for ElementRef<'a, T> {
    type Target = ElemBox<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

// Don't newtype the thing because it won't work correctly with reborrows
pub type ElementMut<'a, T: ?Sized> = RcRefMut<'a, ElemBox<T>>;

/*
impl<'a, T: ?Sized> ElementMut<'a, T> {
    pub fn reborrow(&self) -> ElementMut<T> {
        ElementMut(self.0.reborrow())
    }
}

impl<'a, T: ?Sized> Deref for ElementMut<'a, T> {
    type Target = ElemBox<T>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<'a, T: ?Sized> DerefMut for ElementMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}*/

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Weak reference to an element in the element tree.
pub struct WeakElement<T: ?Sized>(Weak<RcCell<ElemBox<T>>>);

pub type WeakElementAny = WeakElement<dyn Element>;

impl<T: ?Sized> Clone for WeakElement<T> {
    fn clone(&self) -> Self {
        WeakElement(Weak::clone(&self.0))
    }
}

impl<T: ?Sized> WeakElement<T> {
    pub fn upgrade(&self) -> Option<ElementRc<T>> {
        self.0.upgrade().map(ElementRc)
    }
}

impl Default for WeakElementAny {
    fn default() -> Self {
        // dummy element because Weak::new doesn't work with dyn trait
        // this is never instantiated, so it's fine
        struct Dummy;
        impl Element for Dummy {
            fn measure(&mut self, _layout_input: &LayoutInput) -> Size {
                unimplemented!()
            }

            fn layout(&mut self, _size: Size) -> LayoutOutput {
                unimplemented!()
            }

            fn hit_test(&self, _ctx: &mut HitTestCtx, _point: Point) -> bool {
                unimplemented!()
            }

            fn paint(&mut self, _ctx: &mut PaintCtx) {
                unimplemented!()
            }
        }
        let weak = Weak::<RefCell<Dummy>>::new();
        WeakElement(weak)
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

impl Ord for WeakElementAny {
    fn cmp(&self, other: &Self) -> Ordering {
        Weak::as_ptr(&self.0)
            .cast::<()>()
            .cmp(&Weak::as_ptr(&other.0).cast::<()>())
    }
}

impl PartialOrd for WeakElementAny {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Strong reference to an element in the element tree.
// Yes it's a big fat Rc<RefCell>, deal with it.
// We don't publicly allow mut access.
pub struct ElementRc<T: ?Sized>(Rc<RcCell<ElemBox<T>>>);

impl<T: ?Sized> Clone for ElementRc<T> {
    fn clone(&self) -> Self {
        ElementRc(self.0.clone())
    }
}

impl<T: ?Sized> ElementRc<T> {
    /// Returns a weak reference to this element.
    pub fn downgrade(&self) -> WeakElement<T> {
        WeakElement(Rc::downgrade(&self.0))
    }

    // Sets the parent of this element.
    //pub fn set_parent(&self, parent: WeakElementAny) {
    //    todo!()
    //}

    /// Borrows the inner element.
    pub(crate) fn borrow(&self) -> RcRef<ElemBox<T>> {
        self.0.borrow()
    }

    /// Borrows the inner element mutably.
    pub(crate) fn borrow_mut(&self) -> RcRefMut<ElemBox<T>> {
        self.0.borrow_mut()
    }

    pub fn lock_mut(&self) -> ElementMut<T> {
        ElementMut(self.borrow_mut())
    }
}

impl<T: Element + ?Sized> ElementRc<T> {
    /// Returns whether this element has a parent.
    pub fn has_parent(&self) -> bool {
        // TODO: maybe not super efficient
        self.parent().is_some()
    }

    /// Returns the parent of this element, if it has one.
    pub fn parent(&self) -> Option<ElementAny> {
        self.borrow().ctx.parent.upgrade()
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

impl PartialOrd for ElementAny {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ElementAny {
    fn cmp(&self, other: &Self) -> Ordering {
        Rc::as_ptr(&self.0).cast::<()>().cmp(&Rc::as_ptr(&other.0).cast::<()>())
    }
}

impl Eq for ElementAny {}

impl fmt::Debug for ElementAny {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ElementAny#{:08x}", Rc::as_ptr(&self.0) as *const () as usize as u32)
    }
}

impl<T: Element> ElementRc<T> {
    pub fn as_dyn(&self) -> ElementAny {
        ElementRc(self.0.clone())
    }

    /// Invokes a method on this widget.
    pub(crate) fn invoke<R>(&self, f: impl FnOnce(ElementMut<T>) -> R) -> R {
        // TODO don't go through dyn
        self.as_dyn().invoke(move |this| {
            let this = this.downcast_mut().expect("unexpected type of element");
            f(this)
        })
    }
}

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
    /// Propagates the dirty flags up the tree.
    pub(crate) fn invoke<R>(&self, f: impl FnOnce(ElementMut<dyn Element>) -> R) -> R {
        let mut inner = ElementMut(self.borrow_mut());
        let r = f(inner);
        inner.ctx.propagate_dirty_flags();
        r
    }

    /// Registers the parent window of this element.
    ///
    /// This is called on the root widget of each window.
    pub(crate) fn set_parent_window(&self, window: WeakWindow) {
        self.borrow_mut().ctx.window = window;
    }

    /// Returns the parent window of this element.
    ///
    /// This can be somewhat costly since it has to climb up the hierarchy of elements up to the
    /// root to get the window handle.
    pub(crate) fn get_parent_window(&self) -> WeakWindow {
        let mut current = self.clone();
        // climb up to the root element which holds a valid window pointer
        while let Some(parent) = current.parent() {
            current = parent;
        }
        let current = current.borrow();
        current.ctx.window.clone()
    }

    /// Returns the transform of this element.
    pub fn transform(&self) -> Affine {
        self.borrow().ctx.transform
    }

    pub fn measure(&self, layout_input: &LayoutInput) -> Size {
        let ref mut inner = *self.borrow_mut();
        inner.measure(layout_input)
    }

    /// Invokes layout on this element and its children, recursively.
    pub fn layout(&self, size: Size) -> LayoutOutput {
        let ref mut inner = *self.borrow_mut();
        inner.ctx.geometry = size;
        let output = inner.layout(size);
        inner.ctx.change_flags.remove(ChangeFlags::LAYOUT);
        output
    }

    /// Returns the list of children of this element.
    pub fn children(&self) -> Vec<ElementAny> {
        self.borrow().children()
    }

    /// Hit-tests this element and its children.
    pub fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        let ref mut inner = *self.borrow_mut();
        let transform = ctx.transform * inner.ctx.transform;
        let local_point = transform.inverse() * point;
        let prev_transform = mem::replace(&mut ctx.transform, transform);
        let hit = inner.hit_test(ctx, local_point);
        if hit {
            ctx.hits.push(self.downgrade());
        }
        ctx.transform = prev_transform;
        hit
    }

    pub fn send_event(&self, ctx: &mut WindowCtx, event: &mut Event) {
        let ref mut inner = *self.borrow_mut();
        inner.event(ctx, event);
        inner.ctx.propagate_dirty_flags();
    }

    pub fn paint(&self, parent_ctx: &mut PaintCtx) {
        let ref mut inner = *self.borrow_mut();
        let transform = inner.ctx.transform;

        parent_ctx.save();
        // TODO baseline
        parent_ctx.transform(&transform, inner.ctx.geometry.to_rect(), 0.0);
        inner.paint(parent_ctx);
        parent_ctx.restore();

        inner.ctx.change_flags.remove(ChangeFlags::PAINT);
    }

    pub(crate) fn paint_on_surface(&self, surface: &DrawableSurface, scale_factor: f64) {
        let mut ctx = PaintCtx::new(surface, scale_factor);
        self.paint(&mut ctx);
    }

    pub fn add_offset(&self, offset: Vec2) {
        self.borrow_mut().ctx.add_offset(offset);
    }

    pub fn set_offset(&self, offset: Vec2) {
        self.borrow_mut().ctx.set_offset(offset);
    }
}

/// Trait for elements that can be converted into a `ElementAny`.
pub trait IntoElementAny {
    fn into_element(self, parent: WeakElementAny, index_in_parent: usize) -> ElementAny;
}

/*
impl<T> IntoElementAny for T
where
    T: Element,
{
    fn into_element(self, parent: WeakElementAny, _index_in_parent: usize) -> ElementAny {
        let mut urc = UniqueRc::new(RefCell::new(self));
        let weak = UniqueRc::downgrade(&urc);
        let state = urc.get_mut().ctx_mut();
        state.parent = parent;
        state.weak_this = WeakElement(weak);
        ElementRc(UniqueRc::into_rc(urc))
    }
}*/

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct ElementCtxAny {
    parent: WeakElementAny,
    /// Weak pointer to this element (~= `Weak<RefCell<dyn Element>>`)
    weak_this: WeakElementAny,
    /// Weak pointer to this element (~= `Weak<dyn Any>`)
    /// This is used for event and subscription functions which expect a `Weak<dyn Any>`
    /// we can't use `weak_this` because it can't coerce dyn Any, even with trait upcasting.
    weak_this_any: Weak<dyn Any>,
    change_flags: ChangeFlags,
    /// Pointer to the parent owner window. Valid only for the root element the window.
    pub(crate) window: WeakWindow,
    /// Layout: transform from local to parent coordinates.
    transform: Affine,
    /// Layout: geometry (size and baseline) of this element.
    geometry: Size,
    /// Name of the element.
    name: String,
    /// Whether the element is focusable via tab-navigation.
    focusable: bool,
    /// Whether this element currently has focus.
    focused: bool,
}

impl ElementCtxAny {
    pub fn new() -> ElementCtxAny {
        ElementCtxAny {
            parent: WeakElementAny::default(),
            weak_this: WeakElementAny::default(),
            weak_this_any: Weak::<()>::default(),
            change_flags: ChangeFlags::NONE,
            window: WeakWindow::default(),
            transform: Affine::default(),
            geometry: Size::ZERO,
            name: String::new(),
            focusable: false,
            focused: false,
        }
    }

    /// Returns the weak pointer to this element.
    pub fn weak_any(&self) -> WeakElementAny {
        self.weak_this.clone()
    }

    pub fn set_offset(&mut self, offset: Vec2) {
        self.transform = Affine::translate(offset);
    }

    pub fn add_offset(&mut self, offset: Vec2) {
        self.transform *= Affine::translate(offset);
    }

    pub fn rect(&self) -> Rect {
        self.geometry.to_rect()
    }

    pub fn size(&self) -> Size {
        self.geometry
    }

    pub fn transform(&self) -> &Affine {
        &self.transform
    }

    pub fn mark_needs_layout(&mut self) {
        self.change_flags |= ChangeFlags::LAYOUT;
    }

    pub fn mark_needs_paint(&mut self) {
        self.change_flags |= ChangeFlags::PAINT;
    }

    pub fn mark_structure_changed(&mut self) {
        self.change_flags |= ChangeFlags::STRUCTURE;
    }

    /// Handles standard input events for activation, hovering, and clicks.
    ///
    /// Emits the corresponding events (ActivatedEvent, HoveredEvent, ClickedEvent)
    /// when the state changed.
    ///
    /// Returns whether the state changed.
    pub fn update_element_state(&mut self, state: &mut ElementState, event: &Event) -> bool {
        match event {
            Event::PointerDown(_) => {
                state.set_active(true);
                self.emit(ActivatedEvent(true));
                true
            }
            Event::PointerUp(_) => {
                if state.is_active() {
                    state.set_active(false);
                    self.emit(ActivatedEvent(false));
                    self.emit(ClickedEvent);
                    true
                } else {
                    false
                }
            }
            Event::PointerEnter(_) => {
                state.set_hovered(true);
                self.emit(HoveredEvent(true));
                true
            }
            Event::PointerLeave(_) => {
                state.set_hovered(false);
                self.emit(HoveredEvent(false));
                true
            }
            _ => false,
        }
    }

    /// Sets the keyboard focus on this widget on the next run of the event loop.
    ///
    /// This doesn't immediately set the `focused` flag: if the element didn't have
    /// focus, `has_focus` will still return `false` until the next event loop iteration.
    pub fn set_focus(&mut self) {
        set_keyboard_focus(self.weak_this.upgrade().unwrap());
    }

    /// Requests that this element captures the pointer events sent to the parent window.
    pub fn set_pointer_capture(&mut self) {
        let weak_this = self.weak_this.clone();
        run_queued(move || {
            if let Some(this) = weak_this.upgrade() {
                let window = this.get_parent_window();
                if let Some(window) = window.upgrade() {
                    window.set_pointer_capture(weak_this);
                }
            }
        })
    }

    pub fn has_focus(&self) -> bool {
        self.focused
    }

    fn propagate_dirty_flags(&mut self) {
        if let Some(parent) = self.parent.upgrade() {
            let mut parent = parent.borrow_mut();
            if parent.ctx.change_flags.contains(self.change_flags) {
                // the parent already has the flags, no need to propagate
                return;
            }
            parent.ctx.change_flags |= self.change_flags;
            parent.ctx.propagate_dirty_flags();
        }

        if let Some(window) = self.window.upgrade() {
            if self.change_flags.contains(ChangeFlags::LAYOUT) {
                window.mark_needs_layout();
            } else if self.change_flags.contains(ChangeFlags::PAINT) {
                window.mark_needs_paint();
            }
        }
    }
}

impl EventSource for ElementCtxAny {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.weak_this_any.clone()
    }
}

pub struct ElementCtx<T> {
    inner: ElementCtxAny,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: 'static> EventSource for ElementCtx<T> {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.inner.weak_this_any.clone()
    }
}

impl<T: 'static> ElementCtx<T> {
    pub fn new() -> ElementCtx<T> {
        ElementCtx {
            inner: ElementCtxAny::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_parent(parent: WeakElementAny) -> ElementCtx<T> {
        let mut ctx = ElementCtx::new();
        ctx.inner.parent = parent;
        ctx
    }

    fn invoke_helper(weak_this: WeakElementAny, f: impl FnOnce(&mut T)) {
        if let Some(this) = weak_this.upgrade() {
            this.invoke(move |this| {
                let this = this.downcast_mut().expect("unexpected type of element");
                f(this);
            });
        }
    }

    pub fn run_later(&mut self, f: impl FnOnce(&mut T) + 'static) {
        let weak_this = self.weak_this.clone();
        run_queued(move || {
            Self::invoke_helper(weak_this, f);
        })
    }

    pub fn run_after(&mut self, duration: Duration, f: impl FnOnce(&mut T) + 'static) {
        let weak_this = self.weak_this.clone();
        run_after(duration, move || {
            Self::invoke_helper(weak_this, f);
        })
    }

    #[track_caller]
    pub fn watch_once(
        &mut self,
        models: impl IntoIterator<Item = Weak<dyn Any>>,
        on_changed: impl FnOnce(&mut T, Weak<dyn Any>) + 'static,
    ) -> SubscriptionKey {
        let weak_this = self.weak_this.clone();
        watch_multi_once(models, move |source| {
            if let Some(this) = weak_this.upgrade() {
                this.invoke(move |this| {
                    let this = this.downcast_mut().expect("unexpected type of element");
                    on_changed(this, source);
                });
            }
        })
    }

    pub fn with_tracking_scope<R>(
        &mut self,
        scope: impl FnOnce() -> R,
        on_changed: impl FnOnce(&mut T) + 'static,
    ) -> R {
        let weak_this = self.weak_this.clone();
        let (r, tracking_scope) = with_tracking_scope(scope);
        tracking_scope.watch_once(move |_| {
            Self::invoke_helper(weak_this, on_changed);
            false
        });
        r
    }
}

impl<T> Deref for ElementCtx<T> {
    type Target = ElementCtxAny;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for ElementCtx<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// A wrapper for elements as they are being constructed.
///
/// Basically this is a wrapper around Rc that provides a `DerefMut` impl since we know it's the
/// only strong reference to it.
pub struct ElementBuilder<T>(UniqueRc<RcCell<ElemBox<T>>>);

impl<T: Default + Element> Default for ElementBuilder<T> {
    fn default() -> Self {
        ElementBuilder::new(Default::default())
    }
}

impl<T: Element> EventSource for ElementBuilder<T> {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.weak().0
    }
}

impl<T: Element> ElementBuilder<T> {
    /// Creates a new `ElementBuilder` instance.
    pub fn new(inner: T) -> ElementBuilder<T> {
        let mut urc = UniqueRc::new(RcCell::new(ElemBox::new(inner)));
        let weak = UniqueRc::downgrade(&urc);
        urc.get_mut().ctx.weak_this = WeakElement(weak.clone());
        urc.get_mut().ctx.weak_this_any = weak;
        ElementBuilder(urc)
    }

    /*
    pub fn new_cyclic(f: impl FnOnce(WeakElement<T>) -> T) -> ElementBuilder<T> {
        let mut urc = UniqueRc::new(RefCell::new(MaybeUninit::uninit())); // UniqueRc<RefCell<MaybeUninit<T>>>
                                                                          // SAFETY: I'd say it's safe to transmute here even if the value is uninitialized
                                                                          // because the resulting weak pointer can't be upgraded anyway.
        let weak: Weak<RefCell<T>> = unsafe { mem::transmute(UniqueRc::downgrade(&urc)) };
        let weak = WeakElement(weak);
        urc.get_mut().write(f(weak.clone()));
        // SAFETY: the value is now initialized
        let mut urc: UniqueRc<RefCell<T>> = unsafe { mem::transmute(urc) };
        urc.get_mut().ctx_mut().weak_this = WeakElement(weak.0);
        ElementBuilder(urc)
    }*/

    pub fn weak(&self) -> WeakElement<T> {
        let weak = UniqueRc::downgrade(&self.0);
        WeakElement(weak)
    }

    pub fn weak_any(&self) -> WeakElementAny {
        let weak = UniqueRc::downgrade(&self.0);
        WeakElement(weak)
    }

    pub fn set_tab_focusable(self) -> Self {
        todo!("set_tab_focusable")
    }

    /// Assigns a name to the element, for debugging purposes.
    pub fn debug_name(mut self, name: impl Into<String>) -> Self {
        self.ctx.name = name.into();
        self
    }

    /// Runs the specified function when the element emits the specified event.
    #[track_caller]
    pub fn on<Event: 'static>(self, mut f: impl FnMut(ElementMut, &Event) + 'static) -> Self {
        let weak = self.weak();
        self.subscribe(move |_, e| {
            if let Some(this) = weak.upgrade() {
                this.lock_mut().this.invoke(|this| {
                    f(this, e);
                });
                true
            } else {
                false
            }
        });
        self
    }

    /// Runs the specified function on the widget, and runs it again when it changes.
    #[track_caller]
    pub fn dynamic(mut self, func: impl FnMut(ElementMut<T>) + 'static) -> Self {
        fn dynamic_helper<T: Element>(
            this: ElementMut<T>,
            weak: WeakElement<T>,
            mut func: impl FnMut(ElementMut<T>) + 'static,
            caller: &'static Location<'static>,
        ) {
            let (_, deps) = with_tracking_scope(|| func(this));
            if !deps.reads.is_empty() {
                watch_multi_once_with_location(
                    deps.reads.into_iter().map(|w| w.0),
                    move |_source| {
                        if let Some(this) = weak.upgrade() {
                            this.invoke(move |this| {
                                dynamic_helper(this, weak, func, caller);
                            });
                        }
                    },
                    caller,
                );
            }
        }

        let weak = self.weak();
        let this = self.0.get_mut();
        dynamic_helper(this, weak, func, Location::caller());
        self
    }

    pub fn with_tracking_scope<R>(
        &mut self,
        scope: impl FnOnce() -> R,
        on_changed: impl FnOnce(&mut T) + 'static,
    ) -> R {
        let weak_this = self.weak();
        let (r, tracking_scope) = with_tracking_scope(scope);
        tracking_scope.watch_once(move |_source| {
            if let Some(this) = weak_this.upgrade() {
                this.invoke(move |this| {
                    on_changed(this);
                });
            }
            false
        });
        r
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
            self.0.try_borrow_unguarded().unwrap_unchecked()
        }
    }
}

impl<T> DerefMut for ElementBuilder<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // We have mutable access to the inner element, so we can safely return a mutable reference.
        self.0.get_mut()
    }
}

impl<T: Element> IntoElementAny for ElementBuilder<T> {
    fn into_element(mut self, parent: WeakElementAny, _index_in_parent: usize) -> ElementAny {
        //let weak = UniqueRc::downgrade(&self.0);
        self.0.get_mut().ctx_mut().parent = parent;
        ElementRc(UniqueRc::into_rc(self.0))
    }
}

/// Dispatches an event to a target element, bubbling up if requested.
///
/// It will first invoke the event handler of the target element.
/// If the event is "bubbling", it will invoke the event handler of the parent element,
/// and so on until the root element is reached.
pub(crate) fn dispatch_event(target: ElementAny, event: &mut Event, bubbling: bool) {
    // get dispatch chain
    let chain = target.ancestors_and_self();

    // compute local-to-root transforms for each visual in the dispatch chain
    // TODO: do this only for events that need it
    let transforms: Vec<Affine> = chain
        .iter()
        .scan(Affine::default(), |acc, element| {
            *acc = *acc * element.transform();
            Some(*acc)
        })
        .collect();

    if bubbling {
        // dispatch the event, bubbling from the target up the root
        for (element, transform) in chain.iter().rev().zip(transforms.iter().rev()) {
            event.set_transform(transform);
            element.send_event(&mut WindowCtx {}, event);
        }
    } else {
        // dispatch the event to the target only
        event.set_transform(transforms.last().unwrap());
        target.send_event(&mut WindowCtx {}, event);
    }
}
