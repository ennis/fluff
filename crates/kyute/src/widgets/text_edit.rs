use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;
use std::time::Duration;

use keyboard_types::Key;
use kurbo::{Point, Rect, Size, Vec2};
use skia_safe::textlayout::{RectHeightStyle, RectWidthStyle};
use tracing::trace_span;
use unicode_segmentation::GraphemeCursor;

use crate::application::{spawn, wait_for};
use crate::drawing::{FromSkia, Paint, ToSkia};
use crate::element::{Element, ElementAny, ElementBuilder, ElementCtx, EventCtx, HitTestCtx, LayoutCtx};
use crate::event::Event;
use crate::handler::Handler;
use crate::layout::{LayoutInput, LayoutOutput, SizeConstraint};
use crate::text::{get_font_collection, Selection, TextAlign, TextLayout, TextStyle};
use crate::{Color, Notifier, PaintCtx};

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

struct TextEditState {
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
    line_clamp: Option<usize>,
    size: Size,
}

impl TextEditState {
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

    fn set_selection(&mut self, selection: Selection) -> bool {
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

    fn set_cursor_at_text_position(&mut self, pos: usize, keep_anchor: bool) -> bool {
        self.set_selection(if keep_anchor {
            Selection {
                start: self.selection.start,
                end: pos,
            }
        } else {
            Selection::empty(pos)
        })
    }

    fn move_cursor_to_next_word(&mut self, keep_anchor: bool) -> bool {
        let end = next_word_boundary(&self.text, self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    fn move_cursor_to_prev_word(&mut self, keep_anchor: bool) -> bool {
        let end = prev_word_boundary(&self.text, self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    fn move_cursor_to_next_grapheme(&mut self, keep_anchor: bool) -> bool {
        let end = next_grapheme_cluster(&self.text, self.selection.end).unwrap_or(self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    fn move_cursor_to_prev_grapheme(&mut self, keep_anchor: bool) -> bool {
        let end = prev_grapheme_cluster(&self.text, self.selection.end).unwrap_or(self.selection.end);
        self.set_cursor_at_text_position(end, keep_anchor)
    }

    /// Scrolls the text to make the given text position visible.
    fn scroll_in_view(&mut self, text_offset: usize) -> bool {
        let rects = self.paragraph.get_rects_for_range(
            text_offset..text_offset + 1,
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

    fn text_position_for_point(&self, point: Point) -> usize {
        let point = point + self.scroll_offset;
        // NOTE: get_glyph_position_at_coordinate returns a text position in bytes, not a glyph
        // position, as the name suggests.
        self.paragraph
            .get_glyph_position_at_coordinate(point.to_skia())
            .position as usize
    }

    /// NOTE: valid only after first layout.
    fn set_cursor_at_point(&mut self, point: Point, keep_anchor: bool) -> bool {
        let point = point + self.scroll_offset;
        let pos = self.paragraph.get_glyph_position_at_coordinate(point.to_skia());
        self.set_cursor_at_text_position(pos.position as usize, keep_anchor)
    }

    fn select_word_under_cursor(&mut self) -> bool {
        let selection = self.selection;
        let range = self.paragraph.get_word_boundary(selection.end as u32);
        self.set_selection(Selection {
            start: range.start,
            end: range.end,
        })
    }

    fn word_selection_at_text_position(&self, pos: usize) -> Selection {
        let range = self.paragraph.get_word_boundary(pos as u32);
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

    fn select_line_under_cursor(&mut self) -> bool {
        let text = &self.text;
        let selection = self.selection;
        let start = text[..selection.end].rfind('\n').map_or(0, |i| i + 1);
        let end = text[selection.end..]
            .find('\n')
            .map_or(text.len(), |i| selection.end + i);
        self.set_selection(Selection { start, end })
    }
}

const CARET_BLINK_INITIAL_DELAY: Duration = Duration::from_secs(1);
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);

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
pub struct TextEdit {
    selection_changed: Notifier<Selection>,
    state: TextEditState,
    gesture: Cell<Option<Gesture>>,
    blink_phase: Cell<bool>,
    blink_reset: Cell<bool>,
}

impl TextEdit {
    pub fn new() -> ElementBuilder<TextEdit> {
        let text_edit = ElementBuilder::new(TextEdit {
            selection_changed: Default::default(),
            state: TextEditState {
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
            },
            blink_phase: Cell::new(true),
            blink_reset: Cell::new(false),
            gesture: Cell::new(None),
        });

        // TODO
        //text_edit.set_tab_focusable(true);

        /*// spawn the caret blinker task
        let this_weak = text_edit.weak();
        spawn(async move {
            'task: loop {
                // Initial delay before blinking
                wait_for(CARET_BLINK_INITIAL_DELAY).await;
                // blinking
                'blink: loop {
                    if let Some(this) = this_weak.upgrade() {
                        if this.blink_reset.replace(false) {
                            // reset requested
                            this.blink_phase.set(true);
                            this.mark_needs_repaint();
                            break 'blink;
                        }
                        this.blink_phase.set(!this.blink_phase.get());
                        this.mark_needs_repaint();
                    } else {
                        // text edit is dead, exit task
                        break 'task;
                    }
                    wait_for(CARET_BLINK_INTERVAL).await;
                }
            }
        });*/

        text_edit
    }

    pub fn set_wrap_mode(&mut self, ctx: &mut ElementCtx, wrap_mode: WrapMode) {
        let this = &mut *self.state.borrow_mut();
        if this.wrap_mode != wrap_mode {
            this.wrap_mode = wrap_mode;
            this.rebuild_paragraph();
            this.relayout = true;
            ctx.mark_needs_relayout();
        }
    }

    pub fn set_max_lines(&mut self, ctx: &mut ElementCtx, max_lines: usize) {
        let this = &mut *self.state.borrow_mut();
        this.line_clamp = Some(max_lines);
        this.rebuild_paragraph();
        this.relayout = true;
        ctx.mark_needs_relayout();
    }

    pub fn set_text_align(&mut self, ctx: &mut ElementCtx, align: TextAlign) {
        let this = &mut *self.state.borrow_mut();
        if this.align != align {
            this.align = align;
            this.rebuild_paragraph();
            this.relayout = true;
            ctx.mark_needs_relayout();
        }
    }

    pub fn set_overflow(&mut self, ctx: &mut ElementCtx, overflow: TextOverflow) {
        let this = &mut *self.state.borrow_mut();
        if this.text_overflow != overflow {
            this.text_overflow = overflow;
            this.rebuild_paragraph();
            this.relayout = true;
            ctx.mark_needs_relayout();
        }
    }

    /// Resets the phase of the blinking caret.
    pub fn reset_blink(&mut self, ctx: &mut ElementCtx) {
        self.blink_phase.set(true);
        self.blink_reset.set(true);
        ctx.mark_needs_repaint();
    }

    pub fn set_caret_color(&mut self, ctx: &mut ElementCtx, color: Color) {
        let this = &mut *self.state.borrow_mut();
        if this.caret_color != color {
            this.caret_color = color;
            ctx.mark_needs_repaint();
        }
    }

    pub fn set_selection_color(&mut self, ctx: &mut ElementCtx, color: Color) {
        let this = &mut *self.state.borrow_mut();
        if this.selection_color != color {
            this.selection_color = color;
            ctx.mark_needs_repaint();
        }
    }

    pub fn set_text_style(&mut self, ctx: &mut ElementCtx, text_style: TextStyle) {
        let this = &mut *self.state.borrow_mut();
        this.text_style = text_style.into_static();
        this.rebuild_paragraph();
        this.relayout = true;
        ctx.mark_needs_relayout();
    }

    /// Returns the current selection.
    pub fn selection(&self) -> Selection {
        self.state.borrow().selection
    }

    /// Sets the current selection.
    pub fn set_selection(&mut self, ctx: &mut ElementCtx, selection: Selection) -> bool {
        // TODO clamp selection to text length
        let this = &mut *self.state.borrow_mut();
        if this.set_selection(selection) {
            ctx.mark_needs_repaint();
            true
        } else {
            false
        }
    }

    /// Returns the current text.
    pub fn text(&self) -> String {
        self.state.borrow().text.clone()
    }

    /// Sets the current text.
    pub fn set_text(&mut self, ctx: &mut ElementCtx, text: impl Into<String>) {
        // TODO we could compare the previous and new text
        // to relayout only affected lines.
        let this = &mut *self.state.borrow_mut();
        this.text = text.into();
        this.rebuild_paragraph();
        this.relayout = true;
        ctx.mark_needs_relayout();
    }

    pub fn text_position_for_point(&self, point: Point) -> usize {
        self.state.borrow().text_position_for_point(point)
    }

    /// NOTE: valid only after first layout.
    pub fn set_cursor_at_point(&mut self, ctx: &mut ElementCtx, point: Point, keep_anchor: bool) -> bool {
        if self.state.borrow_mut().set_cursor_at_point(point, keep_anchor) {
            ctx.mark_needs_repaint();
            true
        } else {
            false
        }
    }

    pub fn select_word_under_cursor(&mut self, ctx: &mut ElementCtx) {
        if self.state.borrow_mut().select_word_under_cursor() {
            self.mark_needs_repaint();
        }
    }

    pub fn word_selection_at_text_position(&self, pos: usize) -> Selection {
        self.state.borrow().word_selection_at_text_position(pos)
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

    /// Moves the cursor to the next or previous word boundary.
    pub fn move_cursor_to_next_word(&mut self, ctx: &mut ElementCtx, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_next_word(keep_anchor) {
            ctx.mark_needs_repaint();
        }
    }

    pub fn move_cursor_to_prev_word(&mut self, ctx: &mut ElementCtx, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_prev_word(keep_anchor) {
            ctx.mark_needs_repaint();
        }
    }

    pub fn move_cursor_to_next_grapheme(&mut self, ctx: &mut ElementCtx, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_next_grapheme(keep_anchor) {
            ctx.mark_needs_repaint();
        }
    }

    pub fn move_cursor_to_prev_grapheme(&mut self, ctx: &mut ElementCtx, keep_anchor: bool) {
        if self.state.borrow_mut().move_cursor_to_prev_grapheme(keep_anchor) {
            ctx.mark_needs_repaint();
        }
    }

    /// Selects the line under the cursor.
    pub fn select_line_under_cursor(&mut self, ctx: &mut ElementCtx) {
        if self.state.borrow_mut().select_line_under_cursor() {
            ctx.mark_needs_repaint();
        }
    }

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

impl Element for TextEdit {
    fn measure(&mut self, _ctx: &LayoutCtx, layout_input: &LayoutInput) -> Size {
        let _span = trace_span!("TextEdit::measure",).entered();

        let this = &mut *self.state.borrow_mut();
        let space = layout_input.width.available().unwrap_or(f64::INFINITY) as f32;
        this.paragraph.layout(space);
        Size::new(this.paragraph.longest_line() as f64, this.paragraph.height() as f64)
    }

    fn layout(&mut self, _ctx: &LayoutCtx, size: Size) -> LayoutOutput {
        let this = &mut *self.state.borrow_mut();
        this.paragraph.layout(size.width as f32);
        let output = LayoutOutput {
            width: this.paragraph.longest_line() as f64,
            height: this.paragraph.height() as f64,
            baseline: Some(this.paragraph.alphabetic_baseline() as f64),
        };
        this.size = size;
        output
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        todo!()
    }

    fn paint(&mut self, ctx: &mut PaintCtx) {
        let this = &mut *self.state.borrow_mut();
        let bounds = ctx.size.to_rect();

        ctx.with_canvas(|canvas| {
            // draw rect around bounds
            //let paint = Paint::from(Color::from_rgba_u8(255, 0, 0, 80)).to_sk_paint(bounds.to_rect());
            //canvas.draw_rect(bounds.to_rect().to_skia(), &paint);

            canvas.save();
            canvas.translate(-this.scroll_offset.to_skia());

            // paint the paragraph
            this.paragraph.paint(canvas, Point::ZERO.to_skia());

            // paint the selection rectangles
            let selection_rects = this.paragraph.get_rects_for_range(
                this.selection.min()..this.selection.max(),
                RectHeightStyle::Tight,
                RectWidthStyle::Tight,
            );
            let selection_paint = Paint::from(this.selection_color).to_sk_paint(bounds);
            for text_box in selection_rects {
                canvas.draw_rect(text_box.rect, &selection_paint);
            }

            if self.has_focus() && self.blink_phase.get() {
                if let Some(info) = this.paragraph.get_glyph_cluster_at(this.selection.end) {
                    let caret_rect = Rect::from_origin_size(
                        Point::new((info.bounds.left as f64).round(), (info.bounds.top as f64).round()),
                        Size::new(1.0, info.bounds.height() as f64),
                    );
                    //eprintln!("caret_rect: {:?}", caret_rect);
                    let caret_paint = Paint::from(this.caret_color).to_sk_paint(bounds);
                    canvas.draw_rect(caret_rect.to_skia(), &caret_paint);
                }
            }

            canvas.restore();
        });
    }

    fn event(&mut self, ctx: &mut EventCtx, event: &mut Event) {
        let mut selection_changed = false;
        //let mut this = self.state.borrow_mut();
        let mut set_focus = false;

        match event {
            Event::PointerDown(event) => {
                let pos = event.local_position();
                eprintln!("[text_edit] pointer down: {:?}", pos);
                if event.repeat_count == 2 {
                    // select word under cursor
                    self.select_word_under_cursor(ctx);
                    selection_changed = true;
                    self.gesture.set(Some(Gesture::WordSelection {
                        anchor: self.selection(),
                    }));
                } else if event.repeat_count == 3 {
                    // TODO select line under cursor
                } else {
                    selection_changed |= self.set_cursor_at_point(ctx, pos, false);
                    self.gesture.set(Some(Gesture::CharacterSelection));
                }
                self.reset_blink(ctx);
                // Don't immediately call `set_focus` because we'll recurse into this event handler
                // with `self.state` already borrowed mutably.
                set_focus = true;
                ctx.set_pointer_capture();
            }
            Event::PointerMove(event) => {
                //eprintln!("pointer move point: {:?}", event.local_position());
                let pos = event.local_position();

                match self.gesture.get() {
                    Some(Gesture::CharacterSelection) => {
                        selection_changed |= self.set_cursor_at_point(ctx, pos, true);
                    }
                    Some(Gesture::WordSelection { anchor }) => {
                        let text_offset = self.text_position_for_point(pos);
                        let word_selection = self.word_selection_at_text_position(text_offset);
                        selection_changed |= self.set_selection(ctx, add_selections(anchor, word_selection));
                    }
                    _ => {}
                }

                self.reset_blink(ctx);
            }
            Event::PointerUp(_event) => {
                self.gesture.set(None);
            }
            Event::FocusGained => {
                eprintln!("focus gained");
                self.reset_blink(ctx);
            }
            Event::FocusLost => {
                eprintln!("focus lost");
                selection_changed |= self.set_selection(ctx, Selection::empty(0));
            }
            Event::KeyDown(event) => {
                let keep_anchor = event.modifiers.shift();
                let word_nav = event.modifiers.ctrl();
                match event.key {
                    Key::ArrowLeft => {
                        // TODO bidi?
                        if word_nav {
                            self.move_cursor_to_prev_word(ctx, keep_anchor);
                        } else {
                            self.move_cursor_to_prev_grapheme(ctx, keep_anchor);
                        }
                        selection_changed = true;
                        self.reset_blink(ctx);
                    }
                    Key::ArrowRight => {
                        if word_nav {
                            self.move_cursor_to_next_word(ctx, keep_anchor);
                        } else {
                            self.move_cursor_to_next_grapheme(ctx, keep_anchor);
                        }
                        selection_changed = true;
                        self.reset_blink(ctx);
                    }
                    Key::Character(ref s) => {
                        // TODO don't do this, emit the changed text instead
                        let mut text = self.state.text.clone();
                        let selection = self.selection();
                        text.replace_range(selection.byte_range(), &s);
                        self.state.text = text;
                        self.state.rebuild_paragraph();
                        self.state.relayout = true;
                        self.state.selection = Selection::empty(selection.min() + s.len());
                        selection_changed = true;
                        ctx.mark_needs_relayout();
                        self.reset_blink(ctx);
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        if set_focus {
            ctx.set_focus();
        }

        if selection_changed {
            ctx.mark_needs_repaint();
            self.selection_changed.invoke(self.selection());
        }
    }
}
