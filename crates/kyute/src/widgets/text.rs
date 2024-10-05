use crate::drawing::ToSkia;
use crate::element::{Element, ElementMethods};
use crate::event::Event;
use crate::layout::{BoxConstraints, Geometry, IntrinsicSizes};
use crate::PaintCtx;
use kurbo::{Point, Size};
use skia_safe::textlayout;
use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;
use taffy::{AvailableSpace, LayoutInput, LayoutOutput, RequestedAxis};
use tracy_client::span;
use crate::text::{TextRun, TextLayout};

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

    fn layout(&self, _children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> LayoutOutput {
        //let size = self.calculate_intrinsic_size();
        // min line width == max word width


        let paragraph = &mut *self.paragraph.borrow_mut();
        paragraph.layout(f32::INFINITY);
        let min_line_width = paragraph.min_intrinsic_width();
        let max_line_width = paragraph.max_intrinsic_width();


        let width_constraint = layout_input.known_dimensions.width.unwrap_or_else(|| {
            match layout_input.available_space.width {
                AvailableSpace::Definite(width) => {
                    width.clamp(min_line_width, max_line_width)
                }
                AvailableSpace::MinContent => {
                    0.0
                }
                AvailableSpace::MaxContent => {
                    f32::INFINITY
                }
            }
        });

        paragraph.layout(width_constraint);

        // FIXME: we add a 1.0 padding to the width because the text is cut off otherwise
        let width = layout_input.known_dimensions.width.unwrap_or_else(|| paragraph.longest_line());
        let height = layout_input.known_dimensions.height.unwrap_or_else(|| paragraph.height());
        let baseline = paragraph.alphabetic_baseline();

        eprintln!("layout text with axis={:?}, known_dimensions={:?}, available_space={:?} -> {:?}", layout_input.axis, layout_input.known_dimensions, layout_input.available_space, taffy::Size { width, height });

        LayoutOutput::from_sizes_and_baselines(taffy::Size { width, height }, taffy::Size::ZERO, taffy::Point {
            x: None,
            y: Some(baseline),
        })
    }

    /*fn layout(&self, _children: &[Rc<dyn ElementMethods>], size: Size) -> LayoutOutput {
        let paragraph = &mut *self.paragraph.borrow_mut();

        paragraph.layout(size.width as f32);
        let width = paragraph.longest_line();
        let height = paragraph.height();
        let baseline = paragraph.alphabetic_baseline();

        LayoutOutput::from_sizes_and_baselines(taffy::Size { width, height }, taffy::Size::ZERO, taffy::Point {
            x: None,
            y: Some(baseline),
        })
    }*/

    /*
        fn layout(&self, _children: &[Rc<dyn ElementMethods>], constraints: &BoxConstraints) -> Geometry {
            // layout paragraph in available space
            let _span = span!("text layout");

            // available space for layout
            let available_width = constraints.max.width;
            let _available_height = constraints.max.height;

            // We can reuse the previous layout if and only if:
            // - the new available width is >= the current paragraph width (otherwise new line breaks are necessary)
            // - the current layout is still valid (i.e. it hasn't been previously invalidated)

            let paragraph = &mut *self.paragraph.borrow_mut();

            if !self.relayout.get() && paragraph.longest_line() <= available_width as f32 {
                let paragraph_size = Size {
                    width: paragraph.longest_line() as f64,
                    height: paragraph.height() as f64,
                };
                let size = constraints.constrain(paragraph_size);
                return Geometry {
                    size,
                    baseline: Some(paragraph.alphabetic_baseline() as f64),
                    bounding_rect: paragraph_size.to_rect(),
                    paint_bounding_rect: paragraph_size.to_rect(),
                };
            }

            paragraph.layout(available_width as skia_safe::scalar);
            let w = paragraph.longest_line() as f64;
            let h = paragraph.height() as f64;
            let alphabetic_baseline = paragraph.alphabetic_baseline();
            let unconstrained_size = Size::new(w, h);
            let size = constraints.constrain(unconstrained_size);
            self.relayout.set(false);

            Geometry {
                size,
                baseline: Some(alphabetic_baseline as f64),
                bounding_rect: size.to_rect(),
                paint_bounding_rect: size.to_rect(),
            }
        }*/

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
