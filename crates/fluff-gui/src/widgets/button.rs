use crate::colors;
use kyute::drawing::{BASELINE_CENTER, BorderPosition, vec2};
use kyute::element::ElemBox;
use kyute::element::prelude::*;
use kyute::elements::Visual;
use kyute::kurbo::Insets;
use kyute::text::TextLayout;
use kyute::{ElementState, Event, IntoElementAny, PaintCtx, Point, Size, text};
use std::ops::{Add, Sub};

const BUTTON_RADIUS: f64 = 4.;
const BUTTON_MIN_WIDTH: f64 = 80.;
const BUTTON_HEIGHT: f64 = 23.;
const BUTTON_BASELINE: f64 = 16.;
const BUTTON_FONT_SIZE: f64 = 12.;

pub fn button(label: impl Into<String>) -> impl IntoElementAny {
    let label = label.into();
    let label = TextLayout::new(text![size(BUTTON_FONT_SIZE) family("Inter") color(colors::STATIC_TEXT) "{label}"]);

    struct ButtonVisual {
        label: TextLayout,
        state: ElementState,
    }

    impl Element for ButtonVisual {
        fn measure(&mut self, input: &LayoutInput) -> Size {
            self.label.layout(input.width.available().unwrap_or_default());
            let label_width = self.label.size().width + 20.;
            let w = label_width.max(BUTTON_MIN_WIDTH);
            let h = BUTTON_HEIGHT;
            Size::new(w, h)
        }

        fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
            ctx.rect.contains(point)
        }

        fn paint(self: &mut ElemBox<Self>, ctx: &mut PaintCtx) {
            ctx.set_baseline(BUTTON_BASELINE);
            let mut rect = ctx.bounds();
            rect.y1 -= 1.;
            let rect = rect.to_rounded_rect(BUTTON_RADIUS);

            // bevel bg
            if self.state.is_active() {
                ctx.fill_rrect(rect + vec2(0., 1.), colors::BUTTON_BEVEL);
            } else {
                ctx.fill_rrect(rect, colors::BUTTON_BEVEL);
            }

            // rounded rectangle
            let mut color = colors::BUTTON_BACKGROUND;
            if self.state.is_active() {
                color = colors::BUTTON_BACKGROUND.darken(0.1);
            } else if self.state.is_hovered() {
                color = colors::BUTTON_BACKGROUND.lighten(0.1);
            }

            if self.state.is_active() {
                ctx.fill_rrect(rect, color);
            } else {
                ctx.fill_rrect(rect + vec2(0., 1.), color);
            }

            // label
            ctx.draw_text_layout(BASELINE_CENTER, &self.label);
        }

        fn event(self: &mut ElemBox<Self>, _ctx: &mut WindowCtx, event: &mut Event) {
            if self.ctx.update_element_state(&mut self.element.state, event) {
                self.ctx.mark_needs_paint();
            }
        }
    }

    ButtonVisual {
        label,
        state: ElementState::default(),
    }
}
