//! Frame containers
use crate::drawing::{BoxShadow, Paint, ToSkia};
use crate::element::{
    Element, ElementAny, ElementBuilder, HitTestCtx, IntoElementAny, TreeCtx, 
};
use crate::element_state::ElementState;
use crate::elements::{ActivatedEvent, ClickedEvent, ElementStateChanged, HoveredEvent};
use crate::input_event::Event;
use crate::layout::{Axis, LayoutInput, LayoutOutput, SizeConstraint, SizeValue};
use crate::event::EventSource;
use crate::{drawing, Color, PaintCtx};
use kurbo::{Insets, Point, RoundedRect, Size, Vec2};
use skia_safe::PaintStyle;
use tracing::trace_span;

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

    content: Option<ElementAny>,
}

impl Frame {
    /// Creates a new `Frame` with the default styles.
    pub fn new() -> ElementBuilder<Self> {
        ElementBuilder::new(Frame {
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
            content: None,
        })
    }

    /// Specifies a closure to be called when the frame is clicked.
    #[must_use]
    #[track_caller]
    pub fn on_click(self: ElementBuilder<Self>, func: impl Fn() + 'static) -> ElementBuilder<Self> {
        self.subscribe::<ClickedEvent>(move |_| {
            func();
            true
        });
        self
    }

    /// Specifies a closure to be called when the frame is hovered.
    #[must_use]
    #[track_caller]
    pub fn on_hover(self: ElementBuilder<Self>, func: impl Fn() + 'static) -> ElementBuilder<Self> {
        self.subscribe::<HoveredEvent>(move |_| {
            func();
            false
        });
        self
    }

    /// Adds a child item to this frame.
    #[must_use]
    pub fn content(mut self: ElementBuilder<Self>, child: impl IntoElementAny) -> ElementBuilder<Self> {
        self.content = Some(child.into_element_any(self.weak().as_dyn()));
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

    /// Specifies the padding (along all four sides) around the content placed inside the frame.
    #[must_use]
    pub fn padding(mut self: ElementBuilder<Self>, value: f64) -> ElementBuilder<Self> {
        self.padding = Insets::uniform(value);
        self
    }

    /// Specifies the padding (along the right side) around the content placed inside the frame.
    #[must_use]
    pub fn padding_right(mut self: ElementBuilder<Self>, value: impl Into<f64>) -> ElementBuilder<Self> {
        self.padding.x1 = value.into();
        self
    }

    /// Specifies the padding (along the left side) around the content placed inside the frame.
    #[must_use]
    pub fn padding_left(mut self: ElementBuilder<Self>, value: impl Into<f64>) -> ElementBuilder<Self> {
        self.padding.x0 = value.into();
        self
    }

    /// Specifies the padding around the content placed inside the frame.
    #[must_use]
    pub fn padding_top(mut self: ElementBuilder<Self>, value: impl Into<f64>) -> ElementBuilder<Self> {
        self.padding.y0 = value.into();
        self
    }

    /// Specifies the padding around the content placed inside the frame.
    #[must_use]
    pub fn padding_bottom(mut self: ElementBuilder<Self>, value: impl Into<f64>) -> ElementBuilder<Self> {
        self.padding.y1 = value.into();
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

    /// Sets the background color.
    pub fn set_background_color(&mut self, cx: &TreeCtx, color: Color) {
        self.style.background_color = color;
        self.style_changed = true;
        cx.mark_needs_paint();
    }

    fn resolve_style(&mut self) {
        if self.style_changed {
            self.resolved_style = self.style.apply_overrides(self.state);
            self.style_changed = false;
            self.state_affects_style = self.style.affected_by_state();
        }
    }
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
        ctx: &TreeCtx,
        parent_width: Option<f64>,
        parent_height: Option<f64>,
        width_constraint: SizeConstraint,
        height_constraint: SizeConstraint,
    ) -> Size {
        let width = width_constraint.deflate(self.padding.x_value());
        let height = height_constraint.deflate(self.padding.y_value());

        let size = if let Some(content) = &self.content {
            content.measure(
                ctx,
                &LayoutInput {
                    parent_width,
                    parent_height,
                    width,
                    height,
                },
            )
        } else {
            Size::ZERO
        };

        size + self.padding.size()
    }

    /// Measures a box element sized according to the specified constraints.
    fn measure_inner(
        &self,
        ctx: &TreeCtx,
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
                    Some(
                        self.measure_content(ctx, parent_width, parent_height, cstr, height_constraint)
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
            self.measure_content(ctx, parent_width, parent_height, width_constraint, height_constraint)
                .width
        });
        let min_width = eval_width(self.min_width).unwrap_or(0.0);
        let max_width = eval_width(self.max_width).unwrap_or(f64::INFINITY);

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
                    Some(
                        self.measure_content(ctx, parent_width, parent_height, updated_width_constraint, cstr)
                            .height,
                    )
                }
                _ => None,
            }
        };

        let mut height = eval_height(self.height).unwrap_or_else(|| {
            self.measure_content(ctx, parent_width, parent_height, width_constraint, height_constraint)
                .height
        });
        let min_height = eval_height(self.min_height).unwrap_or(0.0);
        let max_height = eval_height(self.max_height).unwrap_or(f64::INFINITY);

        height = height.clamp(min_height, max_height);

        Size { width, height }
    }
}

impl Element for Frame {
    fn children(&self) -> Vec<ElementAny> {
        self.content.clone().into_iter().collect()
    }

    fn measure(&mut self, ctx: &TreeCtx, layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("Frame::measure").entered();
        // TODO vertical direction layout
        let output = self.measure_inner(
            ctx,
            layout_input.parent_width,
            layout_input.parent_height,
            layout_input.width,
            layout_input.height,
        );
        output
    }

    fn layout(&mut self, ctx: &TreeCtx, size: Size) -> LayoutOutput {
        let _span = trace_span!("Frame::layout").entered();
        let content_area = size - self.padding.size();

        let mut output = if let Some(ref content) = self.content {
            let output = content.layout(ctx, content_area);
            content.set_offset(Vec2::new(self.padding.x0, self.padding.y0));
            output
        } else {
            LayoutOutput::default()
        };

        output.width = size.width;
        output.height = size.height;
        output.baseline = output.baseline.map(|b| b + self.padding.y0);
        output
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        if let Some(content) = &self.content {
            content.hit_test(ctx, point);
        }
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ecx: &TreeCtx, ctx: &mut PaintCtx) {
        self.resolve_style();

        let rect = ecx.bounds();
        let s = &self.resolved_style;

        let border_radius = s.border_radius;
        // border shape
        let inner_shape = RoundedRect::from_rect(rect - s.border_size, border_radius - 0.5 * s.border_size.x_value());
        let outer_shape = RoundedRect::from_rect(rect, border_radius);

        let canvas = ctx.canvas();
        // draw drop shadows
        for shadow in &s.shadows {
            if !shadow.inset {
                drawing::draw_box_shadow(canvas, &outer_shape, shadow);
            }
        }

        // fill
        let paint = Paint::Color(s.background_color).to_sk_paint(PaintStyle::Fill);
        canvas.draw_rrect(inner_shape.to_skia(), &paint);

        // draw inset shadows
        for shadow in &s.shadows {
            if shadow.inset {
                drawing::draw_box_shadow(canvas, &inner_shape, shadow);
            }
        }

        // paint border
        if s.border_color.alpha() != 0.0 && s.border_size != Insets::ZERO {
            let paint = Paint::Color(s.border_color).to_sk_paint(PaintStyle::Fill);
            canvas.draw_drrect(outer_shape.to_skia(), inner_shape.to_skia(), &paint);
        }

        // paint children
        if let Some(content) = &self.content {
            ctx.paint_child(content);
            //content.paint(ecx, ctx);
        }
    }

    fn event(&mut self, cx: &TreeCtx, event: &mut Event) {
        fn update_state(this: &mut Frame, cx: &TreeCtx, state: ElementState) {
            this.state = state;
            cx.emit(ElementStateChanged(state));
            if this.state_affects_style {
                this.style_changed = true;
                cx.mark_needs_paint();
            }
        }

        match event {
            Event::PointerDown(_) => {
                self.state.set_active(true);
                update_state(self, cx, self.state);
                cx.emit(ActivatedEvent(true));
            }
            Event::PointerUp(_) => {
                if self.state.is_active() {
                    cx.emit(ActivatedEvent(false));
                    update_state(self, cx, self.state);
                    cx.emit(ClickedEvent);
                }
            }
            Event::PointerEnter(_) => {
                self.state.set_hovered(true);
                update_state(self, cx, self.state);
                cx.emit(HoveredEvent(true));
            }
            Event::PointerLeave(_) => {
                self.state.set_hovered(false);
                update_state(self, cx, self.state);
                cx.emit(HoveredEvent(false));
            }
            _ => {}
        }
    }
}
