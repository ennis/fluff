//! Frame containers
use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use bitflags::bitflags;
use kurbo::{Insets, RoundedRect};
use smallvec::SmallVec;
use tracing::{trace, trace_span};

use crate::{Color, drawing, PaintCtx};
use crate::drawing::{BoxShadow, Paint, ToSkia};
use crate::element::{Element, ElementMethods};
use crate::event::Event;
use crate::handler::Handler;
use crate::layout::{
    FlexSize, LayoutInput, LayoutOutput, LengthOrPercentage, PaddingBottom, PaddingLeft, PaddingRight,
    PaddingTop,
};
use crate::layout::flex::{Axis, CrossAxisAlignment, do_flex_layout, FlexLayoutParams, MainAxisAlignment};

/*
#[derive(Clone, Default)]
pub struct ResolvedFrameStyle {
    baseline: Option<LengthOrPercentage>,
    border_color: Color,
    background_color: Color,
    shadows: Vec<BoxShadow>,
}*/

bitflags! {
    #[derive(Copy, Clone, Debug, Default)]
    pub struct InteractState: u8 {
        const ACTIVE = 0b0001;
        const HOVERED = 0b0010;
        const FOCUSED = 0b0100;
    }
}

impl InteractState {
    pub fn set_active(&mut self, active: bool) {
        self.set(InteractState::ACTIVE, active);
    }

    pub fn set_hovered(&mut self, hovered: bool) {
        self.set(InteractState::HOVERED, hovered);
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.set(InteractState::FOCUSED, focused);
    }

    pub fn is_active(&self) -> bool {
        self.contains(InteractState::ACTIVE)
    }

    pub fn is_hovered(&self) -> bool {
        self.contains(InteractState::HOVERED)
    }
    pub fn is_focused(&self) -> bool {
        self.contains(InteractState::FOCUSED)
    }
}

#[derive(Clone, Default)]
pub struct FrameStyleOverride {
    pub state: InteractState,
    pub border_color: Option<Color>,
    pub border_radius: Option<LengthOrPercentage>,
    pub background_color: Option<Color>,
    pub shadows: Option<SmallVec<BoxShadow, 2>>,
}

#[derive(Clone)]
pub enum FrameLayout {
    Flex {
        direction: Axis,
        main_axis_alignment: MainAxisAlignment,
        cross_axis_alignment: CrossAxisAlignment,
    }, // TODO grid
}

impl Default for FrameLayout {
    fn default() -> Self {
        FrameLayout::Flex {
            direction: Axis::Horizontal,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Start,
        }
    }
}

#[derive(Clone, Default)]
pub struct FrameStyle {
    //pub width: Option<Sizing>,
    //pub height: Option<Sizing>,
    //pub padding_left: LengthOrPercentage,
    //pub padding_right: LengthOrPercentage,
    //pub padding_top: LengthOrPercentage,
    //pub padding_bottom: LengthOrPercentage,
    //pub horizontal_align: Alignment,
    //pub vertical_align: Alignment,
    //pub min_width: Option<LengthOrPercentage>,
    //pub max_width: Option<LengthOrPercentage>,
    //pub min_height: Option<LengthOrPercentage>,
    //pub max_height: Option<LengthOrPercentage>,
    pub layout: FrameLayout,
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

    fn apply_overrides(&self, state: InteractState) -> FrameStyle {
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
    element: Element,
    pub clicked: Handler<()>,
    pub hovered: Handler<bool>,
    pub active: Handler<bool>,
    pub focused: Handler<bool>,
    pub state_changed: Handler<InteractState>,
    state: Cell<InteractState>,
    style: FrameStyle,
    style_changed: Cell<bool>,
    state_affects_style: Cell<bool>,
    resolved_style: RefCell<FrameStyle>,
}

impl Deref for Frame {
    type Target = Element;

    fn deref(&self) -> &Self::Target {
        &self.element
    }
}

impl Frame {
    /// Creates a new `Frame` with the given decoration.
    pub fn new(style: FrameStyle) -> Rc<Frame> {
        Element::new_derived(|element| Frame {
            element,
            clicked: Default::default(),
            hovered: Default::default(),
            active: Default::default(),
            focused: Default::default(),
            state_changed: Default::default(),
            state: Cell::new(Default::default()),
            style: style.clone(),
            style_changed: Cell::new(true),
            state_affects_style: Cell::new(false),
            resolved_style: RefCell::new(style.clone()),
        })
    }

    pub fn set_content(&self, content: &dyn ElementMethods) {
        (self as &dyn ElementMethods).add_child(content);
    }

    pub async fn clicked(&self) {
        self.clicked.wait().await;
    }

    fn calculate_style(&self) {
        if self.style_changed.get() {
            self.resolved_style
                .replace(self.style.apply_overrides(self.state.get()));
            self.style_changed.set(false);
        }
    }
}

/*
#[derive(Debug)]
struct FrameSizes {
    parent_min: f64,
    parent_max: f64,
    content_min: f64,
    content_max: f64,
    self_min: Option<f64>,
    self_max: Option<f64>,
    fixed: Option<Sizing>,
    padding_before: f64,
    padding_after: f64,
}

impl FrameSizes {
    fn compute_child_constraint(&self) -> (f64, f64) {
        assert!(self.parent_min <= self.parent_max);

        /*// sanity check
        if let (Some(ref mut min), Some(ref mut max)) = (self.self_min, self.self_max) {
            if *min > *max {
                warn!("min width is greater than max width");
                *min = *max;
            }
        }*/

        let padding = self.padding_before + self.padding_after;
        let mut min = self.self_min.unwrap_or(0.0).clamp(self.parent_min, self.parent_max);
        let mut max = self.self_max.unwrap_or(f64::INFINITY).clamp(self.parent_min, self.parent_max);

        // apply fixed width
        if let Some(fixed) = self.fixed {
            let w = match fixed {
                Sizing::Length(len) => len.resolve(self.parent_max),
                Sizing::MinContent => self.content_min + padding,
                Sizing::MaxContent => self.content_max + padding,
            };
            let w = w.clamp(min, max);
            min = w;
            max = w;
        }

        // deflate by padding
        min -= padding;
        max -= padding;
        min = min.max(0.0);
        max = max.max(0.0);
        (min, max)
    }

    fn compute_self_size(&self, child_len: f64) -> f64 {
        let mut size = child_len;
        let padding = self.padding_before + self.padding_after;
        if let Some(fixed) = self.fixed {
            size = match fixed {
                Sizing::Length(len) => len.resolve(self.parent_max),
                Sizing::MinContent => self.content_min + padding,
                Sizing::MaxContent => self.content_max + padding,
            };
        } else {
            size += padding;
        }
        // apply min and max width
        let min = self.self_min.unwrap_or(0.0).clamp(self.parent_min, self.parent_max);
        let max = self.self_max.unwrap_or(f64::INFINITY).clamp(self.parent_min, self.parent_max);
        size = size.clamp(min, max);
        size
    }
}

fn compute_intrinsic_sizes(direction: Axis, children: &[Rc<dyn ElementMethods>]) -> IntrinsicSizes {
    let mut isizes = IntrinsicSizes::default();
    for c in children.iter() {
        let s = c.intrinsic_sizes();
        match direction {
            Axis::Horizontal => {
                // horizontal layout
                // width is sum of all children
                // height is max of all children
                isizes.min.width += s.min.width;
                isizes.max.width += s.max.width;
                isizes.min.height = isizes.min.height.max(s.min.height);
                isizes.max.height = isizes.max.height.max(s.max.height);
            }
            Axis::Vertical => {
                // vertical layout
                // width is max of all children
                // height is sum of all children
                isizes.min.height += s.min.height;
                isizes.max.height += s.max.height;
                isizes.min.width = isizes.min.width.max(s.min.width);
                isizes.max.width = isizes.max.width.max(s.max.width);
            }
        }
    }
    isizes
}
*/

impl ElementMethods for Frame {
    fn element(&self) -> &Element {
        &self.element
    }

    fn measure(&self, children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> LayoutOutput {
        let _span = trace_span!(
            "Frame::measure",
        ).entered();
        // defer to layout for now
        self.layout(children, layout_input)
    }

    fn layout(&self, children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> LayoutOutput {
        let _span = trace_span!(
            "Frame::layout",
        ).entered();

        self.calculate_style();

        let s = self.resolved_style.borrow();

        // resolve padding
        let padding_left = layout_input.resolve_length(Axis::Horizontal, self.get(PaddingLeft).unwrap_or_default());
        let padding_right = layout_input.resolve_length(Axis::Horizontal, self.get(PaddingRight).unwrap_or_default());
        let padding_top = layout_input.resolve_length(Axis::Vertical, self.get(PaddingTop).unwrap_or_default());
        let padding_bottom = layout_input.resolve_length(Axis::Vertical, self.get(PaddingBottom).unwrap_or_default());

        let direction = match s.layout {
            FrameLayout::Flex { direction, .. } => direction,
        };

        // layout children
        // TODO other layouts
        let flex_params = FlexLayoutParams {
            axis: direction,
            width_constraint: layout_input.width_constraint.deflate(padding_left + padding_right),
            height_constraint: layout_input.height_constraint.deflate(padding_top + padding_bottom),
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_alignment: MainAxisAlignment::Center,
            gap: FlexSize::NULL,
            initial_gap: FlexSize::NULL,
            final_gap: FlexSize::NULL,
        };

        let mut layout_output = do_flex_layout(&flex_params, children);
        layout_output.width += padding_left + padding_right;
        layout_output.height += padding_top + padding_bottom;
        layout_output.baseline.as_mut().map(|b| *b += padding_top);
        layout_output
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        let size = self.element.size();
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

    async fn event(&self, event: &mut Event)
    where
        Self: Sized,
    {
        async fn update_state(this: &Frame, state: InteractState) {
            this.state.set(state);
            this.state_changed.emit(state).await;
            if this.state_affects_style.get() {
                this.style_changed.set(true);
                this.mark_needs_relayout();
            }
        }

        let mut state = self.state.get();
        match event {
            Event::PointerDown(_) => {
                state.set_active(true);
                update_state(self, state).await;
                self.active.emit(true).await;
            }
            Event::PointerUp(_) => {
                if state.is_active() {
                    state.set_active(false);
                    update_state(self, state).await;
                    self.clicked.emit(()).await;
                }
            }
            Event::PointerEnter(_) => {
                state.set_hovered(true);
                update_state(self, state).await;
                self.hovered.emit(true).await;
            }
            Event::PointerLeave(_) => {
                state.set_hovered(false);
                update_state(self, state).await;
                self.hovered.emit(false).await;
            }
            _ => {}
        }
    }
}
