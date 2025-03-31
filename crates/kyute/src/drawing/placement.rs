use kurbo::{Point, Rect, Size};

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum Alignment {
    // For maximum flexibility this could be a float value (0.0 = start, 0.5 = center, 1.0 = end)
    #[default]
    Start,
    Center,
    End,
    Baseline,
}

impl Alignment {
    pub fn to_pos(self, x0: f64, x1: f64, baseline: f64) -> f64 {
        match self {
            Alignment::Start => x0,
            Alignment::Center => 0.5 * (x0 + x1),
            Alignment::End => x1,
            Alignment::Baseline => x0 + baseline,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Anchor {
    pub alignment: Alignment,
    pub offset: f64,
}

impl Anchor {
    pub fn to_pos(self, x0: f64, x1: f64, baseline: f64) -> f64 {
        match self.alignment {
            Alignment::Start => x0 + self.offset,
            Alignment::Center => x0 + 0.5 * (x1 - x0) + self.offset,
            Alignment::End => x1 + self.offset,
            Alignment::Baseline => x0 + baseline + self.offset,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Anchor2D {
    pub x: Alignment,
    pub y: Alignment,
}

impl Anchor2D {
    pub fn to_point(self, container: Rect, container_baseline: f64) -> Point {
        // TODO: vertical baselines
        let x = self.x.to_pos(container.x0, container.x1, container_baseline);
        let y = self.y.to_pos(container.y0, container.y1, container_baseline);
        Point { x, y }
    }

    pub const TOP_LEFT: Self = Self {
        x: Alignment::Start,
        y: Alignment::Start,
    };
    pub const TOP_RIGHT: Self = Self {
        x: Alignment::End,
        y: Alignment::Start,
    };
    pub const BOTTOM_LEFT: Self = Self {
        x: Alignment::Start,
        y: Alignment::End,
    };
    pub const BOTTOM_RIGHT: Self = Self {
        x: Alignment::End,
        y: Alignment::End,
    };
    pub const TOP: Self = Self {
        x: Alignment::Center,
        y: Alignment::Start,
    };
    pub const BOTTOM: Self = Self {
        x: Alignment::Center,
        y: Alignment::End,
    };
    pub const LEFT: Self = Self {
        x: Alignment::Start,
        y: Alignment::Center,
    };
    pub const RIGHT: Self = Self {
        x: Alignment::End,
        y: Alignment::Center,
    };
    pub const CENTER: Self = Self {
        x: Alignment::Center,
        y: Alignment::Center,
    };
    pub const BASELINE_LEFT: Self = Self {
        x: Alignment::Start,
        y: Alignment::Baseline,
    };
    pub const BASELINE_RIGHT: Self = Self {
        x: Alignment::End,
        y: Alignment::Baseline,
    };
    pub const BASELINE: Self = Self {
        x: Alignment::Center,
        y: Alignment::Baseline,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub container: Anchor2D,
    pub content: Anchor2D,
}

/// A rectangle with a baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectWithBaseline {
    pub rect: Rect,
    pub baseline: f64,
}

impl From<Rect> for RectWithBaseline {
    fn from(rect: Rect) -> Self {
        RectWithBaseline {
            rect,
            baseline: rect.height(),
        }
    }
}

impl From<(Rect, f64)> for RectWithBaseline {
    fn from((rect, baseline): (Rect, f64)) -> Self {
        RectWithBaseline { rect, baseline }
    }
}

impl From<Size> for RectWithBaseline {
    fn from(size: Size) -> Self {
        RectWithBaseline {
            rect: size.to_rect(),
            baseline: size.height,
        }
    }
}

impl From<(Size, f64)> for RectWithBaseline {
    fn from((size, baseline): (Size, f64)) -> Self {
        RectWithBaseline {
            rect: size.to_rect(),
            baseline,
        }
    }
}

pub trait PlacementExt {
    fn place_into(self, container: impl Into<RectWithBaseline>, placement: impl Into<Placement>) -> Point;
}

impl PlacementExt for RectWithBaseline {
    fn place_into(self, container: impl Into<RectWithBaseline>, placement: impl Into<Placement>) -> Point {
        let container = container.into();
        let placement = placement.into();
        place_rect_into(container.rect, container.baseline, self.rect, self.baseline, placement)
    }
}

impl PlacementExt for Size {
    fn place_into(self, container: impl Into<RectWithBaseline>, placement: impl Into<Placement>) -> Point {
        RectWithBaseline::from(self).place_into(container, placement)
    }
}

impl PlacementExt for Rect {
    fn place_into(self, container: impl Into<RectWithBaseline>, placement: impl Into<Placement>) -> Point {
        RectWithBaseline::from(self).place_into(container, placement)
    }
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
    content_anchor: impl Into<Anchor2D>,
    container: Rect,
    container_anchor: impl Into<Anchor2D>,
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

impl From<Anchor2D> for Placement {
    fn from(anchor: Anchor2D) -> Self {
        Placement {
            container: anchor,
            content: anchor,
        }
    }
}

impl From<(Anchor2D, Anchor2D)> for Placement {
    fn from((container, content): (Anchor2D, Anchor2D)) -> Self {
        Placement { container, content }
    }
}

pub const TOP_LEFT: Placement = Placement {
    container: Anchor2D::TOP_LEFT,
    content: Anchor2D::TOP_LEFT,
};
pub const TOP_RIGHT: Placement = Placement {
    container: Anchor2D::TOP_RIGHT,
    content: Anchor2D::TOP_RIGHT,
};
pub const BOTTOM_LEFT: Placement = Placement {
    container: Anchor2D::BOTTOM_LEFT,
    content: Anchor2D::BOTTOM_LEFT,
};
pub const BOTTOM_RIGHT: Placement = Placement {
    container: Anchor2D::BOTTOM_RIGHT,
    content: Anchor2D::BOTTOM_RIGHT,
};
pub const TOP_CENTER: Placement = Placement {
    container: Anchor2D::TOP,
    content: Anchor2D::TOP,
};
pub const BOTTOM_CENTER: Placement = Placement {
    container: Anchor2D::BOTTOM,
    content: Anchor2D::BOTTOM,
};
pub const LEFT_CENTER: Placement = Placement {
    container: Anchor2D::LEFT,
    content: Anchor2D::LEFT,
};
pub const RIGHT_CENTER: Placement = Placement {
    container: Anchor2D::RIGHT,
    content: Anchor2D::RIGHT,
};
pub const CENTER: Placement = Placement {
    container: Anchor2D::CENTER,
    content: Anchor2D::CENTER,
};
pub const BASELINE_LEFT: Placement = Placement {
    container: Anchor2D::BASELINE_LEFT,
    content: Anchor2D::BASELINE_LEFT,
};
pub const BASELINE_RIGHT: Placement = Placement {
    container: Anchor2D::BASELINE_RIGHT,
    content: Anchor2D::BASELINE_RIGHT,
};
pub const BASELINE_CENTER: Placement = Placement {
    container: Anchor2D::BASELINE,
    content: Anchor2D::BASELINE,
};
