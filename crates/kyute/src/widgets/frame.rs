//! Frame containers
use std::cell::{Cell, RefCell};
use std::ops::Deref;

use kurbo::{Insets, RoundedRect, Size, Vec2};
use smallvec::SmallVec;
use tracing::{trace, trace_span};
use crate::ElementState;
use crate::drawing::{BoxShadow, Paint, ToSkia};
use crate::element::{Element, Node, RcElement};
use crate::event::Event;
use crate::layout::flex::{flex_layout, FlexLayoutParams};
use crate::layout::{
    Axis, LayoutInput, LayoutMode, LayoutOutput, LengthOrPercentage, SizeConstraint, SizeValue,
};
use crate::{drawing, layout, register_template, Notifier, Color, PaintCtx};


#[derive(Clone, Default)]
pub struct FrameStyleOverride {
    pub state: ElementState,
    pub border_color: Option<Color>,
    pub border_radius: Option<LengthOrPercentage>,
    pub background_color: Option<Color>,
    pub shadows: Option<SmallVec<BoxShadow, 2>>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum FrameLayout {
    Flex {
        direction: Axis,
        /// Default gap between children.
        gap: SizeValue,
        /// Initial gap before the first child (padding).
        initial_gap: SizeValue,
        /// Final gap after the last child (padding).
        final_gap: SizeValue,
    },
}

impl Default for FrameLayout {
    fn default() -> Self {
        FrameLayout::Flex {
            direction: Axis::Vertical,
            gap: SizeValue::default(),
            initial_gap: SizeValue::default(),
            final_gap: SizeValue::default(),
        }
    }
}

#[derive(Clone, Default)]
pub struct FrameStyle {
    pub border_left: LengthOrPercentage,
    pub border_right: LengthOrPercentage,
    pub border_top: LengthOrPercentage,
    pub border_bottom: LengthOrPercentage,
    pub border_color: Color,
    pub border_radius: LengthOrPercentage,
    pub background_color: Color,
    pub shadows: SmallVec<BoxShadow, 2>,
    pub overrides: SmallVec<FrameStyleOverride, 2>,
}

impl FrameStyle {
    fn apply(&mut self, over: FrameStyleOverride) {
        self.border_color = over.border_color.unwrap_or(self.border_color);
        self.border_radius = over.border_radius.unwrap_or(self.border_radius);
        self.background_color = over.background_color.unwrap_or(self.background_color);
        self.shadows = over.shadows.unwrap_or(self.shadows.clone());
    }

    fn apply_overrides(&self, state: ElementState) -> FrameStyle {
        let mut result = self.clone();
        for over in &self.overrides {
            if state.contains(over.state) {
                result.apply(over.clone());
            }
        }
        result
    }

    fn affected_by_state(&self) -> bool {
        self.overrides.iter().any(|o| !o.state.is_empty())
    }
}


/// A container with a fixed width and height, into which a unique widget is placed.
pub struct Frame {
    node: Node,
    pub clicked: Notifier<()>,
    pub hovered: Notifier<bool>,
    pub active: Notifier<bool>,
    pub focused: Notifier<bool>,
    pub state_changed: Notifier<ElementState>,
    layout: RefCell<FrameLayout>,

    width: Cell<SizeValue>,
    height: Cell<SizeValue>,
    min_width: Cell<SizeValue>,
    min_height: Cell<SizeValue>,
    max_width: Cell<SizeValue>,
    max_height: Cell<SizeValue>,

    padding: Cell<Insets>,

    state: Cell<ElementState>,
    style: RefCell<FrameStyle>,
    style_changed: Cell<bool>,
    state_affects_style: Cell<bool>,
    resolved_style: RefCell<FrameStyle>,
}

impl Deref for Frame {
    type Target = Node;

    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

macro_rules! paint_style_setter {
    ($s:ident, $setter:ident: $ty:ty) => {
        pub fn $setter(&self, value: $ty) {
            self.style.borrow_mut().$s = value;
            self.style_changed.set(true);
            self.mark_needs_repaint();
        }
    };
}

macro_rules! layout_style_setter {
    ($s:ident, $p:pat, $setter:ident: $ty:ty) => {
        pub fn $setter(&self, value: $ty) {
            let $p = &mut *self.layout.borrow_mut();
            *$s = value;
            self.mark_needs_relayout();
        }
    };
}

/*
register_template!(Frame);

impl Default for Frame {
    fn default() -> Self {
        Frame::new()
    }
}*/

impl Frame {
    /// Creates a new `Frame` with the given decoration.
    pub fn new() -> RcElement<Frame> {
        Node::new_derived(|node| Frame {
            node,
            clicked: Default::default(),
            hovered: Default::default(),
            active: Default::default(),
            focused: Default::default(),
            state_changed: Default::default(),
            layout: Default::default(),
            width: Default::default(),
            height: Default::default(),
            min_width: Default::default(),
            min_height: Default::default(),
            max_width: Default::default(),
            max_height: Default::default(),
            padding: Default::default(),
            state: Default::default(),
            style: Default::default(),
            style_changed: Cell::new(true),
            state_affects_style: Cell::new(false),
            resolved_style: Default::default(),
        })
    }

    pub fn set_style(&self, style: FrameStyle) {
        self.style.replace(style);
        self.style_changed.set(true);
        self.mark_needs_relayout();
    }

    paint_style_setter!(border_left, set_border_left: LengthOrPercentage);
    paint_style_setter!(border_right, set_border_right: LengthOrPercentage);
    paint_style_setter!(border_top, set_border_top: LengthOrPercentage);
    paint_style_setter!(border_bottom, set_border_bottom: LengthOrPercentage);

    layout_style_setter!(direction, FrameLayout::Flex{direction, ..}, set_direction: Axis);
    layout_style_setter!(gap, FrameLayout::Flex{gap, ..}, set_gap: SizeValue);
    layout_style_setter!(initial_gap, FrameLayout::Flex{initial_gap, ..}, set_initial_gap: SizeValue);
    layout_style_setter!(final_gap, FrameLayout::Flex{final_gap, ..}, set_final_gap: SizeValue);

    pub fn set_padding(&self, value: f64) {
        self.padding.set(Insets::uniform(value));
        self.mark_needs_relayout();
    }

    pub fn set_padding_left(&self, value: f64) {
        let mut padding = self.padding.get();
        padding.x0 = value;
        self.padding.set(padding);
        self.mark_needs_relayout();
    }

    pub fn set_padding_right(&self, value: f64) {
        let mut padding = self.padding.get();
        padding.x1 = value;
        self.padding.set(padding);
        self.mark_needs_relayout();
    }

    pub fn set_padding_top(&self, value: f64) {
        let mut padding = self.padding.get();
        padding.y0 = value;
        self.padding.set(padding);
        self.mark_needs_relayout();
    }

    pub fn set_padding_bottom(&self, value: f64) {
        let mut padding = self.padding.get();
        padding.y1 = value;
        self.padding.set(padding);
        self.mark_needs_relayout();
    }

    pub fn set_content(&self, content: impl Into<RcElement>) {
        self.clear_children();
        (self as &dyn Element).add_child(content)
    }

    /*pub fn set_direction(&self, direction: Axis) {
        if let FrameLayout::Flex { direction: ref mut d, .. } = *self.layout.borrow_mut() {
            *d = direction;
            self.mark_needs_relayout();
        }
    }*/

    pub fn set_width(&self, value: SizeValue) {
        self.width.set(value);
        self.mark_needs_relayout();
    }

    pub fn set_height(&self, value: SizeValue) {
        self.height.set(value);
        self.mark_needs_relayout();
    }

    pub fn set_min_width(&self, value: SizeValue) {
        self.min_width.set(value);
        self.mark_needs_relayout();
    }

    pub fn set_min_height(&self, value: SizeValue) {
        self.min_height.set(value);
        self.mark_needs_relayout();
    }

    pub fn set_max_width(&self, value: SizeValue) {
        self.max_width.set(value);
        self.mark_needs_relayout();
    }

    pub fn set_max_height(&self, value: SizeValue) {
        self.max_height.set(value);
        self.mark_needs_relayout();
    }

    pub async fn clicked(&self) {
        self.clicked.wait().await;
    }

    fn calculate_style(&self) {
        if self.style_changed.get() {
            self.resolved_style
                .replace(self.style.borrow().apply_overrides(self.state.get()));
            self.style_changed.set(false);
        }
    }
}

struct BoxSizingParams<'a> {
    /// Main axis direction (direction of the text). For now, it's always horizontal.
    axis: Axis,
    children: &'a [RcElement],
}

impl Frame {
    /// Measures the contents of the frame under the specified constraints.
    ///
    /// The measurement includes padding.
    ///
    /// # Arguments
    ///
    /// * `p` - box sizing parameters (axis, padding, children)
    /// * `parent_main_sz` - parent main axis size, if known
    /// * `parent_cross_sz` - parent cross axis size, if known
    /// * `main` - main axis size constraint (available space)
    /// * `cross` - cross axis size constraint (available space)
    fn measure_content(
        &self,
        p: &BoxSizingParams,
        parent_width: Option<f64>,
        parent_height: Option<f64>,
        width_constraint: SizeConstraint,
        height_constraint: SizeConstraint,
    ) -> Size {
        let _span = trace_span!("Frame::measure_content", ?width_constraint, ?height_constraint, ?parent_width, ?parent_height).entered();

        let padding = self.padding.get();
        let width = width_constraint.deflate(padding.x_value());
        let height = height_constraint.deflate(padding.y_value());

        // Measure the children by performing the measure steps of flex layout.
        let FrameLayout::Flex {
            direction,
            gap,
            initial_gap,
            final_gap,
        } = self.layout.borrow().clone();

        let output = flex_layout(
            LayoutMode::Measure,
            &FlexLayoutParams {
                direction,
                width_constraint: width,
                height_constraint: height,
                parent_width,
                parent_height,
                gap,
                initial_gap,
                final_gap,
            },
            p.children,
        );

        Size {
            width: output.width + padding.x_value(),
            height: output.height + padding.y_value(),
        }
    }

    /// Measures a box element sized according to the specified constraints.
    fn measure_inner(
        &self,
        p: &BoxSizingParams,
        parent_width: Option<f64>,
        parent_height: Option<f64>,
        width_constraint: SizeConstraint,
        height_constraint: SizeConstraint,
    ) -> Size {
        let _span = trace_span!("Frame::measure_inner", ?width_constraint, ?height_constraint, ?parent_width, ?parent_height).entered();

        //
        let eval_width = |size: SizeValue| -> Option<f64> {
            match size {
                // Fixed size: use the specified size
                SizeValue::Fixed(s) => Some(s),
                // Percentage size: use the parent size
                SizeValue::Percentage(percent) => Some(parent_width? * percent),
                // MinContent or MaxContent: measure the content using a MIN or MAX constraint on the
                // specified axis
                SizeValue::MinContent | SizeValue::MaxContent => {
                    let cstr = match size {
                        SizeValue::MinContent => SizeConstraint::MIN,
                        SizeValue::MaxContent => SizeConstraint::MAX,
                        _ => unreachable!(),
                    };
                    Some(self.measure_content(p, parent_width, parent_height, cstr, height_constraint).width)
                }
                _ => None,
            }
        };

        //

        let mut width = eval_width(self.width.get()).unwrap_or_else(|| {
            // If the width is not specified, it is calculated from the contents, by propagating
            // the width constraint from above to the children.
            self.measure_content(p, parent_width, parent_height, width_constraint, height_constraint).width
        });
        let min_width = eval_width(self.min_width.get()).unwrap_or(0.0);
        let max_width = eval_width(self.max_width.get()).unwrap_or(f64::INFINITY);

        // Clamp the width to the specified min and max values.
        width = width.clamp(min_width, max_width);

        // updated width constraint due to clamping min/max width
        let updated_width_constraint = SizeConstraint::Available(width);

        let eval_height = |size: SizeValue| -> Option<f64> {
            match size {
                SizeValue::Fixed(s) => Some(s),
                SizeValue::Percentage(percent) => Some(parent_height? * percent),
                SizeValue::MinContent | SizeValue::MaxContent => {
                    let cstr = match size {
                        SizeValue::MinContent => SizeConstraint::MIN,
                        SizeValue::MaxContent => SizeConstraint::MAX,
                        _ => unreachable!(),
                    };
                    Some(self.measure_content(p, parent_width, parent_height, updated_width_constraint, cstr).height)
                }
                _ => None,
            }
        };

        let mut height = eval_height(self.height.get()).unwrap_or_else(|| {
            self.measure_content(p, parent_width, parent_height, width_constraint, height_constraint).height
        });
        let min_height = eval_height(self.min_height.get()).unwrap_or(0.0);
        let max_height = eval_height(self.max_height.get()).unwrap_or(f64::INFINITY);

        height = height.clamp(min_height, max_height);

        Size { width, height }
    }
}

impl Element for Frame {
    fn node(&self) -> &Node {
        &self.node
    }

    //fn property(&self, name: &str) -> Option<&dyn Any> {}

    fn set_property(&self, name: &str, value: &kyute_dsl::PropertyExpr) {
        /*match name {
            "width" => value.cast().map(|v| self.set_width(SizeValue::Fixed(v))),
            "height" => value.cast().map(|v| self.set_height(SizeValue::Fixed(v))),
            "min_width" => value.cast().map(|v| self.set_min_width(SizeValue::Fixed(v))),
            "min_height" => value.cast().map(|v| self.set_min_height(SizeValue::Fixed(v))),
            "max_width" => value.cast().map(|v| self.set_max_width(SizeValue::Fixed(v))),
            "max_height" => value.cast().map(|v| self.set_max_height(SizeValue::Fixed(v))),
            "padding" => value.cast().map(|v| self.set_padding(v)),
            "padding_left" => value.cast().map(|v| self.set_padding_left(v)),
            "padding_right" => value.cast().map(|v| self.set_padding_right(v)),
            "padding_top" => value.cast().map(|v| self.set_padding_top(v)),
            "padding_bottom" => value.cast().map(|v| self.set_padding_bottom(v)),
            "border_left" => value.cast().map(|v| self.set_border_left(LengthOrPercentage::Px(v))),
            "border_right" => value.cast().map(|v| self.set_border_right(LengthOrPercentage::Px(v))),
            "border_top" => value.cast().map(|v| self.set_border_top(LengthOrPercentage::Px(v))),
            "border_bottom" => value.cast().map(|v| self.set_border_bottom(LengthOrPercentage::Px(v))),
            "gap" => value.cast().map(|v| self.set_gap(FlexSize { size: v, flex: 1.0 })),
        };*/
    }

    fn measure(&self, children: &[RcElement], layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("Frame::measure").entered();

        // TODO vertical direction layout
        let p = BoxSizingParams {
            axis: Axis::Horizontal,
            children,
        };
        let output = self.measure_inner(
            &p,
            layout_input.parent_width,
            layout_input.parent_height,
            layout_input.width,
            layout_input.height,
        );
        output
    }

    fn layout(&self, children: &[RcElement], size: Size) -> LayoutOutput {
        let _span = trace_span!("Frame::layout").entered();

        let padding = self.padding.get();
        let content_area_width = size.width - padding.x_value();
        let content_area_height = size.height - padding.y_value();

        let FrameLayout::Flex {
            direction,
            gap,
            initial_gap,
            final_gap,
        } = self.layout.borrow().clone();

        let mut output = flex_layout(
            LayoutMode::Place,
            &FlexLayoutParams {
                direction,
                width_constraint: SizeConstraint::Available(content_area_width),
                height_constraint: SizeConstraint::Available(content_area_height),
                // TODO parent width is unknown, so we can't use it for percentage calculations
                parent_width: None,
                parent_height: None,
                gap,
                initial_gap,
                final_gap,
            },
            children,
        );

        output.width = size.width;
        output.height = size.height;
        output.baseline = output.baseline.map(|b| b + padding.y0);

        let offset = Vec2::new(padding.x0, padding.y0);
        for child in children.iter() {
            child.add_offset(offset);
        }
        output
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        self.calculate_style();

        let size = self.node.size();
        let rect = size.to_rect();
        let s = self.resolved_style.borrow();
        let insets = Insets::new(
            s.border_left.resolve(size.width),
            s.border_top.resolve(size.height),
            s.border_right.resolve(size.width),
            s.border_bottom.resolve(size.height),
        );
        let border_radius = s.border_radius.resolve(size.width);
        // border shape
        let inner_shape = RoundedRect::from_rect(rect - insets, border_radius - 0.5 * insets.x_value());
        let outer_shape = RoundedRect::from_rect(rect, border_radius);

        ctx.with_canvas(|canvas| {
            // draw drop shadows
            for shadow in &s.shadows {
                if !shadow.inset {
                    drawing::draw_box_shadow(canvas, &outer_shape, shadow);
                }
            }

            // fill
            let mut paint = Paint::Color(s.background_color).to_sk_paint(rect);
            paint.set_style(skia_safe::paint::Style::Fill);
            canvas.draw_rrect(inner_shape.to_skia(), &paint);

            // draw inset shadows
            for shadow in &s.shadows {
                if shadow.inset {
                    drawing::draw_box_shadow(canvas, &inner_shape, shadow);
                }
            }

            // paint border
            if s.border_color.alpha() != 0.0 {
                let mut paint = Paint::Color(s.border_color).to_sk_paint(rect);
                paint.set_style(skia_safe::paint::Style::Fill);
                canvas.draw_drrect(outer_shape.to_skia(), inner_shape.to_skia(), &paint);
            }
        });
    }

    fn event(&self, event: &mut Event) {
        fn update_state(this: &Frame, state: ElementState) {
            this.state.set(state);
            if this.state_affects_style.get() {
                this.style_changed.set(true);
                this.mark_needs_relayout();
            }
        }

        let mut state = self.state.get();
        match event {
            Event::PointerDown(_) => {
                state.set_active(true);
                update_state(self, state);
                self.active.invoke(true);
            }
            Event::PointerUp(_) => {
                if state.is_active() {
                    state.set_active(false);
                    update_state(self, state);
                    self.clicked.invoke(());
                }
            }
            Event::PointerEnter(_) => {
                state.set_hovered(true);
                update_state(self, state);
                self.hovered.invoke(true);
            }
            Event::PointerLeave(_) => {
                state.set_hovered(false);
                update_state(self, state);
                self.hovered.invoke(false);
            }
            _ => {}
        }
    }
}
