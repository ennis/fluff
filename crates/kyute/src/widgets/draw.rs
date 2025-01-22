//! Immediate drawing widget.
use crate::drawing::{Paint, ToSkia};
use crate::element::{ElementBuilder, ElementCtx, ElementCtxAny, HitTestCtx};
use crate::layout::{LayoutInput, LayoutOutput};
use crate::text::{TextLayout, TextRun};
use crate::{Element, PaintCtx};
use kurbo::{Insets, Line, Point, Rect, RoundedRect, Size};
use kyute_common::Color;
use crate::model::{with_tracking_scope, SubscriptionKey};

pub mod prelude {
    pub use super::BorderPosition::{Inside, Outside};
    pub use super::HorizontalAlignment::{HCenter, Left, Right};
    pub use super::VerticalAlignment::{Baseline, Bottom, Top, VCenter};
    pub use super::{rgb, BorderPosition, DrawCtx, HorizontalAlignment, VerticalAlignment};
}

pub struct DrawCtx<'a> {
    pub(crate) canvas: &'a skia_safe::Canvas,
    /// Current rectangle.
    rect: Rect,
    /// Current border radius.
    border_radius: f64,
    /// Pixel scale factor (how many physical px per logical px).
    scale_factor: f64,
    /// Current baseline.
    baseline: f64,
}

/// Position of a border relative to the shape boundary.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BorderPosition {
    /// Draw the border inside the shape boundary.
    Inside,
    /// Draw the border outside the shape boundary.
    Outside,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HorizontalAlignment {
    Left,
    HCenter,
    Right,
}

/// Text vertical alignment.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum VerticalAlignment {
    /// Align the top of the text box to the top of the layout box.
    Top,
    /// Centers the text vertically.
    VCenter,
    /// Align the bottom of the text box to the bottom of the layout box.
    Bottom,
    /// Aligns the baseline of the text to the baseline of the layout box.
    Baseline,
    // CenterCapHeight
}

/// Creates a new color from the specified RGB values.
pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba_u8(r, g, b, 255)
}

/// Rounds a logical px value to the nearest physical pixel.
pub fn round_to_px(logical: f64, scale_factor: f64) -> f64 {
    (logical * scale_factor).round() / scale_factor
}

#[macro_export]
macro_rules! linear_gradient {
    ($(in $colorspace:ident;)? $angle:expr; $($colorstop:expr),*) => {
        $crate::drawing::LinearGradient {
            angle: $angle.into(),
            stops: vec![$(
                $crate::drawing::ColorStop::from($colorstop)
            ),*],
            color_space: { $crate::drawing::InterpolationColorSpace::Oklab $(; $crate::drawing::InterpolationColorSpace::$colorspace)? },
        }
    };
}


impl DrawCtx<'_> {
    /// Returns the horizontal midline (from the center of the left edge to the center of the right edge) of the current region.
    pub fn h_midline(&self) -> Line {
        Line::new(
            (self.rect.x0, self.rect.y0 + 0.5 * self.rect.height()),
            (self.rect.x1, self.rect.y0 + 0.5 * self.rect.height()),
        )
    }

    /// Returns the vertical midline (from the center of the top edge to the center of the bottom edge) of the current region.
    pub fn v_midline(&self) -> Line {
        Line::new(
            (self.rect.x0 + 0.5 * self.rect.width(), self.rect.y0),
            (self.rect.x0 + 0.5 * self.rect.width(), self.rect.y1),
        )
    }

    /// Sets the baseline of the current region.
    pub fn baseline(&mut self, baseline: f64) {
        self.baseline = baseline;
    }

    /// Draws a border around the current region.
    pub fn border(&mut self, size: impl Into<Insets>, position: BorderPosition, paint: impl Into<Paint>) {
        let size = size.into();
        if size == Insets::ZERO {
            return;
        }

        let paint = paint.into();
        if let Paint::Color(color) = paint {
            if color.alpha() == 0.0 {
                // fully transparent border
                return;
            }
        }

        let border_radius = self.border_radius;
        let inner_shape = RoundedRect::from_rect(self.rect - size, border_radius - 0.5 * size.x_value());
        let outer_shape = RoundedRect::from_rect(self.rect, border_radius);
        let mut paint = paint.to_sk_paint(self.rect);
        paint.set_style(skia_safe::paint::Style::Fill);
        self.canvas
            .draw_drrect(outer_shape.to_skia(), inner_shape.to_skia(), &paint);
    }

    /// Fills the current region with the specified paint.
    pub fn fill(&mut self, paint: impl Into<Paint>) {
        let mut paint = paint.into().to_sk_paint(self.rect);
        paint.set_style(skia_safe::paint::Style::Fill);
        let rrect = RoundedRect::from_rect(self.rect, self.border_radius);
        self.canvas.draw_rrect(rrect.to_skia(), &paint);
    }

    /// Draws text in the current rectangle with the specified alignment.
    pub fn draw_text(
        &mut self,
        horizontal_alignment: HorizontalAlignment,
        vertical_alignment: VerticalAlignment,
        text: &[TextRun],
    ) {
        let mut layout = TextLayout::new(text);
        layout.layout(self.rect.width());
        let size = layout.size();

        let x = match horizontal_alignment {
            HorizontalAlignment::Left => 0.0,
            HorizontalAlignment::HCenter => 0.5 * (self.rect.width() - size.width),
            HorizontalAlignment::Right => self.rect.width() - size.width,
        };

        let y = match vertical_alignment {
            VerticalAlignment::Top => 0.0,
            VerticalAlignment::VCenter => 0.5 * (self.rect.height() - size.height),
            VerticalAlignment::Bottom => self.rect.height() - size.height,
            VerticalAlignment::Baseline => self.baseline - layout.baseline(),
        };

        layout.paint(&self.canvas, Point::new(self.rect.x0 + x, self.rect.y0 + y));
    }
}

pub struct Draw<F> {
    ctx: ElementCtx<Self>,
    draw_subscription: SubscriptionKey,
    draw_fn: F,
}

impl<F: Fn(&mut DrawCtx) + 'static> Draw<F> {
    pub fn new(draw_fn: F) -> ElementBuilder<Self> {
        ElementBuilder::new(Self {
            ctx: ElementCtx::new(),
            draw_subscription: Default::default(),
            draw_fn,
        })
    }
}

impl<F> Element for Draw<F>
where
    F: Fn(&mut DrawCtx) + 'static,
{
    fn ctx(&self) -> &ElementCtxAny {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut ElementCtxAny {
        &mut self.ctx
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        Size {
            width: layout_input.width.available().unwrap_or_default(),
            height: layout_input.height.available().unwrap_or_default(),
        }
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        LayoutOutput {
            width: size.width,
            height: size.height,
            // FIXME: we ought to be able to retrieve the baseline from the draw function
            baseline: None,
        }
    }

    fn hit_test(&self, _ctx: &mut HitTestCtx, point: Point) -> bool {
        self.ctx.rect().contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        let rect = self.ctx.rect();
        ctx.with_canvas(|canvas| {
            let mut draw_ctx = DrawCtx {
                canvas,
                rect,
                border_radius: 0.0,
                scale_factor: ctx.scale_factor,
                baseline: 0.0,
            };

            self.draw_subscription.unsubscribe();

            let (_, deps) = with_tracking_scope(|| {
                (self.draw_fn)(&mut draw_ctx);
            });

            self.draw_subscription = self.ctx.watch_once(deps.reads, |this, _| {
                this.ctx.mark_needs_paint();
            });
        });
    }
}
