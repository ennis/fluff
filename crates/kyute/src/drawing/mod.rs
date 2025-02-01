//! Drawing-related wrappers and helpers for use with skia.
// re-export kurbo types
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Vec2};
pub use kurbo::{RoundedRect, RoundedRectRadii, Shape};
use skia_safe as sk;

use crate::Color;
pub use box_shadow::{draw_box_shadow, BoxShadow};
pub use image::{Image, StaticImage};
pub use linear_gradient::{ColorStop, InterpolationColorSpace, LinearGradient};
pub use paint::Paint;
pub use placement::{
    align, place, place_rect_into, Anchor, Placement, BASELINE_CENTER, BASELINE_LEFT, BASELINE_RIGHT, BOTTOM_CENTER,
    BOTTOM_LEFT, BOTTOM_RIGHT, CENTER, LEFT_CENTER, RIGHT_CENTER, TOP_CENTER, TOP_LEFT, TOP_RIGHT,
};

mod box_shadow;
mod image;
mod linear_gradient;
mod paint;
mod placement;

/// Represents either a size in logical pixels, or the size of the parent container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Specifies in pixel.
    Pixels(f64),
    /// Specifies the size of the parent container, width or height, depending on context.
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

/// Types that can be converted to their skia equivalent.
pub trait ToSkia {
    type Target;
    fn to_skia(&self) -> Self::Target;
}

/// Types that can be converted from their skia equivalent.
pub trait FromSkia {
    type Source;
    fn from_skia(value: Self::Source) -> Self;
}

impl ToSkia for Rect {
    type Target = sk::Rect;

    fn to_skia(&self) -> Self::Target {
        sk::Rect {
            left: self.x0 as f32,
            top: self.y0 as f32,
            right: self.x1 as f32,
            bottom: self.y1 as f32,
        }
    }
}

impl FromSkia for Rect {
    type Source = sk::Rect;

    fn from_skia(value: Self::Source) -> Self {
        Rect {
            x0: value.left as f64,
            y0: value.top as f64,
            x1: value.right as f64,
            y1: value.bottom as f64,
        }
    }
}

impl ToSkia for Point {
    type Target = sk::Point;

    fn to_skia(&self) -> Self::Target {
        sk::Point {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

impl ToSkia for Vec2 {
    type Target = sk::Vector;

    fn to_skia(&self) -> Self::Target {
        sk::Vector {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

impl FromSkia for Point {
    type Source = sk::Point;

    fn from_skia(value: Self::Source) -> Self {
        Point::new(value.x as f64, value.y as f64)
    }
}

impl ToSkia for Color {
    type Target = sk::Color4f;

    fn to_skia(&self) -> Self::Target {
        let (r, g, b, a) = self.to_rgba();
        skia_safe::Color4f { r, g, b, a }
    }
}

impl FromSkia for Color {
    type Source = skia_safe::Color4f;

    fn from_skia(value: Self::Source) -> Self {
        Color::new(value.r, value.g, value.b, value.a)
    }
}

impl ToSkia for Affine {
    type Target = sk::Matrix;

    fn to_skia(&self) -> Self::Target {
        let [m11, m12, m21, m22, m31, m32] = self.as_coeffs();
        sk::Matrix::new_all(
            m11 as sk::scalar,
            m21 as sk::scalar,
            m31 as sk::scalar,
            m12 as sk::scalar,
            m22 as sk::scalar,
            m32 as sk::scalar,
            0.0,
            0.0,
            1.0,
        )
    }
}

impl ToSkia for BezPath {
    type Target = sk::Path;

    fn to_skia(&self) -> Self::Target {
        let mut path = sk::Path::new();
        for el in self.iter() {
            match el {
                PathEl::MoveTo(p) => {
                    path.move_to(p.to_skia());
                }
                PathEl::LineTo(p) => {
                    path.line_to(p.to_skia());
                }
                PathEl::QuadTo(p1, p2) => {
                    path.quad_to(p1.to_skia(), p2.to_skia());
                }
                PathEl::CurveTo(p1, p2, p3) => {
                    path.cubic_to(p1.to_skia(), p2.to_skia(), p3.to_skia());
                }
                PathEl::ClosePath => {
                    path.close();
                }
            }
        }
        path
    }
}

//--------------------------------------------------------------------------------------------------

/// Describes a blending mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    Clear,
    Src,
    Dst,
    SrcOver,
    DstOver,
    SrcIn,
    DstIn,
    SrcOut,
    DstOut,
    SrcATop,
    DstATop,
    Xor,
    Plus,
    Modulate,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Multiply,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl ToSkia for BlendMode {
    type Target = sk::BlendMode;

    fn to_skia(&self) -> Self::Target {
        match *self {
            BlendMode::Clear => sk::BlendMode::Clear,
            BlendMode::Src => sk::BlendMode::Src,
            BlendMode::Dst => sk::BlendMode::Dst,
            BlendMode::SrcOver => sk::BlendMode::SrcOver,
            BlendMode::DstOver => sk::BlendMode::DstOver,
            BlendMode::SrcIn => sk::BlendMode::SrcIn,
            BlendMode::DstIn => sk::BlendMode::DstIn,
            BlendMode::SrcOut => sk::BlendMode::SrcOut,
            BlendMode::DstOut => sk::BlendMode::DstOut,
            BlendMode::SrcATop => sk::BlendMode::SrcATop,
            BlendMode::DstATop => sk::BlendMode::DstATop,
            BlendMode::Xor => sk::BlendMode::Xor,
            BlendMode::Plus => sk::BlendMode::Plus,
            BlendMode::Modulate => sk::BlendMode::Modulate,
            BlendMode::Screen => sk::BlendMode::Screen,
            BlendMode::Overlay => sk::BlendMode::Overlay,
            BlendMode::Darken => sk::BlendMode::Darken,
            BlendMode::Lighten => sk::BlendMode::Lighten,
            BlendMode::ColorDodge => sk::BlendMode::ColorDodge,
            BlendMode::ColorBurn => sk::BlendMode::ColorBurn,
            BlendMode::HardLight => sk::BlendMode::HardLight,
            BlendMode::SoftLight => sk::BlendMode::SoftLight,
            BlendMode::Difference => sk::BlendMode::Difference,
            BlendMode::Exclusion => sk::BlendMode::Exclusion,
            BlendMode::Multiply => sk::BlendMode::Multiply,
            BlendMode::Hue => sk::BlendMode::Hue,
            BlendMode::Saturation => sk::BlendMode::Saturation,
            BlendMode::Color => sk::BlendMode::Color,
            BlendMode::Luminosity => sk::BlendMode::Luminosity,
        }
    }
}

fn radii_to_skia(radii: &RoundedRectRadii) -> [sk::Vector; 4] {
    let tl = radii.top_left as sk::scalar;
    let tr = radii.top_right as sk::scalar;
    let bl = radii.bottom_left as sk::scalar;
    let br = radii.bottom_right as sk::scalar;
    [
        sk::Vector::new(tl, tl),
        sk::Vector::new(tr, tr),
        sk::Vector::new(br, br),
        sk::Vector::new(bl, bl),
    ]
}

impl ToSkia for RoundedRect {
    type Target = skia_safe::RRect;

    fn to_skia(&self) -> Self::Target {
        if self.radii().as_single_radius() == Some(0.0) {
            sk::RRect::new_rect(self.rect().to_skia())
        } else {
            sk::RRect::new_rect_radii(self.rect().to_skia(), &radii_to_skia(&self.radii()))
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

/// Short-hand for [`Point::new`].
pub const fn point(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// Short-hand for [`Vec2::new`].
pub const fn vec2(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
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
