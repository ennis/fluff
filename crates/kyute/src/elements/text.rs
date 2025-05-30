use kurbo::{Point, Size};
use skia_safe::textlayout;
use tracing::trace_span;

use crate::drawing::ToSkia;
use crate::element::{Element, ElementBuilder, HitTestCtx, Measurement, TreeCtx};
use crate::input_event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::text::{IntoTextLayout, TextLayout, TextRun, TextStyle};
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
    pub fn new(text: impl IntoTextLayout) -> ElementBuilder<Text> {
        let paragraph = text.into_text_layout(&TextStyle::default()).inner;
        ElementBuilder::new(Text { paragraph })
    }

    pub fn set_text(&mut self, cx: &TreeCtx, text_style: &TextStyle, text: &[TextRun]) {
        let paragraph = TextLayout::new(text_style, text).inner;
        self.paragraph = paragraph;
        cx.mark_needs_layout();
    }
}

impl Element for Text {
    fn measure(&mut self, _tree: &TreeCtx, layout_input: &LayoutInput) -> Measurement {
        let _span = trace_span!("Text::measure").entered();

        let p = &mut self.paragraph;
        let space = layout_input.width.available().unwrap_or(f64::INFINITY) as f32;
        p.layout(space);

        let mut size = Size::new(p.longest_line() as f64, p.height() as f64);

        // Round up the size to the nearest enclosing logical rect.
        //
        // This avoids accidents where the caller of `measure` truncates
        // the size to an integer, and then calls `layout` with the truncated size
        // (which can lead to a relayout of the text with additional line breaks).
        size = size.ceil();

        eprintln!("Text::measure: {:?} under constraint {:?}", size, layout_input.width);
        
        let baseline = p.alphabetic_baseline()  as f64;
        Measurement {
            size,
            baseline: Some(baseline),
        }
    }

    fn layout(&mut self, _tree: &TreeCtx, size: Size) {
        let _span = trace_span!("Text::layout").entered();
        eprintln!("Text::layout: {:?}", size);
        let p = &mut self.paragraph;
        p.layout(size.width as f32);
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        let position = ctx.bounds.origin().to_skia();
        self.paragraph.paint(ctx.canvas(), position);
    }

    fn event(&mut self, _cx: &TreeCtx, _event: &mut Event) {}
}
