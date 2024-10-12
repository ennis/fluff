//! Types and functions used for layouting widgets.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Range, RangeBounds};

use kurbo::{Insets, Rect, Size, Vec2};
use tracing::trace;

use crate::element::AttachedProperty;
use crate::ElementMethods;
use crate::layout::flex::Axis;

pub mod flex;
//pub mod grid;

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
// LayoutConstraints
////////////////////////////////////////////////////////////////////////////////////////////////////

/// Layout constraints passed down to child widgets
#[derive(Copy, Clone)]
pub struct BoxConstraints {
    /// Minimum allowed size.
    pub min: Size,
    /// Maximum allowed size (can be infinite).
    pub max: Size,
}

impl Default for BoxConstraints {
    fn default() -> Self {
        BoxConstraints {
            min: Size::ZERO,
            max: Size::new(f64::INFINITY, f64::INFINITY),
        }
    }
}

// required because we also have a custom hash impl
// (https://rust-lang.github.io/rust-clippy/master/index.html#derive_hash_xor_eq)
impl PartialEq for BoxConstraints {
    fn eq(&self, other: &Self) -> bool {
        self.min.width.to_bits() == other.min.width.to_bits()
            && self.min.height.to_bits() == other.min.height.to_bits()
            && self.max.width.to_bits() == other.max.width.to_bits()
            && self.max.height.to_bits() == other.max.height.to_bits()
        //&& self.font_size == other.font_size
    }
}

impl Hash for BoxConstraints {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.min.width.to_bits().hash(state);
        self.min.height.to_bits().hash(state);
        self.max.width.to_bits().hash(state);
        self.max.height.to_bits().hash(state);
        //self.font_size.to_bits().hash(state);
    }
}

/*impl Data for LayoutParams {
    fn same(&self, other: &Self) -> bool {
        self == other
    }
}*/

impl fmt::Debug for BoxConstraints {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.min.width == self.max.width {
            write!(f, "[w={}, ", self.min.width)?;
        } else {
            if self.max.width.is_finite() {
                write!(f, "[{}≤w≤{}, ", self.min.width, self.max.width)?;
            } else {
                write!(f, "[{}≤w≤∞, ", self.min.width)?;
            }
        }

        if self.min.height == self.max.height {
            write!(f, "h={} ", self.min.height)?;
        } else {
            if self.max.height.is_finite() {
                write!(f, "{}≤h≤{}", self.min.height, self.max.height)?;
            } else {
                write!(f, "{}≤h≤∞", self.min.height)?;
            }
        }

        write!(f, "]")
    }
}

fn range_bounds_to_lengths(bounds: impl RangeBounds<f64>) -> (f64, f64) {
    let start = match bounds.start_bound() {
        std::ops::Bound::Included(&x) => x,
        std::ops::Bound::Excluded(&x) => x,
        std::ops::Bound::Unbounded => 0.0,
    };
    let end = match bounds.end_bound() {
        std::ops::Bound::Included(&x) => x,
        std::ops::Bound::Excluded(&x) => x,
        std::ops::Bound::Unbounded => f64::INFINITY,
    };
    (start, end)
}

impl BoxConstraints {
    pub fn deflate(&self, insets: Insets) -> BoxConstraints {
        BoxConstraints {
            max: Size {
                width: (self.max.width - insets.x_value()).max(self.min.width),
                height: (self.max.height - insets.y_value()).max(self.min.height),
            },
            ..*self
        }
    }

    pub fn loose(size: Size) -> BoxConstraints {
        BoxConstraints {
            min: Size::ZERO,
            max: size,
        }
    }

    pub fn loosen(&self) -> BoxConstraints {
        BoxConstraints {
            min: Size::ZERO,
            ..*self
        }
    }

    pub fn finite_max_width(&self) -> Option<f64> {
        if self.max.width.is_finite() {
            Some(self.max.width)
        } else {
            None
        }
    }

    pub fn finite_max_height(&self) -> Option<f64> {
        if self.max.height.is_finite() {
            Some(self.max.height)
        } else {
            None
        }
    }

    pub fn constrain(&self, size: Size) -> Size {
        Size::new(self.constrain_width(size.width), self.constrain_height(size.height))
    }

    pub fn constrain_width(&self, width: f64) -> f64 {
        width.max(self.min.width).min(self.max.width)
    }

    pub fn constrain_height(&self, height: f64) -> f64 {
        height.max(self.min.height).min(self.max.height)
    }

    fn compute_length(&self, length: LengthOrPercentage, max_length: f64) -> f64 {
        match length {
            LengthOrPercentage::Px(px) => px,
            LengthOrPercentage::Percentage(x) => x * max_length,
        }
    }

    pub fn compute_width(&self, width: LengthOrPercentage) -> f64 {
        self.compute_length(width, self.max.width)
    }

    pub fn compute_height(&self, height: LengthOrPercentage) -> f64 {
        self.compute_length(height, self.max.height)
    }

    pub fn intersect_width(&mut self, width: impl RangeBounds<f64>) {
        let (min, max) = range_bounds_to_lengths(width);
        self.min.width = self.min.width.max(min);
        self.max.width = self.max.width.min(max);
    }

    pub fn intersect_height(&mut self, height: impl RangeBounds<f64>) {
        let (min, max) = range_bounds_to_lengths(height);
        self.min.height = self.min.height.max(min);
        self.max.height = self.max.height.min(max);
    }

    pub fn width_range(&self) -> Range<f64> {
        self.min.width..self.max.width
    }

    pub fn height_range(&self) -> Range<f64> {
        self.min.height..self.max.height
    }
}

/// Which axis should be measured.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RequestedAxis {
    /// Compute the layout in the horizontal axis only (i.e. the width).
    Horizontal,
    /// Compute the layout in the vertical axis only (i.e. the height).
    Vertical,
    /// Compute the layout in both axes.
    Both,
}

impl From<Axis> for RequestedAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::Horizontal => RequestedAxis::Horizontal,
            Axis::Vertical => RequestedAxis::Vertical,
        }
    }
}

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
}

/// Specifies the size of a visual element in one dimension.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Sizing {
    /// The preferred size of the item.
    pub value: SizeValue,
    /// Minimum size.
    pub min: SizeValue,
    /// Maximum size.
    pub max: SizeValue,
    /// The element should size itself as a proportion of the remaining available space in the parent container after all fixed lengths have been resolved.
    ///
    /// Equivalent to `flex-grow` in CSS.
    ///
    /// If 0.0, the element doesn't grow. Has no effect for the cross axis in a flex container.
    pub flex: f64,
}

impl Sizing {
    pub const NULL: Sizing = Sizing {
        value: SizeValue::Fixed(0.0),
        min: SizeValue::Fixed(0.0),
        max: SizeValue::Fixed(0.0),
        flex: 0.0,
    };
}

/// Resolved box size values.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BoxMeasurements {
    /// Preferred size.
    pub size: f64,
    /// Minimum size.
    pub min: f64,
    /// Maximum size.
    pub max: f64,
    /// Flex factor. Zero means that the box doesn't grow.
    pub flex: f64,
}

impl BoxMeasurements {
    pub const NULL: BoxMeasurements = BoxMeasurements {
        size: 0.0,
        min: 0.0,
        max: 0.0,
        flex: 0.0,
    };
}

pub fn measure_element(
    element: &dyn ElementMethods,
    axis: Axis,
    sizing: Sizing,
    mut main_axis_constraint: SizingConstraint,
    mut cross_axis_constraint: SizingConstraint,
) -> BoxMeasurements {
    let _span = tracing::trace_span!("measure_element").entered();

    // relax constraints: we only want to inherit available space, min-content and max-content constraints
    // TODO maybe this should be done by the caller
    if let SizingConstraint::Exact(space) = main_axis_constraint {
        main_axis_constraint = SizingConstraint::Available(space);
    }
    if let SizingConstraint::Exact(space) = cross_axis_constraint {
        cross_axis_constraint = SizingConstraint::Available(space);
    }

    let measure_size = |main_axis_constraint: SizingConstraint,
                        cross_axis_constraint: SizingConstraint,
                        size_value: SizeValue,
                        element: &dyn ElementMethods|
                        -> f64 {
        let main_axis_constraint = match size_value {
            SizeValue::Fixed(s) => SizingConstraint::Exact(s),
            SizeValue::Percentage(p) => {
                match main_axis_constraint {
                    SizingConstraint::Available(space) => SizingConstraint::Exact(p * space),
                    _ => {
                        main_axis_constraint
                    }
                }
            }
            SizeValue::MinContent => SizingConstraint::MinContent,
            SizeValue::MaxContent => SizingConstraint::MaxContent,
            _ => main_axis_constraint,
        };

        match main_axis_constraint {
            SizingConstraint::Available(_) | SizingConstraint::MinContent | SizingConstraint::MaxContent => element
                .do_measure(&LayoutInput::from_main_cross_constraints(
                    axis,
                    axis.into(),
                    main_axis_constraint,
                    cross_axis_constraint,
                ))
                .size(axis),
            SizingConstraint::Exact(size) => {
                // We don't need to measure the element if we know its exact size
                size
            }
        }
    };

    let min = measure_size(main_axis_constraint, cross_axis_constraint, sizing.min, element);
    let max = measure_size(main_axis_constraint, cross_axis_constraint, sizing.max, element);
    let preferred = measure_size(main_axis_constraint, cross_axis_constraint, sizing.value, element);
    let size = preferred.clamp(min, max);

    trace!("Measured element: axis={:?}, sizing={:?}, min={}, max={}, preferred={}, size={}", axis, sizing, min, max, preferred, size);

    BoxMeasurements {
        min,
        max,
        size,
        flex: sizing.flex,
    }
}

/// Represents a sizing constraint passed down from a container to a child element during layout.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SizingConstraint {
    /// The element has the specified available space to layout itself.
    Available(f64),
    /// The element should size itself to its minimum content size: the smallest size it can be
    /// without its content overflowing.
    MinContent,
    /// The element should size itself to its maximum content size: the largest size it can be
    /// that still tightly wraps its content.
    MaxContent,
    /// The element should have the specified exact size.
    Exact(f64),
}

impl SizingConstraint {
    /// Resolves a percentage length to a concrete value if the provided sizing constraint is exact.
    /// Otherwise, returns 0.
    pub fn resolve_length(&self, length: LengthOrPercentage) -> f64 {
        let reference = match self {
            SizingConstraint::Exact(size) => *size,
            _ => 0.0,
        };
        length.resolve(reference)
    }

    /// Reserves space if the constraint is `Exact`, or `Available`, then returns the constraint for the remaining space.
    pub fn deflate(&self, space: f64) -> SizingConstraint {
        match self {
            SizingConstraint::Exact(size) => SizingConstraint::Exact((size - space).max(0.0)),
            SizingConstraint::Available(available) => SizingConstraint::Available((available - space).max(0.0)),
            _ => *self,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct LayoutInput {
    /// Which axis should be measured. If not `Both`, then only the corresponding dimension of
    /// the returned `LayoutOutput` needs to be valid.
    pub axis: RequestedAxis,
    /// The sizing constraint in the horizontal axis.
    pub width_constraint: SizingConstraint,
    /// The sizing constraint in the vertical axis.
    pub height_constraint: SizingConstraint,
}

impl LayoutInput {
    pub fn from_main_cross_constraints(
        main_axis: Axis,
        requested_axis: RequestedAxis,
        main: SizingConstraint,
        cross: SizingConstraint,
    ) -> Self {
        match main_axis {
            Axis::Horizontal => LayoutInput {
                axis: requested_axis,
                width_constraint: main,
                height_constraint: cross,
            },
            Axis::Vertical => LayoutInput {
                axis: requested_axis,
                width_constraint: cross,
                height_constraint: main,
            },
        }
    }

    pub fn with_axis_constraint(self, axis: Axis, constraint: SizingConstraint) -> Self {
        match axis {
            Axis::Horizontal => LayoutInput {
                width_constraint: constraint,
                ..self
            },
            Axis::Vertical => LayoutInput {
                height_constraint: constraint,
                ..self
            },
        }
    }

    pub fn set_axis_constraint(&mut self, axis: Axis, constraint: SizingConstraint) {
        match axis {
            Axis::Horizontal => self.width_constraint = constraint,
            Axis::Vertical => self.height_constraint = constraint,
        }
    }

    pub fn with_requested_axis(self, axis: Axis) -> Self {
        LayoutInput {
            axis: axis.into(),
            ..self
        }
    }

    pub fn resolve_length(&self, axis: Axis, length: LengthOrPercentage) -> f64 {
        match axis {
            Axis::Horizontal => self.width_constraint.resolve_length(length),
            Axis::Vertical => self.height_constraint.resolve_length(length),
        }
    }
}


/// Spacing values.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct FlexSize {
    /// Minimum space.
    pub size: f64,
    /// Flex factor (0.0 means no stretching).
    pub flex: f64,
}

impl FlexSize {
    pub const NULL: FlexSize = FlexSize { size: 0.0, flex: 0.0 };

    /// Combines two flex sizes, e.g. two margins that collapse.
    pub fn combine(self, other: FlexSize) -> FlexSize {
        FlexSize {
            size: self.size.max(other.size),
            flex: self.flex.max(other.flex),
        }
    }

    pub fn grow(self, available: f64) -> f64 {
        if self.flex != 0.0 && available.is_finite() {
            self.size.max(available)
        } else {
            self.size
        }
    }
}

/// The output of the layout process.
///
/// Returned by the `measure` and `layout` methods.
#[derive(Copy, Clone, Debug)]
pub struct LayoutOutput {
    /// The width of the element.
    ///
    /// This needs to be valid if the requested axis is `Horizontal` or `Both`.
    pub width: f64,
    /// The height of the element.
    ///
    /// This needs to be valid if the requested axis is `Vertical` or `Both`.
    pub height: f64,
    /// Baseline offset relative to the top of the element box.
    pub baseline: Option<f64>,
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
}

/// Attached property that controls the width of items inside containers.
#[derive(Copy, Clone, Debug)]
pub struct Width;

impl AttachedProperty for Width {
    type Value = Sizing;
}

/// Attached property that controls the height of items inside containers.
#[derive(Copy, Clone, Debug)]
pub struct Height;

impl AttachedProperty for Height {
    type Value = Sizing;
}

/// Attached property that controls start/end margins on flex items.
#[derive(Copy, Clone, Debug)]
pub struct FlexMargins;

impl AttachedProperty for FlexMargins {
    type Value = (FlexSize, FlexSize);
}

/// Attached property that controls horizontal positioning of items inside a container.
#[derive(Copy, Clone, Debug)]
pub struct HorizontalAlignment;

impl AttachedProperty for HorizontalAlignment {
    type Value = Alignment;
}

/// Attached property that controls vertical positioning of items inside a container.
#[derive(Copy, Clone, Debug)]
pub struct VerticalAlignment;

impl AttachedProperty for VerticalAlignment {
    type Value = Alignment;
}

#[derive(Copy, Clone, Debug)]
pub struct PaddingLeft;

impl AttachedProperty for PaddingLeft {
    type Value = LengthOrPercentage;
}

#[derive(Copy, Clone, Debug)]
pub struct PaddingRight;

impl AttachedProperty for PaddingRight {
    type Value = LengthOrPercentage;
}

#[derive(Copy, Clone, Debug)]
pub struct PaddingTop;

impl AttachedProperty for PaddingTop {
    type Value = LengthOrPercentage;
}

#[derive(Copy, Clone, Debug)]
pub struct PaddingBottom;

impl AttachedProperty for PaddingBottom {
    type Value = LengthOrPercentage;
}

// button.set(Margins, (FlexSize::NULL, FlexSize::stretch(1.0)));
// button.set(VerticalPosition, Center);

// Absolute positioning:
// button.set(HorizontalPosition, Positioning::Start(10.0));
// button.set(VerticalPosition, Positioning::Start(10.0));

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

impl LayoutOutput {
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
}

impl Default for LayoutOutput {
    fn default() -> Self {
        LayoutOutput::NULL
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

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct IntrinsicSizes {
    pub min: Size,
    pub max: Size,
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Geometry
////////////////////////////////////////////////////////////////////////////////////////////////////

/// Describes the size of an element and how it should be positioned inside a containing block.
#[derive(Copy, Clone, PartialEq)]
pub struct Geometry {
    /// Element size.
    ///
    /// Note that descendants can overflow and fall outside the bounds defined by `size`.
    /// Use `bounding_rect` to get the size of the element and its descendants combined.
    pub size: Size,

    /// Element baseline.
    pub baseline: Option<f64>,

    /// Bounding box of the content and its descendants. This includes the union of the bounding rectangles of all descendants, if the element allows overflowing content.
    pub bounding_rect: Rect,

    /// Paint bounds.
    ///
    /// This is the region that is dirtied when the content and its descendants needs to be repainted.
    /// It can be different from `bounding_rect` if the element has drawing effects that bleed outside the bounds used for hit-testing (e.g. drop shadows).
    pub paint_bounding_rect: Rect,
}

impl fmt::Debug for Geometry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // [ width x height, baseline:{}, padding=(t r b l), align=(x, y) ]

        write!(f, "[")?;
        write!(f, "{:?}", self.size)?;

        if let Some(baseline) = self.baseline {
            write!(f, ", baseline={}", baseline)?;
        }
        /*if self.padding.x0 != 0.0 || self.padding.x1 != 0.0 || self.padding.y0 != 0.0 || self.padding.y1 != 0.0 {
            write!(
                f,
                ", padding=({} {} {} {})",
                self.padding.x0, self.padding.y0, self.padding.x1, self.padding.y1,
            )?;
        }*/
        //write!(f, ", align=({:?} {:?})", self.x_align, self.y_align)?;
        write!(f, ", bounds={}", self.bounding_rect)?;
        write!(f, ", paint_bounds={}", self.paint_bounding_rect)?;
        write!(f, "]")?;
        Ok(())
    }
}

impl From<Size> for Geometry {
    fn from(value: Size) -> Self {
        Geometry::new(value)
    }
}

impl Geometry {
    /// Zero-sized geometry with no baseline.
    pub const ZERO: Geometry = Geometry::new(Size::ZERO);

    pub const fn new(size: Size) -> Geometry {
        Geometry {
            size,
            baseline: None,
            bounding_rect: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: size.width,
                y1: size.height,
            },
            paint_bounding_rect: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: size.width,
                y1: size.height,
            },
        }
    }

    /*/// Returns the size of the padding box.
    ///
    /// The padding box is the element box inflated by the padding.
    pub fn padding_box_size(&self) -> Size {
        (self.size.to_rect() + self.padding).size()
    }

    /// Baseline from the top of the padding box.
    pub fn padding_box_baseline(&self) -> Option<f64> {
        self.baseline.map(|y| y + self.padding.y0)
    }*/

    /*/// Places the content inside a containing box with the given measurements.
    ///
    /// If this box' vertical alignment is `FirstBaseline` or `LastBaseline`,
    /// it will be aligned to the baseline of the containing box.
    ///
    /// Returns the offset of the element box.
    pub fn place_into(&self, container_size: Size, container_baseline: Option<f64>) -> Vec2 {
        let pad = self.padding;
        //let bounds = container_size.to_rect() - pad;

        let x = match self.x_align {
            Alignment::Relative(x) => pad.x0 + x * (container_size.width - pad.x0 - pad.x1 - self.size.width),
            // TODO vertical baseline alignment
            _ => 0.0,
        };
        let y = match self.y_align {
            Alignment::Relative(x) => pad.y0 + x * (container_size.height - pad.y0 - pad.y1 - self.size.height),
            Alignment::FirstBaseline => {
                // align this box baseline to the containing box baseline
                let mut y = match (container_baseline, self.baseline) {
                    (Some(container_baseline), Some(content_baseline)) => {
                        // containing-box-baseline == y-offset + self-baseline
                        container_baseline - content_baseline
                    }
                    _ => {
                        // the containing box or this box have no baseline
                        0.0
                    }
                };

                // ensure sufficient padding, even if this means breaking the baseline alignment
                if y < pad.y0 {
                    y = pad.y0;
                }
                y
            }
            // TODO last baseline alignment
            _ => 0.0,
        };

        Vec2::new(x, y)
    }*/
}

impl Default for Geometry {
    fn default() -> Self {
        Geometry::ZERO
    }
}

/// Places the content inside a containing box with the given measurements.
///
/// If this box vertical alignment is `FirstBaseline` or `LastBaseline`,
/// it will be aligned to the baseline of the containing box.
///
/// Returns the offset of the element box.
pub fn place_child_box(
    size: Size,
    baseline: Option<f64>,
    container_size: Size,
    container_baseline: Option<f64>,
    x_align: Alignment,
    y_align: Alignment,
    pad: &Insets,
) -> Vec2 {
    let x = match x_align {
        Alignment::Relative(x) => pad.x0 + x * (container_size.width - pad.x0 - pad.x1 - size.width),
        // TODO vertical baseline alignment
        _ => 0.0,
    };
    let y = match y_align {
        Alignment::Relative(x) => pad.y0 + x * (container_size.height - pad.y0 - pad.y1 - size.height),
        Alignment::FirstBaseline => {
            // align this box baseline to the containing box baseline
            let mut y = match (container_baseline, baseline) {
                (Some(container_baseline), Some(content_baseline)) => {
                    // containing-box-baseline == y-offset + self-baseline
                    container_baseline - content_baseline
                }
                _ => {
                    // the containing box or this box have no baseline
                    0.0
                }
            };

            // ensure sufficient padding, even if this means breaking the baseline alignment
            if y < pad.y0 {
                y = pad.y0;
            }
            y
        }
        // TODO last baseline alignment
        _ => 0.0,
    };

    Vec2::new(x, y)
}
