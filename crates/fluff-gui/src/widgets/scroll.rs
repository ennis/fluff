//! Scroll views.

use crate::colors::{SCROLL_BAR, SCROLL_BAR_BACKGROUND};
use kyute::layout::Axis;
use kyute::{Element, Event, HitTestCtx, LayoutInput, Measurement, NodeBuilder, NodeCtx, PaintCtx, Point, Rect, Size};
//const SCROLL_BAR_WIDTH: f64 = 18.0;
//const SCROLL_BAR_BUTTON_SIZE: f64 = 18.0;

pub struct ScrollBarBase {
    direction: Axis,
    thumb_size: f64,
    /// Current thumb position relative to the layout bounds.
    thumb_pos: f64,
    /// Where the user started dragging the thumb.
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
    pub fn horizontal() -> NodeBuilder<Self> {
        NodeBuilder::new_cyclic(|_weak| ScrollBarBase {
            direction: Axis::Horizontal,
            cross_size: 12.,
            thumb_pos: 0.0,
            thumb_drag: None,
            thumb_size: 12.,
            track_length: 0.0,
        })
    }

    pub fn vertical() -> NodeBuilder<Self> {
        NodeBuilder::new_cyclic(|_weak| ScrollBarBase {
            direction: Axis::Vertical,
            cross_size: 12.,
            thumb_pos: 0.0,
            thumb_drag: None,
            thumb_size: 12.,
            track_length: 0.0,
        })
    }

    pub fn thumb_size(mut self: NodeBuilder<Self>, size: f64) -> NodeBuilder<Self> {
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

        //let button_size = self.cross_size;

        //if inline_coord < button_size {
        //    return Some(ScrollbarPart::StartButton);
        //}
        if inline_coord < self.thumb_pos {
            return Some(ScrollbarPart::Track);
        }
        if inline_coord < self.thumb_pos + self.thumb_size {
            return Some(ScrollbarPart::Thumb);
        }
        Some(ScrollbarPart::EndButton)
    }

    fn inline_pos(&self, point: Point) -> f64 {
        match self.direction {
            Axis::Vertical => point.y,
            Axis::Horizontal => point.x,
        }
    }

    /// Sets the position of the thumb.
    ///
    /// # Arguments
    /// * `pos` - The new position of the thumb in logical pixels, relative to the layout bounds.
    fn set_thumb_pos(&mut self, pos: f64) {
        let max_pos = self.track_length - self.thumb_size;
        self.thumb_pos = pos.max(0.0).min(max_pos);
        eprintln!("thumb_pos: {}", self.thumb_pos);
    }
}

impl Element for ScrollBarBase {
    fn measure(&mut self, _cx: &NodeCtx, layout_input: &LayoutInput) -> Measurement {
        let size = match self.direction {
            Axis::Vertical => Size::new(self.cross_size, layout_input.available.height),
            Axis::Horizontal => Size::new(layout_input.available.width, self.cross_size),
        };
        size.into()
    }

    fn layout(&mut self, _cx: &NodeCtx, size: Size) {
        self.track_length = match self.direction {
            Axis::Vertical => size.height,
            Axis::Horizontal => size.width,
        };
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        let bounds = ctx.bounds;

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
        let mut thumb_rect =
            rect_from_inline_cross(self.direction, self.thumb_pos, 0.0, self.thumb_size, self.cross_size);
        thumb_rect.x0 += bounds.x0;
        thumb_rect.y0 += bounds.y0;
        thumb_rect.x1 += bounds.x0;
        thumb_rect.y1 += bounds.y0;
        thumb_rect = ctx.snap_rect_to_device_pixel(thumb_rect);
        ctx.fill_rect(thumb_rect, SCROLL_BAR);
    }

    fn event(&mut self, cx: &NodeCtx, event: &mut Event) {
        let bounds = cx.bounds();

        match event {
            Event::PointerDown(p) => {
                let pos = p.position;
                match self.hit_test_track(pos, bounds) {
                    Some(ScrollbarPart::Thumb) => {
                        let local_pos = (pos - bounds.origin()).to_point();
                        let x = self.inline_pos(local_pos);
                        self.thumb_drag = Some(x - self.thumb_pos);
                        cx.set_pointer_capture();
                        cx.set_focus();
                        cx.mark_needs_paint();
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
                let pos = p.position;
                let local_pos = (pos - bounds.origin()).to_point();
                if let Some(drag) = self.thumb_drag {
                    self.set_thumb_pos(self.inline_pos(local_pos) - drag);
                    cx.mark_needs_paint();
                }
            }
            _ => {}
        }
    }
}
