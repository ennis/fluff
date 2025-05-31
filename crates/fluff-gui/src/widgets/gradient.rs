//! Gradient editor widget base.

use kyute::{LayoutInput, Size};

pub struct GradientEditorBase {}

impl GradientEditorBase {
    pub fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        Size::new(0.0, 0.0)
    }
}
