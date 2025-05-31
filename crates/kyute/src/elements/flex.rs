use crate::element::{ElementAny, ElementBuilder, HitTestCtx, IntoElementAny, Measurement, TreeCtx, WeakElementAny};
use crate::layout::flex::{flex_layout, FlexChild, FlexLayoutParams};
use crate::layout::{Alignment, Axis, LayoutInput, LayoutMode, LayoutOutput, SizeValue};
use crate::{Element, PaintCtx};
use kurbo::{Point, Size};
use tracing::trace_span;

pub struct FlexChildBuilder<E> {
    pub element: E,
    pub flex: f64,
    pub margin_before: SizeValue,
    pub margin_after: SizeValue,
    pub alignment: Alignment,
}

impl<E> FlexChildBuilder<E> {
    pub fn new(element: E) -> Self {
        FlexChildBuilder {
            element,
            flex: 1.0,
            margin_before: SizeValue::Fixed(0.0),
            margin_after: SizeValue::Fixed(0.0),
            alignment: Alignment::default(),
        }
    }

    /// Sets the flex factor of the flex child.
    pub fn flex(mut self, flex: f64) -> Self {
        self.flex = flex;
        self
    }

    /// Sets the margin before the flex child.
    pub fn margin_before(mut self, margin: SizeValue) -> Self {
        self.margin_before = margin;
        self
    }

    /// Sets the margin after the flex child.
    pub fn margin_after(mut self, margin: SizeValue) -> Self {
        self.margin_after = margin;
        self
    }

    /// Sets the flex child's cross axis alignment.
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
}

pub trait DynamicFlexChildren {
    /// Updates the list of children of the flex element.
    fn update(&mut self, parent: WeakElementAny, children: &mut Vec<FlexChild>);
}

pub struct Flex {
    direction: Axis,
    /// Default gap between children.
    gap: SizeValue,
    /// Initial gap before the first child (padding).
    initial_gap: SizeValue,
    /// Final gap after the last child (padding).
    final_gap: SizeValue,
    children: Vec<FlexChild>,
}

impl Flex {
    pub fn new() -> ElementBuilder<Self> {
        ElementBuilder::new(Flex {
            direction: Axis::Vertical,
            gap: SizeValue::Fixed(0.0),
            initial_gap: SizeValue::Fixed(0.0),
            final_gap: SizeValue::Fixed(0.0),
            children: Vec::new(),
        })
    }

    pub fn row() -> ElementBuilder<Self> {
        Self::new().direction(Axis::Horizontal)
    }

    pub fn column() -> ElementBuilder<Self> {
        Self::new().direction(Axis::Vertical)
    }

    /*fn update_dynamic_children(self: &mut ElemBox<Self>, mut children: impl DynamicFlexChildren + 'static) {
        let (_, deps) = with_tracking_scope(|| children.update(self.ctx.weak_any(), &mut self.children));
        self.ctx.mark_needs_layout();
        if !deps.reads.is_empty() {
            self.watch_once(deps.reads.into_iter().map(|w| w.0), move |this, _| {
                this.update_dynamic_children(children);
            });
        }
    }*/

    /*/// Specifies the contents of the flex layout.
    #[must_use]
    pub fn dynamic_children(
        mut self: ElementBuilder<Self>,
        children: impl DynamicFlexChildren + 'static,
    ) -> ElementBuilder<Self> {
        self.update_dynamic_children(children);
        self
    }*/

    /// Adds a child element to the flex layout.
    #[must_use]
    pub fn child(mut self: ElementBuilder<Self>, child: impl IntoElementAny) -> ElementBuilder<Self> {
        self.add_child(child);
        self
    }
    
    pub fn add_child(
        self: &mut ElementBuilder<Self>,
        child: impl IntoElementAny,
    ) {
        let weak_any = self.weak_any();
        self.children.push(FlexChild::new(child.into_element_any(weak_any)));
    }

    /// Adds a child element to the flex layout with additional layout options.
    #[must_use]
    pub fn flex_child(
        mut self: ElementBuilder<Self>,
        child: FlexChildBuilder<impl IntoElementAny>,
    ) -> ElementBuilder<Self> {
        let weak_any = self.weak_any();
        self.children.push(FlexChild {
            element: child.element.into_element_any(weak_any),
            flex: child.flex,
            margin_before: child.margin_before,
            margin_after: child.margin_after,
            cross_axis_alignment: Default::default(),
        });
        self
    }

    /// Specifies a vertical layout direction.
    #[must_use]
    pub fn vertical(mut self: ElementBuilder<Self>) -> ElementBuilder<Self> {
        self.direction = Axis::Vertical;
        self
    }

    /// Specifies a horizontal layout direction.
    #[must_use]
    pub fn horizontal(mut self: ElementBuilder<Self>) -> ElementBuilder<Self> {
        self.direction = Axis::Horizontal;
        self
    }

    /// Specifies layout direction.
    #[must_use]
    pub fn direction(mut self: ElementBuilder<Self>, dir: Axis) -> ElementBuilder<Self> {
        self.direction = dir;
        self
    }

    /// Specifies the gap between items in the layout direction.
    #[must_use]
    pub fn gap(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.gap = value.into();
        self
    }

    /// Specifies the initial gap before the first item in the layout direction.
    #[must_use]
    pub fn initial_gap(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.initial_gap = value.into();
        self
    }

    /// Specifies the final gap after the last item in the layout direction.
    #[must_use]
    pub fn final_gap(mut self: ElementBuilder<Self>, value: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.final_gap = value.into();
        self
    }

    /// Sets the initial, inter-element and final gaps.
    #[must_use]
    pub fn gaps(
        mut self: ElementBuilder<Self>,
        initial_gap: impl Into<SizeValue>,
        inter_element_gap: impl Into<SizeValue>,
        final_gap: impl Into<SizeValue>,
    ) -> ElementBuilder<Self> {
        self.initial_gap = initial_gap.into();
        self.gap = inter_element_gap.into();
        self.final_gap = final_gap.into();
        self
    }
}

impl Element for Flex {
    fn children(&self) -> Vec<ElementAny> {
        self.children.iter().map(|child| child.element.clone()).collect()
    }

    fn measure(&mut self, cx: &TreeCtx, layout_input: &LayoutInput) -> Measurement {
        let _span = trace_span!("Flex::measure", ?layout_input).entered();

        let output = flex_layout(
            LayoutMode::Measure,
            cx,
            &FlexLayoutParams {
                direction: self.direction,
                available: layout_input.available,
                gap: self.gap,
                initial_gap: self.initial_gap,
                final_gap: self.final_gap,
            },
            &self.children,
        );

        Measurement {
            size: Size::new(output.width, output.height),
            baseline: output.baseline,
        }
    }

    fn layout(&mut self, cx: &TreeCtx, size: Size)  {
        flex_layout(
            LayoutMode::Place,
            cx,
            &FlexLayoutParams {
                direction: self.direction,
                // The size here is a hard constraint, not an upper bound,
                // but this makes no difference for flex layout.
                available: size,
                gap: self.gap,
                initial_gap: self.initial_gap,
                final_gap: self.final_gap,
            },
            &self.children[..],
        );
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        for child in self.children.iter() {
            child.element.hit_test(ctx, point);
        }
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        for child in self.children.iter_mut() {
            ctx.paint_child(&mut child.element);
        }
    }
}
