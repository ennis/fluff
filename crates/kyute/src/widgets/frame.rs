//! Frame containers
use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use bitflags::bitflags;
use kurbo::{Insets, RoundedRect, Size};
use smallvec::SmallVec;
use tracing::{trace, trace_span};

use crate::drawing::{BoxShadow, Paint, ToSkia};
use crate::element::{Element, ElementMethods};
use crate::event::Event;
use crate::handler::Handler;
use crate::layout::flex::{do_flex_layout, Axis, CrossAxisAlignment, FlexLayoutParams, MainAxisAlignment};
use crate::layout::{
    FlexSize, LayoutInput, LayoutOutput, LengthOrPercentage, PaddingBottom, PaddingLeft, PaddingRight, PaddingTop,
    SizeConstraint, SizeValue, Sizing,
};
use crate::{drawing, layout, Color, PaintCtx};

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

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FrameLayout {
    Flex {
        direction: Axis,
        /// Default gap between children.
        gap: FlexSize,
        /// Initial gap before the first child (padding).
        initial_gap: FlexSize,
        /// Final gap after the last child (padding).
        final_gap: FlexSize,
    },
}

impl Default for FrameLayout {
    fn default() -> Self {
        FrameLayout::Flex {
            direction: Axis::Vertical,
            gap: FlexSize::NULL,
            initial_gap: FlexSize::NULL,
            final_gap: FlexSize::NULL,
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

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum LayoutCacheEntry {
    // Width and height specified and finite
    FullySpecified = 0,
    // Width=inf, height whatever
    WidthInfinite,
    Count,
}

/// Layout cache.
#[derive(Default)]
struct LayoutCache {
    entries: [Option<(LayoutInput, LayoutOutput)>; LayoutCacheEntry::Count as usize],
}

impl LayoutCache {
    fn get_or_insert_with(
        &mut self,
        layout_input: &LayoutInput,
        f: impl FnOnce(&LayoutInput) -> LayoutOutput,
    ) -> LayoutOutput {
        let entry_index = match layout_input {
            LayoutInput {
                width: SizeConstraint::Available(w),
                height: SizeConstraint::Available(h),
            } if w.is_finite() && h.is_finite() => LayoutCacheEntry::FullySpecified,
            LayoutInput {
                width: SizeConstraint::Available(w),
                ..
            } if *w == f64::INFINITY => LayoutCacheEntry::WidthInfinite,
            _ => LayoutCacheEntry::Count,
        };

        if entry_index < LayoutCacheEntry::Count {
            if let Some(entry) = self.entries[entry_index as usize] {
                if entry.0 == *layout_input {
                    trace!("using cached layout for entry {entry:?}: {layout_input:?}");
                    return entry.1;
                }
            }
            let output = f(layout_input);
            self.entries[entry_index as usize] = Some((*layout_input, output));
            output
        } else {
            f(layout_input)
        }
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
    layout: RefCell<FrameLayout>,
    layout_cache: RefCell<LayoutCache>,
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
            layout: RefCell::new(Default::default()),
            layout_cache: RefCell::new(Default::default()),
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

    pub fn set_layout(&self, layout: FrameLayout) {
        *self.layout.borrow_mut() = layout;
        self.mark_needs_relayout();
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

impl Frame {
    fn measure_content(
        &self,
        children: &[Rc<dyn ElementMethods>],
        axis: Axis,
        main: SizeConstraint,
        cross: SizeConstraint,
    ) -> LayoutOutput {
        let layout_input = LayoutInput::from_main_cross(axis, main, cross);
        self.layout_cache.borrow_mut().get_or_insert_with(&layout_input, |li| {
            let _span = trace_span!("Flex::measure_content", ?layout_input).entered();

            // resolve padding
            let padding_left = layout_input
                .width
                .resolve_length(self.get(PaddingLeft).unwrap_or_default());
            let padding_right = layout_input
                .width
                .resolve_length(self.get(PaddingRight).unwrap_or_default());
            let padding_top = layout_input
                .height
                .resolve_length(self.get(PaddingTop).unwrap_or_default());
            let padding_bottom = layout_input
                .height
                .resolve_length(self.get(PaddingBottom).unwrap_or_default());

            let FrameLayout::Flex {
                direction,
                gap,
                initial_gap,
                final_gap,
            } = self.layout.borrow().clone();

            // layout children
            // TODO other layouts
            let flex_params = FlexLayoutParams {
                axis: direction,
                width: layout_input.width.deflate(padding_left + padding_right),
                height: layout_input.height.deflate(padding_top + padding_bottom),
                gap,
                initial_gap,
                final_gap,
            };

            let mut layout_output = do_flex_layout(&flex_params, children);
            layout_output.width += padding_left + padding_right;
            layout_output.height += padding_top + padding_bottom;
            layout_output.baseline.as_mut().map(|b| *b += padding_top);
            layout_output
        })
    }

    /// Measures a box element sized according to the specified constraints.
    fn measure_inner(
        &self,
        children: &[Rc<dyn ElementMethods>],
        main_axis: Axis,
        main_sz: Sizing,
        cross_sz: Sizing,
        parent_main_sc: SizeConstraint,
        parent_cross_sc: SizeConstraint,
    ) -> LayoutOutput {
        let _span = trace_span!("measure_axis", ?main_axis).entered();
        let cross_axis = main_axis.cross();

        fn sizing_to_constraint(parent: SizeConstraint, sizing: SizeValue) -> SizeConstraint {
            match sizing {
                SizeValue::Auto => parent,
                SizeValue::Fixed(value) => SizeConstraint::Available(value),
                SizeValue::Percentage(p) => SizeConstraint::Available(parent.available().map(|s| p * s).unwrap_or(0.0)),
                SizeValue::MinContent => SizeConstraint::MIN,
                SizeValue::MaxContent => SizeConstraint::MAX,
            }
        }

        let content_main_sc = sizing_to_constraint(parent_main_sc, main_sz.preferred);
        let content_cross_sc = sizing_to_constraint(parent_cross_sc, cross_sz.preferred);

        // Note that if main_sz and cross_sz are both fixed or percentage, the size is already
        // fully determine and we don't need to measure it; however we do need to call `measure_content`
        // to get the baseline.

        let mut layout = self.measure_content(children, main_axis, content_main_sc, content_cross_sc);

        // (main, cross) now holds the content box size

        // it now needs to be constrained; do it one axis at a time, starting from the main axis
        let eval_size = |size: SizeValue,
                         axis: Axis,
                         parent_main_sc: SizeConstraint,
                         parent_cross_sc: SizeConstraint,
                         default: f64|
                         -> f64 {
            match size {
                SizeValue::Auto => default,
                SizeValue::Fixed(s) => s,
                SizeValue::Percentage(p) => parent_main_sc.available().map(|s| p * s).unwrap_or(default),
                SizeValue::MinContent => self
                    .measure_content(children, axis, SizeConstraint::MIN, parent_cross_sc)
                    .size(axis),
                SizeValue::MaxContent => self
                    .measure_content(children, axis, SizeConstraint::MAX, parent_cross_sc)
                    .size(axis),
            }
        };

        let main_min = eval_size(main_sz.min, main_axis, parent_main_sc, parent_cross_sc, 0.0);
        let main_max = eval_size(main_sz.max, main_axis, parent_main_sc, parent_cross_sc, f64::INFINITY);
        let cross_min = eval_size(cross_sz.min, cross_axis, parent_cross_sc, parent_main_sc, 0.0);
        let cross_max = eval_size(cross_sz.max, cross_axis, parent_cross_sc, parent_main_sc, f64::INFINITY);

        // clamp main axis size first
        let main = layout.size(main_axis);
        let clamped_main = main.clamp(main_min, main_max);
        if clamped_main != main {
            // re-measure cross axis under new main axis constraints
            layout = self.measure_content(
                children,
                main_axis,
                SizeConstraint::Available(clamped_main),
                parent_cross_sc,
            );
        }

        // clamp cross axis size
        // we don't ask for a re-measure, because this might in turn affect the main axis size,
        // and we don't want to loop indefinitely, this isn't a fixed-point solver
        let clamped_cross = layout.size(cross_axis).clamp(cross_min, cross_max);
        layout.set_axis(cross_axis, clamped_cross);

        trace!("Measured element: size={:?}", layout);
        layout
    }
}

impl ElementMethods for Frame {
    fn element(&self) -> &Element {
        &self.element
    }

    fn measure(&self, children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> LayoutOutput {
        let (main_constraint, cross_constraint) = layout_input.main_cross(Axis::Horizontal);
        let size = self.measure_inner(
            children,
            Axis::Horizontal,
            self.get(layout::Width).unwrap_or_default(),
            self.get(layout::Height).unwrap_or_default(),
            main_constraint,
            cross_constraint,
        );
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    fn layout(&self, children: &[Rc<dyn ElementMethods>], size: Size) -> LayoutOutput {
        let _span = trace_span!("Frame::layout").entered();

        // defer to measure for now
        let output = self.measure(
            children,
            &LayoutInput {
                width: size.width.into(),
                height: size.height.into(),
            },
        );
        output
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
