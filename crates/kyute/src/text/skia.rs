use skia_safe::textlayout::TextDecorationMode;
use crate::drawing::ToSkia;
use crate::text::style::{FontStretch, FontStyle, FontWeight};
use crate::text::{TextAlign, TextStyle};

impl ToSkia for TextAlign {
    type Target = skia_safe::textlayout::TextAlign;

    fn to_skia(&self) -> Self::Target {
        match self {
            TextAlign::Start => skia_safe::textlayout::TextAlign::Left,
            TextAlign::End => skia_safe::textlayout::TextAlign::Right,
            TextAlign::Middle => skia_safe::textlayout::TextAlign::Center,
            TextAlign::Justify => skia_safe::textlayout::TextAlign::Justify,
        }
    }
}

impl ToSkia for FontWeight {
    type Target = skia_safe::font_style::Weight;

    fn to_skia(&self) -> Self::Target {
        skia_safe::font_style::Weight::from(self.0 as i32)
    }
}

impl ToSkia for FontStyle {
    type Target = skia_safe::font_style::Slant;

    fn to_skia(&self) -> Self::Target {
        match self {
            FontStyle::Normal => skia_safe::font_style::Slant::Upright,
            FontStyle::Italic => skia_safe::font_style::Slant::Italic,
            FontStyle::Oblique => skia_safe::font_style::Slant::Upright,
        }
    }
}

impl ToSkia for FontStretch {
    type Target = skia_safe::font_style::Width;

    fn to_skia(&self) -> Self::Target {
        match *self {
            a if a == Self::ULTRA_CONDENSED => skia_safe::font_style::Width::ULTRA_CONDENSED,
            a if a == Self::EXTRA_CONDENSED => skia_safe::font_style::Width::EXTRA_CONDENSED,
            a if a == Self::CONDENSED => skia_safe::font_style::Width::CONDENSED,
            a if a == Self::SEMI_CONDENSED => skia_safe::font_style::Width::SEMI_CONDENSED,
            a if a == Self::NORMAL => skia_safe::font_style::Width::NORMAL,
            a if a == Self::SEMI_EXPANDED => skia_safe::font_style::Width::SEMI_EXPANDED,
            a if a == Self::EXPANDED => skia_safe::font_style::Width::EXPANDED,
            a if a == Self::EXTRA_EXPANDED => skia_safe::font_style::Width::EXTRA_EXPANDED,
            a if a == Self::ULTRA_EXPANDED => skia_safe::font_style::Width::ULTRA_EXPANDED,
            _ => skia_safe::font_style::Width::NORMAL,
        }
    }
}

impl ToSkia for TextStyle<'_> {
    type Target = skia_safe::textlayout::TextStyle;

    fn to_skia(&self) -> Self::Target {
        let font_style = skia_safe::font_style::FontStyle::new(
            self.font_weight.to_skia(),
            self.font_stretch.to_skia(),
            self.font_style.to_skia(),
        );
        let mut style = skia_safe::textlayout::TextStyle::new();
        style.set_font_families(&[self.font_family.as_ref()]);
        style.set_font_size(self.font_size as f32);
        style.set_font_style(font_style);
        style.set_color(self.color.to_skia().to_color());
        let mut decoration = skia_safe::textlayout::Decoration::default();
        if self.underline {
            decoration.ty = skia_safe::textlayout::TextDecoration::UNDERLINE;
            decoration.style = skia_safe::textlayout::TextDecorationStyle::Solid;
            decoration.color = self.color.to_skia().to_color();
            decoration.mode = TextDecorationMode::Through;
        }
        style.set_decoration(&decoration);
        style
    }
}
