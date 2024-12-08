//! Types and functions used for layouting widgets.
use std::fmt;
use crate::element::AttachedProperty;
use kurbo::{Size, Vec2};
use kyute_dsl::PropertyExpr;

pub mod flex;
mod cache;
//pub mod grid;

pub use cache::{LayoutCacheEntry, LayoutCache};

#[derive(Copy, Clone, PartialEq)]
//#[cfg_attr(feature = "serializing", derive(serde::Deserialize))]
/// Specifies a length, either in device-independent pixels or as a percentage of a reference length.
pub enum LengthOrPercentage {
    /// Length.
    Px(f64),
    /// Percentage of a reference length.
    Percentage(f64),
}

impl LengthOrPercentage {
    /// Zero length.
    pub const ZERO: LengthOrPercentage = LengthOrPercentage::Px(0.0);
}

impl Default for LengthOrPercentage {
    fn default() -> Self {
        Self::ZERO
    }
}

impl LengthOrPercentage {
    /// Converts this length to DIPs, using the specified reference size to resolve percentages.
    pub fn resolve(self, reference: f64) -> f64 {
        match self {
            LengthOrPercentage::Px(x) => x,
            LengthOrPercentage::Percentage(x) => x * reference,
        }
    }
}

impl fmt::Debug for LengthOrPercentage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LengthOrPercentage::Px(px) => write!(f, "{}px", px * 100.0),
            LengthOrPercentage::Percentage(percentage) => write!(f, "{}%", percentage * 100.0),
        }
    }
}

impl From<f64> for LengthOrPercentage {
    /// Creates a `LengthOrPercentage` from a DIP size.
    fn from(px: f64) -> Self {
        LengthOrPercentage::Px(px)
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Axis
////////////////////////////////////////////////////////////////////////////////////////////////////

/// Physical axis of a layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Axis {
    #[default]
    Vertical,
    Horizontal,
}

impl Axis {
    pub fn cross(&self) -> Axis {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }
}

/// Helper trait for main axis/cross axis sizes
pub trait AxisSizeHelper {
    fn main_cross(&self, main_axis: Axis) -> (f64, f64);
    fn axis(&self, axis: Axis) -> f64;
    fn from_main_cross(main_axis: Axis, main: f64, cross: f64) -> Self;
}

impl AxisSizeHelper for Size {
    fn main_cross(&self, main_axis: Axis) -> (f64, f64) {
        match main_axis {
            Axis::Horizontal => (self.width, self.height),
            Axis::Vertical => (self.height, self.width),
        }
    }

    fn axis(&self, axis: Axis) -> f64 {
        match axis {
            Axis::Horizontal => self.width,
            Axis::Vertical => self.height,
        }
    }

    fn from_main_cross(main_axis: Axis, main: f64, cross: f64) -> Self {
        match main_axis {
            Axis::Horizontal => Size {
                width: main,
                height: cross,
            },
            Axis::Vertical => Size {
                width: cross,
                height: main,
            },
        }
    }
}

trait AxisOffsetHelper {
    fn set_axis(&mut self, axis: Axis, offset: f64);
}

impl AxisOffsetHelper for Vec2 {
    fn set_axis(&mut self, axis: Axis, offset: f64) {
        match axis {
            Axis::Horizontal => self.x = offset,
            Axis::Vertical => self.y = offset,
        }
    }
}


////////////////////////////////////////////////////////////////////////////////////////////////////
// Alignment
////////////////////////////////////////////////////////////////////////////////////////////////////

// TODO Alignment is complicated, and what is meant varies under the context:
// - "left" or "right" is valid only when not talking about text.
// - otherwise, it's "trailing" and "leading", which takes into account the current text direction

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Alignment {
    Relative(f64),
    FirstBaseline,
    LastBaseline,
}

impl Alignment {
    pub const CENTER: Alignment = Alignment::Relative(0.5);
    pub const START: Alignment = Alignment::Relative(0.0);
    pub const END: Alignment = Alignment::Relative(1.0);
}

impl Default for Alignment {
    fn default() -> Self {
        Alignment::Relative(0.0)
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Sizing
////////////////////////////////////////////////////////////////////////////////////////////////////

/// Specifies the size of a visual element in one dimension.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum SizeValue {
    /// Automatic size. This inherits the constraints from the parent container.
    #[default]
    Auto,
    /// The element should have a fixed length (in unspecified units for layout, but interpreted as device-independent pixels for painting).
    Fixed(f64),
    /// The element should size itself as a percentage of the available space in the parent container.
    Percentage(f64),
    /// The element should size itself to its minimum content size: the smallest size it can be
    /// without its content overflowing.
    MinContent,
    /// The element should size itself to its maximum content size: the largest size it can be
    /// that still tightly wraps its content.
    MaxContent,
    Stretch,
}

impl SizeValue {
    pub fn resolve(self) -> Option<f64> {
        match self {
            SizeValue::Fixed(size) => Some(size),
            _ => None,
        }
    }

    pub fn is_stretch(self) -> bool {
        matches!(self, SizeValue::Stretch)
    }
}


impl From<f64> for SizeValue {
    fn from(size: f64) -> Self {
        SizeValue::Fixed(size)
    }
}

/// Conversion of SizeValue from DSL expressions.
impl TryFrom<PropertyExpr> for SizeValue {
    type Error = &'static str;

    fn try_from(value: PropertyExpr) -> Result<Self, Self::Error> {
        match value {
            PropertyExpr::String(s) => match s.as_str() {
                "auto" => Ok(SizeValue::Auto),
                "min-content" => Ok(SizeValue::MinContent),
                "max-content" => Ok(SizeValue::MaxContent),
                _ => Err("invalid size value"),
            },
            PropertyExpr::Px(px) => Ok(SizeValue::Fixed(px as f64)),
            PropertyExpr::Fr(_fr) => Ok(SizeValue::Stretch),    // TODO fractional units
            _ => Err("invalid size value"),
        }
    }
}


#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LayoutMode {
    Measure,
    Place,
}

/// Represents a sizing constraint passed down from a container to a child element during layout.
#[derive(Copy, Clone, PartialEq)]
pub enum SizeConstraint {
    /// The element has the specified available space to layout itself.
    /// If 0, the element should return its minimum size, if infinite, it should return its maximum size.
    Available(f64),
    /// Requests the element ideal size.
    Unspecified,
}

impl fmt::Debug for SizeConstraint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SizeConstraint::Available(size) => write!(f, "{:.2}", size),
            SizeConstraint::Unspecified => write!(f, "unspecified"),
        }
    }
}

impl SizeConstraint {
    /// Returns the available space if the constraint is `Available`, otherwise `None`.
    pub fn available(self) -> Option<f64> {
        if let SizeConstraint::Available(space) = self {
            Some(space)
        } else {
            None
        }
    }

    /// Resolves a percentage length to a concrete value if the provided sizing constraint is finite.
    /// Otherwise, returns 0.
    pub fn resolve_length(&self, length: LengthOrPercentage) -> f64 {
        let reference = match self {
            SizeConstraint::Available(size) if size.is_finite() => *size,
            _ => 0.0,
        };
        length.resolve(reference)
    }

    pub fn resolve_percentage(&self, percentage: f64) -> f64 {
        match self {
            SizeConstraint::Available(size) if size.is_finite() => percentage * size,
            _ => 0.0,
        }
    }

    /// Reserves space if the constraint is `Exact`, or `Available`, then returns the constraint for the remaining space.
    pub fn deflate(&self, space: f64) -> SizeConstraint {
        match self {
            SizeConstraint::Available(available) if available.is_finite() => {
                SizeConstraint::Available((available - space).max(0.0))
            }
            _ => *self,
        }
    }

    pub const MAX: SizeConstraint = SizeConstraint::Available(f64::INFINITY);
    pub const MIN: SizeConstraint = SizeConstraint::Available(0.0);
}

impl From<f64> for SizeConstraint {
    fn from(size: f64) -> Self {
        SizeConstraint::Available(size)
    }
}

/// Input parameters passed to the `measure` method of an element.
#[derive(Copy, Clone, PartialEq)]
pub struct LayoutInput {
    /// The sizing constraint in the horizontal axis.
    pub width: SizeConstraint,
    /// The sizing constraint in the vertical axis.
    pub height: SizeConstraint,
    /// The size of the parent container in the horizontal axis, if known.
    pub parent_width: Option<f64>,
    /// The size of the parent container in the vertical axis, if known.
    pub parent_height: Option<f64>,
}

impl fmt::Debug for LayoutInput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}×{:?}", self.width, self.height)
    }
}

impl LayoutInput {
    pub fn from_logical(main_axis: Axis, main: SizeConstraint, cross: SizeConstraint, parent_main: Option<f64>, parent_cross: Option<f64>) -> Self {
        match main_axis {
            Axis::Horizontal => LayoutInput {
                width: main,
                height: cross,
                parent_width: parent_main,
                parent_height: parent_cross,
            },
            Axis::Vertical => LayoutInput {
                width: cross,
                height: main,
                parent_width: parent_cross,
                parent_height: parent_main,
            },
        }
    }

    pub fn with_axis_constraint(self, axis: Axis, constraint: SizeConstraint) -> Self {
        match axis {
            Axis::Horizontal => LayoutInput {
                width: constraint,
                ..self
            },
            Axis::Vertical => LayoutInput {
                height: constraint,
                ..self
            },
        }
    }

    pub fn set_axis_constraint(&mut self, axis: Axis, constraint: SizeConstraint) {
        match axis {
            Axis::Horizontal => self.width = constraint,
            Axis::Vertical => self.height = constraint,
        }
    }

    pub fn resolve_length(&self, axis: Axis, length: LengthOrPercentage) -> f64 {
        match axis {
            Axis::Horizontal => self.width.resolve_length(length),
            Axis::Vertical => self.height.resolve_length(length),
        }
    }

    pub fn main_cross(&self, main_axis: Axis) -> (SizeConstraint, SizeConstraint) {
        match main_axis {
            Axis::Horizontal => (self.width, self.height),
            Axis::Vertical => (self.height, self.width),
        }
    }
}

/// The output of the layout process.
///
/// Returned by the `measure` and `layout` methods.
#[derive(Copy, Clone)]
pub struct LayoutOutput {
    /// The width of the element.
    pub width: f64,
    /// The height of the element.
    pub height: f64,
    /// Baseline offset relative to the top of the element box.
    pub baseline: Option<f64>,
}

impl fmt::Debug for LayoutOutput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:.2}×{:.2}", self.width, self.height)?;
        if let Some(baseline) = self.baseline {
            write!(f, " baseline={:.2}", baseline)?;
        }
        Ok(())
    }
}

impl LayoutOutput {
    pub fn from_main_cross_sizes(axis: Axis, main: f64, cross: f64, baseline: Option<f64>) -> Self {
        match axis {
            Axis::Horizontal => LayoutOutput {
                width: main,
                height: cross,
                baseline,
            },
            Axis::Vertical => LayoutOutput {
                width: cross,
                height: main,
                baseline,
            },
        }
    }

    pub const NULL: LayoutOutput = LayoutOutput {
        width: 0.0,
        height: 0.0,
        baseline: None,
    };

    pub fn size(&self, axis: Axis) -> f64 {
        match axis {
            Axis::Horizontal => self.width,
            Axis::Vertical => self.height,
        }
    }

    pub fn main_cross(&self, axis: Axis) -> (f64, f64) {
        match axis {
            Axis::Horizontal => (self.width, self.height),
            Axis::Vertical => (self.height, self.width),
        }
    }

    pub fn set_axis(&mut self, axis: Axis, size: f64) {
        match axis {
            Axis::Horizontal => self.width = size,
            Axis::Vertical => self.height = size,
        }
    }
}

impl Default for LayoutOutput {
    fn default() -> Self {
        LayoutOutput::NULL
    }
}

macro_rules! attached_properties {
    (
        $(
            $(#[$meta:meta])*
            $name:ident: $ty:ty;
        )*
    ) => {
        $(
            $(#[$meta])*
            #[derive(Copy, Clone, Debug)]
            pub struct $name;

            impl AttachedProperty for $name {
                type Value = $ty;
            }
        )*
    };
}

attached_properties! {
    /// Spacing before the element in a sequential layout.
    SpacingBefore: SizeValue;
    /// Spacing after the element in a sequential layout.
    SpacingAfter: SizeValue;
    /// Minimum spacing before the element in a sequential layout.
    MinSpacingBefore: SizeValue;
    /// Minimum spacing after the element in a sequential layout.
    MinSpacingAfter: SizeValue;
    HorizontalAlignment: Alignment;
    VerticalAlignment: Alignment;
}


////////////////////////////////////////////////////////////////////////////////////////////////////
// Positioning
////////////////////////////////////////////////////////////////////////////////////////////////////

/*
#[derive(Copy, Clone, Debug)]
pub enum Positioning {
    /// Position the start edge of the box at the specified offset relative to the start of the parent box.
    Start(f64),
    /// Position the end edge of the box at the specified offset relative to the end of the parent box.
    End(f64),
    /// Position both the start and edges of the box relative to the start and edges of the parent box.
    ///
    /// Note that this needs the size of the box to be flexible in order to accommodate both constraints.
    Both { start: f64, end: f64 },
    /// Center the box in the parent.
    Center,
    /// Align the baseline of the box with the baseline of the parent.
    Baseline,
}

pub(crate) fn position_box(
    parent_container_size: f64,
    parent_container_baseline: f64,
    box_size: FlexSize,
    positioning: Positioning,
) -> (f64, f64) {
    let offset;
    let actual_size;

    match positioning {
        Positioning::Start(start) => {
            offset = start;
            actual_size = box_size.grow(parent_container_size - start);
        }
        Positioning::End(offset) => {
            offset = parent_container_size - box_size.size - offset;
            actual_size = box_size.grow(offset);
        }
        Positioning::Both { start, end } => {
            let space = parent_container_size - box_size.size;
            start.max(0.0).min(space) + end.max(0.0).min(space)
        }
        Positioning::Center => (parent_container_size - box_size.size) / 2.0,
        Positioning::Baseline => 0.0,
    }
}

impl Positioning {
    pub fn resolve(&self, parent_container_size: f64, parent_container_baseline: f64, box_size: FlexSize) -> f64 {
        match self {
            Positioning::Start(offset) => *offset,
            Positioning::End(offset) => parent_container_size - box_size - offset,
            Positioning::Both { start, end } => {
                let space = parent_container_size - box_size;
                start.max(0.0).min(space) + end.max(0.0).min(space)
            }
            Positioning::Center => (parent_container_size - box_size) / 2.0,
            Positioning::Baseline => 0.0,
        }
    }
}
*/
