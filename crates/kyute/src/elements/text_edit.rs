use bitflags::bitflags;
use keyboard_types::Key;
use kurbo::{Point, Rect, Size, Vec2};
use skia_safe::textlayout::{RectHeightStyle, RectWidthStyle};
use skia_safe::PaintStyle;
use std::ops::Range;
use tracing::{trace_span, warn};
use unicode_segmentation::GraphemeCursor;

use crate::drawing::{FromSkia, Paint, ToSkia};
use crate::element::HitTestCtx;
use crate::input_event::Event;
use crate::layout::{LayoutInput, LayoutOutput};
use crate::text::{get_font_collection, Selection, TextAlign, TextLayout, TextStyle};
use crate::{Color, PaintCtx};

#[derive(Debug, Copy, Clone)]
pub enum Movement {
    Left,
    Right,
    LeftWord,
    RightWord,
}

fn prev_grapheme_cluster(text: &str, offset: usize) -> Option<usize> {
    let mut c = GraphemeCursor::new(offset, text.len(), true);
    c.prev_boundary(text, 0).unwrap()
}

fn next_grapheme_cluster(text: &str, offset: usize) -> Option<usize> {
    let mut c = GraphemeCursor::new(offset, text.len(), true);
    c.next_boundary(text, 0).unwrap()
}

/// If `other` comes before `self`, the cursor is placed at the beginning of the selection.
fn add_selections(this: Selection, other: Selection) -> Selection {
    let min = this.min().min(other.min());
    let max = this.max().max(other.max());
    if other.min() < this.min() {
        Selection { start: max, end: min }
    } else {
        Selection { start: min, end: max }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum WrapMode {
    Wrap,
    NoWrap,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TextOverflow {
    Ellipsis,
    Clip,
}

fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut off = 0;
    for (i, c) in text.char_indices() {
        if off == utf16_offset {
            return i;
        }
        off += c.len_utf16();
    }
    if off == utf16_offset {
        return text.len();
    }
    warn!("invalid utf16 offset (out of bounds or not at a code unit boundary)");
    text.len()
}

fn utf8_to_utf16_offset(text: &str, utf8_offset: usize) -> usize {
    let mut off = 0;
    for (i, c) in text.char_indices() {
        if i == utf8_offset {
            return off;
        }
        off += c.len_utf16();
    }
    if text.len() == utf8_offset {
        return off;
    }
    warn!("invalid utf8 offset (out of bounds or not at a code unit boundary)");
    off
}

impl TextEditBase {
    fn rebuild_paragraph(&mut self) {
        let font_collection = get_font_collection();
        let mut text_style = skia_safe::textlayout::TextStyle::new();
        text_style.set_font_size(16.0); // TODO default font size
        let mut paragraph_style = skia_safe::textlayout::ParagraphStyle::new();
        paragraph_style.set_text_style(&text_style);
        paragraph_style.set_apply_rounding_hack(false);

        if let Some(line_clamp) = self.line_clamp {
            paragraph_style.set_max_lines(line_clamp);
        }

        match self.text_overflow {
            TextOverflow::Ellipsis => {
                paragraph_style.set_ellipsis("…");
            }
            TextOverflow::Clip => {}
        }

        paragraph_style.set_text_align(self.align.to_skia());

        let mut builder = skia_safe::textlayout::ParagraphBuilder::new(&paragraph_style, font_collection);
        let style = self.text_style.to_skia();
        builder.push_style(&style);
        builder.add_text(&self.text);
        builder.pop();

        self.paragraph = builder.build();
    }

    pub fn set_selection(&mut self, selection: Selection) -> bool {
        if selection != self.selection {
            self.selection = selection;
            self.scroll_in_view(self.selection.end);

            // debug the current cursor position by printing the string with <HERE> at the cursor position
            let mut text = self.text.clone();
            text.insert_str(self.selection.end, "<HERE>");
            eprintln!("cursor changed position: {text}");

            true
        } else {
            false
        }
    }

    pub fn set_cursor_at_text_position(&mut self, pos: usize, keep_anchor: bool) -> bool {
        self.set_selection(if keep_anchor {
            Selection {
                start: self.selection.start,
                end: pos,
            }
        } else {
            Selection::empty(pos)
        })
    }

    pub fn move_cursor_to_next_word(&mut self, keep_anchor: bool) -> bool {
        let end = next_word_boundary(&self.text, self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    pub fn move_cursor_to_prev_word(&mut self, keep_anchor: bool) -> bool {
        let end = prev_word_boundary(&self.text, self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    pub fn move_cursor_to_next_grapheme(&mut self, keep_anchor: bool) -> bool {
        let end = next_grapheme_cluster(&self.text, self.selection.end).unwrap_or(self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    pub fn move_cursor_to_prev_grapheme(&mut self, keep_anchor: bool) -> bool {
        let end = prev_grapheme_cluster(&self.text, self.selection.end).unwrap_or(self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    fn utf16_to_utf8_offset(&self, utf16_offset: usize) -> usize {
        utf16_to_utf8_offset(&self.text, utf16_offset)
    }

    fn utf8_to_utf16_offset(&self, utf8_offset: usize) -> usize {
        utf8_to_utf16_offset(&self.text, utf8_offset)
    }

    /// Scrolls the text to make the given text position visible.
    pub fn scroll_in_view(&mut self, text_offset: usize) -> bool {
        let rects = self.paragraph.get_rects_for_range(
            self.utf8_to_utf16_offset(text_offset)..self.utf8_to_utf16_offset(text_offset),
            RectHeightStyle::Tight,
            RectWidthStyle::Tight,
        );
        if rects.is_empty() {
            return false;
        }
        let rect = Rect::from_skia(rects[0].rect);
        let scroll_offset = self.scroll_offset.x;

        let scroll_offset = if rect.x1 > self.size.width + scroll_offset {
            rect.x1 - self.size.width
        } else if rect.x0 < scroll_offset {
            rect.x0
        } else {
            scroll_offset
        };
        if scroll_offset != self.scroll_offset.x {
            self.scroll_offset.x = scroll_offset;
            true
        } else {
            false
        }
    }

    pub fn text_position_for_point(&self, point: Point) -> usize {
        let point = point + self.scroll_offset;
        // NOTE: get_glyph_position_at_coordinate returns a text position in UTF16 code units,
        // supposedly for compatibility with flutter, dart, or javascript. In Skia's docs
        // those are sometimes confusingly referred to as "glyph indices".
        // We use rust's standard UTF-8 strings, so unfortunately we need to pay the price.
        //
        // Hilariously, some methods do take a UTF-8 offset (getLineNumberAt).
        //
        // SkParagraph internally has its own mapping table, but we can't access it from
        // the public API. Hopefully at some point we'll be able to ditch skia for something better.
        self.utf16_to_utf8_offset(
            self.paragraph
                .get_glyph_position_at_coordinate(point.to_skia())
                .position as usize,
        )
    }

    /// NOTE: valid only after first layout.
    pub fn set_cursor_at_point(&mut self, point: Point, keep_anchor: bool) -> bool {
        //let point = point + self.scroll_offset;
        //let pos = self.paragraph.get_glyph_position_at_coordinate(point.to_skia());
        let pos = self.text_position_for_point(point);
        self.set_cursor_at_text_position(pos, keep_anchor)
    }

    fn get_word_boundary(&self, pos: usize) -> Range<usize> {
        // Using a pair of conversion functions, convert back and forth between utf8 and utf16 offsets.
        // This kills the performance.
        let range = self.paragraph.get_word_boundary(self.utf8_to_utf16_offset(pos) as u32);
        Range {
            start: self.utf16_to_utf8_offset(range.start),
            end: self.utf16_to_utf8_offset(range.end),
        }
    }

    pub fn select_word_under_cursor(&mut self) -> bool {
        let selection = self.selection;
        let range = self.get_word_boundary(selection.end);
        self.set_selection(Selection {
            start: range.start,
            end: range.end,
        })
    }

    pub fn word_selection_at_text_position(&self, pos: usize) -> Selection {
        let range = self.get_word_boundary(pos);
        let word = Selection {
            start: range.start,
            end: range.end,
        };
        // skia reports a word boundary for newlines, ignore it
        if self.text[range.clone()].starts_with('\n') {
            return Selection::empty(pos);
        }
        word
    }

    pub fn select_line_under_cursor(&mut self) -> bool {
        let text = &self.text;
        let selection = self.selection;
        let start = text[..selection.end].rfind('\n').map_or(0, |i| i + 1);
        let end = text[selection.end..]
            .find('\n')
            .map_or(text.len(), |i| selection.end + i);
        self.set_selection(Selection { start, end })
    }
}

//const CARET_BLINK_INITIAL_DELAY: Duration = Duration::from_secs(1);

fn next_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = offset;
    enum State {
        LeadingWhitespace,
        Alnum,
        NotAlnum,
    }
    let mut state = State::LeadingWhitespace;
    for ch in text[offset..].chars() {
        match state {
            State::LeadingWhitespace => {
                if !ch.is_whitespace() {
                    if ch.is_alphanumeric() {
                        state = State::Alnum;
                    } else {
                        state = State::NotAlnum;
                    }
                }
            }
            State::Alnum => {
                if !ch.is_alphanumeric() {
                    return pos;
                }
            }
            State::NotAlnum => {
                return pos;
            }
        }
        pos += ch.len_utf8();
    }
    pos
}

fn prev_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = offset;
    enum State {
        LeadingWhitespace,
        Alnum,
        NotAlnum,
    }
    let mut state = State::LeadingWhitespace;
    for ch in text[..offset].chars().rev() {
        match state {
            State::LeadingWhitespace => {
                if !ch.is_whitespace() {
                    if ch.is_alphanumeric() {
                        state = State::Alnum;
                    } else {
                        state = State::NotAlnum;
                    }
                }
            }
            State::Alnum => {
                if !ch.is_alphanumeric() {
                    return pos;
                }
            }
            State::NotAlnum => {
                return pos;
            }
        }
        pos -= ch.len_utf8();
    }
    pos
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Gesture {
    CharacterSelection,
    WordSelection { anchor: Selection },
}

/// Single- or multiline text editor.
#[allow(dead_code)]
pub struct TextEditBase {
    //selection_changed: Notifier<Selection>,
    offset: Vec2,
    text: String,
    selection: Selection,
    text_style: TextStyle<'static>,
    last_available_width: f64,
    paragraph: skia_safe::textlayout::Paragraph,
    selection_color: Color,
    caret_color: Color,
    relayout: bool,
    scroll_offset: Vec2,
    wrap_mode: WrapMode,
    text_overflow: TextOverflow,
    align: TextAlign,
    show_caret: bool,
    line_clamp: Option<usize>,
    size: Size,
    gesture: Option<Gesture>,
    blink_phase: bool,
    blink_pending_reset: bool,
}

impl TextEditBase {
    pub fn new() -> TextEditBase {
        TextEditBase {
            offset: Default::default(),
            text: String::new(),
            selection: Selection::empty(0),
            text_style: TextStyle::default(),
            last_available_width: 0.0,
            paragraph: TextLayout::default().inner,
            selection_color: Color::from_rgba_u8(0, 0, 255, 80),
            caret_color: Color::from_rgba_u8(255, 255, 0, 255),
            relayout: true,
            scroll_offset: Vec2::new(0.0, 0.0),
            wrap_mode: WrapMode::Wrap,
            align: TextAlign::Start,
            size: Default::default(),
            text_overflow: TextOverflow::Clip,
            line_clamp: None,
            blink_phase: true,
            blink_pending_reset: false,
            gesture: None,
            show_caret: false,
        }
    }

    pub fn set_offset(&mut self, offset: Vec2) {
        self.offset = offset;
    }

    pub fn set_text_color(&mut self, color: Color) {
        self.text_style.color = color;
        self.rebuild_paragraph();
        self.relayout = true;
    }

    /*pub fn reset_blink_cursor(self: &mut ElemBox<Self>) {
        if self.blink_pending_reset {
            return;
        }
        self.blink_phase = true;
        self.blink_pending_reset = true;
        self.ctx.mark_needs_paint();
        // Initial delay before blinking
        self.run_after(CARET_BLINK_INITIAL_DELAY, move |this| {
            info!("past caret blink delay");
            this.blink_pending_reset = false;
            this.blink_cursor();
        });
    }

    pub fn blink_cursor(self: &mut ElemBox<Self>) {
        if self.blink_pending_reset {
            // ignore this blink event, a reset is pending, we're in the initial delay
            return;
        }

        self.blink_phase = !self.blink_phase;
        if self.ctx.has_focus() {
            info!("blink");
            self.ctx.mark_needs_paint();
        }
        let caret_blink_time = AppGlobals::get().caret_blink_time();
        self.run_after(caret_blink_time, TextEditBase::blink_cursor);
    }*/

    pub fn set_wrap_mode(&mut self, wrap_mode: WrapMode) -> bool {
        if self.wrap_mode != wrap_mode {
            self.wrap_mode = wrap_mode;
            self.rebuild_paragraph();
            self.relayout = true;
            true
        } else {
            false
        }
    }

    pub fn set_max_lines(&mut self, max_lines: usize) {
        self.line_clamp = Some(max_lines);
        self.rebuild_paragraph();
        self.relayout = true;
    }

    pub fn set_text_align(&mut self, align: TextAlign) {
        if self.align != align {
            self.align = align;
            self.rebuild_paragraph();
            self.relayout = true;
        }
    }

    pub fn set_overflow(&mut self, overflow: TextOverflow) {
        if self.text_overflow != overflow {
            self.text_overflow = overflow;
            self.rebuild_paragraph();
            self.relayout = true;
        }
    }

    /*/// Resets the phase of the blinking caret.
    pub fn reset_blink(&mut self) {
        self.blink_phase.set(true);
        self.blink_pending_reset.set(true);
        self.ctx.mark_needs_paint();
    }*/

    pub fn set_caret_color(&mut self, color: Color) {
        if self.caret_color != color {
            self.caret_color = color;
        }
    }

    pub fn set_selection_color(&mut self, color: Color) {
        if self.selection_color != color {
            self.selection_color = color;
        }
    }

    pub fn set_text_style(&mut self, text_style: TextStyle) {
        self.text_style = text_style.into_static();
        self.rebuild_paragraph();
        self.relayout = true;
    }

    /// Returns the current selection.
    pub fn selection(&self) -> Selection {
        self.selection
    }

    /// Returns the current text.
    pub fn text(&self) -> String {
        self.text.clone()
    }

    /// Returns the length of the text.
    pub fn text_len(&self) -> usize {
        self.text.len()
    }

    /// Sets the current text.
    pub fn set_text(&mut self, text: impl Into<String>) {
        // TODO we could compare the previous and new text
        // to relayout only affected lines.
        self.text = text.into();
        // clamp selection to the new text length
        self.selection = self.selection.clamp(0..self.text.len());

        self.rebuild_paragraph();
        self.relayout = true;
    }

    /*pub fn select_word_at_offset_with_anchor(&self, offset: usize, anchor_selection: Selection) -> bool {
        let this = &mut *self.state.borrow_mut();
        let range = this.paragraph.get_word_boundary(offset as u32);
        let word = Selection {
            start: range.start,
            end: range.end,
        };

        // skia reports a word boundary for newlines, ignore it
        if this.text[range.clone()].starts_with('\n') {
            return false;
        }

        let new_selection = add_selections(anchor_selection, word);


        if new_selection != this.selection {
            this.selection = new_selection;
            this.scroll_in_view(this.selection.end);


            self.mark_needs_repaint();
            true
        } else {
            false
        }
    }*/

    /*/// Emitted when the selection changes as a result of user interaction.
    pub async fn selection_changed(&self) -> Selection {
        self.selection_changed.wait().await
    }*/
}

// Text view layout options:
// - alignment
// - wrapping
// - ellipsis (truncation mode)

// Given input constraints, and text that overflows:
// - wrap text
// - truncate text (with ellipsis)
// - become scrollable

bitflags! {
    pub struct TextEditEventFlags: u32 {
        /// The selection has changed.
        const SELECTION_CHANGED = (1 << 0);
        /// The text has changed.
        const TEXT_CHANGED = (1 << 1);
        /// The layout of the text edit has been invalidated.
        const RELAYOUT = (1 << 2);
        /// The text edit should be repainted.
        const REPAINT = (1 << 3);

        ///// The text edit should gain focus.
        //const GAIN_FOCUS =  (1 << 4);
        ///// The text edit should relinquish focus.
        //const LOSE_FOCUS = (1 << 5);

        // The text edit should acquire pointer capture.
        //const POINTER_CAPTURE = (1 << 6);

        /// Reset the blink phase of the caret.
        const RESET_BLINK = (1 << 7);
        /// Editing cancelled via escape key.
        const CANCELLED = (1 << 8);
        /// Editing finished via enter key.
        const CONFIRMED = (1 << 9);
    }
}

pub struct TextEditEventResult {
    pub flags: TextEditEventFlags,
}

impl TextEditEventResult {
    pub fn text_changed(&self) -> bool {
        self.flags.contains(TextEditEventFlags::TEXT_CHANGED)
    }
    pub fn repaint(&self) -> bool {
        self.flags.contains(TextEditEventFlags::REPAINT)
    }
    pub fn relayout(&self) -> bool {
        self.flags.contains(TextEditEventFlags::RELAYOUT)
    }
    pub fn selection_changed(&self) -> bool {
        self.flags.contains(TextEditEventFlags::SELECTION_CHANGED)
    }
    pub fn reset_blink(&self) -> bool {
        self.flags.contains(TextEditEventFlags::RESET_BLINK)
    }
    pub fn cancelled(&self) -> bool {
        self.flags.contains(TextEditEventFlags::CANCELLED)
    }
    pub fn confirmed(&self) -> bool {
        self.flags.contains(TextEditEventFlags::CONFIRMED)
    }
}

impl TextEditBase {
    pub fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("TextEdit::measure").entered();

        let space = layout_input.width.available().unwrap_or(f64::INFINITY) as f32;
        self.paragraph.layout(space);
        Size::new(self.paragraph.longest_line() as f64, self.paragraph.height() as f64)
    }

    pub fn layout(&mut self, size: Size) -> LayoutOutput {
        self.paragraph.layout(size.width as f32);
        let output = LayoutOutput {
            width: self.paragraph.longest_line() as f64,
            height: self.paragraph.height() as f64,
            baseline: Some(self.paragraph.alphabetic_baseline() as f64),
        };
        self.size = size;
        output
    }

    pub fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        // TODO: self.offset!
        ctx.bounds.contains(point)
    }

    pub fn paint(&mut self, ctx: &mut PaintCtx, bounds: Rect) {
        // Relayout if someone called set_text or set_text_style between the last call to layout() and now.
        if self.relayout {
            self.layout(Size::new(bounds.width(), bounds.height()));
            self.relayout = false;
        }

        let canvas = ctx.canvas();
        canvas.save();
        canvas.translate(-self.scroll_offset.to_skia() + self.offset.to_skia() + bounds.origin().to_vec2().to_skia());

        // paint the paragraph
        self.paragraph.paint(canvas, Point::ZERO.to_skia());

        // paint the selection rectangles
        let selection_rects = self.paragraph.get_rects_for_range(
            self.utf8_to_utf16_offset(self.selection.min())..self.utf8_to_utf16_offset(self.selection.max()),
            RectHeightStyle::Tight,
            RectWidthStyle::Tight,
        );
        let selection_paint = Paint::from(self.selection_color).to_sk_paint(PaintStyle::Fill);
        for text_box in selection_rects {
            canvas.draw_rect(text_box.rect, &selection_paint);
        }

        if self.blink_phase || self.blink_pending_reset {
            if let Some(info) = self.paragraph.get_glyph_cluster_at(self.selection.end) {
                let caret_rect = Rect::from_origin_size(
                    Point::new((info.bounds.left as f64).round(), (info.bounds.top as f64).round()),
                    Size::new(1.0, info.bounds.height() as f64),
                );
                //eprintln!("caret_rect: {:?}", caret_rect);
                let caret_paint = Paint::from(self.caret_color).to_sk_paint(PaintStyle::Fill);
                canvas.draw_rect(caret_rect.to_skia(), &caret_paint);
            }
        }

        canvas.restore();
    }

    //pub fn event(&mut self, event: &mut Event) -> TextEditEventResult {
    //    event.with_offset(self.offset, |event| self.event_inner(event))
    //}

    pub fn event(&mut self, bounds: Rect, event: &mut Event) -> TextEditEventResult {
        use TextEditEventFlags as F;

        let mut flags = TextEditEventFlags::empty();
        let origin = bounds.origin().to_vec2() + self.offset;

        match event {
            Event::PointerDown(event) => {
                let pos = event.position - origin;
                eprintln!("[text_edit] pointer down: {:?}", pos);
                if event.repeat_count == 2 {
                    // select word under cursor
                    flags.set(F::SELECTION_CHANGED, self.select_word_under_cursor());
                    self.gesture = Some(Gesture::WordSelection {
                        anchor: self.selection(),
                    });
                } else if event.repeat_count == 3 {
                    // TODO select line under cursor
                } else {
                    flags.set(F::SELECTION_CHANGED, self.set_cursor_at_point(pos, false));
                    self.gesture = Some(Gesture::CharacterSelection);
                }
                flags |= F::RESET_BLINK;
            }
            Event::PointerMove(event) => {
                //eprintln!("pointer move point: {:?}", event.local_position());
                let pos = event.position - origin;

                match self.gesture {
                    Some(Gesture::CharacterSelection) => {
                        flags.set(F::SELECTION_CHANGED, self.set_cursor_at_point(pos, true));
                        flags |= F::RESET_BLINK;
                    }
                    Some(Gesture::WordSelection { anchor }) => {
                        let text_offset = self.text_position_for_point(pos);
                        let word_selection = self.word_selection_at_text_position(text_offset);
                        flags.set(
                            F::SELECTION_CHANGED,
                            self.set_selection(add_selections(anchor, word_selection)),
                        );
                        flags |= F::RESET_BLINK;
                    }
                    _ => {}
                }
            }
            Event::PointerUp(_event) => {
                self.gesture = None;
            }
            Event::FocusGained => {
                eprintln!("focus gained");
                flags |= F::RESET_BLINK;
            }
            Event::FocusLost => {
                eprintln!("focus lost");
                let s = self.set_selection(Selection::empty(0));
                flags.set(F::SELECTION_CHANGED, s);
            }
            Event::KeyDown(event) => {
                let keep_anchor = event.modifiers.shift();
                let word_nav = event.modifiers.ctrl();
                match event.key {
                    Key::ArrowLeft => {
                        // TODO bidi?
                        if word_nav {
                            self.move_cursor_to_prev_word(keep_anchor);
                        } else {
                            self.move_cursor_to_prev_grapheme(keep_anchor);
                        }
                        flags |= F::SELECTION_CHANGED | F::RESET_BLINK;
                    }
                    Key::ArrowRight => {
                        if word_nav {
                            self.move_cursor_to_next_word(keep_anchor);
                        } else {
                            self.move_cursor_to_next_grapheme(keep_anchor);
                        }
                        flags |= F::SELECTION_CHANGED | F::RESET_BLINK;
                    }
                    Key::Backspace => {
                        if self.selection.is_empty() {
                            if let Some(prev) = prev_grapheme_cluster(&self.text, self.selection.end) {
                                self.set_cursor_at_text_position(prev, false);
                                self.text.remove(prev);
                                flags |= F::TEXT_CHANGED | F::SELECTION_CHANGED | F::RESET_BLINK | F::RELAYOUT;
                            }
                        } else {
                            let selection = self.selection();
                            self.text.replace_range(selection.byte_range(), "");
                            self.set_cursor_at_text_position(selection.min(), false);
                            flags |= F::TEXT_CHANGED | F::SELECTION_CHANGED | F::RESET_BLINK | F::RELAYOUT;
                        }
                    }
                    Key::Escape => {
                        flags |= F::CANCELLED;
                    }
                    Key::Enter => {
                        flags |= F::CONFIRMED;
                    }
                    Key::Character(ref s) => {
                        // TODO don't do this, emit the changed text instead
                        let mut text = self.text.clone();
                        let selection = self.selection();
                        text.replace_range(selection.byte_range(), &s);
                        self.text = text;
                        self.rebuild_paragraph();
                        self.relayout = true;
                        self.selection = Selection::empty(selection.min() + s.len());
                        flags |= F::SELECTION_CHANGED | F::TEXT_CHANGED | F::RESET_BLINK | F::RELAYOUT;
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        flags.set(F::REPAINT, flags.contains(F::SELECTION_CHANGED));
        TextEditEventResult { flags }
    }
}
