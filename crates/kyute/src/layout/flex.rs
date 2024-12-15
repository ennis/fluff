use kurbo::{Size, Vec2};
use tracing::trace;

use crate::element::{AttachedProperty, RcElement};
use crate::layout;
use crate::layout::{Alignment, Axis, AxisSizeHelper, LayoutInput, LayoutMode, LayoutOutput, SizeConstraint, SizeValue, SpacingAfter, SpacingBefore};

/*
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Default)]
pub enum MainAxisAlignment {
    #[default]
    Start,
    End,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}*/

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

/// Size value with a flex growth factor.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
struct FlexSize {
    /// Minimum space.
    size: f64,
    /// Flex factor (0.0 means no stretching).
    flex: f64,
}


impl FlexSize {
    const NULL: FlexSize = FlexSize { size: 0.0, flex: 0.0 };

    /// Combines two flex sizes, e.g. two margins that collapse.
    fn combine(self, other: FlexSize) -> FlexSize {
        FlexSize {
            size: self.size.max(other.size),
            flex: self.flex.max(other.flex),
        }
    }

    fn grow(self, available: f64) -> f64 {
        if self.flex != 0.0 && available.is_finite() {
            self.size.max(available)
        } else {
            self.size
        }
    }
}

impl From<f64> for FlexSize {
    fn from(size: f64) -> Self {
        FlexSize { size, flex: 0.0 }
    }
}

impl From<SizeValue> for FlexSize {
    fn from(size: SizeValue) -> Self {
        match size {
            SizeValue::Fixed(size) => FlexSize { size, flex: 0.0 },
            SizeValue::Stretch => FlexSize { size: 0.0, flex: 1.0 },
            _ => FlexSize::NULL,
        }
    }
}


pub struct FlexLayoutParams {
    /// The direction of the main axis of the flex container (vertical or horizontal).
    pub direction: Axis,
    pub width_constraint: SizeConstraint,
    pub height_constraint: SizeConstraint,
    pub parent_width: Option<f64>,
    pub parent_height: Option<f64>,
    /// Default gap between children.
    pub gap: SizeValue,
    /// Initial gap before the first child (padding).
    pub initial_gap: SizeValue,
    /// Final gap after the last child (padding).
    pub final_gap: SizeValue,
}

pub fn flex_layout(mode: LayoutMode, p: &FlexLayoutParams, children: &[RcElement]) -> LayoutOutput {
    let main_axis = p.direction;
    let cross_axis = main_axis.cross();
    let child_count = children.len();

    let (main_size_constraint, cross_size_constraint, parent_main, parent_cross) =
        match p.direction {
            Axis::Horizontal => {
                (
                    p.width_constraint,
                    p.height_constraint,
                    p.parent_width,
                    p.parent_height,
                )
            }
            Axis::Vertical => {
                (
                    p.height_constraint,
                    p.width_constraint,
                    p.parent_height,
                    p.parent_width,
                )
            }
        };

    // ======
    // ====== Calculate the available space on the main axis ======
    // ======
    // If the parent provided an exact size, or available space, use that as the maximum size,
    // otherwise we can consider the maximum size to be infinite.
    // MinContent/MaxContent has meaning only for the sizing of children with content.
    let main_max = main_size_constraint.available().unwrap_or(f64::INFINITY);

    // ======
    // ====== Measure children & margins along the main axis and calculate the sum of flex factors ======
    // ======

    let mut non_flex_main_total = 0.0; // total size of non-flex children + min size of spacers
    // main_axis_max - non_flex_main_total = remaining space available for growing flex children
    let mut flex_sum = 0.0; // sum of flex factors

    #[derive(Copy, Clone, Default)]
    struct ItemMeasure {
        main: f64,
        cross: f64,
        max: f64,
        flex: f64,
    }
    let mut measures = vec![ItemMeasure::default(); child_count]; // box measurements of children along the main axis
    let mut margins = vec![FlexSize::NULL; child_count + 1]; // margins between children and start/end margins

    // Set the default gaps
    for i in 1..child_count {
        margins[i] = p.gap.into();
    }
    margins[0] = p.initial_gap.into();
    margins[child_count] = p.final_gap.into();

    // Measure each child along the main axis (flex factor, ideal and maximum sizes), including fixed spacing.
    for (i, child) in children.iter().enumerate() {
        // get child flex factor
        //let flex = child.get(layout::FlexFactor).unwrap_or_default();
        // get the element's ideal size along the main axis, using the parent constraints for the size.
        let (item_main, item_cross) = child
            .do_measure(&LayoutInput::from_logical(
                main_axis,
                main_size_constraint,
                cross_size_constraint,
                parent_main,
                parent_cross,
            ))
            .main_cross(main_axis);
        // also measure the max width so that we know how much it can grow
        let max_item_main = child
            .do_measure(&LayoutInput::from_logical(
                main_axis,
                SizeConstraint::MAX,
                cross_size_constraint,
                parent_main,
                parent_cross,
            ))
            .axis(main_axis);

        non_flex_main_total += item_main;
        flex_sum += /*flex*/ 0.0;   // TODO

        measures[i] = ItemMeasure {
            main: item_main,
            cross: item_cross,
            max: max_item_main,
            flex: 0.0,
        };

        // add margin contributions
        let margin_before = child.get(SpacingBefore).unwrap_or_default();
        let margin_after = child.get(SpacingAfter).unwrap_or_default();
        //let min_margin_before = child.get(SpacingAfter).unwrap_or_default();
        //let min_margin_after = child.get(SpacingAfter).unwrap_or_default();

        let margin_before = margin_before.into();
        let margin_after = margin_after.into();

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
            if measures[i].flex > 0.0 {
                let growth = (main_max - main_size) * measures[i].flex / flex_sum;
                if growth > 0.0 {
                    measures[i].main += growth;
                    // invalidate cross size as it may have changed due to more space being allocated on the main axis
                    measures[i].cross = -1.0;
                }
                flex_sum -= measures[i].flex;
                main_size += growth;
                //child_layouts[i] = layout;
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
    // ====== Measure cross axis size
    // ======

    // Same as main_axis_max
    let cross_max = cross_size_constraint.available().unwrap_or(f64::INFINITY);

    let mut max_child_cross_size: f64 = 0.0; // maximum cross size among children
    let mut max_baseline: f64 = 0.0; // max baseline position among children with baseline positioning
    let mut max_below_baseline: f64 = 0.0; // among children with baseline positioning, the maximum distance from the baseline to the bottom edge
    let mut child_layouts = vec![LayoutOutput::NULL; child_count];

    for (i, child) in children.iter().enumerate() {
        // re-measure item cross size if necessary
        if measures[i].cross < 0.0 {
            // NOTE: there's no guarantee that the child will return the grown main size.
            // For instance, it may resize itself only in discrete increments, and
            // even if the provided main size has grown, it may return the same main size.
            // Concrete example: text elements
            measures[i].cross = child
                .do_measure(&LayoutInput::from_logical(
                    main_axis,
                    measures[i].main.into(),
                    cross_size_constraint,
                    parent_main,
                    parent_cross,
                ))
                .axis(cross_axis);
        }

        max_child_cross_size = max_child_cross_size.max(measures[i].cross);

        // If baseline alignment is requested, we need to perform child layout to get the baseline,
        // and perform alignment to know the final cross size.
        let alignment = match cross_axis {
            Axis::Horizontal => child.get(layout::HorizontalAlignment).unwrap_or_default(),
            Axis::Vertical => child.get(layout::VerticalAlignment).unwrap_or_default(),
        };

        if alignment == Alignment::FirstBaseline {
            // calculate max_baseline & max_below_baseline contribution for items with baseline alignment
            let layout = child.do_layout(Size::from_main_cross(main_axis, measures[i].main, measures[i].cross));
            let baseline = layout.baseline.unwrap_or(0.0);
            max_baseline = max_baseline.max(baseline);
            max_below_baseline = max_below_baseline.max(measures[i].cross - baseline);
            child_layouts[i] = layout;
        }
    }

    let non_flex_cross_size = max_child_cross_size.max(max_baseline + max_below_baseline);
    // clamp it to max cross size
    let cross_size = non_flex_cross_size.min(cross_max);
    //let cross_slack = cross_axis_max - cross_size;

    // ======
    // ====== If we're only measuring, we can stop here
    // ======
    if mode == LayoutMode::Measure {
        return LayoutOutput::from_main_cross_sizes(main_axis, main_size, cross_size, Some(max_baseline));
    }

    // ======
    // ====== Layout children
    // ======
    for (i, child) in children.iter().enumerate() {
        // TODO don't layout again if we already have the layout (the child may be already laid out
        // due to baseline alignment)
        child_layouts[i] = child.do_layout(Size::from_main_cross(main_axis, measures[i].main, measures[i].cross));
    }

    trace!(
        "After cross axis size determination: non_flex_cross_size: {}, max_baseline: {}, max_below_baseline: {}",
        non_flex_cross_size,
        max_baseline,
        max_below_baseline
    );

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
            Alignment::FirstBaseline => max_baseline - child_layouts[i].baseline.unwrap_or(0.0),
            Alignment::LastBaseline => {
                // TODO last baseline
                0.0
            }
        };

        trace!(
            "Child {}: size={}, offset_main = {}, offset_cross = {}",
            i,
            measures[i].main,
            offset_main,
            offset_cross
        );
        // set child offset
        match main_axis {
            Axis::Horizontal => {
                child.set_offset(Vec2::new(offset_main, offset_cross));
            }
            Axis::Vertical => {
                child.set_offset(Vec2::new(offset_cross, offset_main));
            }
        }
        offset_main += measures[i].main + margins[i + 1].size;
    }

    // TODO baseline may be wrong here?
    LayoutOutput::from_main_cross_sizes(main_axis, main_size, cross_size, Some(max_baseline))
}
