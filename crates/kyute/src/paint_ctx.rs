use crate::compositor::CompositionBuilder;
use crate::drawing::{round_to_device_pixel, round_to_device_pixel_center, BorderPosition, Image, Paint, ToSkia};
use crate::text::{TextLayout, TextRun, TextStyle};
use crate::{Color, NodeCtx, RcDynNode};
use kurbo::{Affine, BezPath, Insets, Line, PathEl, Point, Rect, RoundedRect, Vec2};
use skia_safe::PaintStyle;
use crate::node::{with_tree_ctx, ChangeFlags};

#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::IDXGISwapChain3;

/// Paint context.
/// 
/// TODO deref to `TreeCtx`
pub struct PaintCtx<'a> {
    pub tree: &'a NodeCtx<'a>,
    pub scale_factor: f64,
    /// Bounds of the element being painted, relative to the current drawing surface.
    pub bounds: Rect,

    /// Transform from local coordinates to window coordinates.
    window_transform: Affine,

    /// Transform from local coordinates to parent layer coordinates.
    layer_transform: Affine,

    comp_builder: &'a mut CompositionBuilder,
    //surface: Option<DrawableSurface>,
}


impl<'a> std::ops::Deref for PaintCtx<'a> {
    type Target = NodeCtx<'a>;

    fn deref(&self) -> &Self::Target {
        self.tree
    }
}

impl<'a> PaintCtx<'a> {
    /// Creates a new paint context.
    pub(crate) fn new(ctx: &'a NodeCtx<'a>, comp_builder: &'a mut CompositionBuilder) -> PaintCtx<'a> {
        PaintCtx {
            tree: ctx,
            scale_factor: comp_builder.scale_factor(),
            bounds: ctx.this.size().to_rect(),
            window_transform: Default::default(),
            layer_transform: Default::default(),
            comp_builder,
        }
    }

    pub fn paint_child(&mut self, element: &RcDynNode) {
        self.paint_child_with_offset(element.offset(), element);
    }

    /// Paints a child element.
    pub fn paint_child_with_offset(&mut self, offset: Vec2, element: &RcDynNode) {
        // create the child painting context
        let tree = self.tree.with_child(element);

        // update window-relative position
        tree.this.window_position.set(self.bounds.origin() + tree.this.offset());

        // update current bounds on the composition builder
        self.comp_builder.set_bounds(element.bounds());

        let mut child_ctx = PaintCtx {
            tree: &tree,
            scale_factor: self.scale_factor,
            bounds: element.bounds(),
            window_transform: self.window_transform * Affine::translate(offset),
            layer_transform: self.layer_transform * Affine::translate(offset),
            comp_builder: self.comp_builder,
        };

        // remove dirty flags before painting, in case the child element (or a descendant) sets
        // them again to request a repaint immediately after this one
        let mut f = tree.this.change_flags.get();
        f.remove(ChangeFlags::PAINT);
        tree.this.change_flags.set(f);

        // paint the child element
        element.borrow_mut().paint(&mut child_ctx);

        // restore the previous bounds
        self.comp_builder.set_bounds(self.bounds);
    }

    pub fn finish_picture_layer(&mut self) {
        self.comp_builder.finish_record_and_push_picture_layer();
        //self.comp_builder.add_swap_chain(pos, swap_chain);
    }

    #[cfg(windows)]
    pub fn add_swap_chain(&mut self, pos: Point, swap_chain: IDXGISwapChain3) {
        self.comp_builder.add_swap_chain(pos, swap_chain);
    }
    

    /*
    /// Creates a color filter layer and invokes the drawing callback with the new context.
    pub fn with_color_filter_layer(&mut self, _color: Color, _callback: impl FnOnce(&mut Self)) {
        // ISSUE: naively, we could allocate a texture, perform further rendering in the texture,
        // and then apply the color filter when rendering the texture.
        // However, we don't know in advance the size of the texture to allocate.
        // Furthermore, child elements may push native layers, which would ignore the color filter.

        self.finish_layer();
        self.scene_builder.push_filter_layer(color);

        self.scene_builder.pop();
        let layer = compositor::FilterLayer::new(color);
        warn!("with_color_filter_layer not implemented");
    }*/

    // push_filter ->
    //    close current drawing
    //    start a new composition layer
    //    draw into the new layer
    //
    // pop ->
    //    apply the filter
    //    draw the filtered image into the previous layer
    //
    // add_native_layer ->
    //    close current drawing
    //    add native layer to parent layer
    //        ** ISSUE: parent layer may be a skia surface without a native layer, with a filter that's not supported by the native layer

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
    // - (E) let paint methods apply the transform and round the coordinates themselves
    //
    // The secondary pain point is the representation of logical pixels, aka "layout units"
    // - should we support fractional layout units? firefox & webkit use 1/60th of a CSS pixel
    //
    // Conclusion:
    // We make the assumption that the current transform is a translation with an offset that is a multiple of a physical pixel size in logical units
    // - all transformations are translations with an offset that is a multiple of a physical pixel
    //
    // Alternate approach:
    // It's extremely unlikely that we'll ever need to support scaling and rotation transformations
    // in the paint context. So we could just replace `Affine` transforms with a simple offset,
    // and let the paint methods round the values themselves.
    // In case scaling / rotation is needed, we won't care about pixel snapping anyway.

    /// Rounds a logical coordinate to the nearest physical pixel boundary.
    ///
    /// This function _does not_ take the current transformation into account. If the current
    /// transformation is not aligned with the pixel grid (e.g. rotation, scaling, or translation
    /// by a subpixel amount), the result may not be pixel-aligned.
    pub fn round_to_device_pixel(&self, logical_coord: f64) -> f64 {
        round_to_device_pixel(logical_coord, self.scale_factor)
    }

    pub fn floor_to_device_pixel(&self, logical_coord: f64) -> f64 {
        (logical_coord * self.scale_factor).floor() / self.scale_factor
    }

    pub fn ceil_to_device_pixel(&self, logical_coord: f64) -> f64 {
        (logical_coord * self.scale_factor).ceil() / self.scale_factor
    }

    /// Rounds a logical coordinate to the center of the nearest physical pixel.
    ///
    /// This function _does not_ take the current transformation into account. If the current
    /// transformation is not aligned with the pixel grid (e.g. rotation, scaling, or translation
    /// by a subpixel amount), the result may not be pixel-aligned.
    pub fn round_to_device_pixel_center(&self, logical_coord: f64) -> f64 {
        round_to_device_pixel_center(logical_coord, self.scale_factor)
    }

    /// Rounds a logical point to the nearest physical pixel center.
    pub fn round_point_to_device_pixel_center(&self, logical: Point) -> Point {
        Point::new(
            self.round_to_device_pixel_center(logical.x),
            self.round_to_device_pixel_center(logical.y),
        )
    }

    /// Returns the smallest rectangle aligned to the device pixel grid that contains the input rectangle.
    pub fn snap_rect_to_device_pixel(&self, rect: Rect) -> Rect {
        Rect {
            x0: self.floor_to_device_pixel(rect.x0),
            y0: self.floor_to_device_pixel(rect.y0),
            x1: self.ceil_to_device_pixel(rect.x1),
            y1: self.ceil_to_device_pixel(rect.y1),
        }
    }

    /// Returns the skia canvas.
    ///
    /// The canvas already has the transform & clip applied.
    pub fn canvas(&mut self) -> &skia_safe::Canvas {
        self.comp_builder.picture_recorder().recording_canvas().unwrap()
    }

    // Returns the current transform.
    //pub fn current_transform(&self) -> Affine {
    //    *self.transforms.last().unwrap()
    //}

    /// Clips the subsequent drawing operations by the specified rectangle.
    ///
    /// FIXME: if anything down the line needs compositing layers, the clip won't be taken into account
    /// FIXME: if the picture recorder is reset (because a new layer started), the clip will be lost
    ///        i.e. clips don't transfer to new layers
    pub fn clip_rect(&mut self, rect: Rect) {
        // TODO:  if child elements need compositing, use a clip layer instead
        self.canvas()
            .clip_rect(rect.to_skia(), skia_safe::ClipOp::Intersect, false);
    }

    ////////////////////////////////////////////////////////////////////////////////////////////////
    // Drawing methods
    // TODO this should not be part of PaintCtx as it is not specific to elements 
    ////////////////////////////////////////////////////////////////////////////////////////////////

    pub fn clear(&mut self, color: Color) {
        self.canvas().clear(color.to_skia());
    }

    /// Draws a line.
    pub fn draw_line(&mut self, line: Line, stroke_width: f64, stroke_paint: impl Into<Paint>) {
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
        let mut paint = stroke_paint.into().to_sk_paint(PaintStyle::Stroke);
        paint.set_stroke_width(stroke_width as f32);
        self.canvas().draw_path(&path, &paint);
    }

    /// Fills a path.
    pub fn fill_path(&mut self, path: impl IntoIterator<Item = PathEl>, fill_paint: impl Into<Paint>) {
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
