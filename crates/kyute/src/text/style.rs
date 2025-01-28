use kyute_common::Color;
use std::borrow::Cow;
use std::num::ParseIntError;
use std::str::FromStr;

/// Describes the style of a text run.
#[derive(Clone)]
pub struct TextStyle<'a> {
    pub font_family: Cow<'a, str>,
    pub font_size: f64,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub font_stretch: FontStretch,
    pub color: Color,
}

impl Default for TextStyle<'static> {
    fn default() -> Self {
        TextStyle::new().into_static()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const THIN: FontWeight = FontWeight(100);
    pub const EXTRA_LIGHT: FontWeight = FontWeight(200);
    pub const LIGHT: FontWeight = FontWeight(300);
    pub const REGULAR: FontWeight = FontWeight(400);
    pub const NORMAL: FontWeight = FontWeight(400);
    pub const MEDIUM: FontWeight = FontWeight(500);
    pub const SEMI_BOLD: FontWeight = FontWeight(600);
    pub const BOLD: FontWeight = FontWeight(700);
    pub const EXTRA_BOLD: FontWeight = FontWeight(800);
    pub const BLACK: FontWeight = FontWeight(900);
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl FromStr for FontWeight {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "thin" => Ok(Self::THIN),
            "extra-light" => Ok(Self::EXTRA_LIGHT),
            "light" => Ok(Self::LIGHT),
            "regular" => Ok(Self::REGULAR),
            "normal" => Ok(Self::NORMAL),
            "medium" => Ok(Self::MEDIUM),
            "semi-bold" => Ok(Self::SEMI_BOLD),
            "bold" => Ok(Self::BOLD),
            "extra-bold" => Ok(Self::EXTRA_BOLD),
            "black" => Ok(Self::BLACK),
            s => s.parse::<u16>().map(FontWeight),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontStretch(f32);

impl FontStretch {
    pub const ULTRA_CONDENSED: FontStretch = FontStretch(0.5);
    pub const EXTRA_CONDENSED: FontStretch = FontStretch(0.625);
    pub const CONDENSED: FontStretch = FontStretch(0.75);
    pub const SEMI_CONDENSED: FontStretch = FontStretch(0.875);
    pub const NORMAL: FontStretch = FontStretch(1.0);
    pub const SEMI_EXPANDED: FontStretch = FontStretch(1.125);
    pub const EXPANDED: FontStretch = FontStretch(1.25);
    pub const EXTRA_EXPANDED: FontStretch = FontStretch(1.5);
    pub const ULTRA_EXPANDED: FontStretch = FontStretch(2.0);
}

impl Default for FontStretch {
    fn default() -> Self {
        Self::NORMAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

impl Default for FontStyle {
    fn default() -> Self {
        Self::Normal
    }
}

impl<'a> TextStyle<'a> {
    pub fn new() -> TextStyle<'a> {
        TextStyle {
            font_family: Cow::Borrowed("Inter Display"),
            font_size: 16.0,
            font_weight: Default::default(),
            font_style: Default::default(),
            font_stretch: Default::default(),
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

    pub fn font_weight(mut self, font_weight: FontWeight) -> Self {
        self.font_weight = font_weight;
        self
    }

    pub fn font_style(mut self, font_style: FontStyle) -> Self {
        self.font_style = font_style;
        self
    }

    pub fn font_stretch(mut self, font_stretch: FontStretch) -> Self {
        self.font_stretch = font_stretch;
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
            font_style: self.font_style,
            font_stretch: self.font_stretch,
            color: self.color,
        }
    }
}
