//! Scroll views.

use crate::colors::{SCROLL_BAR, SCROLL_BAR_BACKGROUND};
use kyute::element::prelude::*;
use kyute::layout::{Axis, AxisSizeHelper};
use kyute::{Point, Rect, Size};

//const SCROLL_BAR_WIDTH: f64 = 18.0;
//const SCROLL_BAR_BUTTON_SIZE: f64 = 18.0;

pub struct ScrollBarBase {
    direction: Axis,
    thumb_size: f64,
    thumb_pos: f64,
    thumb_drag: Option<f64>,
    cross_size: f64,
    track_length: f64,
}

fn rect_from_inline_cross(
    direction: Axis,
    inline_origin: f64,
    cross_origin: f64,
    inline_size: f64,
    cross_size: f64,
) -> Rect {
    match direction {
        Axis::Vertical => Rect {
            x0: cross_origin,
            y0: inline_origin,
            x1: cross_origin + cross_size,
            y1: inline_origin + inline_size,
        },
        Axis::Horizontal => Rect {
            x0: inline_origin,
            y0: cross_origin,
            x1: inline_origin + inline_size,
            y1: cross_origin + cross_size,
        },
    }
}

/*
struct ScrollMetrics {
    content: f64,
    visible: f64,
    track: f64,
    min_thumb_size: f64,
}

impl ScrollMetrics {
    fn thumb_size(&self) -> f64 {
        // content_size / visible_size == bar_size / thumb_size
        // usually visible_size = bar_size if the bar is the same size as the visible area
        // thumb_size = bar_size * visible_size / content_size
        (self.track * self.visible / self.content).min(self.min_thumb_size)
    }

    fn thumb_to_content(&self, thumb_pos: f64) -> f64 {
        let thumb = self.thumb_size();
        let content_pos = thumb_pos / (self.track - thumb) * (self.content - self.visible);
        content_pos.max(0.0)
    }

    fn content_to_thumb(&self, content_pos: f64) -> f64 {
        let thumb_size = self.thumb_size();
        let max_content = self.content - self.visible;
        let thumb_pos = content_pos / max_content * (self.track - thumb_size);
        thumb_pos.max(0.0).min(self.track - thumb_size)
    }
}
*/

enum ScrollbarPart {
    StartButton,
    EndButton,
    Track,
    Thumb,
}

impl ScrollBarBase {
    pub fn horizontal() -> ElementBuilder<Self> {
        ElementBuilder::new_cyclic(|_weak| ScrollBarBase {
            direction: Axis::Horizontal,
            cross_size: 12.,
            thumb_pos: 0.0,
            thumb_drag: None,
            thumb_size: 12.,
            track_length: 0.0,
        })
    }

    pub fn vertical() -> ElementBuilder<Self> {
        ElementBuilder::new_cyclic(|_weak| ScrollBarBase {
            direction: Axis::Vertical,
            cross_size: 12.,
            thumb_pos: 0.0,
            thumb_drag: None,
            thumb_size: 12.,
            track_length: 0.0,
        })
    }

    pub fn thumb_size(mut self: ElementBuilder<Self>, size: f64) -> ElementBuilder<Self> {
        self.thumb_size = size;
        self
    }

    fn hit_test_track(&self, point: Point, bounds: Rect) -> Option<ScrollbarPart> {
        if !bounds.contains(point) {
            return None;
        }
        let local = point - bounds.origin();
        let (inline_coord, _) = match self.direction {
            Axis::Vertical => (local.y, bounds.height()),
            Axis::Horizontal => (local.x, bounds.width()),
        };

        let button_size = self.cross_size;

        //if inline_coord < button_size {
        //    return Some(ScrollbarPart::StartButton);
        //}
        if inline_coord < self.thumb_pos {
            return Some(ScrollbarPart::Track);
        }
        if inline_coord < self.thumb_pos + self.thumb_size {
            return Some(ScrollbarPart::Thumb);
        }
        return Some(ScrollbarPart::EndButton);
    }

    fn inline_pos(&self, point: Point) -> f64 {
        match self.direction {
            Axis::Vertical => point.y,
            Axis::Horizontal => point.x,
        }
    }

    fn set_thumb_pos(&mut self, pos: f64) {
        let max_pos = self.track_length - self.thumb_size;
        self.thumb_pos = pos.max(0.0).min(max_pos);
    }
}

impl Element for ScrollBarBase {
    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        match self.direction {
            Axis::Vertical => Size::new(self.cross_size, layout_input.height.available().unwrap_or_default()),
            Axis::Horizontal => Size::new(layout_input.width.available().unwrap_or_default(), self.cross_size),
        }
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        self.track_length = match self.direction {
            Axis::Vertical => size.height,
            Axis::Horizontal => size.width,
        };
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.rect.contains(point)
    }

    fn paint(&mut self, ectx: &ElementCtx, ctx: &mut PaintCtx) {
        let bounds = ectx.rect();

        // paint scrollbar background
        ctx.fill_rect(bounds, SCROLL_BAR_BACKGROUND);

        //// top & bottom buttons
        //let start_button =
        //    rect_from_inline_cross(self.direction, 0.0, 0.0, SCROLL_BAR_BUTTON_SIZE, SCROLL_BAR_BUTTON_SIZE);
        //ctx.fill_rect(start_button, SCROLL_BAR);
        //
        //let end_button = rect_from_inline_cross(
        //    self.direction,
        //    self.metrics.track - SCROLL_BAR_BUTTON_SIZE,
        //    0.0,
        //    SCROLL_BAR_BUTTON_SIZE,
        //    SCROLL_BAR_BUTTON_SIZE,
        //);
        //ctx.fill_rect(end_button, SCROLL_BAR);

        // knob
        let thumb_rect = rect_from_inline_cross(self.direction, self.thumb_pos, 0.0, self.thumb_size, self.cross_size);
        let thumb_rect = ctx.snap_rect_to_device_pixel(thumb_rect);
        ctx.fill_rect(thumb_rect, SCROLL_BAR);
    }

    fn event(&mut self, cx: &ElementCtx, event: &mut Event) {
        let bounds = cx.rect();

        match event {
            Event::PointerDown(p) => {
                let local = p.local_position();
                let inline = self.inline_pos(local);
                match self.hit_test_track(p.local_position(), bounds) {
                    Some(ScrollbarPart::Thumb) => {
                        self.thumb_drag = Some(inline - self.thumb_pos);
                        cx.set_pointer_capture();
                        cx.set_focus();
                    }
                    Some(ScrollbarPart::Track) => {
                        // TODO
                    }
                    Some(ScrollbarPart::StartButton) => {
                        // scroll up
                        // TODO
                    }
                    Some(ScrollbarPart::EndButton) => {
                        // scroll down
                        // TODO
                    }
                    None => {}
                }
            }
            Event::PointerUp(_) => {
                self.thumb_drag = None;
            }
            Event::PointerMove(p) => {
                let local = p.local_position();
                if let Some(drag) = self.thumb_drag {
                    self.set_thumb_pos(self.inline_pos(local) - drag);
                }
            }
            _ => {}
        }
    }
}
