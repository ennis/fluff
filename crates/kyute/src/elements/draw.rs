//! Immediate drawing widget.
use crate::element::prelude::*;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::model::{with_tracking_scope, SubscriptionKey};
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
    fn layout(&mut self, input: &LayoutInput) -> Size {
        Size::new(
            input.width.available().unwrap_or_default(),
            input.height.available().unwrap_or_default(),
        )
    }

    /// Paints this element on a target surface using the specified `PaintCtx`.
    fn paint(&mut self, ctx: &mut PaintCtx);

    /// Called when an event is sent to this element.
    #[allow(unused_variables)]
    fn event(&mut self, ctx: &mut ElementCtxAny, event: &mut Event) {}
}

// Blanket impl for visuals
impl<V> IntoElementAny for V
where
    V: Visual + 'static,
{
    fn into_element(self, parent: WeakElementAny, index_in_parent: usize) -> ElementAny {
        Draw::new(self).into_element(parent, index_in_parent)
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct Draw<V> {
    ctx: ElementCtx<Self>,
    draw_subscription: SubscriptionKey,
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
            ctx: ElementCtx::new(),
            draw_subscription: Default::default(),
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
    fn ctx(&self) -> &ElementCtxAny {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut ElementCtxAny {
        &mut self.ctx
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        self.visual.layout(layout_input)
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        self.visual.layout(&LayoutInput {
            width: size.width.into(),
            height: size.height.into(),
            parent_width: size.width.into(),
            parent_height: size.height.into(),
        });
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: Some(self.baseline),
        }
    }

    fn hit_test(&self, _ctx: &mut HitTestCtx, point: Point) -> bool {
        self.ctx.rect().contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        // unsubscribe from previous dependencies as we are calling the draw function
        // again and building a new set of dependencies.
        self.draw_subscription.unsubscribe();

        // run the draw function within a tracking scope to collect the list of dependencies
        // (models that we read from).
        let (_, deps) = with_tracking_scope(|| {
            self.visual.paint(ctx);
        });

        // subscribe again to changes
        self.draw_subscription = self.ctx.watch_once(deps.reads.into_iter().map(|w| w.0), |this, _| {
            this.ctx.mark_needs_paint();
        });
    }

    fn event(&mut self, _ctx: &mut WindowCtx, event: &mut Event) {
        self.visual.event(&mut self.ctx, event);
    }
}
