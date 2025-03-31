use crate::colors;
use crate::widgets::TEXT_STYLE;
use kyute::drawing::{BASELINE_CENTER, Image, PlacementExt, vec2};
use kyute::element::prelude::*;
use kyute::element::{TreeCtx, WeakElement};
use kyute::elements::{ActivatedEvent, ClickedEvent, HoveredEvent};
use kyute::kurbo::Vec2;
use kyute::text::TextLayout;
use kyute::{ElementState, Event, EventSource, PaintCtx, Point, Size, text};

const BUTTON_RADIUS: f64 = 4.;
const BUTTON_MIN_WIDTH: f64 = 80.;
const BUTTON_HEIGHT: f64 = 23.;
const BUTTON_BASELINE: f64 = 16.;

/// A button with a text label.
///
/// This element emits the standard events for buttons.
pub struct Button {
    weak: WeakElement<Self>,
    label: TextLayout,
    state: ElementState,
}

impl Button {
    /// Creates a new button with the specified label.
    pub fn new(label: impl Into<String>) -> ElementBuilder<Button> {
        let label = label.into();
        let label = TextLayout::new(&TEXT_STYLE, text!["{label}"]);
        ElementBuilder::new_cyclic(|weak| Button {
            weak,
            label,
            state: ElementState::default(),
        })
    }
}

/*
impl EventSource for Button {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.weak.clone()
    }
}*/

impl Element for Button {
    fn measure(&mut self, cx: &TreeCtx, input: &LayoutInput) -> Size {
        // layout label with available space, but don't go below the minimum width
        self.label
            .layout(input.width.available().unwrap_or_default().max(BUTTON_MIN_WIDTH));
        let label_width = self.label.size().width + 20.;
        let w = label_width.max(BUTTON_MIN_WIDTH);
        let h = BUTTON_HEIGHT;
        Size::new(w, h)
    }

    fn layout(&mut self, cx: &TreeCtx, size: Size) -> LayoutOutput {
        self.label.layout(size.width - 20.);
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, cx: &TreeCtx, ctx: &mut PaintCtx) {
        let mut rect = cx.bounds();
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
            .place_into((cx.bounds(), BUTTON_BASELINE), BASELINE_CENTER);
        ctx.draw_text_layout(pos, &self.label);
    }

    fn event(&mut self, cx: &TreeCtx, event: &mut Event) {
        let repaint = match event {
            Event::PointerDown(_) => {
                self.state.set_active(true);
                cx.set_focus();
                cx.set_pointer_capture();

                self.weak.emit(ActivatedEvent(true));
                true
            }
            Event::PointerUp(_) => {
                if self.state.is_active() {
                    self.state.set_active(false);
                    self.weak.emit(ActivatedEvent(false));
                    self.weak.emit(ClickedEvent);
                    true
                } else {
                    false
                }
            }
            Event::PointerEnter(_) => {
                self.state.set_hovered(true);
                self.weak.emit(HoveredEvent(true));
                true
            }
            Event::PointerLeave(_) => {
                self.state.set_hovered(false);
                self.weak.emit(HoveredEvent(false));
                true
            }
            _ => false,
        };
        if repaint {
            cx.mark_needs_paint();
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
