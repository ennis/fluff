//! Immediate drawing widget.
use crate::drawing::DrawCtx;
use crate::element::{ElementBuilder, ElementCtx, ElementCtxAny, HitTestCtx};
use crate::layout::{LayoutInput, LayoutOutput};
use crate::model::{with_tracking_scope, SubscriptionKey};
use crate::{Element, PaintCtx};
use kurbo::{Point, Size};

pub struct Draw<F> {
    ctx: ElementCtx<Self>,
    draw_subscription: SubscriptionKey,
    draw_fn: F,
}

impl<F: Fn(&mut DrawCtx) + 'static> Draw<F> {
    pub fn new(draw_fn: F) -> ElementBuilder<Self> {
        ElementBuilder::new(Self {
            ctx: ElementCtx::new(),
            draw_subscription: Default::default(),
            draw_fn,
        })
    }
}

impl<F> Element for Draw<F>
where
    F: Fn(&mut DrawCtx) + 'static,
{
    fn ctx(&self) -> &ElementCtxAny {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut ElementCtxAny {
        &mut self.ctx
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        Size {
            width: layout_input.width.available().unwrap_or_default(),
            height: layout_input.height.available().unwrap_or_default(),
        }
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        LayoutOutput {
            width: size.width,
            height: size.height,
            // FIXME: we ought to be able to retrieve the baseline from the draw function
            baseline: None,
        }
    }

    fn hit_test(&self, _ctx: &mut HitTestCtx, point: Point) -> bool {
        self.ctx.rect().contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        let rect = self.ctx.rect();
        ctx.with_canvas(|canvas| {
            let mut draw_ctx = DrawCtx::new(canvas, rect, ctx.scale_factor);
            self.draw_subscription.unsubscribe();

            let (_, deps) = with_tracking_scope(|| {
                (self.draw_fn)(&mut draw_ctx);
            });

            self.draw_subscription = self.ctx.watch_once(deps.reads.into_iter().map(|w| w.0), |this, _| {
                this.ctx.mark_needs_paint();
            });
        });
    }
}
