//! Container that places its items in a uniform grid.

use kyute::drawing::vec2;
use kyute::layout::{LayoutInput, LayoutOutput};
use kyute::{Element, HitTestCtx, IntoNode, Measurement, NodeBuilder, NodeCtx, PaintCtx, Point, RcDynNode, RcNode, Size, WeakNode};

pub struct UniformGrid {
    weak_this: WeakNode<Self>,
    cell_size: Size,
    h_gap: f64,
    v_gap: f64,
    elements: Vec<RcDynNode>,
}

impl UniformGrid {
    pub fn new(cell_size: Size) -> NodeBuilder<Self> {
        NodeBuilder::new_cyclic(|weak| UniformGrid {
            weak_this: weak,
            cell_size,
            h_gap: 0.0,
            v_gap: 0.0,
            elements: vec![],
        })
    }

    pub fn child(mut self: NodeBuilder<Self>, element: impl IntoNode) -> NodeBuilder<Self> {
        let weak_this = self.weak_this.clone().as_dyn();
        self.elements.push(element.into_dyn_node(weak_this));
        self
    }

    pub fn h_gap(mut self: NodeBuilder<Self>, h_gap: f64) -> NodeBuilder<Self> {
        self.h_gap = h_gap;
        self
    }

    pub fn v_gap(mut self: NodeBuilder<Self>, v_gap: f64) -> NodeBuilder<Self> {
        self.v_gap = v_gap;
        self
    }

    fn row_column_count(&self, width: f64) -> (usize, usize) {
        let n = self.elements.len() as f64;
        let columns = ((width + self.h_gap) / (self.cell_size.width + self.h_gap))
            .floor()
            .min(n);
        let rows = (n / columns).ceil();
        (rows as usize, columns as usize)
    }
}

impl Element for UniformGrid {
    fn measure(&mut self, _cx: &NodeCtx, layout_input: &LayoutInput) -> Measurement {
        // TODO compute the baseline
        let (rows, columns) = self.row_column_count(layout_input.available.width);
        let width = columns as f64 * self.cell_size.width + (columns - 1) as f64 * self.h_gap;
        let height = rows as f64 * self.cell_size.height + (rows - 1) as f64 * self.v_gap;
        Size::new(width, height).into()
    }

    fn layout(&mut self, cx: &NodeCtx, size: Size) {
        let (_rows, columns) = self.row_column_count(size.width);

        for i in 0..self.elements.len() {
            let row = i / columns;
            let column = i % columns;
            let x = column as f64 * (self.cell_size.width + self.h_gap);
            let y = row as f64 * (self.cell_size.height + self.v_gap);
            let child_size = Size::new(self.cell_size.width, self.cell_size.height);
            self.elements[i].layout(cx, child_size);
            self.elements[i].set_offset(vec2(x, y));
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, painter: &mut PaintCtx) {
        for element in &mut self.elements {
            painter.paint_child(element);
            //element.paint(cx, painter);
        }
    }
}
