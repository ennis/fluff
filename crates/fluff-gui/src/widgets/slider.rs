use crate::colors;
use crate::widgets::{INPUT_WIDTH, PaintExt, WIDGET_BASELINE, WIDGET_LINE_HEIGHT};
use kyute::drawing::point;
use kyute::element::{ElementBuilder, HitTestCtx, Measurement, TreeCtx, WeakElement};
use kyute::elements::ValueChangedEvent;
use kyute::input_event::ScrollDelta;
use kyute::kurbo::{Line, PathEl, Vec2};
use kyute::layout::{LayoutInput, LayoutOutput};
use kyute::{Element, Event, EventSource, PaintCtx, Point, Rect, Size};
use std::ops::Range;

pub struct SliderBase {
    pub value: f64,
    pub range: Range<f64>,
    pub draw_background: bool,
    pub increment: f64,
    pub offset: Vec2,
    width: f64,
}

#[derive(Default)]
pub struct SliderBaseEventResult {
    pub value_changed: Option<f64>,
    // pub flags: SliderBaseEventFlags,
}

impl SliderBase {
    pub fn new(value: f64, range: Range<f64>) -> Self {
        SliderBase {
            value,
            range,
            draw_background: true,
            increment: 1.0,
            offset: Vec2::ZERO,
            width: 0.0,
        }
    }

    pub fn increment(mut self, increment: f64) -> Self {
        self.increment = increment;
        self
    }

    pub fn set_value(&mut self, value: f64) {
        self.value = value;
    }

    fn set_value_from_pos(&mut self, x_pos: f64) -> Option<f64> {
        let value_norm = ((x_pos - self.offset.x) / self.width).clamp(0., 1.);
        let value = self.range.start + value_norm * (self.range.end - self.range.start);
        if self.value != value {
            self.value = value;
            Some(value)
        } else {
            None
        }
    }

    pub fn paint(&mut self, ctx: &mut PaintCtx, rect: Rect) {
        if self.draw_background {
            ctx.draw_display_background(rect);
        }

        // Normalize value
        let value_norm = (self.value - self.range.start) / (self.range.end - self.range.start);
        let x_pos = rect.x0 + value_norm * rect.width();
        let x_pos_snapped = ctx.round_to_device_pixel(x_pos);

        // Draw the slider line background
        let mid_y = ctx.round_to_device_pixel_center(rect.y0 + 0.5 * rect.height());
        let midline = Line::new((rect.x0, mid_y), (rect.x1, mid_y));
        ctx.draw_line(midline, 1.0, colors::SLIDER_LINE_BACKGROUND);

        // Draw the slider line
        let slider_line = Line::new((rect.x0, mid_y), (x_pos_snapped, mid_y));
        ctx.draw_line(slider_line, 1.0, colors::SLIDER_LINE);

        // Draw the knob
        let knob_rect = Rect {
            x0: x_pos,
            y0: rect.y0 + 4.0,
            x1: x_pos + 1.0,
            y1: rect.y1 - 4.0,
        };
        let knob_rect_snapped = ctx.snap_rect_to_device_pixel(knob_rect);
        ctx.fill_rect(knob_rect_snapped, colors::SLIDER_LINE);

        // Draw a small triangle on top of the knob
        {
            let x = ctx.round_to_device_pixel_center(x_pos_snapped);
            let top = ctx.round_to_device_pixel(rect.y0 + 4.0);
            let triangle = [
                PathEl::MoveTo(point(x, mid_y)),
                PathEl::LineTo(point(x + 4.0, top)),
                PathEl::LineTo(point(x - 4.0, top)),
                PathEl::ClosePath,
            ];
            ctx.fill_path(triangle, colors::SLIDER_LINE);
        }
    }

    pub fn measure(&self, layout_input: &LayoutInput) -> Measurement {
        let mut width = layout_input.available.width;
        if !width.is_finite() || width < INPUT_WIDTH {
            width = INPUT_WIDTH;
        }
        let height = WIDGET_LINE_HEIGHT;
        Measurement {size:
        Size { width, height }, baseline: Some(WIDGET_BASELINE) }
    }

    pub fn layout(&mut self, size: Size) {
        self.width = size.width;
    }

    // Sets the position of the slider in window coordinates.
    //pub fn set_offset(&mut self, offset: Vec2) {
    //    self.offset = offset;
    //}

    pub fn event(&mut self, bounds: Rect, event: &mut Event) -> SliderBaseEventResult {
        let mut value_changed = None;

        match event {
            Event::PointerDown(p) => {
                // TODO self.offset!
                let local_pos = p.position - bounds.origin();
                value_changed = self.set_value_from_pos(local_pos.x);
            }
            Event::PointerMove(p) if p.capturing_pointer() => {
                // TODO self.offset!
                let local_pos = p.position - bounds.origin();
                value_changed = self.set_value_from_pos(local_pos.x);
            }
            Event::Wheel(w) => match w.delta {
                ScrollDelta::Lines { x, y } => {
                    self.value += y.max(x) * self.increment;
                }
                _ => {}
            },
            _ => {}
        }

        SliderBaseEventResult { value_changed }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Standalone slider widget.
pub struct Slider {
    base: SliderBase,
}

impl Slider {
    pub fn new(value: f64, range: Range<f64>) -> ElementBuilder<Self> {
        ElementBuilder::new(Slider {
            base: SliderBase::new(value, range),
        })
    }

    pub fn set_value(&mut self, cx: &TreeCtx, value: f64) {
        self.base.set_value(value);
        cx.mark_needs_paint();
    }
}

impl Element for Slider {
    fn measure(&mut self, _cx: &TreeCtx, layout_input: &LayoutInput) -> Measurement {
        self.base.measure(layout_input)
    }

    fn layout(&mut self, _cx: &TreeCtx, size: Size) {
        self.base.layout(size)
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        self.base.paint(ctx, ctx.bounds);
    }

    fn event(&mut self, cx: &TreeCtx, event: &mut Event) {
        match event {
            Event::PointerDown(_) => {
                cx.set_pointer_capture();
                cx.set_focus();
            }
            _ => {}
        }
        let result = self.base.event(cx.bounds(), event);

        if let Some(value) = result.value_changed {
            cx.mark_needs_paint();
            cx.emit(ValueChangedEvent(value));
        }
    }
}
