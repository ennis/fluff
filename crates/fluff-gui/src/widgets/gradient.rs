//! Gradient editor widget base.

use kyute::Size;
use kyute::element::prelude::*;

pub struct GradientEditorBase {}

impl GradientEditorBase {
    pub fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        Size::new(0.0, 0.0)
    }
}
