use crate::colors;
use crate::widgets::TEXT_STYLE;
use kyute::drawing::{vec2, Image, PlacementExt, BASELINE_CENTER};
use kyute::element::prelude::*;
use kyute::element::ElemBox;
use kyute::kurbo::Vec2;
use kyute::text::TextLayout;
use kyute::{text, ElementState, Event, PaintCtx, Point, Size};

const BUTTON_RADIUS: f64 = 4.;
const BUTTON_MIN_WIDTH: f64 = 80.;
const BUTTON_HEIGHT: f64 = 23.;
const BUTTON_BASELINE: f64 = 16.;

/// A button with a text label.
///
/// This element emits the standard events for buttons.
pub struct Button {
    label: TextLayout,
    state: ElementState,
}

impl Button {
    /// Creates a new button with the specified label.
    pub fn new(label: impl Into<String>) -> Button {
        let label = label.into();
        let label = TextLayout::new(&TEXT_STYLE, text!["{label}"]);
        Button {
            label,
            state: ElementState::default(),
        }
    }
}

impl Element for Button {
    fn measure(&mut self, input: &LayoutInput) -> Size {
        // layout label with available space, but don't go below the minimum width
        self.label
            .layout(input.width.available().unwrap_or_default().max(BUTTON_MIN_WIDTH));
        let label_width = self.label.size().width + 20.;
        let w = label_width.max(BUTTON_MIN_WIDTH);
        let h = BUTTON_HEIGHT;
        Size::new(w, h)
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        self.label.layout(size.width - 20.);
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.rect.contains(point)
    }

    fn paint(self: &mut ElemBox<Self>, ctx: &mut PaintCtx) {
        let mut rect = self.ctx.rect();
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
        let pos = self
            .label
            .rect_with_baseline()
            .place_into((self.ctx.rect(), BUTTON_BASELINE), BASELINE_CENTER);
        ctx.draw_text_layout(pos, &self.label);
    }

    fn event(self: &mut ElemBox<Self>, _ctx: &mut WindowCtx, event: &mut Event) {
        if self.ctx.update_element_state(&mut self.element.state, event) {
            self.ctx.mark_needs_paint();
        }
    }
}

struct ButtonVisual {
    icon: Option<Image>,
    label: Option<TextLayout>,
    offset: Vec2,
}

impl ButtonVisual {
    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        todo!()
    }
}

/*
/// A group of buttons sharing the same visual.
pub struct ButtonGroup {
    pub buttons: Vec<ButtonVisual>,
}

impl Element for ButtonGroup {
    fn measure(&mut self, layout_input: &LayoutInput) -> Size {

    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        todo!()
    }

    fn paint(self: &mut ElemBox<Self>, ctx: &mut PaintCtx) {
        todo!()
    }
}*/
