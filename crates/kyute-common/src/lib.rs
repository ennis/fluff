mod color;

// Reexport palette & kurbo
pub use palette;
pub use kurbo;

// Reexport common types from kurbo
pub use kurbo::{Point, Rect, Size, Vec2, Affine, Insets};
pub use color::{Color, ColorParseError};