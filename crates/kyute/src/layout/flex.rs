use std::rc::Rc;

use kurbo::{Point, Rect, Size, Vec2};
use tracing::trace;

use crate::element::{AttachedProperty, ElementMethods};
use crate::layout;
use crate::layout::{
    Alignment, BoxMeasurements, FlexMargins, FlexSize,
    LayoutInput, LayoutOutput, RequestedAxis, SizeConstraint,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Default)]
pub enum MainAxisAlignment {
    #[default]
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Default)]
pub enum CrossAxisAlignment {
    #[default]
    Start,
    End,
    Center,
    Stretch,
    Baseline,
}

pub struct FlexFactor;

impl AttachedProperty for FlexFactor {
    type Value = f64;
}

/*
////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct Flex {
    pub element: Element,
    pub axis: Axis,
    pub main_axis_alignment: MainAxisAlignment,
    pub cross_axis_alignment: CrossAxisAlignment,
}

impl Flex {
    pub fn new(axis: Axis) -> Rc<Flex> {
        Element::new_derived(|element| Flex {
            element,
            axis,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Start,
        })
    }

    pub fn row() -> Rc<Flex> {
        Flex::new(Axis::Horizontal)
    }

    pub fn column() -> Rc<Flex> {
        Flex::new(Axis::Vertical)
    }

    pub fn push(&self, item: &dyn Visual) {
        // FIXME yeah that's not very good looking
        (self as &dyn Visual).add_child(item);
    }

    pub fn push_flex(&self, item: &dyn Visual, flex: f64) {
        FlexFactor.set(item, flex);
        (self as &dyn Visual).add_child(item);
    }
}
*/

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone, Copy, Debug, PartialEq, Default)]
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

/*
impl Axis {
    fn constraints(
        &self,
        main_axis_min: f64,
        main_axis_max: f64,
        cross_axis_min: f64,
        cross_axis_max: f64,
    ) -> BoxConstraints {
        match self {
            Axis::Horizontal => BoxConstraints {
                min: Size {
                    width: main_axis_min,
                    height: cross_axis_min,
                },
                max: Size {
                    width: main_axis_max,
                    height: cross_axis_max,
                },
            },
            Axis::Vertical => BoxConstraints {
                min: Size {
                    width: cross_axis_min,
                    height: main_axis_min,
                },
                max: Size {
                    width: cross_axis_max,
                    height: main_axis_max,
                },
            },
        }
    }
}*/

/// Helper trait for main axis/cross axis sizes
trait AxisSizeHelper {
    fn main_length(&self, main_axis: Axis) -> f64;
    fn cross_length(&self, main_axis: Axis) -> f64;

    fn from_main_cross(main_axis: Axis, main: f64, cross: f64) -> Self;
}

impl AxisSizeHelper for Size {
    fn main_length(&self, main_axis: Axis) -> f64 {
        match main_axis {
            Axis::Horizontal => self.width,
            Axis::Vertical => self.height,
        }
    }

    fn cross_length(&self, main_axis: Axis) -> f64 {
        match main_axis {
            Axis::Horizontal => self.height,
            Axis::Vertical => self.width,
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

/*
fn main_cross_constraints(axis: Axis, min_main: f64, max_main: f64, min_cross: f64, max_cross: f64) -> BoxConstraints {
    match axis {
        Axis::Horizontal => BoxConstraints {
            min: Size {
                width: min_main,
                height: min_cross,
            },
            max: Size {
                width: max_main,
                height: max_cross,
            },
        },
        Axis::Vertical => BoxConstraints {
            min: Size {
                width: min_cross,
                height: min_main,
            },
            max: Size {
                width: max_cross,
                height: max_main,
            },
        },
    }
}*/

pub struct FlexLayoutParams {
    /// The direction of the flex
    pub axis: Axis,
    /// Sizing constraint in the horizontal direction.
    pub width_constraint: SizeConstraint,
    /// Sizing constraint in the vertical direction.
    pub height_constraint: SizeConstraint,
    /// Default gap between children.
    pub gap: FlexSize,
    /// Initial gap before the first child (padding).
    pub initial_gap: FlexSize,
    /// Final gap after the last child (padding).
    pub final_gap: FlexSize,
}

pub fn do_flex_layout(p: &FlexLayoutParams, children: &[Rc<dyn ElementMethods>]) -> LayoutOutput {
    let main_axis = p.axis;
    let cross_axis = main_axis.cross();
    let child_count = children.len();
    let (main_axis_sizing, cross_axis_sizing) = match main_axis {
        Axis::Horizontal => (p.width_constraint, p.height_constraint),
        Axis::Vertical => (p.height_constraint, p.width_constraint),
    };

    // ======
    // ====== Calculate the available space on the main axis ======
    // ======
    // If the parent provided an exact size, or available space, use that as the maximum size,
    // otherwise we can consider the maximum size to be infinite.
    // MinContent/MaxContent has meaning only for the sizing of children with content.
    let main_max = main_axis_sizing.available().unwrap_or(f64::INFINITY);

    // ======
    // ====== Measure children & margins along the main axis and calculate the sum of flex factors ======
    // ======

    let mut non_flex_main_total = 0.0; // total size of non-flex children + min size of spacers
    // main_axis_max - non_flex_main_total = remaining space available for growing flex children
    let mut flex_sum = 0.0; // sum of flex factors

    #[derive(Copy, Clone, Default)]
    struct ItemMeasure {
        size: f64,
        max: f64,
        flex: f64,
    }
    let mut main_measures = vec![ItemMeasure::default(); child_count]; // box measurements of children along the main axis
    let mut margins = vec![FlexSize::NULL; child_count + 1]; // margins between children

    // Set the initial and final gaps
    margins[0] = p.initial_gap;
    margins[child_count] = p.final_gap;

    trace!(
        "Before flex layout: width_constraint: {:?}, height_constraint: {:?}",
        p.width_constraint,
        p.height_constraint
    );

    // Measure each child along the main axis (flex factor, ideal and maximum sizes), including fixed spacing.
    for (i, child) in children.iter().enumerate() {

        // get child flex factor
        let flex = child.get(layout::FlexFactor).unwrap_or_default();

        // get the element's ideal size along the main axis, using the parent constraints for the size.
        let item_main = child.do_measure(&LayoutInput::main_cross(main_axis, main_axis_sizing, cross_axis_sizing)).size(main_axis);
        // if flex != 0, also measure the max width so that we know how much it can grow
        let max_item_main = if flex != 0.0 {
            child.do_measure(&LayoutInput::main_cross(main_axis, SizeConstraint::MAX, cross_axis_sizing)).size(main_axis)
        } else {
            0.0
        };

        non_flex_main_total += item_main;
        flex_sum += flex;

        main_measures[i] = ItemMeasure {
            size: item_main,
            max: max_item_main,
            flex,
        };

        // add margin contributions
        let (margin_before, margin_after) = child.get(FlexMargins).unwrap_or_default();
        margins[i] = margins[i].combine(margin_before);
        margins[i + 1] = margins[i + 1].combine(margin_after);
        non_flex_main_total += margins[i].size;
        flex_sum += margins[i].flex;
    }

    // don't forget to take into account the last margin
    non_flex_main_total += margins[child_count].size;
    flex_sum += margins[child_count].flex;

    trace!(
        "After first flex pass: non_flex_main_total: {}, flex_sum: {}",
        non_flex_main_total,
        flex_sum
    );

    // ======
    // ====== Grow children & margins according to their flex factors to fill any remaining space. ======
    // ======

    let remaining_main = main_max - non_flex_main_total;
    let mut main_size = non_flex_main_total; // Size of the container along the main axis

    // We can skip growth if:
    // - there aren't any flex items (flex_sum == 0)
    // - there isn't any remaining space (remaining_main <= 0), or the remaining space is infinite
    // TODO honor growth factors even if the remaining space is negative, to keep alignment
    if remaining_main > 0.0 && remaining_main.is_finite() && flex_sum > 0.0 {
        for i in 0..child_count {
            // grow margins with flex factors
            if margins[i].flex > 0.0 {
                let growth = (main_max - main_size) * margins[i].flex / flex_sum;
                margins[i].size += growth;
                flex_sum -= margins[i].flex;
                //remaining_main -= growth;
                main_size += growth;
                margins[i].flex = 0.0;
            }

            // grow children with flex factors
            //let size = child_layouts[i].size(main_axis);
            if main_measures[i].flex != 0.0 {
                let growth = (main_max - main_size) * main_measures[i].flex / flex_sum;
                main_measures[i].size += growth;
                flex_sum -= main_measures[i].flex;
                main_size += growth;
                //child_layouts[i] = layout;
                // TODO respect max_width in the measure
            }
        }

        // Grow the last spacer
        if margins[child_count].flex > 0.0 {
            // at this point spacer.flex should be equal to flex_sum
            let growth = main_max - main_size;
            margins[child_count].size += growth;
            main_size += growth;
            flex_sum -= margins[child_count].flex;
        }
    }

    trace!(
        "After second flex pass: main_total: {}, main_axis_max: {}, remaining flex = {} (should be zero)",
        main_size,
        main_max,
        flex_sum
    );

    // ======
    // ====== Layout children, and measure cross axis size + baselines ======
    // ======

    // Same as main_axis_max
    let cross_max = cross_axis_sizing.available().unwrap_or(f64::INFINITY);

    let mut max_child_cross_size: f64 = 0.0; // maximum cross size among children
    let mut max_baseline: f64 = 0.0; // max baseline position among children with baseline positioning
    let mut max_below_baseline: f64 = 0.0; // among children with baseline positioning, the maximum distance from the baseline to the bottom edge
    let mut child_layouts = vec![LayoutOutput::NULL; child_count];

    for (i, child) in children.iter().enumerate() {
        let sizing = match cross_axis {
            Axis::Horizontal => child.get(layout::Width).unwrap_or_default(),
            Axis::Vertical => child.get(layout::Height).unwrap_or_default(),
        };

        let item_cross = child.do_measure(&LayoutInput::main_cross(main_axis, main_measures[i].size.into(), cross_axis_sizing)).size(cross_axis);

        // perform final child layout, since we know the size in both axes
        let layout = child.do_layout(Size::from_main_cross(main_axis, main_measures[i].size, item_cross));

        max_child_cross_size = max_child_cross_size.max(item_cross);

        // calculate max_baseline & max_below_baseline contribution for items with baseline alignment
        let alignment = match cross_axis {
            Axis::Horizontal => child.get(layout::HorizontalAlignment).unwrap_or_default(),
            Axis::Vertical => child.get(layout::VerticalAlignment).unwrap_or_default(),
        };
        if alignment == Alignment::FirstBaseline {
            let baseline = layout.baseline.unwrap_or(0.0);
            max_baseline = max_baseline.max(baseline);
            max_below_baseline = max_below_baseline.max(item_cross - baseline);
        }
        child_layouts[i] = layout;
    }

    let mut non_flex_cross_size = max_child_cross_size.max(max_baseline + max_below_baseline);

    trace!(
        "After cross axis size determination: non_flex_cross_size: {}, max_baseline: {}, max_below_baseline: {}",
        non_flex_cross_size,
        max_baseline,
        max_below_baseline
    );

    // clamp it to max cross size
    let cross_size = non_flex_cross_size.min(cross_max);
    //let cross_slack = cross_axis_max - cross_size;

    // ======
    // ====== align children along the cross axis
    // ======
    let mut offset_main = margins[0].size;
    for (i, child) in children.iter().enumerate() {
        let cross_child_size = child_layouts[i].size(cross_axis);

        let alignment = match cross_axis {
            Axis::Horizontal => child.get(layout::HorizontalAlignment).unwrap_or_default(),
            Axis::Vertical => child.get(layout::VerticalAlignment).unwrap_or_default(),
        };

        let offset_cross = match alignment {
            Alignment::Relative(p) => p * (cross_size - cross_child_size),
            Alignment::FirstBaseline => {
                max_baseline - child_layouts[i].baseline.unwrap_or(0.0)
            }
            Alignment::LastBaseline => {
                // TODO last baseline
                0.0
            }
        };

        trace!("Child {}: size={}, offset_main = {}, offset_cross = {}", i, main_measures[i].size, offset_main, offset_cross);
        // set child offset
        match main_axis {
            Axis::Horizontal => {
                child.set_offset(Vec2::new(offset_main, offset_cross));
            }
            Axis::Vertical => {
                child.set_offset(Vec2::new(offset_cross, offset_main));
            }
        }
        offset_main += main_measures[i].size + margins[i + 1].size;
    }

    // TODO baseline may be wrong here
    LayoutOutput::from_main_cross_sizes(main_axis, main_size, cross_size, Some(max_baseline))
}
