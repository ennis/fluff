use crate::drawing::ToSkia;
use kurbo::{Rect, Vec2};
use kyute_common::Color;
use skia_safe as sk;
use skia_safe::gradient_shader::Interpolation;
use tracing::warn;

/// Represents a color stop of a gradient.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ColorStop {
    /// Position of the stop along the gradient segment, normalized between zero and one.
    ///
    /// If `None`, the position is inferred from the position of the surrounding stops.
    pub position: Option<f64>,
    /// Stop color.
    pub color: Color,
}

impl From<(f64, Color)> for ColorStop {
    fn from((position, color): (f64, Color)) -> Self {
        ColorStop {
            position: Some(position),
            color,
        }
    }
}

impl From<Color> for ColorStop {
    fn from(color: Color) -> Self {
        ColorStop {
            position: None,
            color: color.into(),
        }
    }
}

/// Color space for gradient interpolation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum InterpolationColorSpace {
    #[default]
    SrgbLinear,
    Srgb,
    Oklab,
}

/// Describes a linear color gradient.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    /// Direction of the gradient line in degrees.
    //#[serde(deserialize_with = "deserialize_angle")]
    pub angle_degrees: f64,
    /// List of color stops.
    pub stops: Vec<ColorStop>,
    /// Color space for interpolation. The stop colors are converted to this space before interpolation.
    pub color_space: InterpolationColorSpace,
}

impl LinearGradient {
    /// Creates a new `LinearGradient`, with no stops.
    pub fn new() -> LinearGradient {
        LinearGradient {
            angle_degrees: Default::default(),
            stops: vec![],
            color_space: InterpolationColorSpace::SrgbLinear,
        }
    }

    /// Sets the gradient angle in degrees.
    pub fn angle(mut self, angle_degrees: f64) -> Self {
        self.angle_degrees = angle_degrees;
        self
    }

    /// Appends a color stop to this gradient.
    pub fn stop(mut self, color: Color, position: impl Into<Option<f64>>) -> Self {
        self.stops.push(ColorStop {
            color,
            position: position.into(),
        });
        self
    }

    pub fn to_skia_paint(&self, bounds: Rect) -> sk::Paint {
        let c = bounds.center();
        let w = bounds.width();
        let h = bounds.height();

        let angle = self.angle_degrees.to_radians();
        let tan_th = angle.tan();
        let (x, y) = if tan_th > h / w {
            (h / (2.0 * tan_th), 0.5 * h)
        } else {
            (0.5 * w, 0.5 * w * tan_th)
        };

        let a = c + Vec2::new(-x, y);
        let b = c + Vec2::new(x, -y);
        let a = a.to_skia();
        let b = b.to_skia();

        let mut resolved_gradient = self.clone();
        resolved_gradient.resolve_stop_positions();

        let positions: Vec<_> = resolved_gradient
            .stops
            .iter()
            .map(|stop| stop.position.unwrap() as f32)
            .collect();
        let colors: Vec<_> = resolved_gradient
            .stops
            .iter()
            .map(|stop| stop.color.to_skia())
            .collect();

        let interpolation = Interpolation {
            in_premul: sk::gradient_shader::interpolation::InPremul::No,
            color_space: match self.color_space {
                InterpolationColorSpace::SrgbLinear => sk::gradient_shader::interpolation::ColorSpace::SRGBLinear,
                InterpolationColorSpace::Srgb => sk::gradient_shader::interpolation::ColorSpace::SRGB,
                InterpolationColorSpace::Oklab => sk::gradient_shader::interpolation::ColorSpace::OKLab,
            },
            hue_method: sk::gradient_shader::interpolation::HueMethod::Increasing,
        };

        let shader = sk::Shader::linear_gradient_with_interpolation(
            (a, b),
            (&colors, Some(sk::ColorSpace::new_srgb_linear())),
            &positions[..],
            sk::TileMode::Clamp,
            interpolation,
            None,
        )
        .unwrap();

        let mut paint = sk::Paint::default();
        paint.set_shader(shader);
        paint.set_anti_alias(true);
        paint
    }

    /// Resolves color stop positions.
    ///
    /// See https://www.w3.org/TR/css-images-3/#color-stop-fixup
    pub(crate) fn resolve_stop_positions(&mut self) {
        if self.stops.len() < 2 {
            warn!("invalid gradient (must have at least two stops)");
            return;
        }

        // CSS Images Module Level 3 - 3.4.3. Color Stop “Fixup”
        //
        //      If the first color stop does not have a position, set its position to 0%.
        //      If the last color stop does not have a position, set its position to 100%.
        //
        self.stops.first_mut().unwrap().position.get_or_insert(0.0);
        self.stops.last_mut().unwrap().position.get_or_insert(1.0);

        //
        //      If a color stop or transition hint has a position that is less than the specified position
        //      of any color stop or transition hint before it in the list, set its position to be equal
        //      to the largest specified position of any color stop or transition hint before it.
        //
        let mut cur_pos = self.stops.first().unwrap().position.unwrap();
        for stop in self.stops.iter_mut() {
            if let Some(mut pos) = stop.position {
                if pos < cur_pos {
                    pos = cur_pos;
                }
                cur_pos = pos;
            }
        }

        //
        //      If any color stop still does not have a position, then, for each run of adjacent color stops without positions,
        //      set their positions so that they are evenly spaced between the preceding and following color stops with positions.
        //
        let mut i = 0;
        while i < self.stops.len() {
            if self.stops[i].position.is_none() {
                let mut j = i + 1;
                while self.stops[j].position.is_none() {
                    j += 1;
                }
                let len = j - i + 1;
                let a = self.stops[i - 1].position.unwrap();
                let b = self.stops[j].position.unwrap();
                for k in i..j {
                    self.stops[i].position = Some(a + (b - a) * (k - i + 1) as f64 / len as f64);
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
}

impl Default for LinearGradient {
    fn default() -> Self {
        Self::new()
    }
}
