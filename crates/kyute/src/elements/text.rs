use kurbo::{Point, Size};
use skia_safe::textlayout;
use tracing::trace_span;

use crate::drawing::ToSkia;
use crate::element::{Element, ElementBuilder, ElementCtx, ElementRc, HitTestCtx};
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::text::{TextLayout, TextRun, TextStyle};
use crate::PaintCtx;

/// A run of styled text.
pub struct Text {
    paragraph: textlayout::Paragraph,
}

impl Text {
    /// Creates a new text element with the specified text.
    ///
    /// # Example
    ///
    /// ```
    /// use kyute::text;
    /// use kyute::elements::text::Text;
    /// use kyute::text::TextRun;
    ///
    /// let text = Text::new(text![FontSize(20.0) "Hello, " { FontWeight(FontWeight::BOLD) "world!" }]);
    /// ```
    pub fn new(text: impl Into<TextLayout>) -> ElementBuilder<Text> {
        let paragraph = text.into().inner;
        ElementBuilder::new(Text {
            paragraph,
        })
    }

    pub fn set_text(&mut self, cx: &ElementCtx, text_style: &TextStyle, text: &[TextRun]) {
        let paragraph = TextLayout::new(text_style, text).inner;
        self.paragraph = paragraph;
        cx.mark_needs_layout();
    }
}

impl Element for Text {
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

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.rect.contains(point)
    }

    fn paint(&mut self, _cx: &ElementCtx, ctx: &mut PaintCtx) {
        self.paragraph.paint(ctx.canvas(), Point::ZERO.to_skia());
    }

    fn event(&mut self, _cx: &ElementCtx, _event: &mut Event) {}
}
