//! Frame containers
use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::pin::Pin;
use std::rc::Rc;

use bitflags::bitflags;
use kurbo::{Insets, RoundedRect, Size, Vec2};
use smallvec::SmallVec;
use tracing::{trace, trace_span};

use crate::drawing::{BoxShadow, Paint, ToSkia};
use crate::element::{Element, Node, RcElement};
use crate::event::Event;
use crate::handler::Handler;
use crate::layout::flex::{flex_layout, CrossAxisAlignment, FlexLayoutParams, MainAxisAlignment};
use crate::layout::{
    Axis, FlexSize, LayoutInput, LayoutMode, LayoutOutput, LengthOrPercentage, PaddingBottom, PaddingLeft,
    PaddingRight, PaddingTop, SizeConstraint, SizeValue, Sizing,
};
use crate::{drawing, layout, Callbacks, Color, PaintCtx};

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
    fn get_cached(&self, entry: LayoutCacheEntry, input: &LayoutInput) -> Option<LayoutOutput> {
        let Some((cached_input, cached_output)) = self.entries[entry as usize] else {
            // no entry in cache
            return None;
        };

        if cached_input == *input {
            // exact match
            return Some(cached_output);
        }

        let (
            SizeConstraint::Available(w),
            SizeConstraint::Available(h),
            SizeConstraint::Available(cached_w),
            SizeConstraint::Available(cached_h),
        ) = (input.width, input.height, cached_input.width, cached_input.height)
        else {
            return None;
        };

        // if we returned a box of size w x h for request W1 x H1, and now we're asked for W2 x H2,
        // with w < W2 < W1  and h < H2 < H1, we can still use the cached layout
        // (the new box is smaller but the previous result still fits inside)
        // We can't do that if the new request is larger than the previous one, because given a larger
        // request the element might choose to layout itself differently.

        if cached_output.width <= w && w <= cached_w && cached_output.height <= h && h <= cached_h {
            return Some(cached_output);
        }

        // No match
        return None;
    }

    fn get_or_insert_with(
        &mut self,
        layout_input: &LayoutInput,
        mode: LayoutMode,
        f: impl FnOnce(&LayoutInput) -> LayoutOutput,
    ) -> LayoutOutput {
        if mode == LayoutMode::Place {
            return f(layout_input);
        }

        let entry_index = match layout_input {
            LayoutInput {
                width: SizeConstraint::Available(w),
                height: SizeConstraint::Available(h),
                ..
            } if w.is_finite() && h.is_finite() => LayoutCacheEntry::FullySpecified,
            LayoutInput {
                width: SizeConstraint::Available(w),
                ..
            } if *w == f64::INFINITY => LayoutCacheEntry::WidthInfinite,
            _ => LayoutCacheEntry::Count,
        };

        if entry_index < LayoutCacheEntry::Count {
            if let Some(layout) = self.get_cached(entry_index, layout_input) {
                trace!("using cached layout for entry {entry_index:?}: {layout:?}");
                return layout;
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
    node: Node,
    pub clicked: Callbacks<()>,
    pub hovered: Callbacks<bool>,
    pub active: Callbacks<bool>,
    pub focused: Callbacks<bool>,
    pub state_changed: Callbacks<InteractState>,
    layout: RefCell<FrameLayout>,
    layout_cache: RefCell<LayoutCache>,
    state: Cell<InteractState>,
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
            if let $p = &mut *self.layout.borrow_mut() {
                *$s = value;
                self.mark_needs_relayout();
            }
        }
    };
}

impl Frame {
    /// Creates a new `Frame` with the given decoration.
    pub fn new() -> RcElement<Frame> {
        Node::new_derived(
            |node| Frame {
                node,
                clicked: Default::default(),
                hovered: Default::default(),
                active: Default::default(),
                focused: Default::default(),
                state_changed: Default::default(),
                layout: Default::default(),
                layout_cache: Default::default(),
                state: Default::default(),
                style: Default::default(),
                style_changed: Cell::new(true),
                state_affects_style: Cell::new(false),
                resolved_style: Default::default(),
            },
        )
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
    layout_style_setter!(gap, FrameLayout::Flex{gap, ..}, set_gap: FlexSize);
    layout_style_setter!(initial_gap, FrameLayout::Flex{initial_gap, ..}, set_initial_gap: FlexSize);
    layout_style_setter!(final_gap, FrameLayout::Flex{final_gap, ..}, set_final_gap: FlexSize);

    pub fn set_padding(&self, value: LengthOrPercentage) {
        self.set_padding_left(value);
        self.set_padding_right(value);
        self.set_padding_top(value);
        self.set_padding_bottom(value);
    }

    pub fn set_padding_left(&self, value: LengthOrPercentage) {
        self.set(PaddingLeft, value);
        self.mark_needs_relayout();
    }

    pub fn set_padding_right(&self, value: LengthOrPercentage) {
        self.set(PaddingRight, value);
        self.mark_needs_relayout();
    }

    pub fn set_padding_top(&self, value: LengthOrPercentage) {
        self.set(PaddingTop, value);
        self.mark_needs_relayout();
    }

    pub fn set_padding_bottom(&self, value: LengthOrPercentage) {
        self.set(PaddingBottom, value);
        self.mark_needs_relayout();
    }

    pub fn set_content(&self, content: impl Into<RcElement>) {
        self.clear_children();
        (self as &dyn Element).add_child(content)
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
                .replace(self.style.borrow().apply_overrides(self.state.get()));
            self.style_changed.set(false);
        }
    }
}

struct Padding {
    left: LengthOrPercentage,
    right: LengthOrPercentage,
    top: LengthOrPercentage,
    bottom: LengthOrPercentage,
}

struct BoxSizingParams<'a> {
    axis: Axis,
    padding: Padding,
    children: &'a [RcElement],
}

impl Frame {
    fn layout_content(
        &self,
        p: &BoxSizingParams,
        mode: LayoutMode,
        main: SizeConstraint,
        cross: SizeConstraint,
    ) -> LayoutOutput {
        // TODO parent size
        let layout_input = LayoutInput::from_main_cross(p.axis, main, cross, None, None);
        self.layout_cache
            .borrow_mut()
            .get_or_insert_with(&layout_input, mode, |li| {
                let _span = trace_span!("Frame::layout_content", ?mode, ?layout_input).entered();

                // resolve padding
                let padding_left = layout_input.width.resolve_length(p.padding.left);
                let padding_right = layout_input.width.resolve_length(p.padding.right);
                let padding_top = layout_input.height.resolve_length(p.padding.top);
                let padding_bottom = layout_input.height.resolve_length(p.padding.bottom);

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
                    parent_width: None,
                    parent_height: None,
                    gap,
                    initial_gap,
                    final_gap,
                };

                let mut output = flex_layout(mode, &flex_params, p.children);

                // don't forget to apply box padding
                // on the main axis it's redundant with `initial_gap` but we keep it for consistency
                // with other layout modes
                if mode == LayoutMode::Place {
                    for child in p.children {
                        child.add_offset(Vec2::new(padding_left, padding_top));
                    }
                }

                output.width += padding_left + padding_right;
                output.height += padding_top + padding_bottom;
                output.baseline.as_mut().map(|b| *b += padding_top);

                output
            })
    }

    /// Measures a box element sized according to the specified constraints.
    fn layout_inner(
        &self,
        p: &BoxSizingParams,
        mode: LayoutMode,
        main_sz: Sizing,
        cross_sz: Sizing,
        parent_main_sc: SizeConstraint,
        parent_cross_sc: SizeConstraint,
    ) -> LayoutOutput {
        let _span = trace_span!("Frame::layout_inner", ?mode, ?p.axis).entered();
        let cross_axis = p.axis.cross();

        // Helper function to convert a user-provided sizing constraint to
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
        // fully determined, and we don't need to measure it.
        let mut layout = match (main_sz.preferred, cross_sz.preferred) {
            (SizeValue::Fixed(_) | SizeValue::Percentage(_), SizeValue::Fixed(_) | SizeValue::Percentage(_)) =>
                {
                    // must call layout_content to place things
                    if mode == LayoutMode::Place {
                        self.layout_content(p, mode, content_main_sc, content_cross_sc);
                    }
                    LayoutOutput::from_main_cross_sizes(
                        p.axis,
                        content_main_sc.available().unwrap(),
                        content_cross_sc.available().unwrap(),
                        None,
                    )
                }
            _ => self.layout_content(p, mode, content_main_sc, content_cross_sc),
        };

        // layout now holds the content box size
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
                SizeValue::Percentage(percent) => parent_main_sc.available().map(|s| percent * s).unwrap_or(default),
                SizeValue::MinContent => self
                    .layout_content(p, LayoutMode::Measure, SizeConstraint::MIN, parent_cross_sc)
                    .size(axis),
                SizeValue::MaxContent => self
                    .layout_content(p, LayoutMode::Measure, SizeConstraint::MAX, parent_cross_sc)
                    .size(axis),
            }
        };

        // TODO: why do we need to clamp to the maximum size here?
        // can't we apply the max size before sizing the content, by clamping the available size?

        let main_min = eval_size(main_sz.min, p.axis, parent_main_sc, parent_cross_sc, 0.0);
        let main_max = eval_size(main_sz.max, p.axis, parent_main_sc, parent_cross_sc, f64::INFINITY);
        let cross_min = eval_size(cross_sz.min, cross_axis, parent_cross_sc, parent_main_sc, 0.0);
        let cross_max = eval_size(cross_sz.max, cross_axis, parent_cross_sc, parent_main_sc, f64::INFINITY);

        // clamp main axis size first
        let main = layout.size(p.axis);
        let clamped_main = main.clamp(main_min, main_max);

        if clamped_main != main {
            // re-layout cross axis under new main axis constraints
            layout = self.layout_content(p, mode, SizeConstraint::Available(clamped_main), parent_cross_sc);
            layout.set_axis(p.axis, clamped_main);
        }

        // clamp cross axis size
        // we don't ask for a re-measure, because this might in turn affect the main axis size,
        // and we don't want to loop indefinitely, this isn't a fixed-point solver
        let clamped_cross = layout.size(cross_axis).clamp(cross_min, cross_max);
        layout.set_axis(cross_axis, clamped_cross);

        trace!("Measured element: size={:?}", layout);
        layout
    }

    fn get_padding(&self) -> Padding {
        Padding {
            left: self.get(PaddingLeft).unwrap_or_default(),
            right: self.get(PaddingRight).unwrap_or_default(),
            top: self.get(PaddingTop).unwrap_or_default(),
            bottom: self.get(PaddingBottom).unwrap_or_default(),
        }
    }
}

impl Element for Frame {
    fn node(&self) -> &Node {
        &self.node
    }

    fn measure(&self, children: &[RcElement], layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("Frame::measure").entered();
        // TODO vertical direction layout
        let (main_constraint, cross_constraint) = layout_input.main_cross(Axis::Horizontal);
        let p = BoxSizingParams {
            axis: Axis::Horizontal,
            padding: self.get_padding(),
            children,
        };
        let output = self.layout_inner(
            &p,
            LayoutMode::Measure,
            self.get(layout::Width).unwrap_or_default(),
            self.get(layout::Height).unwrap_or_default(),
            main_constraint,
            cross_constraint,
        );
        Size::new(output.width, output.height)
    }

    fn layout(&self, children: &[RcElement], size: Size) -> LayoutOutput {
        let _span = trace_span!("Frame::layout").entered();
        // TODO vertical direction layout
        let p = BoxSizingParams {
            axis: Axis::Horizontal,
            padding: self.get_padding(),
            children,
        };
        // defer to measure for now
        let output = self.layout_inner(
            &p,
            LayoutMode::Place,
            self.get(layout::Width).unwrap_or_default(),
            self.get(layout::Height).unwrap_or_default(),
            size.width.into(),
            size.height.into(),
        );
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

    fn event(&self, event: &mut Event)
    {
        fn update_state(this: &Frame, state: InteractState) {
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
