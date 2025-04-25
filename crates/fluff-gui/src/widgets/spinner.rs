use crate::colors::DISPLAY_TEXT;
use crate::widgets::menu::ContextMenuExt;
use crate::widgets::{DISPLAY_TEXT_STYLE, INPUT_WIDTH, PaintExt, WIDGET_BASELINE, WIDGET_LINE_HEIGHT};
use kyute::drawing::{BorderPosition, PlacementExt, RIGHT_CENTER, vec2};
use kyute::element::TreeCtx;
use kyute::element::prelude::*;
use kyute::elements::{TextEditBase, ValueChangedEvent};
use kyute::input_event::{Key, PointerButton, ScrollDelta};
use kyute::kurbo::PathEl::{LineTo, MoveTo};
use kyute::kurbo::{Insets, Vec2};
use kyute::event::EventSource;
use kyute::text::Selection;
use kyute::{Color, Point, Rect, Size};

#[derive(Copy, Clone)]
pub struct SpinnerUpButtonEvent;

#[derive(Copy, Clone)]
pub struct SpinnerDownButtonEvent;

/// Two spinner buttons (up & down), standard input widget height.
struct SpinnerButtons {
    pos: Point,
}

const PADDING: Vec2 = vec2(4., 2.);

#[derive(Copy, Clone, Eq, PartialEq)]
enum Part {
    UpButton,
    DownButton,
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

    fn hit_test(&self, point: Point) -> Option<Part> {
        if Rect::new(0., 0., 13., 8.).contains(point - self.pos.to_vec2()) {
            Some(Part::UpButton)
        } else if Rect::new(0., 8., 13., 16.).contains(point - self.pos.to_vec2()) {
            Some(Part::DownButton)
        } else {
            None
        }
    }
}

pub struct SpinnerOptions<'a> {
    pub initial_value: f64,
    pub unit: &'a str,
    pub increment: f64,
    /// Number of decimal places to display.
    pub precision: u8,
    /// Whether to clamp all values to the nearest integer.
    pub clamp_to_integer: bool,
}

impl<'a> Default for SpinnerOptions<'a> {
    fn default() -> Self {
        SpinnerOptions {
            initial_value: 0.,
            unit: "",
            increment: 1.,
            precision: 2,
            clamp_to_integer: false,
        }
    }
}

/// Numeric spinner input widget.
pub struct SpinnerBase {
    weak: WeakElement<Self>,
    /// The value to display.
    value: f64,
    /// The value before editing began.
    value_before_editing: f64,
    show_background: bool,
    text_edit: TextEditBase,
    /// Number of decimal places to display.
    precision: u8,
    /// Whether to clamp all values to the nearest integer.
    clamp_to_integer: bool,
    unit: String,
    increment: f64,
    /// Currently editing the value
    editing: bool,
}

impl SpinnerBase {
    pub fn new(options: SpinnerOptions) -> ElementBuilder<Self> {
        let mut text_edit = TextEditBase::new();
        text_edit.set_text_style(DISPLAY_TEXT_STYLE.clone());
        text_edit.set_caret_color(DISPLAY_TEXT);

        let mut spinner = ElementBuilder::new_cyclic(|weak| SpinnerBase {
            weak,
            value: options.initial_value,
            value_before_editing: options.initial_value,
            show_background: true,
            text_edit,
            precision: options.precision,
            clamp_to_integer: options.clamp_to_integer,
            unit: options.unit.into(),
            increment: options.increment,
            editing: false,
        });
        let str = spinner.format_value();
        spinner.text_edit.set_text(str);
        spinner
    }

    /// Whether to paint the background of the spinner.
    pub fn show_background(mut self: ElementBuilder<Self>, show: bool) -> ElementBuilder<Self> {
        self.show_background = show;
        self
    }

    /// Sets the text color of the spinner.
    pub fn set_text_color(mut self: ElementBuilder<Self>, color: Color) -> ElementBuilder<Self> {
        self.text_edit.set_text_color(color);
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
    pub fn value(mut self: ElementBuilder<Self>, mut value: f64) -> ElementBuilder<Self> {
        // FIXME: deduplicate
        if self.clamp_to_integer {
            value = value.round();
        }
        self.value = value;
        let str = self.format_value();
        self.text_edit.set_text(str);
        self
    }

    /// Sets the current value of the spinner.
    pub fn set_value(&mut self, cx: &TreeCtx, mut value: f64) {
        if self.clamp_to_integer {
            value = value.round();
        }
        self.value = value;
        cx.emit(ValueChangedEvent(value));
        self.update_text(cx);
    }

    /////////////////////////////

    fn update_text(&mut self, cx: &TreeCtx) {
        let str = self.format_value();
        self.text_edit.set_text(str);
        cx.mark_needs_layout();
    }

    fn place_buttons(&self, rect: Rect) -> SpinnerButtons {
        SpinnerButtons {
            pos: SpinnerButtons::SIZE.place_into(rect - Insets::new(0., 0., 2., 0.), RIGHT_CENTER),
        }
    }

    fn format_value(&self) -> String {
        if self.editing {
            // don't display the unit during editing
            format!("{0:.1$}", self.value, self.precision as usize)
        } else {
            format!("{0:.1$} {2}", self.value, self.precision as usize, self.unit)
        }
    }

    fn is_valid_key_input(key: &Key) -> bool {
        match key {
            Key::Character(c) => {
                if let Some(c) = c.chars().next() {
                    if !c.is_digit(10) && c != '.' && c != '-' {
                        return false;
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn handle_scroll(&mut self, cx: &TreeCtx, scroll_delta_lines: f64) {
        if self.editing {
            let text = self.text_edit.text();
            if !text.parse::<f64>().is_ok() {
                return;
            }

            // TODO: edge case:
            //       -1.00
            //       ^ cursor here
            //       If we scroll up the increment is determined to be 10 (we're two characters away from the decimal point)
            //       and the value becomes 9.00. Not sure what we should do when scrolling at the minus sign.
            // TODO: there should be unit tests; this is not complicated to test

            // determine which decimal place to change
            // determine position relative to the decimal point
            let selection = self.text_edit.selection();
            let cursor_from_end = text.len() - selection.end;
            let point = text.find('.').unwrap_or(text.len());
            let mut decimal_place = point as i32 - selection.end as i32;
            if decimal_place > 0 {
                // cursor to the left of the decimal point
                decimal_place -= 1;
            }
            let increment = 10_f64.powi(decimal_place) * scroll_delta_lines.signum();
            let new_value = self.value + increment;
            self.set_value(cx, new_value);
            // we may have added digits to the left, reset the cursor position
            // relative to the end
            let new_len = self.text_edit.text_len();
            self.text_edit
                .set_selection(Selection::empty(new_len.saturating_sub(cursor_from_end)));
        } else {
            let new_value = self.value + self.increment * scroll_delta_lines.signum();
            self.set_value(cx, new_value);
        }
    }

    fn handle_context_menu(&mut self, entry: i32) {
        eprintln!("Context menu entry: {:?}", entry);
    }
}

impl Element for SpinnerBase {
    fn measure(&mut self, _cx: &TreeCtx, layout_input: &LayoutInput) -> Size {
        let width = layout_input.width.available().unwrap_or(INPUT_WIDTH);
        let height = WIDGET_LINE_HEIGHT;
        Size { width, height }
    }

    fn layout(&mut self, _cx: &TreeCtx, size: Size) -> LayoutOutput {
        let baseline = self.text_edit.layout(size).baseline.unwrap_or(0.);
        self.text_edit.set_offset(vec2(PADDING.x, WIDGET_BASELINE - baseline));

        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: Some(WIDGET_BASELINE),
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ecx: &TreeCtx, ctx: &mut PaintCtx) {
        let bounds = ecx.bounds();
        let rrect = bounds.to_rounded_rect(0.);

        // paint background
        if self.show_background {
            ctx.draw_display_background(bounds);
        }

        // contents
        self.text_edit.paint(ctx, bounds);

        if ecx.has_focus() {
            // draw the focus ring
            ctx.draw_border(rrect, 1., BorderPosition::Inside, DISPLAY_TEXT.darken(0.2));
        }

        // paint buttons
        let buttons = self.place_buttons(bounds);
        buttons.paint(ctx);
    }

    fn event(&mut self, cx: &TreeCtx, event: &mut Event) {
        let bounds = cx.bounds();
        let buttons = self.place_buttons(cx.bounds());

        match event {
            // filter non-numeric input
            Event::KeyDown(key) | Event::KeyUp(key) => {
                if !Self::is_valid_key_input(&key.key) {
                    return;
                }
            }

            // mouse wheel
            Event::Wheel(wheel) => match wheel.delta {
                ScrollDelta::Lines { y, .. } => {
                    self.handle_scroll(cx, y);
                }
                _ => {}
            },

            Event::PointerDown(p) => {
                // acquire focus on pointer down
                cx.set_focus();
                cx.set_pointer_capture();

                // check for button clicks
                // let local_pos = (p.position - cx.bounds().origin()).to_point();

                match buttons.hit_test(p.position) {
                    Some(Part::UpButton) => {
                        self.set_value(cx, self.value + self.increment);
                        cx.mark_needs_layout();
                        return;
                    }
                    Some(Part::DownButton) => {
                        self.set_value(cx, self.value - self.increment);
                        cx.mark_needs_layout();
                        return;
                    }
                    None => {}
                }

                // context menu
                if p.buttons.test(PointerButton::RIGHT) {
                    use crate::widgets::menu::ContextMenuExt;
                    use crate::widgets::menu::MenuItem::{Entry, Separator, Submenu};

                    let weak = self.weak.clone();

                    cx.open_context_menu(
                        p.position,
                        &[
                            Entry("Copy", 0),
                            Entry("Cut", 1),
                            Entry("Paste", 2),
                            Separator,
                            Entry("Delete", 3),
                            Separator,
                            Submenu(
                                "Advanced",
                                &[
                                    Entry("Advanced 1", 5),
                                    Submenu("Advanced 2", &[Entry("Advanced 2.1", 7), Entry("Advanced 2.2", 8)]),
                                    Submenu("Advanced 3", &[Entry("Advanced 3.1", 9), Entry("Advanced 3.2", 10)]),
                                    Entry("Advanced 4", 11),
                                ],
                            ),
                        ],
                        move |entry| {
                            if let Some(this) = weak.upgrade() {
                                this.invoke(move |this, _cx| this.handle_context_menu(entry));
                            }
                        },
                    );
                }
            }

            Event::FocusGained => {
                // Focus gained either by pointer down or by tabbing
                // Go in editing mode
                self.editing = true;
                self.value_before_editing = self.value;
                self.update_text(cx);
            }

            _ => {}
        }

        // pass events to the text edit
        let r = self.text_edit.event(bounds, event);

        // clamp selection to the actual numbers, not the unit
        if r.selection_changed() {
            //let mut selection = self.text_edit.selection();
            //let unit_len = self.unit.len() + 1; // +1 for the space
            //let text_len = self.text_edit.text().len();
            //let text_without_unit = text_len - unit_len;
            //selection.start = selection.start.min(text_without_unit);
            //selection.end = selection.end.min(text_without_unit);
            //self.text_edit.set_selection(selection);
        }
        if r.text_changed() {
            // TODO: parse and update
        }
        if r.relayout() {
            cx.mark_needs_layout();
        }
        if r.repaint() {
            cx.mark_needs_paint();
        }
        if r.reset_blink() {
            //self.text_edit.reset_blink();
        }
        if matches!(event, Event::FocusLost) || r.confirmed() {
            // When the spinner loses focus, or the user presses enter
            // keep the current value and exit editing mode
            self.editing = false;
            cx.clear_focus();
            // Parse value and update
            let text = self.text_edit.text();
            if let Some(value) = text.parse().ok() {
                self.set_value(cx, value);
            } else {
                // restore the text
                self.update_text(cx);
            }
        }
        if r.cancelled() {
            // user pressed escape
            // restore the previous value
            self.editing = false;
            cx.clear_focus();
            self.set_value(cx, self.value_before_editing);
        }
    }
}

/*
impl SpinnerBase {
    pub fn new() -> ElementBuilder<Self> {
        let mut text_edit = TextEditBase::new();
        text_edit.set_text_style(DISPLAY_TEXT_STYLE.clone());
        text_edit.set_caret_color(DISPLAY_TEXT);

        ElementBuilder::new(SpinnerBase {
            value: 0.,
            unit: String::new(),
            show_background: true,
            text_edit,
            formatting: Formatting::Integer,
        })
    }
}
*/
