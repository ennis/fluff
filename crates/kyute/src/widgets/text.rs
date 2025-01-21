use kurbo::{Point, Size};
use skia_safe::textlayout;
use tracing::{trace, trace_span, warn};

use crate::drawing::ToSkia;
use crate::element::{Element, ElementAny, ElementCtxAny, HitTestCtx, LayoutCtx, WindowCtx};
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::text::{TextLayout, TextRun};
use crate::PaintCtx;

/// A run of styled text.
pub struct Text {
    ctx: ElementCtxAny,
    relayout: bool,
    intrinsic_size: Option<Size>,
    paragraph: textlayout::Paragraph,
}


impl Text {
    /// Creates a new text element with the specified text.
    ///
    /// # Example
    ///
    /// ```
    /// use kyute::text;
    /// use kyute::widgets::text::Text;
    /// use kyute::text::TextRun;
    ///
    /// let text = Text::new(text![size(20.0) "Hello, " { b "world!" }]);
    /// ```
    pub fn new(text: &[TextRun]) -> Text {
        let paragraph = TextLayout::new(text).inner;
        Text {
            ctx: ElementCtxAny::new(),
            relayout: true,
            intrinsic_size: None,
            paragraph,
        }
    }

    /*pub fn set_text(&self, text: &[TextRun]) {
        let paragraph = TextLayout::new(text).inner;
        self.paragraph.replace(paragraph);
        self.intrinsic_size.set(None);
        self.relayout.set(true);
        self.mark_needs_relayout();
    }*/
}

impl Element for Text {
    fn ctx(&self) -> &ElementCtxAny {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut ElementCtxAny {
        &mut self.ctx
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("Text::measure").entered();

        let p = &mut self.paragraph;
        let space = layout_input.width.available().unwrap_or(f64::INFINITY) as f32;
        p.layout(space);
        Size::new(p.longest_line() as f64, p.height() as f64)
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        let _span = trace_span!("Text::layout").entered();
        let p = &mut self.paragraph;
        p.layout(size.width as f32);
        let output = LayoutOutput {
            width: p.longest_line() as f64,
            height: p.height() as f64,
            baseline: Some(p.alphabetic_baseline() as f64),
        };
        output
    }

    fn hit_test(&self, _ctx: &mut HitTestCtx, point: Point) -> bool {
        self.ctx.rect().contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        ctx.with_canvas(|canvas| {
            self.paragraph.paint(canvas, Point::ZERO.to_skia());
        })
    }

    fn event(&mut self, _ctx: &mut WindowCtx, _event: &mut Event)
    {}
}
