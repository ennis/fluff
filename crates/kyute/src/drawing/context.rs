use crate::drawing::{ColorStop, Image, InterpolationColorSpace, LinearGradient, Paint, ToSkia};
use crate::text::{TextLayout, TextRun};
use kurbo::{Insets, Line, Point, Rect, RoundedRect, Size};
use kyute_common::Color;

pub mod prelude {
    pub use super::Anchor::*;
    pub use super::BorderPosition::{Inside, Outside};
    pub use super::{
        align, linear_gradient, place, rgb, BorderPosition, DrawCtx, BASELINE_CENTER, BASELINE_LEFT, BASELINE_RIGHT,
        BOTTOM_CENTER, BOTTOM_LEFT, BOTTOM_RIGHT, CENTER, LEFT_CENTER, RIGHT_CENTER, TOP_CENTER, TOP_LEFT, TOP_RIGHT,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Pixels(f64),
    Stretch,
}

impl Length {
    pub fn resolve(self, container_size: f64) -> f64 {
        match self {
            Length::Pixels(x) => x,
            Length::Stretch => container_size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawSize {
    width: Length,
    height: Length,
}

impl DrawSize {
    /// Resolves this value to a concrete size.
    pub fn resolve(self, container_size: Size) -> Size {
        fn resolve_length(container_size: f64, length: Length) -> f64 {
            match length {
                Length::Pixels(length) => length,
                Length::Stretch => container_size,
            }
        }

        Size {
            width: resolve_length(container_size.width, self.width),
            height: resolve_length(container_size.height, self.height),
        }
    }
}

impl From<Size> for DrawSize {
    fn from(size: Size) -> Self {
        DrawSize {
            width: Length::Pixels(size.width),
            height: Length::Pixels(size.height),
        }
    }
}

impl From<(f64, f64)> for DrawSize {
    fn from((width, height): (f64, f64)) -> Self {
        DrawSize {
            width: Length::Pixels(width),
            height: Length::Pixels(height),
        }
    }
}

pub struct DrawCtx<'a> {
    pub(crate) canvas: &'a skia_safe::Canvas,
    /// Current container rectangle.
    pub rect: Rect,
    /// Pixel scale factor (how many physical px per logical px).
    scale_factor: f64,
    /// Container baseline.
    pub baseline: f64,
}

impl<'a> DrawCtx<'a> {
    pub fn new(canvas: &'a skia_safe::Canvas, rect: Rect, scale_factor: f64) -> Self {
        Self {
            canvas,
            rect,
            scale_factor,
            baseline: 0.0,
        }
    }
}

/// Position of a border relative to the shape boundary.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BorderPosition {
    /// Draw the border inside the shape boundary.
    Inside,
    /// Draw the border outside the shape boundary.
    Outside,
}

/// Creates a new color from the specified RGB values.
pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba_u8(r, g, b, 255)
}

/// Creates a new color from the specified RGBA values.
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba_u8(r, g, b, a)
}

/// Rounds a logical px value to the nearest physical pixel.
pub fn round_to_px(logical: f64, scale_factor: f64) -> f64 {
    (logical * scale_factor).round() / scale_factor
}

pub fn linear_gradient<I, C>(
    color_space: InterpolationColorSpace,
    angle_degrees: impl Into<f64>,
    stops: I,
) -> LinearGradient
where
    I: IntoIterator<Item = C>,
    C: Into<ColorStop>,
{
    LinearGradient {
        color_space,
        angle_degrees: angle_degrees.into(),
        stops: stops.into_iter().map(Into::into).collect(),
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum Anchor {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,    // == TopCenter
    Bottom, // == BottomCenter
    Left,   // == LeftCenter
    Right,  // == RightCenter
    Center,
    BaselineLeft,
    BaselineRight,
    Baseline, // == BaselineCenter
    Absolute(Point),
}

impl Anchor {
    pub fn to_point(self, container: Rect, container_baseline: f64) -> Point {
        match self {
            Anchor::TopLeft => Point::new(container.x0, container.y0),
            Anchor::TopRight => Point::new(container.x1, container.y0),
            Anchor::BottomLeft => Point::new(container.x0, container.y1),
            Anchor::BottomRight => Point::new(container.x1, container.y1),
            Anchor::Top => Point::new(container.x0 + 0.5 * container.width(), container.y0),
            Anchor::Bottom => Point::new(container.x0 + 0.5 * container.width(), container.y1),
            Anchor::Left => Point::new(container.x0, container.y0 + 0.5 * container.height()),
            Anchor::Right => Point::new(container.x1, container.y0 + 0.5 * container.height()),
            Anchor::Center => Point::new(
                container.x0 + 0.5 * container.width(),
                container.y0 + 0.5 * container.height(),
            ),
            Anchor::BaselineLeft => Point::new(container.x0, container.y0 + container_baseline),
            Anchor::BaselineRight => Point::new(container.x1, container.y0 + container_baseline),
            Anchor::Baseline => Point::new(
                container.x0 + 0.5 * container.width(),
                container.y0 + container_baseline,
            ),
            Anchor::Absolute(point) => point,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Placement {
    pub container: Anchor,
    pub content: Anchor,
}

fn place_rect_into(
    container: Rect,
    container_baseline: f64,
    content: Rect,
    content_baseline: f64,
    placement: Placement,
) -> Point {
    let container_anchor = placement.container.to_point(container, container_baseline);
    let content_anchor = placement.content.to_point(content, content_baseline);
    let offset = container_anchor - content_anchor;
    content.origin() + offset
}

pub fn align(
    content: Rect,
    content_anchor: impl Into<Anchor>,
    container: Rect,
    container_anchor: impl Into<Anchor>,
) -> Rect {
    let pos = place_rect_into(
        container,
        0.0,
        content,
        0.0,
        Placement {
            container: container_anchor.into(),
            content: content_anchor.into(),
        },
    );
    Rect::from_origin_size(pos, content.size())
}

pub fn place(content: Rect, placement: impl Into<Placement>, container: Rect) -> Rect {
    let placement = placement.into();
    align(content, placement.content, container, placement.container)
}

impl From<Anchor> for Placement {
    fn from(anchor: Anchor) -> Self {
        Placement {
            container: anchor,
            content: anchor,
        }
    }
}

impl From<(Anchor, Anchor)> for Placement {
    fn from((container, content): (Anchor, Anchor)) -> Self {
        Placement { container, content }
    }
}

pub const TOP_LEFT: Placement = Placement {
    container: Anchor::TopLeft,
    content: Anchor::TopLeft,
};
pub const TOP_RIGHT: Placement = Placement {
    container: Anchor::TopRight,
    content: Anchor::TopRight,
};
pub const BOTTOM_LEFT: Placement = Placement {
    container: Anchor::BottomLeft,
    content: Anchor::BottomLeft,
};
pub const BOTTOM_RIGHT: Placement = Placement {
    container: Anchor::BottomRight,
    content: Anchor::BottomRight,
};
pub const TOP_CENTER: Placement = Placement {
    container: Anchor::Top,
    content: Anchor::Top,
};
pub const BOTTOM_CENTER: Placement = Placement {
    container: Anchor::Bottom,
    content: Anchor::Bottom,
};
pub const LEFT_CENTER: Placement = Placement {
    container: Anchor::Left,
    content: Anchor::Left,
};
pub const RIGHT_CENTER: Placement = Placement {
    container: Anchor::Right,
    content: Anchor::Right,
};
pub const CENTER: Placement = Placement {
    container: Anchor::Center,
    content: Anchor::Center,
};
pub const BASELINE_LEFT: Placement = Placement {
    container: Anchor::BaselineLeft,
    content: Anchor::BaselineLeft,
};
pub const BASELINE_RIGHT: Placement = Placement {
    container: Anchor::BaselineRight,
    content: Anchor::BaselineRight,
};
pub const BASELINE_CENTER: Placement = Placement {
    container: Anchor::Baseline,
    content: Anchor::Baseline,
};

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

    /// Rounds a logical point to the nearest physical pixel.
    pub fn round_to_px(&self, logical: Point) -> Point {
        Point::new(
            round_to_px(logical.x, self.scale_factor) + 0.5 / self.scale_factor,
            round_to_px(logical.y, self.scale_factor) + 0.5 / self.scale_factor,
        )
    }

    /// Sets the baseline of the current region.
    pub fn set_baseline(&mut self, baseline: f64) {
        self.baseline = baseline;
    }

    /// Draws a border around or inside the specified rectangle.
    pub fn draw_border(
        &mut self,
        rrect: RoundedRect,
        insets: impl Into<Insets>,
        _position: BorderPosition,
        paint: impl Into<Paint>,
    ) {
        let insets = insets.into();
        if insets == Insets::ZERO {
            return;
        }

        let paint = paint.into();
        if let Paint::Color(color) = paint {
            if color.alpha() == 0.0 {
                // fully transparent border
                return;
            }
        }

        let rect = rrect.rect();
        // FIXME support non-uniform radius
        let radius = rrect.radii().as_single_radius().unwrap_or(1.0);
        let inner = RoundedRect::from_rect(rect - insets, radius - 0.5 * insets.x_value());
        let outer = RoundedRect::from_rect(rect, radius);
        let paint = paint.to_sk_paint(self.rect, skia_safe::PaintStyle::Fill);
        self.canvas.draw_drrect(outer.to_skia(), inner.to_skia(), &paint);
    }

    /// Fills the current region with the specified paint.
    pub fn fill_rect(&mut self, rect: impl Into<Rect>, paint: impl Into<Paint>) {
        let rect = rect.into();
        let paint = paint.into().to_sk_paint(rect, skia_safe::PaintStyle::Fill);
        self.canvas.draw_rect(rect.to_skia(), &paint);
    }

    /// Fills the current region with the specified paint.
    pub fn fill_rrect(&mut self, rrect: impl Into<RoundedRect>, paint: impl Into<Paint>) {
        let rrect = rrect.into();
        let paint = paint.into().to_sk_paint(rrect.rect(), skia_safe::PaintStyle::Fill);
        self.canvas.draw_rrect(rrect.to_skia(), &paint);
    }

    /// Draws the specified image.
    pub fn draw_image(&mut self, placement: impl Into<Placement>, image: &Image) {
        // TODO image baseline?
        let pos = place_rect_into(self.rect, self.baseline, image.size().to_rect(), 0.0, placement.into());
        self.canvas.draw_image(image.to_skia(), pos.to_skia(), None);
    }

    /// Draws text in the current rectangle with the specified alignment.
    pub fn draw_text(&mut self, placement: impl Into<Placement>, text: &[TextRun]) {
        let mut layout = TextLayout::new(text);
        layout.layout(self.rect.width());
        let pos = place_rect_into(
            self.rect,
            self.baseline,
            layout.size().to_rect(),
            layout.baseline(),
            placement.into(),
        );
        layout.paint(&self.canvas, pos);
    }
}
