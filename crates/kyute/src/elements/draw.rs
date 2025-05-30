//! Immediate drawing widget.
use crate::element::prelude::*;
use crate::element::{Measurement, TreeCtx};
use crate::layout::{LayoutInput, LayoutOutput};
use crate::{Event, PaintCtx};
use kurbo::{Point, Size};

/// Represents a visual on the screen.
///
/// `Visual`s can be seen as light-weight, stateless elements: they are not part of the element tree themselves,
/// but they are embedded in an element that is responsible to draw them and handle event propagation.
///
/// Contrary to elements, it's not possible to obtain a weak reference to a visual. Since they
/// are not in the element tree, they can't receive events directly, or be focused.
/// The host element must propagate the events
pub trait Visual {
    /// Layouts this element.
    fn layout(&mut self, input: &LayoutInput) -> Measurement {
        Size::new(
            input.width.available().unwrap_or_default(),
            input.height.available().unwrap_or_default(),
        ).into()
    }

    /// Paints this element on a target surface using the specified `PaintCtx`.
    fn paint(&mut self, ctx: &mut PaintCtx);

    /// Called when an event is sent to this element.
    #[allow(unused_variables)]
    fn event(&mut self, ctx: &mut ElementCtx, event: &mut Event) {}
}

////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct Draw<V> {
    //draw_subscription: SubscriptionKey,
    width: Option<f64>,
    height: Option<f64>,
    baseline: f64,
    visual: V,
}

impl<V> Draw<V>
where
    V: Visual + 'static,
{
    /// Creates a new draw element with the specified visual.
    pub fn new(visual: V) -> ElementBuilder<Self> {
        ElementBuilder::new(Self {
            //draw_subscription: Default::default(),
            width: None,
            height: None,
            baseline: 0.0,
            visual,
        })
    }

    /// Specifies a fixed width for this element.
    pub fn width(mut self: ElementBuilder<Self>, width: impl Into<f64>) -> ElementBuilder<Self> {
        self.width = Some(width.into());
        self
    }

    /// Specifies a fixed height for this element.
    pub fn height(mut self: ElementBuilder<Self>, height: impl Into<f64>) -> ElementBuilder<Self> {
        self.height = Some(height.into());
        self
    }

    /// Specifies the baseline of this element.
    pub fn baseline(mut self: ElementBuilder<Self>, baseline: impl Into<f64>) -> ElementBuilder<Self> {
        self.baseline = baseline.into();
        self
    }
}

impl<V> Element for Draw<V>
where
    V: Visual + 'static,
{
    fn measure(&mut self, _cx: &TreeCtx, layout_input: &LayoutInput) -> Measurement {
        self.visual.layout(layout_input)
    }

    fn layout(&mut self, _cx: &TreeCtx, size: Size) {
        self.visual.layout(&LayoutInput {
            width: size.width.into(),
            height: size.height.into(),
            parent_width: size.width.into(),
            parent_height: size.height.into(),
        });
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        // unsubscribe from previous dependencies as we are calling the draw function
        // again and building a new set of dependencies.
        //self.draw_subscription.unsubscribe();

        // run the draw function within a tracking scope to collect the list of dependencies
        // (models that we read from).
        self.visual.paint(ctx); //with_tracking_scope(|| {

        //});

        // subscribe again to changes
        //self.draw_subscription = self.watch_once(deps.reads.into_iter().map(|w| w.0), |this, _| {
        //    this.ctx.mark_needs_paint();
        //});
    }

    fn event(&mut self, _ectx: &TreeCtx, _event: &mut Event) {
        //self.visual.event(&mut self.ctx, event);
    }
}
