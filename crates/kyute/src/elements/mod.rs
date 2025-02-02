//! Several widgets.

pub mod button;
pub mod draw;
pub mod flex;
pub mod frame;
pub mod text;
pub mod text_edit;

use crate::element_state::ElementState;
pub use draw::{Draw, Visual};
pub use flex::{Flex, FlexChildBuilder};
pub use frame::{Frame, FrameStyle, FrameStyleOverride};
pub use text::Text;
pub use text_edit::TextEditBase;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
#[repr(transparent)]
pub struct ElementId(pub u32);

/// Event emitted by some elements when they are clicked.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ClickedEvent(pub ElementId);

/// Event emitted by some elements when the pointer is entering or leaving the element.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct HoveredEvent(pub ElementId, pub bool);

/// Event emitted by some elements.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ElementStateChanged(pub ElementId, pub ElementState);

/// Event emitted by some elements.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ActivatedEvent(pub ElementId, pub bool);
