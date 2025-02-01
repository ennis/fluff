use kurbo::{Point, Rect};

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum Anchor {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,    // == TopCenter
    Bottom, // == BottomCenter
    Left,   // == LeftCenter
    Right,  // == RightCenter
    Center,
    BaselineLeft,
    BaselineRight,
    Baseline, // == BaselineCenter
    Absolute(Point),
}

impl Anchor {
    pub fn to_point(self, container: Rect, container_baseline: f64) -> Point {
        match self {
            Anchor::TopLeft => Point::new(container.x0, container.y0),
            Anchor::TopRight => Point::new(container.x1, container.y0),
            Anchor::BottomLeft => Point::new(container.x0, container.y1),
            Anchor::BottomRight => Point::new(container.x1, container.y1),
            Anchor::Top => Point::new(container.x0 + 0.5 * container.width(), container.y0),
            Anchor::Bottom => Point::new(container.x0 + 0.5 * container.width(), container.y1),
            Anchor::Left => Point::new(container.x0, container.y0 + 0.5 * container.height()),
            Anchor::Right => Point::new(container.x1, container.y0 + 0.5 * container.height()),
            Anchor::Center => Point::new(
                container.x0 + 0.5 * container.width(),
                container.y0 + 0.5 * container.height(),
            ),
            Anchor::BaselineLeft => Point::new(container.x0, container.y0 + container_baseline),
            Anchor::BaselineRight => Point::new(container.x1, container.y0 + container_baseline),
            Anchor::Baseline => Point::new(
                container.x0 + 0.5 * container.width(),
                container.y0 + container_baseline,
            ),
            Anchor::Absolute(point) => point,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Placement {
    pub container: Anchor,
    pub content: Anchor,
}

pub fn place_rect_into(
    container: Rect,
    container_baseline: f64,
    content: Rect,
    content_baseline: f64,
    placement: Placement,
) -> Point {
    let container_anchor = placement.container.to_point(container, container_baseline);
    let content_anchor = placement.content.to_point(content, content_baseline);
    let offset = container_anchor - content_anchor;
    content.origin() + offset
}

pub fn align(
    content: Rect,
    content_anchor: impl Into<Anchor>,
    container: Rect,
    container_anchor: impl Into<Anchor>,
) -> Rect {
    let pos = place_rect_into(
        container,
        0.0,
        content,
        0.0,
        Placement {
            container: container_anchor.into(),
            content: content_anchor.into(),
        },
    );
    Rect::from_origin_size(pos, content.size())
}

pub fn place(content: Rect, placement: impl Into<Placement>, container: Rect) -> Rect {
    let placement = placement.into();
    align(content, placement.content, container, placement.container)
}

impl From<Anchor> for Placement {
    fn from(anchor: Anchor) -> Self {
        Placement {
            container: anchor,
            content: anchor,
        }
    }
}

impl From<(Anchor, Anchor)> for Placement {
    fn from((container, content): (Anchor, Anchor)) -> Self {
        Placement { container, content }
    }
}

pub const TOP_LEFT: Placement = Placement {
    container: Anchor::TopLeft,
    content: Anchor::TopLeft,
};
pub const TOP_RIGHT: Placement = Placement {
    container: Anchor::TopRight,
    content: Anchor::TopRight,
};
pub const BOTTOM_LEFT: Placement = Placement {
    container: Anchor::BottomLeft,
    content: Anchor::BottomLeft,
};
pub const BOTTOM_RIGHT: Placement = Placement {
    container: Anchor::BottomRight,
    content: Anchor::BottomRight,
};
pub const TOP_CENTER: Placement = Placement {
    container: Anchor::Top,
    content: Anchor::Top,
};
pub const BOTTOM_CENTER: Placement = Placement {
    container: Anchor::Bottom,
    content: Anchor::Bottom,
};
pub const LEFT_CENTER: Placement = Placement {
    container: Anchor::Left,
    content: Anchor::Left,
};
pub const RIGHT_CENTER: Placement = Placement {
    container: Anchor::Right,
    content: Anchor::Right,
};
pub const CENTER: Placement = Placement {
    container: Anchor::Center,
    content: Anchor::Center,
};
pub const BASELINE_LEFT: Placement = Placement {
    container: Anchor::BaselineLeft,
    content: Anchor::BaselineLeft,
};
pub const BASELINE_RIGHT: Placement = Placement {
    container: Anchor::BaselineRight,
    content: Anchor::BaselineRight,
};
pub const BASELINE_CENTER: Placement = Placement {
    container: Anchor::Baseline,
    content: Anchor::Baseline,
};
