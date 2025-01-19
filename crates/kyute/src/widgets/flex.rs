use kurbo::{Point, Size};
use tracing::trace_span;
use crate::{Element, PaintCtx};
use crate::element::{ElementAny, ElementBuilder, ElementCtx, ElementCtxAny, HitTestCtx, IntoElementAny};
use crate::layout::{Alignment, Axis, LayoutInput, LayoutMode, LayoutOutput, SizeConstraint, SizeValue};
use crate::layout::flex::{flex_layout, FlexChild, FlexLayoutParams};

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

pub struct Flex {
    ctx: ElementCtx<Self>,
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
            ctx: ElementCtx::new(),
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


    /// Adds a child element to the flex layout.
    #[must_use]
    pub fn child(mut self: ElementBuilder<Self>, child: impl IntoElementAny) -> ElementBuilder<Self> {
        let weak_any = self.weak_any();
        self.children.push(FlexChild::new(child.into_element(weak_any, 0)));
        self
    }

    /// Adds a child element to the flex layout with additional layout options.
    #[must_use]
    pub fn flex_child(mut self: ElementBuilder<Self>, child: FlexChildBuilder<impl IntoElementAny>) -> ElementBuilder<Self> {
        let weak_any = self.weak_any();
        self.children.push(FlexChild {
            element: child.element.into_element(weak_any, 0),
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
    pub fn gaps(mut self: ElementBuilder<Self>, initial_gap: impl Into<SizeValue>, inter_element_gap: impl Into<SizeValue>, final_gap: impl Into<SizeValue>) -> ElementBuilder<Self> {
        self.initial_gap = initial_gap.into();
        self.gap = inter_element_gap.into();
        self.final_gap = final_gap.into();
        self
    }
}


impl Element for Flex {
    fn ctx(&self) -> &ElementCtxAny {
        &self.ctx
    }

    fn ctx_mut(&mut self) -> &mut ElementCtxAny {
        &mut self.ctx
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("Flex::measure", ?layout_input)
            .entered();

        let output = flex_layout(LayoutMode::Measure, &FlexLayoutParams {
            direction: self.direction,
            width_constraint: layout_input.width,
            height_constraint: layout_input.height,
            parent_width: layout_input.parent_width,
            parent_height: layout_input.parent_height,
            gap: self.gap,
            initial_gap: self.initial_gap,
            final_gap: self.final_gap,
        }, &self.children);

        Size {
            width: output.width,
            height: output.height,
        }
    }

    fn children(&self) -> Vec<ElementAny> {
        self.children.iter().map(|child| child.element.clone()).collect()
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        let mut output = flex_layout(
            LayoutMode::Place,
            &FlexLayoutParams {
                direction: self.direction,
                width_constraint: SizeConstraint::Available(size.width),
                height_constraint: SizeConstraint::Available(size.height),
                // TODO parent width is unknown, so we can't use it for percentage calculations
                parent_width: None,
                parent_height: None,
                gap: self.gap,
                initial_gap: self.initial_gap,
                final_gap: self.final_gap,
            },
            &self.children[..],
        );

        output.width = size.width;
        output.height = size.height;
        output
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        for child in self.children.iter() {
            child.element.hit_test(ctx, point);
        }
        self.ctx.rect().contains(point)
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        for child in self.children.iter_mut() {
            child.element.paint(ctx);
        }
    }
}