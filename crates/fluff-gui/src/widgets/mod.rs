use crate::colors::STATIC_TEXT;
use kyute::text::{FontStretch, FontStyle, FontWeight, TextStyle};
use std::borrow::Cow;

pub mod button;
pub mod spinner;

/// Standard line height for widgets
pub const WIDGET_LINE_HEIGHT: f64 = 23.;

/// Standard baseline for widgets & labels.
pub const WIDGET_BASELINE: f64 = 16.;

/// Default width of input widgets, like numeric inputs or text inputs, when no width is specified.
pub const INPUT_WIDTH: f64 = 100.;

/// Font size for input widgets.
pub const WIDGET_FONT_SIZE: f64 = 12.;

/// Default font family for input widgets.
pub const WIDGET_FONT_FAMILY: &str = "Inter";

/// Default font style for input widgets.
pub const TEXT_STYLE: TextStyle = TextStyle {
    font_family: Cow::Borrowed("Inter"),
    font_size: 12.0,
    font_weight: FontWeight::NORMAL,
    font_style: FontStyle::Normal,
    font_stretch: FontStretch::NORMAL,
    color: STATIC_TEXT,
};
