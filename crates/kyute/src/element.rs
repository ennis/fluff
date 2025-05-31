//! Definition of the `Element` trait.
use crate::event::{EventEmitter, EventSource};
use crate::input_event::Event;
use crate::layout::{LayoutInput, Measurement};
use crate::node::WeakDynNode;
use crate::{HitTestCtx, NodeCtx, PaintCtx, RcDynNode};
use kurbo::{Point, Size};
use std::any::Any;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

/// Commonly used types and traits for elements in the UI tree.
pub mod prelude {
    pub use crate::input_event::Event;
    pub use crate::layout::{LayoutInput, LayoutOutput, SizeValue};
    pub use crate::PaintCtx;
}



////////////////////////////////////////////////////////////////////////////////////////////////////


/// Methods of elements in the element tree.
pub trait Element: Any {
    /// Returns the list of child nodes.
    ///
    /// If your element manages child nodes, you should override this method to return them.
    fn children(&self) -> Vec<RcDynNode> {
        Vec::new()
    }

    /*/// Called when the element is added to the tree.
    fn added(&mut self, ctx: &TreeCtx) {
        // by default, propagate to children
        for child in self.children() {
            child.borrow_mut().added(ctx);
        }
    }

    /// Called when the element is removed from the tree.
    fn removed(&mut self, ctx: &TreeCtx) {
        for child in self.children() {
            // FIXME: context
            child.borrow_mut().removed(ctx);
        }
    }*/

    /// Asks the element to measure itself under the specified constraints, but without actually laying
    /// out the children.
    ///
    /// # Note
    ///
    /// Implementations shouldn't add/remove children, or otherwise change the dirty flags
    /// in ElementCtx.
    fn measure(&mut self, tree: &NodeCtx, layout_input: &LayoutInput) -> Measurement;

    /// Specifies the size of the element, and lays out its children.
    ///
    /// # Arguments
    /// * `children` - the children of the element.
    /// * `size` - the exact size of the element. Typically, this is one of the sizes returned by a
    /// previous call to `measure`.
    ///
    /// # Note
    ///
    /// Implementations shouldn't add/remove children, or otherwise change the dirty flags
    /// in ElementCtx.
    #[allow(unused_variables)]
    fn layout(&mut self, tree: &NodeCtx, size: Size) {}

    /// Called to perform hit-testing on this element and its children, recursively.
    ///
    /// This is used primarily to determine which element should receive a pointer event.
    ///
    /// Implementations should also hit-test their children recursively by calling `RcNode::hit_test`
    /// (unless the element explicitly filters out pointer events for its children).
    ///
    /// The default implementation checks whether the point is inside the layout bounds of this
    /// element. You should reimplement this method if your element has children, so that they
    /// can be hit-tested as well.
    ///
    /// # Arguments
    ///
    /// * `ctx` - hit-test context. Should be passed to child elements.
    /// * `point` - the point to test, in window coordinates
    ///
    /// # Example
    ///
    /// ```rust
    /// use kurbo::Point;
    /// use kyute::Element;
    /// use kyute::element::{HitTestCtx, RcDynNode};
    ///
    /// pub struct MyElement {
    ///    child: RcDynNode,
    /// }
    ///
    /// impl Element for MyElement {
    ///    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
    ///       // Check if the point is inside the bounds of this element
    ///       if ctx.bounds.contains(point) {
    ///          // assume that the child is fully contained in the parent
    ///          // so hit-test it only if the point is inside the parent
    ///          self.child.hit_test(ctx, point);
    ///          true
    ///      } else {
    ///          false
    ///     }
    /// }
    /// ```
    ///
    /// FIXME: this should receive a TreeCtx like the other methods
    /// TODO: this could be removed entirely and instead rely only on node data to perform hit-testing.
    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

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
    fn paint(&mut self, ctx: &mut PaintCtx);

    /// Called when an event is sent to this element.
    #[allow(unused_variables)]
    fn event(&mut self, ctx: &NodeCtx, event: &mut Event) {}
}


// New repaint system:
// - mark_paint_dirty may not propagate up to the root: stops at compositor layers (aka "repaint barrier" elements)
// - repaint barrier elements added to the list of dirty elements
// - when repaint requested, on repaint, repaint everything in the list of dirty elements
