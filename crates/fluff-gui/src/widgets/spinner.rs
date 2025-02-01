use crate::colors;
use kyute::drawing::point;
use kyute::element::prelude::*;
use kyute::elements::draw::Visual;
use kyute::kurbo::PathEl::{LineTo, MoveTo};
use kyute::model::EventSource;
use kyute::{IntoElementAny, Rect, Size};

#[derive(Copy, Clone)]
pub struct SpinnerUpButtonEvent;

#[derive(Copy, Clone)]
pub struct SpinnerDownButtonEvent;

/// Two spinner buttons (up & down), standard input widget height.
pub fn spinner_buttons() -> impl IntoElementAny {
    struct SpinnerButtons;
    impl Visual for SpinnerButtons {
        fn layout(&mut self, _input: &LayoutInput) -> Size {
            Size::new(13., 16.)
        }

        fn paint(&mut self, ctx: &mut PaintCtx) {
            // Upper chevron
            ctx.stroke_path(
                [MoveTo(point(2., 6.)), LineTo(point(6.5, 1.5)), LineTo(point(11., 6.))],
                1.,
                colors::DISPLAY_TEXT,
            );
            // Lower chevron
            ctx.stroke_path(
                [
                    MoveTo(point(2., 10.)),
                    LineTo(point(6.5, 14.5)),
                    LineTo(point(11., 10.)),
                ],
                1.,
                colors::DISPLAY_TEXT,
            );
        }

        fn event(&mut self, ctx: &mut ElementCtxAny, event: &mut Event) {
            if event.is_pointer_up() {
                // Upper click region
                if event.is_inside(Rect::new(0., 0., 13., 8.)) {
                    ctx.emit(SpinnerUpButtonEvent);
                }
                // Lower click region
                if event.is_inside(Rect::new(0., 8., 13., 8.)) {
                    ctx.emit(SpinnerDownButtonEvent);
                }
            }
        }
    }

    SpinnerButtons
}
