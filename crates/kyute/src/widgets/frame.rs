//! Frame containers
use std::cell::{Cell, RefCell};
use std::iter::Map;
use std::ops::Deref;
use std::rc::Rc;
use std::slice;
use bitflags::{bitflags, Flags};

use kurbo::{Insets, RoundedRect, Size, Vec2};
use smallvec::{SmallVec, smallvec};
use taffy::{AvailableSpace, Cache, compute_block_layout, compute_flexbox_layout, compute_grid_layout, compute_root_layout, Display, Layout, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, TraversePartialTree};

use crate::drawing::{BoxShadow, Paint, ToSkia};
use crate::element::{AttachedProperty, Element, ElementMethods};
use crate::event::Event;
use crate::handler::Handler;
use crate::layout::flex::{Axis, CrossAxisAlignment, MainAxisAlignment};
use crate::layout::{place_child_box, Alignment, BoxConstraints, Geometry, IntrinsicSizes, LengthOrPercentage, Sizing};
use crate::style::{
    Active, BackgroundColor, Baseline, BorderBottom, BorderColor, BorderLeft, BorderRadius, BorderRight, BorderTop,
    BoxShadows, Direction, Focus, Height, HorizontalAlign, Hover, MaxHeight, MaxWidth, MinHeight, MinWidth,
    PaddingBottom, PaddingLeft, PaddingRight, PaddingTop, Style, VerticalAlign, Width,
};
use crate::{drawing, style, Color, PaintCtx};

const PARENT_NODE_ID: NodeId = NodeId::new(!0);

#[derive(Copy, Clone, Default)]
pub struct TaffyStyle;

impl AttachedProperty for TaffyStyle {
    type Value = taffy::Style;
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Taffy Integration

struct LayoutTree;

unsafe fn element_from_id<'a>(node_id: NodeId) -> &'a Element {
    &*(usize::from(node_id) as *const Element)
}

fn element_to_id(element: &Element) -> NodeId {
    NodeId::from(element as *const _ as usize)
}

impl TraversePartialTree for LayoutTree {
    type ChildIter<'a> = Map<slice::Iter<'a, Rc<dyn ElementMethods>>, fn(&Rc<dyn ElementMethods>) -> NodeId>;

    fn child_ids(&self, parent_node_id: NodeId) -> Self::ChildIter<'_> {
        let node = unsafe { element_from_id(parent_node_id) };
        unsafe {
            // SAFETY:
            node.children_ref().iter().map(|c| element_to_id(c))
        }
    }

    fn child_count(&self, parent_node_id: NodeId) -> usize {
        let node = unsafe { element_from_id(parent_node_id) };
        node.child_count()
    }

    fn get_child_id(&self, parent_node_id: NodeId, child_index: usize) -> NodeId {
        unsafe {
            element_from_id(parent_node_id).children_ref().get(child_index).map(|c| element_to_id(c)).unwrap()
        }
    }
}

static DEFAULT_STYLE: taffy::Style = taffy::Style::DEFAULT;

impl LayoutPartialTree for LayoutTree {
    fn get_style(&self, node_id: NodeId) -> &taffy::Style {
        // SAFETY: during layout the style information isn't accessed.
        // Unfortunately this contract isn't verifiable within the bounds of this function.
        // See `Frame::layout` for more information.
        unsafe {
            element_from_id(node_id).try_get_ref::<TaffyStyle>().unwrap_or(&DEFAULT_STYLE)
        }
    }

    fn set_unrounded_layout(&mut self, node_id: NodeId, layout: &Layout) {
        unsafe {
            let element = element_from_id(node_id);
            // eprintln!("set_unrounded_layout: node_id = {:?}, layout = {:?}", node_id, layout);
            element.set_offset(Vec2::new(layout.location.x as f64, layout.location.y as f64));
        }
    }

    fn get_cache_mut(&mut self, _node_id: NodeId) -> &mut Cache {
        unimplemented!()
    }

    fn compute_child_layout(&mut self, node_id: NodeId, input: LayoutInput) -> LayoutOutput {
        let child = unsafe { element_from_id(node_id) };
        child.rc().do_layout(&input)
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/*
/// Convert`taffy::LayoutInput` to `xilem::BoxConstraints`
/// Borrowed from https://github.com/linebender/xilem/blob/c4398858661bd75c15d5efc3c4e67f8ec9319250/src/widget/taffy_layout.rs#L98
pub(super) fn to_box_constraints(input: &taffy::LayoutInput) -> BoxConstraints {
    /// Converts Taffy's known_dimension and available space into a min box constraint
    fn to_min_constraint(known_dimension: Option<f32>, available_space: taffy::AvailableSpace) -> f64 {
        known_dimension.unwrap_or(match available_space {
            taffy::AvailableSpace::Definite(_) => 0.0,
            taffy::AvailableSpace::MaxContent => 0.0,
            taffy::AvailableSpace::MinContent => -0.0,
        }) as f64
    }

    /// Converts Taffy's known_dimension and available_spaceinto a min box constraint
    fn to_max_constraint(known_dimension: Option<f32>, available_space: taffy::AvailableSpace) -> f64 {
        known_dimension.unwrap_or(match available_space {
            taffy::AvailableSpace::Definite(val) => val,
            taffy::AvailableSpace::MaxContent => f32::INFINITY,
            taffy::AvailableSpace::MinContent => f32::INFINITY,
        }) as f64
    }

    BoxConstraints::new(
        Size {
            width: to_min_constraint(input.known_dimensions.width, input.available_space.width),
            height: to_min_constraint(input.known_dimensions.height, input.available_space.height),
        },
        Size {
            width: to_max_constraint(input.known_dimensions.width, input.available_space.width),
            height: to_max_constraint(input.known_dimensions.height, input.available_space.height),
        },
    )
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

#[derive(Clone, Default)]
pub struct FrameStyle {
    pub border_color: Color,
    pub border_radius: LengthOrPercentage,
    pub background_color: Color,
    pub shadows: SmallVec<BoxShadow, 2>,
    pub overrides: SmallVec<FrameStyleOverride, 2>,
}


impl FrameStyle
{
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
            self.resolved_style.replace(self.style.apply_overrides(self.state.get()));
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
        let mut max = self
            .self_max
            .unwrap_or(f64::INFINITY)
            .clamp(self.parent_min, self.parent_max);

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
        let max = self
            .self_max
            .unwrap_or(f64::INFINITY)
            .clamp(self.parent_min, self.parent_max);
        size = size.clamp(min, max);
        size
    }
}*/


/*
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

    fn layout(&self, _children: &[Rc<dyn ElementMethods>], layout_input: &LayoutInput) -> LayoutOutput {

        // why do we need to fetch the display kind here? can't this be done by taffy with
        // a `compute_layout` function that would dispatch to the appropriate layout function?
        // taffy will need to access the style anyway, and all algorithm take the same input
        let style = unsafe { self.get_ref::<TaffyStyle>() };

        /*// size to content
        let layout_input = LayoutInput {
            available_space: taffy::Size {
                width: AvailableSpace::MaxContent,
                height: AvailableSpace::MaxContent,
            },
            ..*layout_input
        };*/


        let output = match style.display {
            taffy::Display::Flex => {
                compute_flexbox_layout(&mut LayoutTree, element_to_id(self), *layout_input)
            }
            taffy::Display::Block => {
                compute_block_layout(&mut LayoutTree, element_to_id(self), *layout_input)
            }
            taffy::Display::Grid => {
                compute_grid_layout(&mut LayoutTree, element_to_id(self), *layout_input)
            }
            _ => {
                unimplemented!()
            }
        };

        eprintln!("layout frame with axis={:?}, known_dimensions={:?}, available_space={:?} -> {:?}", layout_input.axis, layout_input.known_dimensions, layout_input.available_space, output.size);
        output
    }

    fn paint(&self, ctx: &mut PaintCtx) {
        let rect = self.element.size().to_rect();

        // SAFETY: we don't modify the attached properties during painting
        let layout_style = unsafe {
            TaffyStyle.get_ref(self)
        };

        let s = self.resolved_style.borrow();

        fn resolve_length_or_percentage(len: taffy::LengthPercentage, parent: f32) -> f64 {
            match len {
                taffy::LengthPercentage::Length(len) => len as f64,
                taffy::LengthPercentage::Percent(pct) => (pct * parent) as f64,
            }
        }

        let insets = Insets::new(
            resolve_length_or_percentage(layout_style.border.left, rect.width() as f32),
            resolve_length_or_percentage(layout_style.border.top, rect.height() as f32),
            resolve_length_or_percentage(layout_style.border.right, rect.width() as f32),
            resolve_length_or_percentage(layout_style.border.bottom, rect.height() as f32),
        );

        // border shape
        let border_radius = s.border_radius.resolve(rect.width());
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

#[test]
fn test_im() {
    let mut ordmap_1 = imbl::ordmap![
        1 => "a",
        2 => "b",
        3 => "c"
    ];
    let ordmap_2 = imbl::ordmap![
        1 => "d"
        //2 => "e"
        //3 => "f"
    ];

    //let mut ordmap_1 = im::ordmap!{1 => 1, 3 => 3};
    //let ordmap_2 = im::ordmap!{2 => 2, 3 => 4};

    ordmap_1 = ordmap_2.union(ordmap_1);

    dbg!(ordmap_1);
}
