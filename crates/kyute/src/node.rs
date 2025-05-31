use crate::application::run_queued;
use crate::event::{EmitterHandle, EmitterKey, EventEmitter};
use crate::focus::{clear_keyboard_focus, set_keyboard_focus};
use crate::layout::{LayoutInput, LayoutOutput};
use crate::platform::PlatformWindowHandle;
use crate::window::WindowHandle;
use crate::{Element, Event, EventSource, Measurement, PaintCtx};
use bitflags::bitflags;
use kurbo::{Point, Rect, Size, Vec2};
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, UniqueRc, Weak};
use std::{fmt, mem, ptr};
use typed_arena::Arena;
use crate::compositor::CompositionBuilder;

bitflags! {
    #[derive(Copy, Clone, Default)]
    pub struct ChangeFlags: u32 {
        const PAINT = 0b0001;
        const LAYOUT = 0b0010;
        const NONE = 0b0000;
    }
}

/// Context passed to [`Element::hit_test`] to perform hit-testing.
pub struct HitTestCtx {
    /// Bounds of the current element in window space.
    pub bounds: Rect,
    /// List of elements that passed the hit-test.
    hits: Vec<WeakDynNode>,
}

/// Provides access to a node's [data](NodeData) and all parent contexts up
/// to the root node.
pub struct NodeCtx<'a> {
    pub parent: Option<&'a NodeCtx<'a>>,
    pub this: &'a NodeData,
    // Pixel scale factor of the parent window.
    //scale_factor: f64,
}

impl<'a> NodeCtx<'a> {
    /// Returns the parent window of this element.
    ///
    /// This can be somewhat costly since it has to climb up the hierarchy of elements up to the
    /// root to get the window handle.
    pub fn get_window(&self) -> WindowHandle {
        if let Some(parent) = self.parent {
            parent.get_window()
        } else {
            // no parent, this is the root element
            self.this.window.clone()
        }
    }

    ///// Returns the pixel scale factor of the parent window.
    //pub fn scale_factor(&self) -> f64 {
    //    self.scale_factor
    //}

    /// Returns the parent platform window of this element.
    ///
    /// TODO remove the `get_` prefix?
    pub fn get_platform_window(&self) -> PlatformWindowHandle {
        self.get_window().platform_window().unwrap()
    }

    /// Maps a point in window coordinates to screen coordinates.
    pub fn map_to_monitor(&self, window_point: Point) -> Point {
        //let window_point = self.map_to_window(local_point);
        self.get_window().map_to_screen(window_point)
    }

    /// Maps a rectangle in window coordinates to screen coordinates.
    pub fn map_rect_to_monitor(&self, window_rect: Rect) -> Rect {
        window_rect.with_origin(self.map_to_monitor(window_rect.origin()))
    }

    fn propagate_dirty_flags(&self) {
        let flags = self.change_flags.get();
        if let Some(parent) = self.parent {
            //let mut parent = parent.borrow_mut();
            let parent_flags = parent.this.change_flags.get();
            if parent_flags.contains(flags) {
                // the parent already has the flags, no need to propagate
                return;
            }
            parent.this.change_flags.set(parent_flags | flags);
            parent.propagate_dirty_flags();
        } else {
            // no parent, this is the root element, and it should have a window
            if flags.contains(ChangeFlags::LAYOUT) {
                self.window.mark_needs_layout();
            } else if flags.contains(ChangeFlags::PAINT) {
                self.window.mark_needs_paint();
            }
        }
    }

    pub fn mark_needs_layout(&self) {
        self.this
            .change_flags
            .set(self.this.change_flags.get() | ChangeFlags::LAYOUT);
        self.propagate_dirty_flags();
    }

    pub fn mark_needs_paint(&self) {
        self.this
            .change_flags
            .set(self.this.change_flags.get() | ChangeFlags::PAINT);
        self.propagate_dirty_flags();
    }

    /// Creates a `NodeCtx` for a child element of the current element.
    pub(crate) fn with_child<'b>(&'b self, child: &'b RcDynNode) -> NodeCtx<'b> {
        assert!(
            child.0.data.parent == self.this.weak_this,
            "element is not a child of the current element"
        );
        NodeCtx {
            parent: Some(self),
            this: &child.0.data,
            //scale_factor: self.scale_factor,
        }
    }
}

impl<'a> Deref for NodeCtx<'a> {
    type Target = NodeData;

    fn deref(&self) -> &Self::Target {
        self.this
    }
}

/// Represents a node in the UI tree (a bundle of an [element](Element) and its associated [data](NodeData)).
///
/// This is always allocated on the heap and accessed through reference-counted pointers ([`RcNode`]).
struct Node<T: ?Sized> {
    data: NodeData,
    // Yes it's a big fat Rc<RefCell>, deal with it.
    element: RefCell<T>,
}

impl<T: Element> Node<T> {
    fn new(element: T) -> UniqueRc<Node<T>> {
        let mut rc = UniqueRc::new(Node {
            data: NodeData::new(),
            element: RefCell::new(element),
        });
        let weak = UniqueRc::downgrade(&rc);
        rc.data.weak_this = WeakNode(weak.clone());
        //rc.ctx.weak_this_any = weak.clone();
        rc
    }

    fn new_cyclic(f: impl FnOnce(WeakNode<T>) -> T) -> UniqueRc<Node<T>> {
        let mut urc = UniqueRc::new(MaybeUninit::<Node<T>>::uninit());
        // SAFETY: I'd say it's safe to transmute here even if the value is uninitialized
        // because the resulting weak pointer can't be upgraded anyway.
        let weak: Weak<Node<T>> = unsafe { mem::transmute(UniqueRc::downgrade(&urc)) };
        urc.write(Node {
            data: NodeData::new(),
            element: RefCell::new(f(WeakNode(weak.clone()))),
        });
        // SAFETY: the value is now initialized
        let mut urc: UniqueRc<Node<T>> = unsafe { mem::transmute(urc) };
        urc.data.weak_this = WeakNode(weak.clone());
        //urc.ctx.weak_this_any = weak;
        urc
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Weak reference to a [node](RcNode) in the UI tree.
pub struct WeakNode<T: ?Sized>(Weak<Node<T>>);

/// A short-hand for a dynamically-typed `WeakNode` (i.e. a `WeakNode<dyn Element>`).
pub type WeakDynNode = WeakNode<dyn Element>;

impl<T: ?Sized> Clone for WeakNode<T> {
    fn clone(&self) -> Self {
        WeakNode(Weak::clone(&self.0))
    }
}

impl<T: ?Sized> WeakNode<T> {
    pub fn upgrade(&self) -> Option<RcNode<T>> {
        self.0.upgrade().map(RcNode)
    }
}

impl<T: ?Sized + Element + 'static> WeakNode<T> {
    pub fn run_later(&self, f: impl FnOnce(&mut T, &NodeCtx) + 'static) {
        let this = self.clone();
        run_queued(move || {
            if let Some(this) = this.upgrade() {
                this.invoke(f);
            }
        });
    }
}

impl<T: Element> WeakNode<T> {
    pub fn as_dyn(&self) -> WeakDynNode {
        WeakNode(self.0.clone())
    }
}

/*
impl<T: 'static> EventSource for WeakElement<T> {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.0.clone()
    }
}*/

impl WeakDynNode {
    pub unsafe fn downcast_unchecked<T: 'static>(self) -> WeakNode<T> {
        unsafe {
            let ptr = self.0.into_raw() as *const Node<T>;
            WeakNode(Weak::from_raw(ptr))
        }
    }
}

impl Default for WeakDynNode {
    fn default() -> Self {
        // dummy element because Weak::new doesn't work with dyn trait
        // this is never instantiated, so it's fine
        struct Dummy;
        impl Element for Dummy {
            fn measure(&mut self, _tree: &NodeCtx, _layout_input: &LayoutInput) -> Measurement {
                unimplemented!()
            }

            fn layout(&mut self, _tree: &NodeCtx, _size: Size) {
                unimplemented!()
            }

            fn hit_test(&self, _ctx: &mut HitTestCtx, _point: Point) -> bool {
                unimplemented!()
            }

            fn paint(&mut self, _ctx: &mut PaintCtx) {
                unimplemented!()
            }
        }
        let weak = Weak::<Node<Dummy>>::new();
        WeakNode(weak)
    }
}

// Element refs are compared by pointer equality.
impl PartialEq for WeakDynNode {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for WeakDynNode {}

impl Hash for WeakDynNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Weak::as_ptr(&self.0).hash(state)
    }
}

impl Ord for WeakDynNode {
    fn cmp(&self, other: &Self) -> Ordering {
        Weak::as_ptr(&self.0)
            .cast::<()>()
            .cmp(&Weak::as_ptr(&other.0).cast::<()>())
    }
}

impl PartialOrd for WeakDynNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Pointers to UI nodes.
///
/// A reference-counted pointer to a UI _node_, which holds an instance of an [`Element`] and its
/// associated [node data](NodeData).
///
// NOTE: the wrapper is here for a reason: it implements by-reference Eq/Ord/Hash.
// We can't do that with a typedef.
pub struct RcNode<T: ?Sized>(Rc<Node<T>>);

impl<T: ?Sized> Clone for RcNode<T> {
    fn clone(&self) -> Self {
        RcNode(self.0.clone())
    }
}

// Methods that require impl Element
impl<T: ?Sized + Element> RcNode<T> {
    /// Returns a weak reference to this element.
    pub fn downgrade(&self) -> WeakNode<T> {
        WeakNode(Rc::downgrade(&self.0))
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
    pub fn invoke<R>(&self, f: impl FnOnce(&mut T, &NodeCtx) -> R) -> R {
        with_tree_ctx(self, |_element, tree| f(&mut *self.0.element.borrow_mut(), tree))
    }
}

// Methods that only access the node data
impl<T: ?Sized> RcNode<T> {
    /// Returns the data associated to this node.
    pub fn data(&self) -> &NodeData {
        &self.0.data
    }

    /// Returns whether this element has a parent.
    pub fn has_parent(&self) -> bool {
        self.parent().is_some()
    }

    /// Returns the parent of this element, if it has one.
    pub fn parent(&self) -> Option<RcDynNode> {
        self.0.data.parent.upgrade()
    }

    /// Returns the change flags of this element.
    pub fn change_flags(&self) -> ChangeFlags {
        self.0.data.change_flags.get()
    }

    /// Returns the bounds of this element in logical window coordinates.
    pub fn bounds(&self) -> Rect {
        self.0.data.bounds()
    }

    /// Sets the focused flag on this element.
    ///
    /// This is only called by `set_keyboard_focus` and should not be called directly.
    pub fn set_focused(&self, focused: bool) {
        self.0.data.focused.set(focused);
    }
}

pub type RcDynNode = RcNode<dyn Element>;

// Element refs are compared by pointer equality.
impl PartialEq for RcDynNode {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl PartialEq<WeakDynNode> for RcDynNode {
    fn eq(&self, other: &WeakDynNode) -> bool {
        ptr::addr_eq(other.0.as_ptr(), Rc::as_ptr(&self.0))
    }
}

impl PartialEq<RcDynNode> for WeakDynNode {
    fn eq(&self, other: &RcDynNode) -> bool {
        ptr::addr_eq(self.0.as_ptr(), Rc::as_ptr(&other.0))
    }
}

impl PartialOrd for RcDynNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RcDynNode {
    fn cmp(&self, other: &Self) -> Ordering {
        Rc::as_ptr(&self.0).cast::<()>().cmp(&Rc::as_ptr(&other.0).cast::<()>())
    }
}

impl Eq for RcDynNode {}

impl fmt::Debug for RcDynNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ElementAny#{:08x}", Rc::as_ptr(&self.0) as *const () as usize as u32)
    }
}

impl<T: Element> RcNode<T> {
    pub fn as_dyn(&self) -> RcDynNode {
        RcNode(self.0.clone())
    }
}

impl RcDynNode {
    /// Returns the list of ancestors of this visual, plus this visual itself, sorted from the root
    /// to this visual.
    pub fn ancestors_and_self(&self) -> Vec<RcDynNode> {
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
        current.0.data.window.clone()
    }

    /// Returns the transform of this element.
    pub fn offset(&self) -> Vec2 {
        self.0.data.offset.get()
    }

    pub fn measure(&self, parent: &NodeCtx, layout_input: &LayoutInput) -> Measurement {
        let ref mut inner = *self.borrow_mut();
        inner.measure(
            &NodeCtx {
                parent: Some(parent),
                this: &self.0.data,
            },
            layout_input,
        )
    }

    /// Invokes layout on this element and its children, recursively.
    fn layout_inner(&self, parent: Option<&NodeCtx>, size: Size) {
        let ctx = &self.0.data;
        let child_tree = NodeCtx { parent, this: ctx };
        let ref mut inner = *self.borrow_mut();
        ctx.geometry.set(LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        });
        inner.layout(&child_tree, size);
        let mut f = ctx.change_flags.get();
        f.remove(ChangeFlags::LAYOUT);
        ctx.change_flags.set(f);
    }

    pub fn layout(&self, tree: &NodeCtx, size: Size) {
        self.layout_inner(Some(tree), size)
    }

    /// Returns the list of children of this element.
    pub fn children(&self) -> Vec<RcDynNode> {
        self.borrow().children()
    }

    /// Hit-tests this element and its children.
    pub fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        let ref mut inner = *self.borrow_mut();
        let this_ctx = &self.0.data;
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

    pub(crate) fn send_event(&self, tree: &NodeCtx, event: &mut Event) {
        let ref mut inner = *self.borrow_mut();
        inner.event(tree, event);
        //ctx.propagate_dirty_flags()
        //inner.ctx.propagate_dirty_flags();
    }

    pub fn add_offset(&self, offset: Vec2) {
        self.0.data.add_offset(offset);
    }

    pub fn set_offset(&self, offset: Vec2) {
        self.0.data.set_offset(offset);
    }
}

/// Types that can be converted into a UI node.
///
/// This is implemented for any type that implements the `Element` trait,
/// and also `NodeBuilder`.
pub trait IntoNode {
    /// The element type of the created node.
    type Element: Element;

    /// Builds a `Node` with the specified parent.
    fn into_node(self, parent: WeakDynNode) -> RcNode<Self::Element>;

    /// Builds an `ElementAny` with the specified parent window.
    fn into_root_node(self, parent_window: WindowHandle) -> RcNode<Self::Element>;

    /// Builds a type-erased UI node with the specified parent.
    fn into_dyn_node(self, parent: WeakDynNode) -> RcDynNode
    where
        Self: Sized,
        Self::Element: Sized,
    {
        self.into_node(parent).as_dyn()
    }

    fn into_root_dyn_node(self, parent_window: WindowHandle) -> RcDynNode
    where
        Self: Sized,
        Self::Element: Sized,
    {
        self.into_root_node(parent_window).as_dyn()
    }
}

impl<T> IntoNode for T
where
    T: Element,
{
    type Element = T;

    fn into_node(self, parent: WeakDynNode) -> RcNode<Self> {
        let mut urc = Node::new(self);
        urc.data.parent = parent;
        RcNode(UniqueRc::into_rc(urc))
    }

    fn into_root_node(self, parent_window: WindowHandle) -> RcNode<Self::Element> {
        let mut urc = Node::new(self);
        urc.data.window = parent_window;
        RcNode(UniqueRc::into_rc(urc))
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Common data associated to each [node](RcNode) in the UI tree.
///
/// Each node has its own instance of `ElementCtx` which contains various information about the
/// node, such as its focus state, parent, geometry, and change flags. This information can be
/// access through the various contexts passed to the methods of the `Element` trait
/// (see [`TreeCtx`](NodeCtx), etc.).
pub struct NodeData {
    // _pinned: PhantomPinned,

    // TODO: make this a raw pointer (somehow). This is because with weak pointers it's impossible
    //       to borrow stuff from a parent element.
    //       To be fully safe, this implies that the parent is pinned in memory (which should be
    //       the case anyway), but this means that we can't use Rc.
    parent: WeakDynNode,
    /// Weak pointer to this element (~= `Weak<RefCell<dyn Element>>`)
    weak_this: WeakDynNode,
    /// Event emitter handle,
    emitter_handle: EmitterHandle,

    pub(crate) change_flags: Cell<ChangeFlags>,
    /// Pointer to the parent owner window. Valid only for the root element the window.
    pub(crate) window: WindowHandle,
    /// Layout: offset from local to parent coordinates.
    pub(crate) offset: Cell<Vec2>,
    /// Transform from local to window coordinates.
    pub(crate) window_position: Cell<Point>,
    /// Layout: geometry (size and baseline) of this element.
    /// FIXME: this doesn't store the baseline anymore
    geometry: Cell<LayoutOutput>,
    /// Name of the element.
    name: String,
    /// Whether the element is focusable via tab-navigation.
    _focusable: bool,
    /// Whether this element currently has keyboard focus.
    focused: Cell<bool>,
}

impl EventSource for NodeData {
    fn emitter_key(&self) -> EmitterKey {
        self.emitter_handle.key()
    }
}

impl NodeData {
    pub fn new() -> NodeData {
        NodeData {
            parent: WeakDynNode::default(),
            weak_this: WeakDynNode::default(),
            //weak_this_any: Weak::<()>::default(),
            emitter_handle: EmitterHandle::new(),
            change_flags: Cell::new(ChangeFlags::PAINT | ChangeFlags::LAYOUT),
            window: WindowHandle::default(),
            offset: Default::default(),
            window_position: Default::default(),
            geometry: Default::default(),
            name: String::new(),
            _focusable: false,
            focused: Cell::new(false),
        }
    }

    /// Returns the weak pointer to this element.
    pub fn weak_any(&self) -> WeakDynNode {
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

    /*pub fn mark_structure_changed(&mut self) {
        self.change_flags |= ChangeFlags::STRUCTURE;
    }*/

    /// Maps a point in local coordinates to window coordinates.
    //pub fn map_to_window(&self, local_point: Point) -> Point {
    //    local_point + self.window_position.get()
    //}

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
        });
    }

    pub fn has_focus(&self) -> bool {
        self.focused.get()
    }
}

/*
impl EventSource for ElementCtx {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.weak_this_any.clone()
    }
}*/

////////////////////////////////////////////////////////////////////////////////////////////////////

/// A wrapper for UI nodes before they are added to the UI tree.
///
/// This is used to allow mutable access to the node before it is added to a UI tree.
///
/// This type implements the `IntoNode` trait, so it can be used in function arguments
/// that expect `IntoNode`.
///
/// Internally, this is a wrapper around `Rc` that guarantees that it is the only strong reference
/// to the node, so that it can be safely mutated.
pub struct NodeBuilder<T>(UniqueRc<Node<T>>);

impl<T: Default + Element> Default for NodeBuilder<T> {
    fn default() -> Self {
        NodeBuilder::new(Default::default())
    }
}

impl<T: Element> EventSource for NodeBuilder<T> {
    fn emitter_key(&self) -> EmitterKey {
        self.0.data.emitter_handle.key()
    }
}

impl<T: Element> NodeBuilder<T> {
    /// Creates a new `ElementBuilder` instance.
    pub fn new(inner: T) -> NodeBuilder<T> {
        NodeBuilder(Node::new(inner))
    }

    pub fn new_cyclic(f: impl FnOnce(WeakNode<T>) -> T) -> NodeBuilder<T> {
        NodeBuilder(Node::new_cyclic(f))
    }

    pub fn weak(&self) -> WeakNode<T> {
        let weak = UniqueRc::downgrade(&self.0);
        WeakNode(weak)
    }

    pub fn weak_any(&self) -> WeakDynNode {
        let weak = UniqueRc::downgrade(&self.0);
        WeakNode(weak)
    }

    pub fn set_tab_focusable(self) -> Self {
        todo!("set_tab_focusable")
    }

    pub fn set_focus(self) -> Self {
        self.0.data.set_focus();
        self
    }

    /// Assigns a name to the element, for debugging purposes.
    pub fn debug_name(mut self, name: impl Into<String>) -> Self {
        self.0.data.name = name.into();
        self
    }

    /// Measures the element with the specified layout input.
    pub fn measure(&self, layout_input: &LayoutInput) -> Measurement {
        let ref mut inner = *self.0.element.borrow_mut();
        let child_tree = NodeCtx {
            parent: None,
            this: &self.0.data,
        };
        inner.measure(&child_tree, layout_input)
    }

    /// Runs the specified function when an event source emits an event.
    pub fn connect<Event, Source>(&self, source: &Source, mut f: impl FnMut(&mut T, &NodeCtx, &Event) + 'static)
    where
        Event: 'static,
        Source: EventSource + EventEmitter<Event>,
    {
        let weak = self.weak();
        source.subscribe(move |e| {
            if let Some(this) = weak.upgrade() {
                this.invoke(|this, cx| {
                    f(this, cx, e);
                });
                true
            } else {
                false
            }
        });
    }

    /// Runs the specified function when the element emits the specified event.
    #[track_caller]
    pub fn on<Event: 'static>(self, mut f: impl FnMut(&mut T, &NodeCtx, &Event) + 'static) -> Self {
        let weak = self.weak();
        self.subscribe(move |e| {
            if let Some(this) = weak.upgrade() {
                this.invoke(|this, cx| {
                    f(this, cx, e);
                });
                true
            } else {
                false
            }
        });
        self
    }

    /*
    /// Runs the specified function on the widget, and runs it again when it changes.
    #[track_caller]
    pub fn dynamic(mut self, func: impl FnMut(&mut T, &TreeCtx) + 'static) -> Self {
        fn dynamic_helper<T: Element>(
            this: &mut T,
            ctx: &TreeCtx,
            weak: WeakElement<T>,
            mut func: impl FnMut(&mut T, &TreeCtx) + 'static,
            caller: &'static Location<'static>,
        ) {
            let (_, deps) = with_tracking_scope(|| func(this, ctx));
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
        dynamic_helper(
            this.element.get_mut(),
            &TreeCtx {
                this: &this.ctx,
                parent: None,
            },
            weak,
            func,
            Location::caller(),
        );
        self
    }

    pub fn with_tracking_scope<R>(
        &mut self,
        scope: impl FnOnce() -> R,
        on_changed: impl FnOnce(&mut T, &TreeCtx) + 'static,
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
    }*/
}

impl<T> Deref for NodeBuilder<T> {
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

impl<T> DerefMut for NodeBuilder<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // We have mutable access to the inner element, so we can safely return a mutable reference.
        self.0.element.get_mut()
    }
}

impl<T: Element> IntoNode for NodeBuilder<T> {
    type Element = T;

    fn into_node(mut self, parent: WeakDynNode) -> RcNode<T> {
        self.0.data.parent = parent;
        RcNode(UniqueRc::into_rc(self.0))
    }

    fn into_root_node(mut self, parent_window: WindowHandle) -> RcNode<Self::Element> {
        self.0.data.window = parent_window;
        RcNode(UniqueRc::into_rc(self.0))
    }

    fn into_dyn_node(self, parent: WeakDynNode) -> RcDynNode {
        self.into_node(parent).as_dyn()
    }

    fn into_root_dyn_node(self, parent_window: WindowHandle) -> RcDynNode {
        self.into_root_node(parent_window).as_dyn()
    }
}

/// Represents the root of a UI tree.
///
/// This manages the root node of a UI tree, and by extension, also manages a complete UI tree
/// attached to a window.
///
/// This type provides the entry points to measure, layout, and paint the UI tree,
/// as well as to dispatch events.
pub struct Root {
    /// The root element.
    root: RcDynNode,
}

impl Root {
    /// Creates a new UI root attached to the specified window.
    pub fn new(root: impl IntoNode, window: WindowHandle) -> Self {
        Root {
            root: root.into_root_dyn_node(window),
        }
    }

    /// Returns the root node.
    pub fn root(&self) -> &RcDynNode {
        &self.root
    }

    /// Measures the root node under the specified available size.
    pub fn measure(&self, available: Size) -> Measurement {
        let ref mut element = *self.root.borrow_mut();
        element.measure(
            &NodeCtx {
                parent: None,
                this: &self.root.data(),
            },
            &LayoutInput { available },
        )
    }

    /// Sets the size of the root node, and lays out the UI tree.
    pub fn layout(&self, size: Size) {
        self.root.layout_inner(None, size)
    }

    /// Paints the UI tree.
    pub fn paint(&self, composition_builder: &mut CompositionBuilder) {
        with_tree_ctx(&self.root, |element, tree| {
            // clear the paint dirty flag
            let mut f = tree.change_flags.get();
            f.remove(ChangeFlags::PAINT);
            tree.change_flags.set(f);

            let mut ctx = PaintCtx::new(tree, composition_builder);
            element.borrow_mut().paint(&mut ctx);
        })
    }

    /// Returns the change flags of the root node.
    pub fn change_flags(&self) -> ChangeFlags {
        self.root.change_flags()
    }



    /// Hit-tests the UI tree.
    ///
    /// # Arguments
    ///
    /// * `point` - the point in logical window coordinates to hit-test
    pub fn hit_test(&self, point: Point) -> Vec<WeakDynNode> {
        let mut ctx = HitTestCtx {
            hits: Vec::new(),
            bounds: Rect::ZERO,
        };
        self.root.hit_test(&mut ctx, point);
        ctx.hits
    }
}

/// Dispatches an event to a target node, bubbling up if requested.
///
/// It will first invoke the event handler of the node.
/// If the event is "bubbling", it will invoke the event handler of the parent node,
/// and so on until the root node is reached.
pub(crate) fn dispatch_event(target: RcDynNode, event: &mut Event, bubbling: bool) {
    with_tree_ctx(&target, |target, tree| {
        if bubbling {
            // dispatch the event, bubbling from the target up the root
            let mut current_ctx = Some(tree);
            let mut current_elem = Some(target.clone());
            while let Some(ctx) = current_ctx {
                current_elem.as_ref().unwrap().send_event(ctx, event);
                current_elem = current_elem.as_ref().unwrap().parent();
                current_ctx = ctx.parent;
            }
        } else {
            // dispatch the event to the target only
            target.send_event(tree, event);
        }
    });
}

/// Builds the `NodeCtx` for the specified node and invokes a closure.
pub(crate) fn with_tree_ctx<T: Element + ?Sized, R>(
    target: &RcNode<T>,
    f: impl FnOnce(&RcNode<T>, &NodeCtx) -> R,
) -> R {
    // get list of ancestors
    let mut ancestors = Vec::new(); // Vec<ElementAny>
    let mut current = target.parent();
    while let Some(parent) = current {
        ancestors.push(parent.clone());
        current = parent.parent();
    }
    ancestors.reverse();

    // build chain of TreeCtx for ancestors
    let arena = Arena::new();
    let mut tree = None;
    for e in ancestors.iter() {
        tree = Some(&*arena.alloc(NodeCtx {
            parent: tree,
            this: &e.0.data,
        }));
    }

    // TreeCtx for target
    //
    // It would be shorter if we could just push `target` in `ancestors`, but the vector contains
    // `ElementAny` whereas target is `ElementRc<T: Element+?Sized>`, which cannot coerce
    // to `ElementAny`. This is incredibly annoying.
    // (see https://stackoverflow.com/questions/57398118/why-cant-sized-trait-be-cast-to-dyn-trait)
    let tree = NodeCtx {
        parent: tree,
        this: &target.0.data,
    };

    f(target, &tree)
}
