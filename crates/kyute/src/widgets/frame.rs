//! Frame containers
use std::cell::{Cell, RefCell};
use std::ops::Deref;

use crate::drawing::{BoxShadow, Paint, ToSkia};
use crate::element::{Element, ElementAny, ElementBuilder, ElementCtx, ElementCtxAny, HitTestCtx, IntoElementAny, LayoutCtx, MeasureCtx, WindowCtx};
use crate::event::Event;
use crate::layout::flex::{flex_layout, FlexLayoutParams};
use crate::layout::{Axis, LayoutInput, LayoutMode, LayoutOutput, LengthOrPercentage, SizeConstraint, SizeValue};
use crate::{drawing, layout, register_template, Color, ElementState, Notifier, PaintCtx};
use kurbo::{Insets, Point, RoundedRect, Size, Vec2};
use smallvec::SmallVec;
use tracing::{trace, trace_span};
use crate::application::run_queued;

#[derive(Clone, Default)]
pub struct FrameStyleOverride {
    pub state: ElementState,
    pub border_color: Option<Color>,
    pub border_radius: Option<f64>,
    pub background_color: Option<Color>,
    pub shadows: Option<Vec<BoxShadow>>,
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
    pub border_size: Insets,
    pub border_color: Color,
    pub border_radius: f64,
    pub background_color: Color,
    pub shadows: Vec<BoxShadow>,
    pub overrides: Vec<FrameStyleOverride>,
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

/// A container with a fixed width and height, into which a widget is placed.
pub struct Frame {
    ctx: ElementCtx<Self>,
    pub clicked: Notifier<()>,
    pub hovered: Notifier<bool>,
    pub active: Notifier<bool>,
    pub focused: Notifier<bool>,
    pub state_changed: Notifier<ElementState>,
    layout: FrameLayout,

    width: SizeValue,
    height: SizeValue,
    min_width: SizeValue,
    min_height: SizeValue,
    max_width: SizeValue,
    max_height: SizeValue,
    padding: Insets,

    state: ElementState,
    style: FrameStyle,
    style_changed: bool,
    state_affects_style: bool,
    resolved_style: FrameStyle,

    children: Vec<ElementAny>,
}

impl Frame {
    /// Creates a new `Frame` with the default styles.
    pub fn new() -> ElementBuilder<Frame> {
        ElementBuilder::new(Frame {
            ctx: ElementCtx::new(),
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
            style_changed: true,
            state_affects_style: false,
            resolved_style: Default::default(),
            children: vec![],
        })
    }

    /// Adds a child item to this frame.
    #[must_use]
    pub fn child(mut self: ElementBuilder<Self>, child: impl IntoElementAny) -> ElementBuilder<Self> {
        let weak_self = self.weak_any();
        self.children.push(child.into_element(weak_self, 0));
        self
    }

    /// Sets the visual style of the frame.
    #[must_use]
    pub fn style(mut self: ElementBuilder<Self>, style: FrameStyle) -> ElementBuilder<Self> {
        self.style = style;
        self
    }

    /// Specifies the size of all four borders around the frame.
    #[must_use]
    pub fn border_width(mut self: ElementBuilder<Self>, width: f64) -> ElementBuilder<Self> {
        self.style.border_size = Insets::uniform(width);
        self
    }

    /// Specifies the border color.
    #[must_use]
    pub fn border_color(mut self: ElementBuilder<Self>, color: Color) -> ElementBuilder<Self> {
        self.style.border_color = color;
        self
    }

    /// Specifies the border radius.
    #[must_use]
    pub fn border_radius(mut self: ElementBuilder<Self>, radius: f64) -> ElementBuilder<Self> {
        self.style.border_radius = radius;
        self
    }

    /// Specifies the background color.
    #[must_use]
    pub fn background_color(mut self: ElementBuilder<Self>, color: Color) -> ElementBuilder<Self> {
        self.style.background_color = color;
        self
    }

    /// Specifies layout direction.
    #[must_use]
    pub fn direction(mut self: ElementBuilder<Self>, dir: Axis) -> ElementBuilder<Self> {
        let FrameLayout::Flex { ref mut direction, .. } = self.layout;
        *direction = dir;
        self
    }

    /// Specifies the gap between items in the layout direction.
    #[must_use]
    pub fn gap(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        let FrameLayout::Flex { ref mut gap, .. } = self.layout;
        *gap = value.into();
        self
    }

    /// Specifies the initial gap before the first item in the layout direction.
    #[must_use]
    pub fn initial_gap(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        let FrameLayout::Flex {
            ref mut initial_gap, ..
        } = self.layout;
        *initial_gap = value.into();
        self
    }

    /// Specifies the final gap after the last item in the layout direction.
    #[must_use]
    pub fn final_gap(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        let FrameLayout::Flex { ref mut final_gap, .. } = self.layout;
        *final_gap = value.into();
        self
    }

    /// Specifies the padding (along all four sides) around the content placed inside the frame.
    #[must_use]
    pub fn padding(mut self: ElementBuilder<Self>, value: f64) -> ElementBuilder<Self> {
        self.padding = Insets::uniform(value);
        self
    }

    /// Specifies the width of the frame.
    #[must_use]
    pub fn width(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.width = value.into();
        self
    }

    /// Specifies the height of the frame.
    #[must_use]
    pub fn height(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.height = value.into();
        self
    }

    /// Specifies the minimum width of the frame.
    #[must_use]
    pub fn min_width(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.min_width = value.into();
        self
    }

    /// Specifies the minimum height of the frame.
    #[must_use]
    pub fn min_height(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.min_height = value.into();
        self
    }

    /// Specifies the maximum width of the frame.
    #[must_use]
    pub fn max_width(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.max_width = value.into();
        self
    }

    /// Specifies the maximum height of the frame.
    #[must_use]
    pub fn max_height(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.max_height = value.into();
        self
    }

    //layout_style_setter!(direction, FrameLayout::Flex{direction, ..}, set_direction: Axis);
    //layout_style_setter!(gap, FrameLayout::Flex{gap, ..}, set_gap: SizeValue);
    //layout_style_setter!(initial_gap, FrameLayout::Flex{initial_gap, ..}, set_initial_gap: SizeValue);
    //layout_style_setter!(final_gap, FrameLayout::Flex{final_gap, ..}, set_final_gap: SizeValue);

    /*pub fn set_padding(&self, value: f64) {
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
    }*/

    //pub fn set_content(&self, content: impl Into<RcElement>) {
    //    self.clear_children();
    //    (self as &dyn Element).add_child(content)
    //}

    /*pub fn set_direction(&self, direction: Axis) {
        if let FrameLayout::Flex { direction: ref mut d, .. } = *self.layout.borrow_mut() {
            *d = direction;
            self.mark_needs_relayout();
        }
    }*/

    //pub fn set_width(&self, value: SizeValue) {
    //    self.width.set(value);
    //    self.mark_needs_relayout();
    //}
    //
    //pub fn set_height(&self, value: SizeValue) {
    //    self.height.set(value);
    //    self.mark_needs_relayout();
    //}
    //
    //pub fn set_min_width(&self, value: SizeValue) {
    //    self.min_width.set(value);
    //    self.mark_needs_relayout();
    //}
    //
    //pub fn set_min_height(&self, value: SizeValue) {
    //    self.min_height.set(value);
    //    self.mark_needs_relayout();
    //}
    //
    //pub fn set_max_width(&self, value: SizeValue) {
    //    self.max_width.set(value);
    //    self.mark_needs_relayout();
    //}
    //
    //pub fn set_max_height(&self, value: SizeValue) {
    //    self.max_height.set(value);
    //    self.mark_needs_relayout();
    //}

    //fn calculate_style(&self) {
    //    if self.style_changed.get() {
    //        self.resolved_style
    //            .replace(self.style.borrow().apply_overrides(self.state.get()));
    //        self.style_changed.set(false);
    //    }
    //}
}

struct BoxSizingParams<'a> {
    /// Main axis direction (direction of the text). For now, it's always horizontal.
    axis: Axis,
    children: &'a [ElementAny],
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
        let _span = trace_span!(
            "Frame::measure_content",
            ?width_constraint,
            ?height_constraint,
            ?parent_width,
            ?parent_height
        )
            .entered();

        let width = width_constraint.deflate(self.padding.x_value());
        let height = height_constraint.deflate(self.padding.y_value());

        // Measure the children by performing the measure steps of flex layout.
        let FrameLayout::Flex {
            direction,
            gap,
            initial_gap,
            final_gap,
        } = self.layout.clone();

        let output = flex_layout(LayoutMode::Measure, &FlexLayoutParams {
            direction,
            width_constraint: width,
            height_constraint: height,
            parent_width,
            parent_height,
            gap,
            initial_gap,
            final_gap,
        }, p.children);

        Size {
            width: output.width + self.padding.x_value(),
            height: output.height + self.padding.y_value(),
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
        let _span = trace_span!(
            "Frame::measure_inner",
            ?width_constraint,
            ?height_constraint,
            ?parent_width,
            ?parent_height
        )
            .entered();

        //
        let mut eval_width = |size: SizeValue| -> Option<f64> {
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
                    Some(
                        self.measure_content(p, parent_width, parent_height, cstr, height_constraint)
                            .width,
                    )
                }
                _ => None,
            }
        };

        //

        let mut width = eval_width(self.width).unwrap_or_else(|| {
            // If the width is not specified, it is calculated from the contents, by propagating
            // the width constraint from above to the children.
            self.measure_content(p, parent_width, parent_height, width_constraint, height_constraint)
                .width
        });
        let min_width = eval_width(self.min_width).unwrap_or(0.0);
        let max_width = eval_width(self.max_width).unwrap_or(f64::INFINITY);

        // Clamp the width to the specified min and max values.
        width = width.clamp(min_width, max_width);

        // updated width constraint due to clamping min/max width
        let updated_width_constraint = SizeConstraint::Available(width);

        let mut eval_height = |size: SizeValue| -> Option<f64> {
            match size {
                SizeValue::Fixed(s) => Some(s),
                SizeValue::Percentage(percent) => Some(parent_height? * percent),
                SizeValue::MinContent | SizeValue::MaxContent => {
                    let cstr = match size {
                        SizeValue::MinContent => SizeConstraint::MIN,
                        SizeValue::MaxContent => SizeConstraint::MAX,
                        _ => unreachable!(),
                    };
                    Some(
                        self.measure_content(p, parent_width, parent_height, updated_width_constraint, cstr)
                            .height,
                    )
                }
                _ => None,
            }
        };

        let mut height = eval_height(self.height).unwrap_or_else(|| {
            self.measure_content(p, parent_width, parent_height, width_constraint, height_constraint)
                .height
        });
        let min_height = eval_height(self.min_height).unwrap_or(0.0);
        let max_height = eval_height(self.max_height).unwrap_or(f64::INFINITY);

        height = height.clamp(min_height, max_height);

        Size { width, height }
    }
}

impl Element for Frame {
    fn ctx(&self) -> &ElementCtxAny {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut ElementCtxAny {
        &mut self.ctx
    }

    fn children(&self) -> Vec<ElementAny> {
        self.children.clone()
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("Frame::measure").entered();

        // TODO vertical direction layout
        let p = BoxSizingParams {
            axis: Axis::Horizontal,
            children: &self.children[..],
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

    fn layout(&mut self, size: Size) -> LayoutOutput {
        let _span = trace_span!("Frame::layout").entered();

        let padding = self.padding;
        let content_area_width = size.width - padding.x_value();
        let content_area_height = size.height - padding.y_value();

        let FrameLayout::Flex {
            direction,
            gap,
            initial_gap,
            final_gap,
        } = self.layout;

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
            &self.children[..],
        );

        output.width = size.width;
        output.height = size.height;
        output.baseline = output.baseline.map(|b| b + padding.y0);

        let offset = Vec2::new(padding.x0, padding.y0);
        for child in self.children.iter() {
            child.add_offset(offset);
        }
        output
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        todo!()
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        //self.calculate_style();

        let rect = ctx.size.to_rect();
        let s = self.resolved_style.clone();

        let border_radius = s.border_radius;
        // border shape
        let inner_shape = RoundedRect::from_rect(rect - s.border_size, border_radius - 0.5 * s.border_size.x_value());
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
            if s.border_color.alpha() != 0.0 && s.border_size != Insets::ZERO {
                let mut paint = Paint::Color(s.border_color).to_sk_paint(rect);
                paint.set_style(skia_safe::paint::Style::Fill);
                canvas.draw_drrect(outer_shape.to_skia(), inner_shape.to_skia(), &paint);
            }
        });

        // paint children
        for child in self.children.iter() {
            child.paint(ctx);
        }
    }

    fn event(&mut self, ctx: &mut WindowCtx, event: &mut Event) {
        fn update_state(this: &mut Frame, ctx: &mut WindowCtx, state: ElementState) {
            this.state = state;
            this.state_changed.invoke(state);
            if this.state_affects_style {
                this.style_changed = true;
                this.ctx.mark_needs_layout();
            }
        }

        match event {
            Event::PointerDown(_) => {
                self.state.set_active(true);
                update_state(self, ctx, self.state);
                self.active.invoke(true);
            }
            Event::PointerUp(_) => {
                if self.state.is_active() {
                    self.state.set_active(false);
                    update_state(self, ctx, self.state);
                    self.clicked.invoke(());
                }
            }
            Event::PointerEnter(_) => {
                self.state.set_hovered(true);
                update_state(self, ctx, self.state);
                self.hovered.invoke(true);
            }
            Event::PointerLeave(_) => {
                self.state.set_hovered(false);
                update_state(self, ctx, self.state);
                self.hovered.invoke(false);
            }
            _ => {}
        }
    }
}
