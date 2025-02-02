use crate::colors;
use crate::colors::DISPLAY_TEXT;
use crate::widgets::{INPUT_WIDTH, TEXT_STYLE, WIDGET_BASELINE, WIDGET_LINE_HEIGHT};
use kyute::drawing::{BASELINE_LEFT, RIGHT_CENTER, place, point, vec2};
use kyute::element::prelude::*;
use kyute::element::{ElemBox, ElementRc};
use kyute::elements::TextEdit;
use kyute::elements::draw::Visual;
use kyute::kurbo::Insets;
use kyute::kurbo::PathEl::{LineTo, MoveTo};
use kyute::model::{EventSource, Model};
use kyute::text::TextLayout;
use kyute::{IntoElementAny, Point, Rect, Size, text};

#[derive(Copy, Clone)]
pub struct SpinnerUpButtonEvent;

#[derive(Copy, Clone)]
pub struct SpinnerDownButtonEvent;

/// Two spinner buttons (up & down), standard input widget height.
struct SpinnerButtons {
    pos: Point,
}

impl SpinnerButtons {
    const SIZE: Size = Size::new(13., 16.);

    fn paint(&self, ctx: &mut PaintCtx) {
        // Upper chevron
        ctx.stroke_path(
            [
                MoveTo(self.pos + vec2(2., 6.)),
                LineTo(self.pos + vec2(6.5, 1.5)),
                LineTo(self.pos + vec2(11., 6.)),
            ],
            1.,
            DISPLAY_TEXT,
        );
        // Lower chevron
        ctx.stroke_path(
            [
                MoveTo(self.pos + vec2(2., 10.)),
                LineTo(self.pos + vec2(6.5, 14.5)),
                LineTo(self.pos + vec2(11., 10.)),
            ],
            1.,
            DISPLAY_TEXT,
        );
    }

    fn up_clicked(&self, event: &mut Event) -> bool {
        event.is_inside(Rect::new(0., 0., 13., 8.) + self.pos.to_vec2()) && event.is_pointer_down()
    }

    fn down_clicked(&self, event: &mut Event) -> bool {
        event.is_inside(Rect::new(0., 8., 13., 8.) + self.pos.to_vec2()) && event.is_pointer_down()
    }
}

/// Numeric spinner input widget.
pub struct SpinnerBase {
    value: f64,
    unit: String,
    show_background: bool,
    // This would be easier if we had exclusive ownership of the text edit,
    // but there are several considerations to take into account:
    // -
    text_edit: ElementRc<TextEdit>,
}

impl SpinnerBase {
    /// Whether to paint the background of the spinner.
    pub fn show_background(mut self: ElementBuilder<Self>, show: bool) -> ElementBuilder<Self> {
        self.show_background = show;
        self
    }

    /// Sets the unit of the spinner value.
    ///
    /// The unit is displayed after the value, e.g. "42.00 kg".
    pub fn unit(mut self: ElementBuilder<Self>, unit: impl Into<String>) -> ElementBuilder<Self> {
        self.unit = unit.into();
        self
    }

    /// Sets the current value of the spinner.
    pub fn value(mut self: ElementBuilder<Self>, value: f64) -> ElementBuilder<Self> {
        self.value = value;
        self
    }

    /// Sets the current value of the spinner.
    pub fn set_value(self: &mut ElemBox<Self>, value: f64) {
        self.value = value;

        // -> &mut ElemBox<TextEdit>
        self.map(|this| &this.text_edit).set_text(formatted);

        // problem: flags not propagated correctly when calling set_text directly
        // idea: return a RefMut like wrapper that borrows the parent ElemBox
        // and propagate the flags upwards when the RefMut is dropped

        self.ctx.mark_needs_paint();
    }

    /////////////////////////////

    fn place_buttons(&self, rect: Rect) -> SpinnerButtons {
        SpinnerButtons {
            pos: place(
                SpinnerButtons::SIZE.to_rect(),
                RIGHT_CENTER,
                rect - Insets::new(0., 2., 0., 0.),
            )
            .origin(),
        }
    }

    fn format_value(&self) -> String {
        format!("{:.2} {}", self.value, self.unit)
    }
}

impl Element for SpinnerBase {
    fn children(&self) -> Vec<ElementAny> {
        vec![self.text_edit.clone()]
    }

    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        // fill the available width, use the fixed height
        let width = layout_input.width.available().unwrap_or(INPUT_WIDTH);
        let height = WIDGET_LINE_HEIGHT;
        Size { width, height }
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: Some(WIDGET_BASELINE),
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.rect.contains(point)
    }

    fn paint(self: &mut ElemBox<Self>, ctx: &mut PaintCtx) {
        // paint background
        if self.show_background {
            ctx.fill_rrect(ctx.bounds().to_rounded_rect(4.), colors::DISPLAY_BACKGROUND);
        }

        // format & paint value
        //let value_fmt = self.format_value();
        //ctx.pad_left(4.);
        //ctx.draw_text(BASELINE_LEFT, &TEXT_STYLE, text![Color(DISPLAY_TEXT) "{value_fmt}"]);
        self.content.paint(ctx);

        // paint buttons
        let buttons = self.place_buttons(ctx.bounds());
        buttons.paint(ctx);
    }

    fn event(self: &mut ElemBox<Self>, _ctx: &mut WindowCtx, event: &mut Event) {
        let buttons = self.place_buttons(self.ctx.rect());
        if buttons.up_clicked(event) {
            self.ctx.emit(SpinnerUpButtonEvent);
        } else if buttons.down_clicked(event) {
            self.ctx.emit(SpinnerDownButtonEvent);
        } else {
            // Handle mouse events over the spinner value
        }
    }
}

impl SpinnerBase {
    pub fn new() -> ElementBuilder<Self> {
        let text_edit = TextEdit::new();

        ElementBuilder::new_cyclic(|weak_this| SpinnerBase {
            value: 0.,
            unit: String::new(),
            show_background: true,
            content: text_edit.into_element(weak_this, 0),
        })
    }
}
