use crate::compositor::DrawableSurface;
use crate::drawing::{ round_to_device_pixel, round_to_device_pixel_center, BorderPosition, Image, Paint,  ToSkia};
use crate::text::{TextLayout, TextRun, TextStyle};
use kurbo::{Affine, BezPath, Insets, Line, PathEl, Point, Rect, RoundedRect, Vec2};
use skia_safe::PaintStyle;
use tracing::warn;

/// Paint context.
pub struct PaintCtx<'a> {
    pub scale_factor: f64,
    /// Drawable surface.
    surface: &'a DrawableSurface,
    skia: skia_safe::Surface,
    /// Accumulated transforms.
    transforms: Vec<Affine>,
}

impl<'a> PaintCtx<'a> {
    pub(crate) fn new(surface: &'a DrawableSurface, scale_factor: f64) -> PaintCtx<'a> {
        let mut skia = surface.skia();
        skia.canvas().scale((scale_factor as f32, scale_factor as f32));

        PaintCtx {
            scale_factor,
            surface,
            skia,
            transforms: vec![Affine::IDENTITY],
        }
    }

    /*/// Returns the horizontal midline (from the center of the left edge to the center of the right edge) of the current paint bounds.
    pub fn h_midline(&self) -> Line {
        let rect = self.bounds();
        Line::new(
            (rect.x0, rect.y0 + 0.5 * rect.height()),
            (rect.x1, rect.y0 + 0.5 * rect.height()),
        )
    }

    /// Returns the vertical midline (from the center of the top edge to the center of the bottom edge) of the current paint bounds.
    pub fn v_midline(&self) -> Line {
        let rect = self.bounds();
        Line::new(
            (rect.x0 + 0.5 * rect.width(), rect.y0),
            (rect.x0 + 0.5 * rect.width(), rect.y1),
        )
    }*/

    // === PIXEL SNAPPING ===
    // issue: correct pixel rounding is complicated:
    // - we can't round logical coordinates to integers, as integer logical pixels may end up in the middle of physical pixels
    // - when rounding a logical value, you have to take into account the current transformation
    //      - (which means inverting the transformation to get a pixel value, round it here, and then transform it back)
    // - for lines/strokes you usually want a coordinate that is in the middle of a pixel, not at the edge
    // - it may affect hit-testing
    //
    // The main pain point here is that we need to round once the physical pixel coordinates
    // are known, but we don't know them until we've applied the transformation,
    // which, currently, is done internally by Skia.
    //
    // Skia has no way of rounding the resulting physical pixel coordinates to the nearest pixel,
    // so this leaves us with those options:
    // - (A) convert to physical coordinates, round, and convert back to logical coordinates
    //     - this is an expensive round-trip (2x mat mult + inverse) which is totally accidental, and I hate this
    // - (B) convert all coordinates to physical pixels ourselves, bypassing the skia transform stack
    // - (C) ensure that all transforms are translations with an offset that is a multiple of a physical pixel
    //       - i.e. no scaling or rotation
    // - (D) convert affine transforms to physical units before pushing them on the transform stack
    //       - not sure it solves anything
    //
    // The secondary pain point is the representation of logical pixels, aka "layout units"
    // - should we support fractional layout units? firefox & webkit use 1/60th of a CSS pixel
    //
    // Conclusion:
    // We make the assumption that the current transform is a translation with an offset that is a multiple of a physical pixel size in logical units
    // - all transformations are translations with an offset that is a multiple of a physical pixel
    //

    /// Rounds a logical coordinate to the nearest physical pixel boundary.
    ///
    /// This function _does not_ take the current transformation into account. If the current
    /// transformation is not aligned with the pixel grid (e.g. rotation, scaling, or translation
    /// by a subpixel amount), the result may not be pixel-aligned.
    pub fn round_to_device_pixel(&self, logical_coord:f64) -> f64 {
        round_to_device_pixel(logical_coord, self.scale_factor)
    }

    /// Rounds a logical coordinate to the center of the nearest physical pixel.
    ///
    /// This function _does not_ take the current transformation into account. If the current
    /// transformation is not aligned with the pixel grid (e.g. rotation, scaling, or translation
    /// by a subpixel amount), the result may not be pixel-aligned.
    pub fn round_to_device_pixel_center(&self, logical_coord:f64) -> f64 {
        round_to_device_pixel_center(logical_coord, self.scale_factor)
    }

    /// Rounds a logical point to the nearest physical pixel center.
    pub fn round_point_to_device_pixel_center(&self, logical: Point) -> Point {
        Point::new(
            self.round_to_device_pixel_center(logical.x),
            self.round_to_device_pixel_center(logical.y),
        )
    }

    /// Rounds a logical rectangle to the device pixel grid.
    pub fn snap_rect_to_device_pixel(&self, rect: Rect) -> Rect {
        // FIXME: either floor everything or floor/ceil
        Rect {
            x0: self.round_to_device_pixel(rect.x0),
            y0: self.round_to_device_pixel(rect.y0),
            x1: self.round_to_device_pixel(rect.x1),
            y1: self.round_to_device_pixel(rect.y1),
        }
    }

    /// Returns the skia canvas.
    ///
    /// The canvas already has the transform & clip applied.
    pub fn canvas(&mut self) -> &skia_safe::Canvas {
        self.skia.canvas()
    }

    /// Returns the current transform.
    pub fn current_transform(&self) -> Affine {
        *self.transforms.last().unwrap()
    }

    /// Saves the current clip region, transform and paint bounds.
    pub fn save(&mut self) {
        self.transforms.push(self.current_transform());
        self.skia.canvas().save();
    }

    /// Restores the current clip region, transform and paint bounds.
    pub fn restore(&mut self) {
        self.skia.canvas().restore();
        self.transforms.pop();
    }

    /// Appends to the current transform and sets new paint bounds.
    pub fn transform(&mut self, transform: &Affine) {
        self.skia.canvas().concat(&transform.to_skia());
    }

    /// Appends to the current transform and sets new paint bounds.
    pub fn translate(&mut self, offset: Vec2) {
        if self.round_to_device_pixel(offset.x) != offset.x || self.round_to_device_pixel(offset.y) != offset.y {
            warn!("translate(): offset value is not aligned to a device pixel: {:?}", offset);
        }
        self.transform(&Affine::translate(offset));
    }

    /// Clips the subsequent drawing operations by the specified rectangle.
    pub fn clip_rect(&mut self, rect: Rect) {
        self.skia
            .canvas()
            .clip_rect(rect.to_skia(), skia_safe::ClipOp::Intersect, false);
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////
    // Drawing methods
    ////////////////////////////////////////////////////////////////////////////////////////////////


    /// Draws a line.
    pub fn draw_line(&mut self, line: Line,
                     stroke_width: f64,  stroke_paint: impl Into<Paint>) {
        let mut paint = stroke_paint.into().to_sk_paint(PaintStyle::Stroke);
        paint.set_stroke_width(stroke_width as f32);
        self.canvas().draw_line(line.p0.to_skia(), line.p1.to_skia(), &paint);
    }

    /// Draws a border around or inside the specified rectangle.
    pub fn draw_border(
        &mut self,
        rrect: impl Into<RoundedRect>,
        insets: impl Into<Insets>,
        _position: BorderPosition,
        paint: impl Into<Paint>,
    ) {
        let insets = insets.into();
        if insets == Insets::ZERO {
            return;
        }

        let paint = paint.into();
        if let Paint::Color(color) = paint {
            if color.alpha() == 0.0 {
                // fully transparent border
                return;
            }
        }

        let rrect = rrect.into();
        let rect = rrect.rect();
        let mut radii = rrect.radii();
        radii.top_left -= 0.5 * insets.x0.min(insets.y0);
        radii.top_right -= 0.5 * insets.x1.min(insets.y0);
        radii.bottom_right -= 0.5 * insets.x1.min(insets.y1);
        radii.bottom_left -= 0.5 * insets.x0.min(insets.y1);
        let inner = RoundedRect::from_rect(rect - insets, radii);
        let outer = rrect;
        let paint = paint.to_sk_paint(PaintStyle::Fill);
        self.canvas().draw_drrect(outer.to_skia(), inner.to_skia(), &paint);
    }

    /// Fills the current region with the specified paint.
    pub fn fill_rect(&mut self, rect: Rect, paint: impl Into<Paint>) {
        let paint = paint.into().to_sk_paint(PaintStyle::Fill);
        self.canvas().draw_rect(rect.to_skia(), &paint);
    }

    /// Fills the current region with the specified paint.
    pub fn fill_rrect(&mut self, rrect: RoundedRect, paint: impl Into<Paint>) {
        let paint = paint.into().to_sk_paint(PaintStyle::Fill);
        self.canvas().draw_rrect(rrect.to_skia(), &paint);
    }

    /// Strokes a path.
    pub fn stroke_path(
        &mut self,
        path: impl IntoIterator<Item = PathEl>,
        stroke_width: f64,
        stroke_paint: impl Into<Paint>,
    ) {
        // TODO: build skia path directly
        let path: BezPath = BezPath::from_iter(path);
        let path = path.to_skia();
        let mut paint = stroke_paint
            .into()
            .to_sk_paint(PaintStyle::Stroke);
        paint.set_stroke_width(stroke_width as f32);
        self.canvas().draw_path(&path, &paint);
    }

    /// Fills a path.
    pub fn fill_path(
        &mut self,
        path: impl IntoIterator<Item = PathEl>,
        fill_paint: impl Into<Paint>,
    ) {
        // TODO: build skia path directly
        let path = BezPath::from_iter(path).to_skia();
        let paint = fill_paint.into().to_sk_paint(PaintStyle::Fill);
        self.canvas().draw_path(&path, &paint);
    }

    /// Draws the specified image.
    ///
    /// # Arguments
    ///
    /// * `position`: the position of the upper-left corner of the image.
    /// * `image`: the image to draw.
    pub fn draw_image(&mut self, position: Point, image: &Image) {
        // TODO image baseline?
        self.canvas().draw_image(image.to_skia(), position.to_skia(), None);
    }

    /// Draws text in the specified rectangle with the specified alignment.
    ///
    /// # Arguments
    ///
    /// * `position`: the position of the upper-left corner of the text.
    /// * `style`: the text style.
    pub fn draw_text(&mut self, bounds: Rect, style: &TextStyle, text: &[TextRun]) {
        let mut text = TextLayout::new(style, text);
        text.layout(bounds.width());
        text.paint(&self.canvas(), bounds.origin());
    }

    pub fn draw_text_layout(&mut self, position: Point, text: &TextLayout) {
        text.paint(&self.canvas(), position);
    }
}
