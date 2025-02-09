use kyute::Color;
use kyute::drawing::rgb;

pub const DISPLAY_BACKGROUND: Color = rgb(24, 14, 2);
pub const DISPLAY_TEXT: Color = rgb(255, 202, 76);
pub const DISPLAY_TEXT_INACTIVE: Color = rgb(66, 41, 0);

pub const STATIC_BACKGROUND: Color = rgb(26, 26, 26);
pub const STATIC_TEXT: Color = rgb(171, 168, 160);

pub const BUTTON_BACKGROUND: Color = rgb(37, 37, 37);
pub const BUTTON_BEVEL: Color = rgb(80, 80, 75);

pub const SLIDER_LINE_BACKGROUND: Color = DISPLAY_TEXT_INACTIVE;
pub const SLIDER_LINE: Color = DISPLAY_TEXT;

pub const MENU_SEPARATOR: Color = rgb(80, 80, 75);
pub const MENU_BACKGROUND: Color = rgb(26, 26, 26);

pub const SCROLL_BAR_BACKGROUND: Color = rgb(37, 37, 37);
pub const SCROLL_BAR: Color = rgb(80, 80, 75);
pub const SCROLL_BAR_ACCENT: Color = rgb(255, 202, 76);