use crate::colors::{DISPLAY_TEXT, STATIC_TEXT};
use kyute::text::{FontStretch, FontStyle, FontWeight, TextStyle};
use std::borrow::Cow;
use kyute::{PaintCtx, Rect};
use crate::colors;

pub mod button;
pub mod spinner;
pub mod slider;
mod fcurve;
mod gradient;
pub mod menu;
pub mod scroll;

/// Standard line height for widgets
pub const WIDGET_LINE_HEIGHT: f64 = 23.;

/// Standard line height for menu items
pub const MENU_ITEM_HEIGHT: f64 = 19.;

/// Standard baseline for menu items.
pub const MENU_ITEM_BASELINE: f64 = 14.;

/// Standard line height for menu separators
pub const MENU_SEPARATOR_HEIGHT: f64 = 5.;

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
    underline: false,
};

/// Default font style for input widgets.
pub const DISPLAY_TEXT_STYLE: TextStyle = TextStyle {
    font_family: Cow::Borrowed("Inter"),
    font_size: 12.0,
    font_weight: FontWeight::NORMAL,
    font_style: FontStyle::Normal,
    font_stretch: FontStretch::NORMAL,
    color: DISPLAY_TEXT,
    underline: false,
};

/// Extension trait for painting widgets.
pub trait PaintExt {
    /// Draws the standard background of input widgets with the "display" style.
    fn draw_display_background(&mut self, bounds: Rect);
}

impl PaintExt for PaintCtx<'_> {
    fn draw_display_background(&mut self, bounds: Rect) {
        let rrect = bounds.to_rounded_rect(4.0);
        self.fill_rrect(rrect, colors::DISPLAY_BACKGROUND);
    }
}
