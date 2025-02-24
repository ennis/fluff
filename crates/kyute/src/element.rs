use crate::application::run_queued;
use crate::compositor::DrawableSurface;
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::model::{
    watch_multi_once_with_location, with_tracking_scope, EventSource,
};
use crate::window::WindowHandle;
use crate::{PaintCtx};
use bitflags::{bitflags};
use kurbo::{Point, Rect, Size, Vec2};
use std::any::Any;
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::rc::{Rc, UniqueRc, Weak};
use std::{fmt, mem, ptr};

pub mod prelude {
    pub use crate::element::{
        Element, ElementAny, ElementBuilder, ElementCtx, HitTestCtx, IntoElementAny, WeakElement,
        WeakElementAny,
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
    // The window in which the element is located.
    //pub window: WindowHandle,
    /// The element that has focus.
    pub element: WeakElementAny,
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
pub fn set_keyboard_focus(target: WeakElementAny) {
    run_queued(move || {
        //let parent_window = target.get_parent_window();
        let prev_focus = FOCUSED_ELEMENT.take();
        if let Some(prev_focus) = prev_focus {
            if prev_focus.element == target {
                // Element already has focus. This should be handled earlier.
                //warn!("{:?} already focused", target);
                FOCUSED_ELEMENT.replace(Some(prev_focus));
                return;
            }

            // Send FocusLost event
            if let Some(prev_focus) = prev_focus.element.upgrade() {
                prev_focus.0.ctx.focused.set(false);
                prev_focus.send_event(&mut Event::FocusLost);
            }
        }

        if let Some(target) = target.upgrade() {
            // Send a FocusGained event to the newly focused element.
            target.0.ctx.focused.set(true);
            target.send_event(&mut Event::FocusGained);
        }

        // Update the global focus.
        FOCUSED_ELEMENT.replace(Some(FocusedElement {
            //window: parent_window,
            element: target,
        }));

        // If necessary, activate the target window.
        //if let Some(_parent_window) = parent_window.shared.upgrade() {
            //parent_window.
            //war!("activate window")
        //}

    });
}

pub fn clear_keyboard_focus() {
    run_queued(|| {
        let prev_focus = FOCUSED_ELEMENT.take();
        if let Some(prev_focus) = prev_focus {
            if let Some(prev_focus) = prev_focus.element.upgrade() {
                prev_focus.0.ctx.focused.set(false);
                prev_focus.send_event(&mut Event::FocusLost);
            }
        }
        FOCUSED_ELEMENT.replace(None);
    });
}

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct HitTestCtx {
    pub hits: Vec<WeakElementAny>,
    /// Bounds of the current element in window space.
    pub bounds: Rect,
    //offset: Vec2,
}

impl HitTestCtx {
    pub fn new() -> HitTestCtx {
        HitTestCtx {
            hits: Vec::new(),
            bounds: Rect::ZERO,
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
    ///
    /// FIXME: this should return a baseline
    ///
    /// FIXME: this is not practical, there's no way for the element to fill its parent space.
    ///        for flex layouts, returning input.width.available() is broken because it will
    ///        use up all the space regardless of other elements in the flex, leading to overflow.
    ///        Basically, elements by themselves can NEVER be flexible, they can only be flexible
    ///        by wrapping them in FlexItem.
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
    ///
    /// FIXME: LayoutOutput is useless here, it should always return the size passed in argument
    fn layout(&mut self, size: Size) -> LayoutOutput {
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    /// Called to perform hit-testing on the bounds of this element.
    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool;

    /// Paints this element on a target surface using the specified `PaintCtx`.
    ///
    /// FIXME: I really don't like having to pass `ElementCtx` because it's **always** stored
    ///        next to `self` in memory (we can guarantee that by passing `self: &mut ElemBox<Self>`
    ///        and control the creation of `ElemBox`).
    ///        Also, it makes mutating methods not usable during creation because we can't call them
    ///        directly on an `ElementBuilder`.
    ///        Unfortunately, rust memory semantics don't make it easy.
    ///        Storing the context in `ElemBox` wouldn't work because we want it to be shareable,
    ///        but `&mut ElemBox<Self>` would give exclusive access to it (barring stuff like UnsafePinned).
    #[allow(unused_variables)]
    fn paint(&mut self, ectx: &ElementCtx, ctx: &mut PaintCtx);

    /// Called when an event is sent to this element.
    #[allow(unused_variables)]
    fn event(&mut self, ctx: &ElementCtx, event: &mut Event) {}
}

/*
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
}*/

/*
impl<'a, T:?Sized> Deref for ElemMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.element
    }
}

impl<'a, T:?Sized> DerefMut for ElemMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.element
    }
}*/

/*
impl<'a> ElemMut<'a, dyn Element> {
    /// Downcasts the element to a concrete type.
    pub fn downcast<T: 'static>(&self) -> Option<&ElemMut<T>> {
        if self.element.type_id() == TypeId::of::<T>() {
            unsafe {
                // SAFETY: we just checked that the type matches
                let raw = self as *const ElemMut<dyn Element> as *const ElemMut<T>;
                Some(&*raw)
            }
        } else {
            None
        }
    }

    /// Downcasts the element to a concrete type.
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut ElemMut<T>> {
        // (*self) because of https://users.rust-lang.org/t/calling-the-any-traits-type-id-on-a-mutable-reference-causes-a-weird-compiler-error/84658/2
        if self.element.type_id() == TypeId::of::<T>() {
            unsafe {
                // SAFETY: we just checked that the type matches
                let raw = self as *mut ElemMut<dyn Element> as *mut ElemMut<T>;
                Some(&mut *raw)
            }
        } else {
            None
        }
    }
}*/

impl<T: Element> ElemCell<T> {
    fn new(element: T) -> UniqueRc<ElemCell<T>> {
        let mut rc = UniqueRc::new(ElemCell {
            ctx: ElementCtx::new(),
            element: RefCell::new(element),
        });
        let weak = UniqueRc::downgrade(&rc);
        rc.ctx.weak_this = WeakElement(weak.clone());
        rc.ctx.weak_this_any = weak.clone();
        rc
    }

    fn new_cyclic(f: impl FnOnce(WeakElement<T>) -> T) -> UniqueRc<ElemCell<T>> {
        let mut urc = UniqueRc::new(MaybeUninit::<ElemCell<T>>::uninit());
        // SAFETY: I'd say it's safe to transmute here even if the value is uninitialized
        // because the resulting weak pointer can't be upgraded anyway.
        let weak: Weak<ElemCell<T>> = unsafe { mem::transmute(UniqueRc::downgrade(&urc)) };
        urc.write(ElemCell {
            ctx: ElementCtx::new(),
            element: RefCell::new(f(WeakElement(weak.clone()))),
        });
        // SAFETY: the value is now initialized
        let mut urc: UniqueRc<ElemCell<T>> = unsafe { mem::transmute(urc) };
        urc.ctx.weak_this = WeakElement(weak.clone());
        urc.ctx.weak_this_any = weak;
        urc
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Weak reference to an element in the element tree.
pub struct WeakElement<T: ?Sized>(Weak<ElemCell<T>>);

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

impl<T: ?Sized + 'static> WeakElement<T> {
    pub fn run_later(&self, f: impl FnOnce(&mut T, &ElementCtx) + 'static) {
        let this = self.clone();
        run_queued(move || {
            if let Some(this) = this.upgrade() {
                this.invoke(f);
            }
        })
    }
}

impl<T: Element> WeakElement<T> {
    pub fn as_dyn(&self) -> WeakElementAny {
        WeakElement(self.0.clone())
    }
}

impl<T: 'static> EventSource for WeakElement<T> {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.0.clone()
    }
}

/*
impl EventSource for WeakElementAny {
    fn as_weak(&self) -> Weak<dyn Any> {
        // FIXME: that's not great, we need to upgrade to get the Weak<Any> inside
        //        the ElemBox. This may fail in reasonable situations.
        //        This is doubly stupid because we don't care about the Any in the
        //        subscription system, we just need to be able to compare Weak pointers.
        self.0.upgrade().unwrap().borrow().ctx.weak_this_any.clone()
    }
}*/

impl WeakElementAny {
    pub unsafe fn downcast_unchecked<T: 'static>(self) -> WeakElement<T> {
        unsafe {
            let ptr = self.0.into_raw() as *const ElemCell<T>;
            WeakElement(Weak::from_raw(ptr))
        }
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

            fn paint(&mut self, _ectx: &ElementCtx, _ctx: &mut PaintCtx) {
                unimplemented!()
            }
        }
        let weak = Weak::<ElemCell<Dummy>>::new();
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

struct ElemCell<T:?Sized> {
    ctx: ElementCtx,
    element: RefCell<T>,
}

/// Strong reference to an element in the element tree.
// Yes it's a big fat Rc<RefCell>, deal with it.
// FIXME: consider eliminating the wrapper and use a typedef instead (move methods to ElemCell)
pub struct ElementRc<T: ?Sized>(Rc<ElemCell<T>>);

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
    pub fn borrow(&self) -> Ref<T> {
        self.0.element.borrow()
    }

    /// Borrows the inner element mutably.
    pub fn borrow_mut(&self) -> RefMut<T> {
        self.0.element.borrow_mut()
    }

    /// Invokes a method on this widget.
    ///
    /// Propagates the dirty flags up the tree.
    pub fn invoke<R>(&self, f: impl FnOnce(&mut T, &ElementCtx) -> R) -> R {
        let r = f(&mut *self.0.element.borrow_mut(), &self.0.ctx);
        self.0.ctx.propagate_dirty_flags();
        r
    }
}

impl<T: Element + ?Sized> ElementRc<T> {
    /// Returns whether this element has a parent.
    pub fn has_parent(&self) -> bool {
        self.parent().is_some()
    }

    /// Returns the parent of this element, if it has one.
    pub fn parent(&self) -> Option<ElementAny> {
        self.0.ctx.parent.upgrade()
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

    /// Returns the parent window of this element.
    ///
    /// This can be somewhat costly since it has to climb up the hierarchy of elements up to the
    /// root to get the window handle.
    pub fn get_parent_window(&self) -> WindowHandle {
        let mut current = self.clone();
        // climb up to the root element which holds a valid window pointer
        while let Some(parent) = current.parent() {
            current = parent;
        }
        current.0.ctx.window.clone()
    }

    /// Returns the transform of this element.
    pub fn offset(&self) -> Vec2 {
        self.0.ctx.offset.get()
    }

    pub fn measure(&self, layout_input: &LayoutInput) -> Size {
        let ref mut inner = *self.borrow_mut();
        inner.measure(layout_input)
    }

    /// Invokes layout on this element and its children, recursively.
    pub fn layout(&self, size: Size) -> LayoutOutput {
        let ctx = &self.0.ctx;
        let ref mut inner = *self.borrow_mut();
        ctx.geometry.set(LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        });
        let output =  inner.layout(size);
        ctx.geometry.set(LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: output.baseline,
        });
        let mut f = ctx.change_flags.get();
        f.remove(ChangeFlags::LAYOUT);
        ctx.change_flags.set(f);
        output
    }

    /// Returns the list of children of this element.
    pub fn children(&self) -> Vec<ElementAny> {
        self.borrow().children()
    }

    /// Hit-tests this element and its children.
    pub fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        let ref mut inner = *self.borrow_mut();
        let this_ctx = &self.0.ctx;
        let old_bounds = ctx.bounds;
        ctx.bounds = this_ctx.bounds();
        //let new_origin = ctx.bounds.origin() + this_ctx.offset.get();
        //let prev_transform = mem::replace(&mut ctx.transform, transform);
        //let local_point = this_ctx.offset.get().inverse() * point;
        //let prev_rect = ctx.rect;
        //ctx.rect = this_ctx.bounds();
        let hit = inner.hit_test(ctx, point);
        if hit {
            ctx.hits.push(self.downgrade());
        }
        ctx.bounds = old_bounds;
        //ctx.transform = prev_transform;
        hit
    }

    pub fn send_event(&self, event: &mut Event) {
        let ref mut inner = *self.borrow_mut();
        let ctx = &self.0.ctx;
        inner.event(ctx, event);
        //ctx.propagate_dirty_flags()
        //inner.ctx.propagate_dirty_flags();
    }

    pub fn paint(&self, parent_ctx: &mut PaintCtx) {
        let ref mut inner = *self.borrow_mut();
        let ctx = &self.0.ctx;

        let offset = ctx.offset.get();
        ctx.window_position.set(parent_ctx.bounds.origin() + offset);

        // remove flags before painting, in case the element sets them again
        let mut f = ctx.change_flags.get();
        f.remove(ChangeFlags::PAINT);
        ctx.change_flags.set(f);

        let prev_bounds = parent_ctx.bounds;
        parent_ctx.bounds = Rect::from_origin_size(ctx.window_position.get(), ctx.size());
        inner.paint(ctx, parent_ctx);
        parent_ctx.bounds = prev_bounds;
    }

    pub(crate) fn paint_on_surface(&self, surface: &DrawableSurface, scale_factor: f64) {
        let mut ctx = PaintCtx::new(surface, scale_factor);
        self.paint(&mut ctx);
    }

    pub fn add_offset(&self, offset: Vec2) {
        self.0.ctx.add_offset(offset);
    }

    pub fn set_offset(&self, offset: Vec2) {
        self.0.ctx.set_offset(offset);
    }
}

/// Trait for elements that can be converted into a `ElementAny`.
pub trait IntoElementAny {
    type Element: Element;
    fn into_element(self, parent: WeakElementAny) -> ElementRc<Self::Element>;
    fn into_root_element(self, parent_window: WindowHandle) -> ElementRc<Self::Element>;

    fn into_element_any(self, parent: WeakElementAny) -> ElementAny
    where
        Self: Sized,
        Self::Element: Sized,
    {
        self.into_element(parent).as_dyn()
    }

    fn into_root_element_any(self, parent_window: WindowHandle) -> ElementAny
    where
        Self: Sized,
        Self::Element: Sized,
    {
        self.into_root_element(parent_window).as_dyn()
    }

}

impl<T> IntoElementAny for T
where
    T: Element,
{
    type Element = T;

    fn into_element(self, parent: WeakElementAny) -> ElementRc<Self> {
        let mut urc = ElemCell::new(self);
        urc.ctx.parent = parent;
        ElementRc(UniqueRc::into_rc(urc))
    }

    fn into_root_element(self, parent_window: WindowHandle) -> ElementRc<Self::Element> {
        let mut urc = ElemCell::new(self);
        urc.ctx.window = parent_window;
        ElementRc(UniqueRc::into_rc(urc))
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Context associated to each element and passed to `Element` methods.
pub struct ElementCtx {
   // _pinned: PhantomPinned,
    parent: WeakElementAny,
    /// Weak pointer to this element (~= `Weak<RefCell<dyn Element>>`)
    weak_this: WeakElementAny,
    /// Weak pointer to this element (~= `Weak<dyn Any>`)
    /// This is used for event and subscription functions which expect a `Weak<dyn Any>`
    /// we can't use `weak_this` because it can't coerce dyn Any, even with trait upcasting.
    weak_this_any: Weak<dyn Any>,
    change_flags: Cell<ChangeFlags>,
    /// Pointer to the parent owner window. Valid only for the root element the window.
    pub(crate) window: WindowHandle,
    /// Layout: offset from local to parent coordinates.
    offset: Cell<Vec2>,
    /// Transform from local to window coordinates.
    window_position: Cell<Point>,
    /// Layout: geometry (size and baseline) of this element.
    geometry: Cell<LayoutOutput>,
    /// Name of the element.
    name: String,
    /// Whether the element is focusable via tab-navigation.
    focusable: bool,
    /// Whether this element currently has focus.
    focused: Cell<bool>,
}

impl ElementCtx {
    pub fn new() -> ElementCtx {
        ElementCtx {
            parent: WeakElementAny::default(),
            weak_this: WeakElementAny::default(),
            weak_this_any: Weak::<()>::default(),
            change_flags: Default::default(),
            window: WindowHandle::default(),
            offset: Default::default(),
            window_position: Default::default(),
            geometry: Default::default(),
            name: String::new(),
            focusable: false,
            focused: Cell::new(false),
        }
    }

    /// Returns the weak pointer to this element.
    pub fn weak_any(&self) -> WeakElementAny {
        self.weak_this.clone()
    }

    pub fn set_offset(&self, offset: Vec2) {
        self.offset.set(offset);
    }

    pub fn add_offset(&self, offset: Vec2) {
        self.offset.set(self.offset.get() + offset);
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_origin_size(self.window_position.get(), self.size())
    }

    pub fn size(&self) -> Size {
        let geometry = self.geometry.get();
        Size::new(geometry.width, geometry.height)
    }

    pub fn baseline(&self) -> f64 {
        self.geometry.get().baseline.unwrap_or(0.0)
    }

    pub fn offset(&self) -> Vec2 {
        self.offset.get()
    }

    pub fn mark_needs_layout(&self) {
        self.change_flags.set(self.change_flags.get() | ChangeFlags::LAYOUT);
        self.propagate_dirty_flags();
    }

    pub fn mark_needs_paint(&self) {
        self.change_flags.set(self.change_flags.get() | ChangeFlags::PAINT);
        self.propagate_dirty_flags();
    }

    /*pub fn mark_structure_changed(&mut self) {
        self.change_flags |= ChangeFlags::STRUCTURE;
    }*/

    /// Maps a point in local coordinates to window coordinates.
   //pub fn map_to_window(&self, local_point: Point) -> Point {
   //    local_point + self.window_position.get()
   //}

    /// Maps a point in window coordinates to screen coordinates.
    pub fn map_to_monitor(&self, window_point: Point) -> Point {
        //let window_point = self.map_to_window(local_point);
        self.get_parent_window().map_to_screen(window_point)
    }
    
    /// Maps a rectangle in window coordinates to screen coordinates.
    pub fn map_rect_to_monitor(&self, window_rect: Rect) -> Rect {
        window_rect.with_origin(self.map_to_monitor(window_rect.origin()))
    }

    /// Sets the keyboard focus on this widget on the next run of the event loop.
    ///
    /// This doesn't immediately set the `focused` flag: if the element didn't have
    /// focus, `has_focus` will still return `false` until the next event loop iteration.
    pub fn set_focus(&self) {
        set_keyboard_focus(self.weak_this.clone());
    }

    /// Relinquishes the keyboard focus from this widget.
    pub fn clear_focus(&self) {
        if self.focused.get() {
            clear_keyboard_focus();
        }
    }

    /// Requests that this element captures the pointer events sent to the parent window.
    pub fn set_pointer_capture(&self) {
        let weak_this = self.weak_this.clone();
        run_queued(move || {
            if let Some(this) = weak_this.upgrade() {
                let window = this.get_parent_window();
                window.set_pointer_capture(weak_this);
            }
        })
    }

    pub fn has_focus(&self) -> bool {
        self.focused.get()
    }

    fn propagate_dirty_flags(&self) {
        let flags = self.change_flags.get();
        if let Some(parent) = self.parent.upgrade() {
            //let mut parent = parent.borrow_mut();
            let parent_flags = parent.0.ctx.change_flags.get();
            if parent_flags.contains(flags) {
                // the parent already has the flags, no need to propagate
                return;
            }
            parent.0.ctx.change_flags.set(parent_flags | flags);
            parent.0.ctx.propagate_dirty_flags();
        } else {
            // no parent, this is the root element and it should have a window
            if flags.contains(ChangeFlags::LAYOUT) {
                self.window.mark_needs_layout();
            } else if flags.contains(ChangeFlags::PAINT) {
                self.window.mark_needs_paint();
            }
        }
    }

    /// Returns the parent window of this element.
    ///
    /// This can be somewhat costly since it has to climb up the hierarchy of elements up to the
    /// root to get the window handle.
    pub fn get_parent_window(&self) -> WindowHandle {
        if let Some(parent) = self.parent.upgrade() {
            parent.get_parent_window()
        } else {
            // no parent, this is the root element
            self.window.clone()
        }
    }
}

/*
impl EventSource for ElementCtx {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.weak_this_any.clone()
    }
}*/

////////////////////////////////////////////////////////////////////////////////////////////////////

/// A wrapper for elements as they are being constructed.
///
/// Basically this is a wrapper around Rc that provides a `DerefMut` impl since we know it's the
/// only strong reference to it.
pub struct ElementBuilder<T>(UniqueRc<ElemCell<T>>);

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
        ElementBuilder(ElemCell::new(inner))
    }

    pub fn new_cyclic(f: impl FnOnce(WeakElement<T>) -> T) -> ElementBuilder<T> {
        ElementBuilder(ElemCell::new_cyclic(f))
    }

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

    pub fn set_focus(self) -> Self {
        self.0.ctx.set_focus();
        self
    }

    /// Assigns a name to the element, for debugging purposes.
    pub fn debug_name(mut self, name: impl Into<String>) -> Self {
        self.0.ctx.name = name.into();
        self
    }

    /// Runs the specified function when the element emits the specified event.
    #[track_caller]
    pub fn on<Event: 'static>(self, mut f: impl FnMut(&mut T, &ElementCtx, &Event) + 'static) -> Self {
        let weak = self.weak();
        self.subscribe(move |e| {
            if let Some(this) = weak.upgrade() {
                this.invoke(|this, cx| {
                    f(this,cx, e);
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
    pub fn dynamic(mut self, func: impl FnMut(&mut T, &ElementCtx) + 'static) -> Self {
        fn dynamic_helper<T: Element>(
            this: &mut T,
            ctx: &ElementCtx,
            weak: WeakElement<T>,
            mut func: impl FnMut(&mut T, &ElementCtx) + 'static,
            caller: &'static Location<'static>,
        ) {
            let (_, deps) = with_tracking_scope(|| func(this,ctx));
            if !deps.reads.is_empty() {
                watch_multi_once_with_location(
                    deps.reads.into_iter().map(|w| w.0),
                    move |_source| {
                        if let Some(this) = weak.upgrade() {
                            this.invoke(move |this, ctx| {
                                dynamic_helper(this, ctx, weak, func, caller);
                            });
                        }
                    },
                    caller,
                );
            }
        }

        let weak = self.weak();
        let this = &mut *self.0;
        dynamic_helper(this.element.get_mut(), &this.ctx, weak, func, Location::caller());
        self
    }

    pub fn with_tracking_scope<R>(
        &mut self,
        scope: impl FnOnce() -> R,
        on_changed: impl FnOnce(&mut T, &ElementCtx) + 'static,
    ) -> R {
        let weak_this = self.weak();
        let (r, tracking_scope) = with_tracking_scope(scope);
        tracking_scope.watch_once(move |_source| {
            if let Some(this) = weak_this.upgrade() {
                this.invoke(move |this, cx| {
                    on_changed(this, cx);
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
            self.0.element.try_borrow_unguarded().unwrap_unchecked()
        }
    }
}

impl<T> DerefMut for ElementBuilder<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // We have mutable access to the inner element, so we can safely return a mutable reference.
        self.0.element.get_mut()
    }
}

impl<T: Element> IntoElementAny for ElementBuilder<T> {
    type Element = T;

    fn into_element(mut self, parent: WeakElementAny) -> ElementRc<T> {
        self.0.ctx.parent = parent;
        ElementRc(UniqueRc::into_rc(self.0))
    }

    fn into_root_element(mut self, parent_window: WindowHandle) -> ElementRc<Self::Element> {
        self.0.ctx.window = parent_window;
        ElementRc(UniqueRc::into_rc(self.0))
    }

    fn into_element_any(self, parent: WeakElementAny) -> ElementAny {
        self.into_element(parent).as_dyn()
    }

    fn into_root_element_any(self, parent_window: WindowHandle) -> ElementAny {
        self.into_root_element(parent_window).as_dyn()
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

    //// compute local-to-root transforms for each visual in the dispatch chain
    //// TODO: do this only for events that need it
    //let transforms: Vec<Affine> = chain
    //    .iter()
    //    .scan(Affine::default(), |acc, element| {
    //        *acc = *acc * element.transform();
    //        Some(*acc)
    //    })
    //    .collect();

    if bubbling {
        // dispatch the event, bubbling from the target up the root
        for element in chain.iter() {
            element.send_event(event);
        }
    } else {
        // dispatch the event to the target only
        target.send_event(event);
    }
}
