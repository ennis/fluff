use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use kurbo::{Point, Size};
use skia_safe::textlayout;
use tracing::{trace, trace_span};

use crate::drawing::ToSkia;
use crate::element::{Element, ElementMethods};
use crate::event::Event;
use crate::layout::{LayoutInput, LayoutOutput, SizeConstraint};
use crate::PaintCtx;
use crate::text::{TextLayout, TextRun};

pub struct Text {
    element: Element,
    relayout: Cell<bool>,
    intrinsic_size: Cell<Option<Size>>,
    paragraph: RefCell<textlayout::Paragraph>,
}

impl Deref for Text {
    type Target = Element;

    fn deref(&self) -> &Self::Target {
        &self.element
    }
}

impl Text {
    pub fn new(text: &[TextRun]) -> Rc<Text> {
        let paragraph = TextLayout::new(text).inner;
        Element::new_derived(|element| Text {
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
}

impl ElementMethods for Text {
    fn element(&self) -> &Element {
        &self.element
    }

    fn measure(&self, _children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> LayoutOutput {
        let _span = trace_span!(
            "Text::measure"
        ).entered();

        let paragraph = &mut *self.paragraph.borrow_mut();

        let space = layout_input.width.available().unwrap_or(f64::INFINITY) as f32;
        paragraph.layout(space);

        let output = LayoutOutput {
            width: paragraph.longest_line() as f64,
            height: paragraph.height() as f64,
            baseline: Some(paragraph.alphabetic_baseline() as f64),
        };

        trace!("Measured text: {:?} -> {:?}", layout_input, output);
        self.relayout.set(false);
        output
    }

    fn layout(&self, children: &[Rc<dyn ElementMethods>], size: Size) -> LayoutOutput {
        self.measure(children, &LayoutInput {
            width: SizeConstraint::Available(size.width),
            height: SizeConstraint::Available(size.height),
        })
    }

    fn hit_test(&self, _point: Point) -> bool {
        false
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        ctx.with_canvas(|canvas| {
            self.paragraph.borrow().paint(canvas, Point::ZERO.to_skia());
        })
    }

    async fn event(&self, _event: &mut Event)
    where
        Self: Sized,
    {}
}
