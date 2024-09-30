use std::borrow::Cow;
use kyute_common::Color;

/// Common font weights.
pub mod weights {
    pub const THIN: i32 = 100;
    pub const EXTRA_LIGHT: i32 = 200;
    pub const LIGHT: i32 = 300;
    pub const NORMAL: i32 = 400;
    pub const MEDIUM: i32 = 500;
    pub const SEMI_BOLD: i32 = 600;
    pub const BOLD: i32 = 700;
    pub const EXTRA_BOLD: i32 = 800;
    pub const BLACK: i32 = 900;
}

/// Common font widths.
pub mod widths {
    pub const ULTRA_CONDENSED: f32 = 0.5;
    pub const EXTRA_CONDENSED: f32 = 0.625;
    pub const CONDENSED: f32 = 0.75;
    pub const SEMI_CONDENSED: f32 = 0.875;
    pub const NORMAL: f32 = 1.0;
    pub const SEMI_EXPANDED: f32 = 1.125;
    pub const EXPANDED: f32 = 1.25;
    pub const EXTRA_EXPANDED: f32 = 1.5;
    pub const ULTRA_EXPANDED: f32 = 2.0;
}

/// Describes the style of a text run.
#[derive(Clone)]
pub struct TextStyle<'a> {
    pub font_family: Cow<'a, str>,
    pub font_size: f64,
    pub font_weight: i32,
    pub font_italic: bool,
    pub font_oblique: bool,
    pub font_width: i32,
    pub color: Color,
}

impl Default for TextStyle<'static> {
    fn default() -> Self {
        TextStyle::new().into_static()
    }
}

impl<'a> TextStyle<'a> {
    pub fn new() -> TextStyle<'a> {
        TextStyle {
            font_family: Cow::Borrowed("Inter Display"),
            font_size: 16.0,
            font_weight: weights::NORMAL,
            font_italic: false,
            font_oblique: false,
            font_width: *Width::NORMAL,
            color: Color::from_rgb_u8(0, 0, 0),
        }
    }

    pub fn font_family(mut self, font_family: impl Into<Cow<'a, str>>) -> Self {
        self.font_family = font_family.into();
        self
    }
    pub fn font_size(mut self, font_size: f64) -> Self {
        self.font_size = font_size;
        self
    }

    pub fn font_weight(mut self, font_weight: i32) -> Self {
        self.font_weight = font_weight;
        self
    }

    pub fn font_italic(mut self, font_italic: bool) -> Self {
        self.font_italic = font_italic;
        self
    }

    pub fn font_oblique(mut self, font_oblique: bool) -> Self {
        self.font_oblique = font_oblique;
        self
    }

    pub fn font_width(mut self, font_width: i32) -> Self {
        self.font_width = font_width;
        self
    }

    pub fn color(mut self, text_color: Color) -> Self {
        self.color = text_color;
        self
    }

    pub fn into_static(self) -> TextStyle<'static> {
        TextStyle {
            font_family: Cow::Owned(self.font_family.into_owned()),
            font_size: self.font_size,
            font_weight: self.font_weight,
            font_italic: self.font_italic,
            font_oblique: self.font_oblique,
            font_width: self.font_width,
            color: self.color,
        }
    }
}