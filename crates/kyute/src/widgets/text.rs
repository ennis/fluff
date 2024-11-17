use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use kurbo::{Point, Size};
use skia_safe::textlayout;
use tracing::{trace, trace_span};

use crate::drawing::ToSkia;
use crate::element::{Node, Element};
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput, SizeConstraint};
use crate::text::{TextLayout, TextRun};
use crate::PaintCtx;

pub struct Text {
    element: Node,
    relayout: Cell<bool>,
    intrinsic_size: Cell<Option<Size>>,
    paragraph: RefCell<textlayout::Paragraph>,
}

impl Deref for Text {
    type Target = Node;

    fn deref(&self) -> &Self::Target {
        &self.element
    }
}

impl Text {
    pub fn new(text: &[TextRun]) -> Rc<Text> {
        let paragraph = TextLayout::new(text).inner;
        Node::new_derived(|element| Text {
            element,
            relayout: Cell::new(true),
            intrinsic_size: Cell::new(None),
            paragraph: RefCell::new(paragraph),
        })
    }

    fn calculate_intrinsic_size(&self) -> Size {
        // FIXME intrinsic height
        Size::new(self.paragraph.borrow().max_intrinsic_width() as f64, 16.0)
    }

    pub fn set_text(&self, text: &[TextRun]) {
        let paragraph = TextLayout::new(text).inner;
        self.paragraph.replace(paragraph);
        self.intrinsic_size.set(None);
        self.relayout.set(true);
        self.mark_needs_relayout();
    }
}

impl Element for Text {
    fn node(&self) -> &Node {
        &self.element
    }

    fn measure(&self, _children: &[Rc<dyn Element>], layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("TextEdit::measure",).entered();

        let p = &mut *self.paragraph.borrow_mut();
        let space = layout_input.width.available().unwrap_or(f64::INFINITY) as f32;
        p.layout(space);
        Size::new(p.longest_line() as f64, p.height() as f64)
    }

    fn layout(&self, _children: &[Rc<dyn Element>], size: Size) -> LayoutOutput {
        let _span = trace_span!("Text::layout").entered();
        let p = &mut *self.paragraph.borrow_mut();
        p.layout(size.width as f32);
        let output = LayoutOutput {
            width: p.longest_line() as f64,
            height: p.height() as f64,
            baseline: Some(p.alphabetic_baseline() as f64),
        };
        output
    }

    fn hit_test(&self, _point: Point) -> bool {
        false
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        ctx.with_canvas(|canvas| {
            self.paragraph.borrow().paint(canvas, Point::ZERO.to_skia());
        })
    }

    fn event(&self, _event: &mut Event)
    {}
}
